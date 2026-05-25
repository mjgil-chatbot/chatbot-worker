use super::super::*;
use super::{temp_path, MemTestGuard, MockMemMachineServer};
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
fn retrieve_memory_context_reads_workspace_memmachine() {
    let _guard = MemTestGuard::new();
    let _server = MockMemMachineServer::start();
    let workspace = temp_path("workspace");
    fs::create_dir_all(&workspace).unwrap();

    let session = super::make_session(
        &workspace,
        "chat-1",
        "Debug Session",
        vec![(
            "Need to inspect the worker request path for memory retrieval.",
            true,
            None,
        )],
    );

    let summary = index_workspace_sessions(&workspace, &[session], &Default::default()).unwrap();
    assert_eq!(summary.indexed_messages, 1);

    let context = retrieve_memory_context(&workspace, "memory retrieval")
        .unwrap()
        .unwrap();
    assert_eq!(context.hits.len(), 1);
    assert_eq!(context.hits[0].doc_key, "chat-1:0:user");
    assert_eq!(context.hits[0].chat_id, "chat-1");
    assert_eq!(context.hits[0].role, "user");
    assert_eq!(context.hits[0].session_name, "Debug Session");
    assert_eq!(context.hits[0].message_index, Some(0));
    assert_eq!(context.hits[0].turn_index, Some(0));

    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn retrieve_memory_context_prefers_manual_memories() {
    let _guard = MemTestGuard::new();
    let _server = MockMemMachineServer::start();
    let workspace = temp_path("workspace-manual-priority");
    fs::create_dir_all(&workspace).unwrap();

    remember_manual_memory(
        &workspace,
        "The staging API base URL is https://staging.internal.example.",
        ManualMemoryPriority::High,
    )
    .unwrap();

    let session = super::make_session(
        &workspace,
        "chat-1",
        "Debug Session",
        vec![(
            "The staging API uses a temporary host during migrations.",
            true,
            None,
        )],
    );
    index_workspace_sessions(&workspace, &[session], &Default::default()).unwrap();

    let context = retrieve_memory_context(&workspace, "staging API base URL")
        .unwrap()
        .unwrap();
    assert!(!context.hits.is_empty());
    assert!(context.hits[0].doc_key.starts_with("manual:"));
    assert!(context.hits[0]
        .snippet
        .contains("https://staging.internal.example"));

    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn retrieve_memory_context_skips_invalidated_doc_keys() {
    let _guard = MemTestGuard::new();
    let _server = MockMemMachineServer::start();
    let workspace = temp_path("workspace-invalidated");
    fs::create_dir_all(&workspace).unwrap();

    let session = super::make_session(
        &workspace,
        "chat-1",
        "Debug Session",
        vec![
            (
                "Need to inspect the worker request path for memory retrieval.",
                true,
                None,
            ),
            (
                "Memory retrieval should keep the valid hit available.",
                true,
                None,
            ),
        ],
    );
    index_workspace_sessions(&workspace, &[session], &Default::default()).unwrap();

    invalidate_memory_doc_key(
        &workspace,
        "chat-1:0:user",
        Some("stale memory".to_string()),
    )
    .unwrap();

    let context = retrieve_memory_context(&workspace, "memory retrieval")
        .unwrap()
        .unwrap();
    assert_eq!(context.hits.len(), 1);
    assert_eq!(context.hits[0].doc_key, "chat-1:1:user");

    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn retrieve_memory_context_works_without_remote_project() {
    let _guard = MemTestGuard::new();
    let workspace = temp_path("manual-only");
    fs::create_dir_all(&workspace).unwrap();

    remember_manual_memory(
        &workspace,
        "The API key lives in env var OPENAI_KEY",
        ManualMemoryPriority::High,
    )
    .unwrap();

    let context = retrieve_memory_context(&workspace, "API key")
        .unwrap()
        .unwrap();
    assert!(context
        .hits
        .iter()
        .any(|hit| hit.snippet.contains("OPENAI_KEY")));

    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn retrieve_memory_context_retains_transcript_only_assistant_tool_failures() {
    let _guard = MemTestGuard::new();
    let _server = MockMemMachineServer::start();
    let workspace = temp_path("workspace-transcript-only");
    fs::create_dir_all(&workspace).unwrap();

    let session = super::make_session(
        &workspace,
        "chat-1",
        "Transcript Only",
        vec![
            ("Investigate memory noise.", true, None),
            (
                "[May 21 01:01am] MM-2.7 | Grep max_iterations?|max.*iteration\n[May 21 01:01am] MM-2.7 | Grep max_iterations?|max.*iteration| Tool execution failed: missing field case_sensitive",
                false,
                None,
            ),
        ],
    );

    index_workspace_sessions(&workspace, &[session], &Default::default()).unwrap();

    let context = retrieve_memory_context(&workspace, "missing field case_sensitive")
        .unwrap()
        .unwrap();
    assert_eq!(context.hits.len(), 1);
    assert!(context.hits[0]
        .snippet
        .contains("Tool execution failed: missing field case_sensitive"));

    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn retrieve_memory_context_uses_assistant_body_without_transcript_preamble() {
    let _guard = MemTestGuard::new();
    let _server = MockMemMachineServer::start();
    let workspace = temp_path("workspace-transcript-body");
    fs::create_dir_all(&workspace).unwrap();

    let session = super::make_session(
        &workspace,
        "chat-1",
        "Transcript Body",
        vec![
            ("Investigate memory noise.", true, None),
            (
                "[May 21 01:01am] MM-2.7 | Grep max_iterations?|max.*iteration\n[May 21 01:01am] MM-2.7 | Grep max_iterations?|max.*iteration| Tool execution failed: missing field case_sensitive\nThe root cause is a required grep schema field mismatch.",
                false,
                None,
            ),
        ],
    );

    index_workspace_sessions(&workspace, &[session], &Default::default()).unwrap();

    let context = retrieve_memory_context(&workspace, "required grep schema field mismatch")
        .unwrap()
        .unwrap();
    assert_eq!(context.hits.len(), 1);
    assert!(context.hits[0]
        .snippet
        .contains("The root cause is a required grep schema field mismatch."));
    assert!(!context.hits[0].snippet.contains("[May 21 01:01am]"));

    let _ = fs::remove_dir_all(workspace);
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
