use super::*;
use chrono::Utc;
use rusqlite::Connection;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::Path;
struct HomeGuard {
    previous: Option<OsString>,
}
impl HomeGuard {
    fn set(path: &Path) -> Self {
        let previous = std::env::var_os("HOME");
        std::env::set_var("HOME", path);
        Self { previous }
    }}
impl Drop for HomeGuard {
    fn drop(&mut self) {
        match self.previous.as_ref() {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
    }
}
#[test]
fn open_index_bootstraps_fresh_schema_with_workspace_columns() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp_home = tempfile::tempdir().unwrap();
    let _guard = HomeGuard::set(temp_home.path());

    let path = index_db_path();
    assert!(
        !path.exists(),
        "fresh test home should not have index.db yet"
    );

    let conn = open_index().unwrap();
    assert!(
        path.exists(),
        "open_index should create the file-backed index"
    );

    let mut stmt = conn.prepare("PRAGMA table_info(session_index)").unwrap();
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(columns.iter().any(|column| column == "workspace_id"));
    assert!(columns.iter().any(|column| column == "workspace_path"));

    let version: String = conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION.to_string());

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM session_index", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
