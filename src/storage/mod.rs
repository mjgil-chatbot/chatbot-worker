//! Shared storage helpers for the rebuildable SQLite read-side index.

pub mod atomic;
pub mod index;
pub use atomic::{atomic_write, write_string};
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
    }}
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
fn push_drift(messages: &mut Vec<String>, label: &str, expected: u64, actual: u64) {
    if expected != actual {
        messages.push(format!(
            "{label} drift: authoritative={expected}, indexed={actual}"
        ));
    }
}
