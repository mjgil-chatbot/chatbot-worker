use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use crate::data::{ChatSession, SessionInfo, SessionMetadata};
const SCHEMA_VERSION: i64 = 2;
const INDEX_DIR_NAME: &str = ".rust-chatbot";
const INDEX_FILE_NAME: &str = "index.db";
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS schema_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS session_index (
    session_id    TEXT PRIMARY KEY,
    provider      TEXT NOT NULL,
    name          TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    message_count INTEGER NOT NULL DEFAULT 0,
    fingerprint   INTEGER,
    workspace_id  TEXT,
    workspace_path TEXT,
    hidden        BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE INDEX IF NOT EXISTS idx_session_provider ON session_index(provider);
CREATE INDEX IF NOT EXISTS idx_session_updated ON session_index(updated_at);
CREATE INDEX IF NOT EXISTS idx_session_provider_workspace ON session_index(provider, workspace_id);

CREATE VIRTUAL TABLE IF NOT EXISTS session_fts USING fts5(
    session_id UNINDEXED,
    content,
    tokenize='porter unicode61'
);
"#;
#[derive(Clone)]
pub struct StorageIndex {
    pub(crate) conn: Arc<Mutex<Connection>>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StorageRowCounts {
    pub session_index: u64,
    pub session_fts: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionCounts {
    pub total: u64,
    pub hidden: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedWorkspace {
    pub workspace_id: String,
    pub workspace_path: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RebuildReport {
    pub sessions_indexed: u64,
    pub source_counts: StorageRowCounts,
    pub indexed_counts: StorageRowCounts,
    pub drifted_tables: Vec<String>,
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StorageIntegrityReport {
    pub authoritative_counts: StorageRowCounts,
    pub indexed_counts: StorageRowCounts,
    pub drifted_tables: Vec<String>,
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug)]
struct SessionDocument {
    session_id: String,
    provider: String,
    name: String,
    created_at: String,
    updated_at: String,
    message_count: i64,
    fingerprint: i64,
    workspace_id: Option<String>,
    workspace_path: Option<String>,
    hidden: bool,
    fts_content: String,
}
pub fn index_db_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(INDEX_DIR_NAME)
        .join(INDEX_FILE_NAME)
}
pub fn open_index() -> Result<Connection> {
    let path = index_db_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let conn =
        Connection::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
    configure_connection(&conn)?;
    ensure_schema(&conn)?;
    Ok(conn)
}
impl StorageIndex {
    pub fn open() -> Result<Self> {
        Ok(Self {
            conn: Arc::new(Mutex::new(open_index()?)),
        })
    }}
impl StorageIndex {
    pub fn open_default() -> Result<Self> {
        Self::open()
    }}
impl StorageIndex {
    pub fn path(&self) -> PathBuf {
        index_db_path()
    }}
impl StorageIndex {
    pub fn is_available(&self) -> bool {
        self.conn.lock().is_ok()
    }}
impl StorageIndex {
    pub fn connection(&self) -> MutexGuard<'_, Connection> {
        match self.conn.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }}
impl StorageIndex {
    pub fn row_counts(&self) -> Result<StorageRowCounts> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("storage index lock poisoned"))?;
        row_counts_with_conn(&conn)
    }}
impl StorageIndex {
    pub fn session_counts(&self) -> Result<SessionCounts> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("storage index lock poisoned"))?;
        let total: i64 =
            conn.query_row("SELECT COUNT(*) FROM session_index", [], |row| row.get(0))?;
        let hidden: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_index WHERE hidden = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(SessionCounts {
            total: total.max(0) as u64,
            hidden: hidden.max(0) as u64,
        })
    }}
pub fn upsert_session(
    conn: &Connection,
    session_id: &str,
    provider: &str,
    name: &str,
    created_at: &str,
    updated_at: &str,
    message_count: i64,
    fingerprint: Option<i64>,
    workspace_id: Option<&str>,
    workspace_path: Option<&str>,
    hidden: bool,
) -> Result<()> {
    conn.execute(
        "INSERT INTO session_index (session_id, provider, name, created_at, updated_at, message_count, fingerprint, workspace_id, workspace_path, hidden)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(session_id) DO UPDATE SET
             provider = excluded.provider,
             name = excluded.name,
             created_at = excluded.created_at,
             updated_at = excluded.updated_at,
             message_count = excluded.message_count,
             fingerprint = excluded.fingerprint,
             workspace_id = excluded.workspace_id,
             workspace_path = excluded.workspace_path,
             hidden = excluded.hidden",
        params![
            session_id,
            provider,
            name,
            created_at,
            updated_at,
            message_count,
            fingerprint,
            workspace_id,
            workspace_path,
            hidden,
        ],
    )?;
    Ok(())
}
pub fn set_session_hidden(conn: &Connection, session_id: &str, hidden: bool) -> Result<()> {
    conn.execute(
        "UPDATE session_index SET hidden = ?1 WHERE session_id = ?2",
        params![hidden, session_id],
    )?;
    Ok(())
}
pub fn upsert_session_fts(conn: &Connection, session_id: &str, content: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM session_fts WHERE session_id = ?1",
        params![session_id],
    )?;
    conn.execute(
        "INSERT INTO session_fts (session_id, content) VALUES (?1, ?2)",
        params![session_id, content],
    )?;
    Ok(())
}
pub fn delete_session(conn: &Connection, session_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM session_fts WHERE session_id = ?1",
        params![session_id],
    )?;
    conn.execute(
        "DELETE FROM session_index WHERE session_id = ?1",
        params![session_id],
    )?;
    Ok(())
}
pub fn index_has_sessions(conn: &Connection) -> Result<bool> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM session_index", [], |row| row.get(0))?;
    Ok(count > 0)
}
pub fn list_sessions_from_index(
    conn: &Connection,
    provider_name: &str,
) -> Result<Vec<SessionInfo>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, name, created_at, updated_at, message_count, hidden
         FROM session_index
         WHERE provider = ?1 AND hidden = 0
         ORDER BY updated_at DESC",
    )?;
    let mut sessions = Vec::new();
    let rows = stmt.query_map([provider_name], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, bool>(5)?,
        ))
    })?;

    for row in rows {
        let (id, name, created_at, updated_at, message_count, hidden) = row?;
        sessions.push(SessionInfo {
            id,
            name,
            created_at: parse_rfc3339(&created_at)?,
            updated_at: parse_rfc3339(&updated_at)?,
            message_count: message_count.max(0) as usize,
            hidden,
        });
    }
    Ok(sessions)
}
pub fn list_workspaces_from_index(
    conn: &Connection,
    provider_name: &str,
) -> Result<Vec<IndexedWorkspace>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT workspace_id, workspace_path
         FROM session_index
         WHERE provider = ?1
           AND hidden = 0
           AND workspace_id IS NOT NULL
           AND workspace_path IS NOT NULL
         ORDER BY workspace_path ASC",
    )?;
    let rows = stmt.query_map([provider_name], |row| {
        Ok(IndexedWorkspace {
            workspace_id: row.get(0)?,
            workspace_path: row.get(1)?,
        })
    })?;
    let mut workspaces = Vec::new();
    for row in rows {
        workspaces.push(row?);
    }
    Ok(workspaces)
}
pub fn list_session_ids_for_workspace(
    conn: &Connection,
    provider_name: &str,
    workspace_id: &str,
) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT session_id
         FROM session_index
         WHERE provider = ?1 AND workspace_id = ?2 AND hidden = 0
         ORDER BY updated_at DESC",
    )?;
    let rows = stmt.query_map(params![provider_name, workspace_id], |row| row.get(0))?;
    let mut session_ids = Vec::new();
    for row in rows {
        session_ids.push(row?);
    }
    Ok(session_ids)
}
pub fn search_sessions_fts(conn: &Connection, query: &str) -> Result<Vec<String>> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(sanitize_fts_term)
        .filter(|term| !term.is_empty())
        .map(|term| format!("{term}*"))
        .collect();
    if terms.is_empty() {
        return Ok(Vec::new());
    }

    let mut stmt = conn.prepare("SELECT session_id FROM session_fts WHERE session_fts MATCH ?1")?;
    let rows = stmt.query_map([terms.join(" ")], |row| row.get::<_, String>(0))?;
    let mut ids = Vec::new();
    for row in rows {
        ids.push(row?);
    }
    Ok(ids)
}
pub fn session_fts_content(session: &ChatSession) -> String {
    let mut out = String::new();
    append_fts_chunk(&mut out, &session.name);
    if let Some(system_prompt) = session.system_prompt.as_ref() {
        append_fts_chunk(&mut out, system_prompt);
    }
    for message in &session.messages {
        append_fts_chunk(&mut out, &message.content);
        if let Some(command) = message.command.as_ref() {
            append_fts_chunk(&mut out, command);
        }
    }
    out
}
fn append_fts_chunk(out: &mut String, chunk: &str) {
    if chunk.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(chunk);
}
pub fn session_fingerprint_i64(session: &ChatSession) -> i64 {
    session.history_fingerprint() as i64
}
fn configure_connection(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}
fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    // Existing installations may pre-date the current schema columns.
    // Backfill them after the base tables/indexes are present.
    ensure_column(conn, "session_index", "workspace_id", "TEXT")?;
    ensure_column(conn, "session_index", "workspace_path", "TEXT")?;
    let version = read_schema_version(conn)?;
    if version != SCHEMA_VERSION {
        conn.execute("DELETE FROM schema_meta WHERE key = 'schema_version'", [])?;
        conn.execute(
            "INSERT INTO schema_meta (key, value) VALUES ('schema_version', ?1)",
            [SCHEMA_VERSION.to_string()],
        )?;
    }
    Ok(())
}
fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(());
        }
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
        [],
    )?;
    Ok(())
}
fn read_schema_version(conn: &Connection) -> Result<i64> {
    let version = conn.query_row(
        "SELECT value FROM schema_meta WHERE key = 'schema_version'",
        [],
        |row| row.get::<_, String>(0),
    );
    match version {
        Ok(value) => Ok(value.parse::<i64>().unwrap_or_default()),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
        Err(err) => Err(err.into()),
    }
}
fn row_counts_with_conn(conn: &Connection) -> Result<StorageRowCounts> {
    Ok(StorageRowCounts {
        session_index: count_rows(conn, "session_index")?,
        session_fts: count_rows(conn, "session_fts")?,
    })
}
fn count_rows(conn: &Connection, table: &str) -> Result<u64> {
    let query = format!("SELECT COUNT(*) FROM {table}");
    let count: i64 = conn.query_row(&query, [], |row| row.get(0))?;
    Ok(count.max(0) as u64)
}
fn clear_session_rows_for_provider(conn: &Connection, provider_name: &str) -> Result<()> {
    let session_ids = {
        let mut stmt = conn.prepare("SELECT session_id FROM session_index WHERE provider = ?1")?;
        let rows = stmt.query_map([provider_name], |row| row.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row?);
        }
        ids
    };

    for session_id in session_ids {
        conn.execute(
            "DELETE FROM session_fts WHERE session_id = ?1",
            params![session_id],
        )?;
    }
    conn.execute(
        "DELETE FROM session_index WHERE provider = ?1",
        params![provider_name],
    )?;
    Ok(())
}
fn load_hidden_sessions(data_dir: &Path) -> Result<HashMap<String, bool>> {
    let metadata_path = data_dir.join("sessions.json");
    if !metadata_path.is_file() {
        return Ok(HashMap::new());
    }

    let raw = fs::read_to_string(&metadata_path)
        .with_context(|| format!("failed to read {}", metadata_path.display()))?;
    let metadata: SessionMetadata = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", metadata_path.display()))?;
    Ok(metadata
        .sessions
        .into_iter()
        .map(|(session_id, info)| (session_id, info.hidden))
        .collect())
}
fn authoritative_counts(sessions: &[SessionDocument]) -> StorageRowCounts {
    let count = sessions.len() as u64;
    StorageRowCounts {
        session_index: count,
        session_fts: count,
    }
}
fn collect_index_drift(
    authoritative: &StorageRowCounts,
    indexed: &StorageRowCounts,
) -> Vec<String> {
    let mut drift = Vec::new();
    push_drift(
        &mut drift,
        "session_index rows",
        authoritative.session_index,
        indexed.session_index,
    );
    push_drift(
        &mut drift,
        "session_fts rows",
        authoritative.session_fts,
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
fn sanitize_fts_term(term: &str) -> String {
    term.chars()
        .filter(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-'))
        .collect()
}
fn parse_rfc3339(input: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(input)?.with_timezone(&Utc))
}
#[cfg(test)]
mod tests;
