use super::super::*;
use super::{make_session, temp_path, MemTestGuard, MockMemMachineServer};
use crate::model::ModelProvider;
use chrono::{TimeZone, Utc};
use std::collections::BTreeMap;
use std::fs;

fn session_with_provider(
    workspace: &std::path::Path,
    provider: ModelProvider,
    session_id: &str,
) -> ChatSession {
    let mut session = make_session(workspace, provider.as_str(), "S", vec![]);
    for candidate in ModelProvider::ALL {
        candidate.clear_session_id_on_chat(&mut session);
    }
    provider.set_session_id_on_chat(&mut session, session_id.to_string());
    session
}

#[test]
fn ingest_state_round_trips() {
    let dir = temp_path("ingest-state");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(INGEST_STATE_FILE_NAME);

    let mut state = IngestState::default();
    state.sessions.insert(
        "chat-1".to_string(),
        SessionIngestState {
            indexed_message_count: 3,
            history_fingerprint: "abc123".to_string(),
            updated_at: Utc::now(),
        },
    );
    state.invalidated_doc_keys.insert(
        "chat-1:2:assistant".to_string(),
        MemoryInvalidation {
            invalidated_at: Utc::now(),
            reason: Some("stale memory".to_string()),
        },
    );

    save_ingest_state(&path, &state).unwrap();
    let loaded = load_ingest_state(&path).unwrap();
    assert_eq!(loaded.sessions.len(), 1);
    assert_eq!(loaded.invalidated_doc_keys.len(), 1);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn index_workspace_sessions_backfills_memmachine_project() {
    let _guard = MemTestGuard::new();
    let server = MockMemMachineServer::start();
    let workspace = temp_path("workspace-backfill");
    fs::create_dir_all(&workspace).unwrap();

    let session_one = make_session(
        &workspace,
        "chat-1",
        "Debug Session",
        vec![
            (
                "Need to inspect memory retrieval.",
                true,
                Some("codex exec"),
            ),
            ("The worker request needs memory context.", false, None),
        ],
    );
    let session_two = make_session(
        &workspace,
        "chat-2",
        "Earlier Session",
        vec![
            ("Previous conversation about backfill.", true, None),
            ("Backfill should ingest older chats too.", false, None),
        ],
    );

    let summary = index_workspace_sessions(
        &workspace,
        &[session_one.clone(), session_two.clone()],
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(summary.indexed_sessions, 2);
    assert_eq!(summary.indexed_messages, 4);

    let workspace_id = workspace_id_for_path(&workspace).unwrap();
    assert_eq!(server.episode_count(&workspace_id), 4);

    let state = load_ingest_state(
        &workspace_memory_paths(&workspace)
            .unwrap()
            .ingest_state_path,
    )
    .unwrap();
    assert_eq!(state.sessions.len(), 2);
    assert_eq!(
        state.sessions.get("chat-1").unwrap().indexed_message_count,
        session_one.messages.len()
    );
    assert_eq!(
        state.sessions.get("chat-2").unwrap().indexed_message_count,
        session_two.messages.len()
    );

    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn index_workspace_sessions_appends_new_messages_without_rebuild() {
    let _guard = MemTestGuard::new();
    let server = MockMemMachineServer::start();
    let workspace = temp_path("workspace-append");
    fs::create_dir_all(&workspace).unwrap();

    let mut session = make_session(
        &workspace,
        "chat-1",
        "Append Session",
        vec![("Need memory append coverage.", true, None)],
    );
    let first = index_workspace_sessions(&workspace, &[session.clone()], &BTreeMap::new()).unwrap();
    assert_eq!(first.indexed_messages, 1);

    session.messages.push(ChatMessage {
        content: "Append path now indexes the assistant reply.".to_string(),
        is_user: false,
        timestamp: Utc.with_ymd_and_hms(2026, 3, 25, 12, 1, 0).unwrap(),
        command: None,
        retrieved_memory: None,
    });
    session.updated_at = session.messages.last().unwrap().timestamp;

    let second =
        index_workspace_sessions(&workspace, &[session.clone()], &BTreeMap::new()).unwrap();
    assert!(!second.rebuilt);
    assert_eq!(second.indexed_sessions, 1);
    assert_eq!(second.indexed_messages, 1);

    let workspace_id = workspace_id_for_path(&workspace).unwrap();
    assert_eq!(server.episode_count(&workspace_id), 2);

    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn should_rebuild_when_indexed_session_removed() {
    let _guard = MemTestGuard::new();
    let server = MockMemMachineServer::start();
    let workspace = temp_path("rebuild-removed");
    fs::create_dir_all(&workspace).unwrap();

    let session1 = make_session(&workspace, "chat-1", "S1", vec![("msg1", true, None)]);
    let session2 = make_session(&workspace, "chat-2", "S2", vec![("msg2", true, None)]);

    index_workspace_sessions(
        &workspace,
        &[session1.clone(), session2.clone()],
        &BTreeMap::new(),
    )
    .unwrap();

    let summary = index_workspace_sessions(
        &workspace,
        std::slice::from_ref(&session1),
        &BTreeMap::new(),
    )
    .unwrap();
    assert!(summary.rebuilt);

    let workspace_id = workspace_id_for_path(&workspace).unwrap();
    assert_eq!(server.episode_count(&workspace_id), 1);

    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn rebuild_workspace_memory_preserves_invalidated_doc_keys() {
    let _guard = MemTestGuard::new();
    let _server = MockMemMachineServer::start();
    let workspace = temp_path("rebuild-invalidation-workspace");
    fs::create_dir_all(&workspace).unwrap();

    let session = make_session(
        &workspace,
        "chat-1",
        "Test Session",
        vec![("Hello world", true, None), ("Hi there", false, None)],
    );
    index_workspace_sessions(&workspace, std::slice::from_ref(&session), &BTreeMap::new()).unwrap();

    invalidate_memory_doc_key(
        &workspace,
        "chat-1:0:user",
        Some("test invalidation".to_string()),
    )
    .unwrap();

    let mut modified_session = session.clone();
    modified_session.messages[1] = ChatMessage {
        content: "Hi there, updated reply".to_string(),
        is_user: false,
        timestamp: modified_session.messages[1].timestamp,
        command: None,
        retrieved_memory: None,
    };
    let rebuild_summary =
        index_workspace_sessions(&workspace, &[modified_session], &BTreeMap::new()).unwrap();
    assert!(rebuild_summary.rebuilt);

    let state = load_ingest_state(
        &workspace_memory_paths(&workspace)
            .unwrap()
            .ingest_state_path,
    )
    .unwrap();
    assert!(state.invalidated_doc_keys.contains_key("chat-1:0:user"));

    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn session_provider_detects_codex_and_gemini() {
    let workspace = temp_path("provider-detect");
    fs::create_dir_all(&workspace).unwrap();

    let codex = session_with_provider(&workspace, ModelProvider::Codex, "codex-session");
    let gemini = session_with_provider(&workspace, ModelProvider::Gemini, "gemini-session");

    assert_eq!(session_provider(&codex), "codex");
    assert_eq!(session_provider(&gemini), "gemini");

    let _ = fs::remove_dir_all(workspace);
}
