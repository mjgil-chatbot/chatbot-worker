//! Shared storage helpers for the rebuildable SQLite read-side index.

pub mod atomic;
pub mod index;

pub use atomic::{atomic_write, write_string};
pub use index::{
    delete_session, index_db_path, index_has_sessions, list_session_ids_for_workspace,
    list_sessions_from_index, list_workspaces_from_index, open_index, rebuild_session_index,
    search_sessions_fts, session_fts_content, set_session_hidden, upsert_session,
    upsert_session_fts, IndexedWorkspace, RebuildReport, SessionCounts, StorageIndex,
    StorageRowCounts,
};

use anyhow::Result;
use std::fs;
use std::path::Path;
use std::sync::{OnceLock, RwLock};

use crate::config;
use crate::model::ModelProvider;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuthoritativeRowCounts {
    pub session_files: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexIntegrityStatus {
    Healthy,
    Missing,
    Drifted,
    Unavailable,
}

impl IndexIntegrityStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Missing => "missing",
            Self::Drifted => "drifted",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexIntegrityReport {
    pub status: IndexIntegrityStatus,
    pub authoritative: AuthoritativeRowCounts,
    pub indexed: Option<StorageRowCounts>,
    pub warnings: Vec<String>,
    pub read_fallback_forced: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct IndexReadPolicy {
    enabled: bool,
    reason: Option<String>,
}

fn index_read_policy() -> &'static RwLock<IndexReadPolicy> {
    static POLICY: OnceLock<RwLock<IndexReadPolicy>> = OnceLock::new();
    POLICY.get_or_init(|| {
        RwLock::new(IndexReadPolicy {
            enabled: true,
            reason: None,
        })
    })
}

fn set_index_read_policy(enabled: bool, reason: Option<String>) {
    if let Ok(mut policy) = index_read_policy().write() {
        policy.enabled = enabled;
        policy.reason = reason;
    }
}

pub fn index_reads_enabled() -> bool {
    index_read_policy()
        .read()
        .map(|policy| policy.enabled)
        .unwrap_or(false)
}

pub fn index_reads_disabled_reason() -> Option<String> {
    index_read_policy()
        .read()
        .ok()
        .and_then(|policy| policy.reason.clone())
}

pub fn run_startup_integrity_check() -> IndexIntegrityReport {
    let authoritative = collect_authoritative_row_counts();
    let mut warnings = authoritative.warnings;
    let index_path = index_db_path();

    if !index_path.exists() {
        let report = IndexIntegrityReport {
            status: IndexIntegrityStatus::Missing,
            authoritative: authoritative.counts,
            indexed: None,
            warnings,
            read_fallback_forced: true,
        };
        set_index_read_policy(
            false,
            Some(format!(
                "startup integrity check: {} is missing",
                index_path.display()
            )),
        );
        return report;
    }

    match StorageIndex::open() {
        Ok(index) => match index.row_counts() {
            Ok(indexed) => {
                let drift = collect_index_drift(&authoritative.counts, &indexed);
                if drift.is_empty() {
                    set_index_read_policy(true, None);
                    IndexIntegrityReport {
                        status: IndexIntegrityStatus::Healthy,
                        authoritative: authoritative.counts,
                        indexed: Some(indexed),
                        warnings,
                        read_fallback_forced: false,
                    }
                } else {
                    warnings.extend(drift);
                    set_index_read_policy(
                        false,
                        Some(format!(
                            "startup integrity check detected drift in {}",
                            index_path.display()
                        )),
                    );
                    IndexIntegrityReport {
                        status: IndexIntegrityStatus::Drifted,
                        authoritative: authoritative.counts,
                        indexed: Some(indexed),
                        warnings,
                        read_fallback_forced: true,
                    }
                }
            }
            Err(err) => {
                warnings.push(format!(
                    "failed to read SQLite row counts from {}: {}",
                    index_path.display(),
                    err
                ));
                set_index_read_policy(
                    false,
                    Some(format!(
                        "startup integrity check could not read SQLite row counts from {}",
                        index_path.display()
                    )),
                );
                IndexIntegrityReport {
                    status: IndexIntegrityStatus::Unavailable,
                    authoritative: authoritative.counts,
                    indexed: None,
                    warnings,
                    read_fallback_forced: true,
                }
            }
        },
        Err(err) => {
            warnings.push(format!(
                "failed to open SQLite index {}: {}",
                index_path.display(),
                err
            ));
            set_index_read_policy(
                false,
                Some(format!(
                    "startup integrity check could not open SQLite index {}",
                    index_path.display()
                )),
            );
            IndexIntegrityReport {
                status: IndexIntegrityStatus::Unavailable,
                authoritative: authoritative.counts,
                indexed: None,
                warnings,
                read_fallback_forced: true,
            }
        }
    }
}

/// Returns a shared storage handle when the index can be opened.
pub fn get_index() -> Option<StorageIndex> {
    if !index_reads_enabled() {
        return None;
    }
    StorageIndex::open().ok()
}

/// Rebuild the full SQLite index from authoritative files.
pub fn rebuild_all() -> Result<RebuildReport> {
    StorageIndex::open()?.rebuild_all()
}

/// Rebuild the session rows for a single provider from authoritative files.
pub fn rebuild_index_for_provider(data_dir: &Path, provider_name: &str) -> Result<()> {
    let index = StorageIndex::open()?;
    let conn = index.connection();
    index::rebuild_session_index(&conn, data_dir, provider_name)
}

/// Dual-write a saved session into the SQLite read-side index.
pub fn index_session(
    _data_dir: &Path,
    session: &crate::data::ChatSession,
    provider_name: &str,
) -> Result<()> {
    let index = StorageIndex::open()?;
    let conn = index.connection();
    let (workspace_id, workspace_path) = session
        .launched_from
        .as_deref()
        .and_then(|launched_from| crate::memory::workspace_memory_paths(launched_from).ok())
        .map(|paths| {
            (
                Some(paths.workspace_id),
                Some(paths.workspace_root.to_string_lossy().into_owned()),
            )
        })
        .unwrap_or((None, None));

    index::upsert_session(
        &conn,
        &session.id,
        provider_name,
        &session.name,
        &session.created_at.to_rfc3339(),
        &session.updated_at.to_rfc3339(),
        session
            .messages
            .iter()
            .filter(|message| message.is_user)
            .count() as i64,
        Some(index::session_fingerprint_i64(session)),
        workspace_id.as_deref(),
        workspace_path.as_deref(),
        false,
    )?;
    index::upsert_session_fts(&conn, &session.id, &index::session_fts_content(session))?;
    Ok(())
}

/// Marks a session hidden in the SQLite read-side index.
pub fn hide_session(session_id: &str) -> Result<()> {
    let index = StorageIndex::open()?;
    let conn = index.connection();
    index::set_session_hidden(&conn, session_id, true)
}

/// Removes a session from the SQLite read-side index.
pub fn remove_session_from_index(session_id: &str) -> Result<()> {
    let index = StorageIndex::open()?;
    let conn = index.connection();
    index::delete_session(&conn, session_id)
}

#[derive(Debug, Default)]
struct AuthoritativeRowCountScan {
    counts: AuthoritativeRowCounts,
    warnings: Vec<String>,
}

fn collect_authoritative_row_counts() -> AuthoritativeRowCountScan {
    let mut scan = AuthoritativeRowCountScan::default();

    let home = match dirs::home_dir() {
        Some(home) => home,
        None => {
            scan.warnings
                .push("could not resolve home directory for startup integrity check".to_string());
            return scan;
        }
    };

    for provider in ModelProvider::ALL {
        let sessions_dir = config::resolve_data_dir_under_home(provider, &home).join("sessions");
        scan.counts.session_files += count_json_files(&sessions_dir, &mut scan.warnings);
    }

    scan
}

fn count_json_files(path: &Path, warnings: &mut Vec<String>) -> u64 {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return 0,
        Err(err) => {
            warnings.push(format!("failed to read {}: {}", path.display(), err));
            return 0;
        }
    };

    let mut count = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            count += 1;
        }
    }
    count
}

fn collect_index_drift(
    authoritative: &AuthoritativeRowCounts,
    indexed: &StorageRowCounts,
) -> Vec<String> {
    let mut drift = Vec::new();

    push_drift(
        &mut drift,
        "session_index rows",
        authoritative.session_files,
        indexed.session_index,
    );
    push_drift(
        &mut drift,
        "session_fts rows",
        authoritative.session_files,
        indexed.session_fts,
    );

    drift
}

fn push_drift(messages: &mut Vec<String>, label: &str, expected: u64, actual: u64) {
    if expected != actual {
        messages.push(format!(
            "{label} drift: authoritative={expected}, indexed={actual}"
        ));
    }
}
