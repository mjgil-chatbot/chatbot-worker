use super::super::*;
use super::temp_path;
use super::MemTestGuard;
use chrono::{Datelike, TimeZone, Utc};
use std::fs;
#[test]
fn build_query_with_memory_appends_preamble() {
    let query = build_query_with_memory(
        "fix the worker path",
        Some(&MemoryContext {
            workspace_id: "workspace-1".to_string(),
            query: "worker path".to_string(),
            hits: vec![MemoryHit {
                doc_key: "chat-1:0:user".to_string(),
                chat_id: "chat-1".to_string(),
                role: "user".to_string(),
                session_name: "Debug Session".to_string(),
                timestamp: Utc.with_ymd_and_hms(2026, 3, 25, 12, 0, 0).unwrap(),
                snippet: "Need to inspect the worker request path.".to_string(),
                title: None,
                message_index: Some(0),
                turn_index: Some(0),
                score: Some(0.5),
            }],
        }),
    );

    assert!(query.contains("Retrieved memory:"));
    assert!(query.contains("Debug Session"));
    assert!(query.contains("User request:\nfix the worker path"));
}
#[test]
fn build_query_with_memory_returns_raw_query_when_no_context() {
    assert_eq!(build_query_with_memory("fix the bug", None), "fix the bug");
}
#[test]
fn build_query_with_memory_skips_empty_snippets() {
    let query = build_query_with_memory(
        "fix the bug",
        Some(&MemoryContext {
            workspace_id: "ws".to_string(),
            query: "bug".to_string(),
            hits: vec![
                MemoryHit {
                    doc_key: "chat-1:0:user".to_string(),
                    chat_id: "chat-1".to_string(),
                    role: "user".to_string(),
                    session_name: "Session".to_string(),
                    timestamp: Utc.with_ymd_and_hms(2026, 3, 25, 12, 0, 0).unwrap(),
                    snippet: "   ".to_string(),
                    title: None,
                    message_index: None,
                    turn_index: None,
                    score: None,
                },
                MemoryHit {
                    doc_key: "chat-2:0:user".to_string(),
                    chat_id: "chat-2".to_string(),
                    role: "user".to_string(),
                    session_name: "Session 2".to_string(),
                    timestamp: Utc.with_ymd_and_hms(2026, 3, 25, 13, 0, 0).unwrap(),
                    snippet: "Actual content here".to_string(),
                    title: None,
                    message_index: None,
                    turn_index: None,
                    score: None,
                },
            ],
        }),
    );
    assert!(query.contains("Actual content here"));
    assert!(!query.contains("[session=Session role=user"));
}
#[test]
fn sanitize_memory_text_normalizes_whitespace() {
    assert_eq!(sanitize_memory_text("  hello   world  "), "hello world");
    assert_eq!(
        sanitize_memory_text("no\nnewlines\there"),
        "no newlines here"
    );
}
#[test]
fn parse_timestamp_rfc3339() {
    let ts = parse_timestamp("2026-03-25T12:00:00Z").unwrap();
    assert_eq!(ts.year(), 2026);
    assert_eq!(ts.month(), 3);
    assert_eq!(ts.day(), 25);
}
#[test]
fn parse_timestamp_unix_seconds() {
    let ts = parse_timestamp("1711368000").unwrap();
    assert!(ts.year() >= 2024);
}
#[test]
fn workspace_memory_paths_use_local_state_files_only() {
    let _guard = MemTestGuard::new();
    let workspace = temp_path("paths-filenames");
    fs::create_dir_all(&workspace).unwrap();
    let paths = workspace_memory_paths(&workspace).unwrap();

    assert!(paths
        .ingest_state_path
        .to_string_lossy()
        .ends_with(INGEST_STATE_FILE_NAME));
    assert!(paths
        .manual_memories_path
        .to_string_lossy()
        .ends_with(MANUAL_MEMORIES_FILE_NAME));
    assert!(paths
        .writer_lock_path
        .to_string_lossy()
        .ends_with(WRITER_LOCK_FILE_NAME));

    let _ = fs::remove_dir_all(workspace);
}
