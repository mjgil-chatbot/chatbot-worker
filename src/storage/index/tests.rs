use super::*;
use chrono::Utc;
use rusqlite::Connection;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::Path;

fn write_session_fixture(root: &Path, session_id: &str, hidden: bool) -> Result<()> {
    let sessions_dir = root.join("sessions");
    fs::create_dir_all(&sessions_dir)?;
    let now = Utc::now();
    let session = ChatSession {
        id: session_id.to_string(),
        name: "Hidden Session".to_string(),
        system_prompt: None,
        json_summary: None,
        claude_session_id: None,
        minimax_session_id: None,
        qwen_session_id: None,
        codex_session_id: None,
        nvidia_session_id: None,
        gemini_session_id: None,
        openrouter_session_id: None,
        omniforge_session_id: None,
        omniforge_model_id: None,
        kimi_session_id: None,
        opencode_session_id: None,
        hermes_session_id: None,
        forge_session_id: None,
        claw_session_id: None,
        launched_from: None,
        runsc_dirs: Vec::new(),
        runsc_image: None,
        created_at: now,
        updated_at: now,
        messages: vec![],
    };
    fs::write(
        sessions_dir.join(format!("{session_id}.json")),
        serde_json::to_string_pretty(&session)?,
    )?;

    let mut sessions = HashMap::new();
    sessions.insert(
        session_id.to_string(),
        SessionInfo {
            id: session_id.to_string(),
            name: session.name.clone(),
            created_at: now,
            updated_at: now,
            message_count: 0,
            hidden,
        },
    );
    fs::write(
        root.join("sessions.json"),
        serde_json::to_string_pretty(&SessionMetadata { sessions })?,
    )?;
    Ok(())
}

struct HomeGuard {
    previous: Option<OsString>,
}

impl HomeGuard {
    fn set(path: &Path) -> Self {
        let previous = std::env::var_os("HOME");
        std::env::set_var("HOME", path);
        Self { previous }
    }
}

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

#[test]
fn rebuild_session_index_preserves_hidden_flag_from_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let data_dir = temp.path().join("claude");
    write_session_fixture(&data_dir, "session-hidden", true).unwrap();

    let conn = Connection::open_in_memory().unwrap();
    configure_connection(&conn).unwrap();
    ensure_schema(&conn).unwrap();
    rebuild_session_index(&conn, &data_dir, "claude").unwrap();

    let hidden: bool = conn
        .query_row(
            "SELECT hidden FROM session_index WHERE session_id = ?1",
            ["session-hidden"],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        hidden,
        "rebuild should preserve the soft-delete hidden flag"
    );
}

#[test]
fn rebuild_session_index_visible_session_is_not_hidden() {
    let temp = tempfile::tempdir().unwrap();
    let data_dir = temp.path().join("claude");
    write_session_fixture(&data_dir, "session-visible", false).unwrap();

    let conn = Connection::open_in_memory().unwrap();
    configure_connection(&conn).unwrap();
    ensure_schema(&conn).unwrap();
    rebuild_session_index(&conn, &data_dir, "claude").unwrap();

    let hidden: bool = conn
        .query_row(
            "SELECT hidden FROM session_index WHERE session_id = ?1",
            ["session-visible"],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!hidden, "rebuild must not mark a visible session as hidden");
}

#[test]
fn load_hidden_sessions_returns_empty_when_metadata_absent() {
    let temp = tempfile::tempdir().unwrap();
    let result = load_hidden_sessions(temp.path()).unwrap();
    assert!(
        result.is_empty(),
        "missing sessions.json should produce an empty map, not an error"
    );
}
