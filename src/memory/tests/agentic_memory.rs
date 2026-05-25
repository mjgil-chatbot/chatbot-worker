use super::super::*;
use super::{make_session, temp_path, MemTestGuard, MockMemMachineServer};
use agentic_memory::InMemoryAgentStore;
use std::collections::BTreeMap;
use std::fs;

#[test]
fn index_workspace_sessions_rebuilds_agent_memory_store() {
    let _guard = MemTestGuard::new();
    let _server = MockMemMachineServer::start();
    let workspace = temp_path("workspace-agent-memory-store");
    fs::create_dir_all(&workspace).unwrap();

    let session = make_session(
        &workspace,
        "chat-1",
        "Retry Session",
        vec![
            ("Need the retry backoff guidance.", true, None),
            (
                "[May 19 01:28am] GPT-5.4 | Bash `rg retry`\nUse exponential backoff and cap retries at 3.",
                false,
                None,
            ),
        ],
    );

    index_workspace_sessions(&workspace, std::slice::from_ref(&session), &BTreeMap::new()).unwrap();

    let paths = workspace_memory_paths(&workspace).unwrap();
    assert!(paths.agent_memory_store_path.is_file());

    let store = InMemoryAgentStore::load_json(&paths.agent_memory_store_path).unwrap();
    assert_eq!(store.episodes.len(), 1);
    assert_eq!(
        store.episodes[0].initial_goal,
        "Need the retry backoff guidance."
    );
    assert!(store.episodes[0].final_outcome.contains("resolved"));
    assert!(store.episodes[0]
        .header
        .tags
        .iter()
        .any(|tag| tag == "doc_key:chat-1:1:assistant"));

    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn retrieve_memory_context_surfaces_agent_memory_packet_hits() {
    let _guard = MemTestGuard::new();
    let _server = MockMemMachineServer::start();
    let workspace = temp_path("workspace-agent-memory-retrieval");
    fs::create_dir_all(&workspace).unwrap();

    let session = make_session(
        &workspace,
        "chat-1",
        "Retry Session",
        vec![
            (
                "Need to update the retry backoff in the worker.",
                true,
                None,
            ),
            (
                "Use exponential backoff and cap retries at 3 before surfacing the failure.",
                false,
                None,
            ),
        ],
    );

    index_workspace_sessions(&workspace, std::slice::from_ref(&session), &BTreeMap::new()).unwrap();

    let context = retrieve_memory_context(&workspace, "retry backoff cap")
        .unwrap()
        .unwrap();
    assert!(!context.hits.is_empty());
    assert_eq!(context.hits[0].doc_key, "chat-1:1:assistant");
    assert_eq!(
        context.hits[0].title.as_deref(),
        Some("Agent Memory (episode)")
    );
    assert!(context.hits[0].snippet.contains("Past episode"));

    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn retrieve_memory_context_skips_invalidated_agent_memory_hits() {
    let _guard = MemTestGuard::new();
    let _server = MockMemMachineServer::start();
    let workspace = temp_path("workspace-agent-memory-invalidated");
    fs::create_dir_all(&workspace).unwrap();

    let session = make_session(
        &workspace,
        "chat-1",
        "Retry Session",
        vec![
            (
                "Need to update the retry backoff in the worker.",
                true,
                None,
            ),
            (
                "Use exponential backoff and cap retries at 3 before surfacing the failure.",
                false,
                None,
            ),
        ],
    );

    index_workspace_sessions(&workspace, std::slice::from_ref(&session), &BTreeMap::new()).unwrap();
    invalidate_memory_doc_key(
        &workspace,
        "chat-1:1:assistant",
        Some("stale assistant memory".to_string()),
    )
    .unwrap();

    let context = retrieve_memory_context(&workspace, "retry backoff cap").unwrap();
    assert!(context.as_ref().is_none_or(|context| {
        context
            .hits
            .iter()
            .all(|hit| hit.doc_key != "chat-1:1:assistant")
    }));

    let _ = fs::remove_dir_all(workspace);
}
