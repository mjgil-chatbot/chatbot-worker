use super::super::*;
use super::{temp_path, MemTestGuard};
use chrono::{TimeZone, Utc};
use std::fs;
#[test]
fn manual_memories_round_trip_and_forget() {
    let _guard = MemTestGuard::new();
    let workspace = temp_path("workspace-manual-memory");
    fs::create_dir_all(&workspace).unwrap();

    let first = remember_manual_memory(
        &workspace,
        "Always use the repo-local config for this workspace.",
        ManualMemoryPriority::High,
    )
    .unwrap();
    let second = remember_manual_memory(
        &workspace,
        "Keep a note about a lower-priority fallback.",
        ManualMemoryPriority::Normal,
    )
    .unwrap();

    let memories = list_manual_memories(&workspace).unwrap();
    assert_eq!(memories.len(), 2);
    assert_eq!(memories[0].id, first.id);
    assert_eq!(memories[0].priority, ManualMemoryPriority::High);
    assert_eq!(memories[1].id, second.id);

    assert!(forget_manual_memory(&workspace, &first.id).unwrap());
    let memories = list_manual_memories(&workspace).unwrap();
    assert_eq!(memories.len(), 1);
    assert_eq!(memories[0].id, second.id);

    let _ = fs::remove_dir_all(workspace);
}
#[test]
fn remember_manual_memory_rejects_empty_text() {
    let _guard = MemTestGuard::new();
    let workspace = temp_path("manual-empty-text");
    fs::create_dir_all(&workspace).unwrap();

    let result = remember_manual_memory(&workspace, "", ManualMemoryPriority::High);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("empty"));

    let result = remember_manual_memory(&workspace, "   ", ManualMemoryPriority::Normal);
    assert!(result.is_err());

    let _ = fs::remove_dir_all(workspace);
}
#[test]
fn forget_manual_memory_rejects_empty_id() {
    let workspace = temp_path("forget-empty-id");
    fs::create_dir_all(&workspace).unwrap();

    let result = forget_manual_memory(&workspace, "");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("empty"));

    let result = forget_manual_memory(&workspace, "   ");
    assert!(result.is_err());

    let _ = fs::remove_dir_all(workspace);
}
#[test]
fn forget_manual_memory_nonexistent_returns_false() {
    let workspace = temp_path("forget-nonexist");
    fs::create_dir_all(&workspace).unwrap();

    let removed = forget_manual_memory(&workspace, "nonexistent-id").unwrap();
    assert!(!removed);

    let _ = fs::remove_dir_all(workspace);
}
#[test]
fn remember_manual_memory_updates_existing_with_same_text() {
    let _guard = MemTestGuard::new();
    let workspace = temp_path("manual-update");
    fs::create_dir_all(&workspace).unwrap();

    let first = remember_manual_memory(
        &workspace,
        "Same fact text here",
        ManualMemoryPriority::Normal,
    )
    .unwrap();

    let updated = remember_manual_memory(
        &workspace,
        "Same fact text here",
        ManualMemoryPriority::High,
    )
    .unwrap();

    // Same ID, updated priority
    assert_eq!(first.id, updated.id);
    assert_eq!(updated.priority, ManualMemoryPriority::High);
    assert!(updated.updated_at >= first.updated_at);

    // Only one memory should exist
    let memories = list_manual_memories(&workspace).unwrap();
    assert_eq!(memories.len(), 1);

    let _ = fs::remove_dir_all(workspace);
}
#[test]
fn invalidate_memory_doc_key_rejects_empty() {
    let _guard = MemTestGuard::new();
    let workspace = temp_path("invalidate-empty");
    fs::create_dir_all(&workspace).unwrap();

    let result = invalidate_memory_doc_key(&workspace, "", None);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("empty"));

    let result = invalidate_memory_doc_key(&workspace, "   ", None);
    assert!(result.is_err());

    let _ = fs::remove_dir_all(workspace);
}
#[test]
fn invalidate_memory_doc_key_returns_false_on_duplicate() {
    let _guard = MemTestGuard::new();
    let workspace = temp_path("invalidate-dup");
    fs::create_dir_all(&workspace).unwrap();

    let first =
        invalidate_memory_doc_key(&workspace, "chat-1:0:user", Some("reason".to_string())).unwrap();
    assert!(first, "first invalidation should return true");

    let second =
        invalidate_memory_doc_key(&workspace, "chat-1:0:user", Some("new reason".to_string()))
            .unwrap();
    assert!(!second, "duplicate invalidation should return false");

    let _ = fs::remove_dir_all(workspace);
}
#[test]
fn restore_memory_doc_key_rejects_empty() {
    let _guard = MemTestGuard::new();
    let workspace = temp_path("restore-empty");
    fs::create_dir_all(&workspace).unwrap();

    let result = restore_memory_doc_key(&workspace, "");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("empty"));

    let _ = fs::remove_dir_all(workspace);
}
#[test]
fn restore_memory_doc_key_nonexistent_returns_false() {
    let _guard = MemTestGuard::new();
    let workspace = temp_path("restore-nonexist");
    fs::create_dir_all(&workspace).unwrap();

    let result = restore_memory_doc_key(&workspace, "never-invalidated").unwrap();
    assert!(!result);

    let _ = fs::remove_dir_all(workspace);
}
#[test]
fn score_manual_memory_returns_none_for_no_term_match() {
    let _guard = MemTestGuard::new();
    let memory = ManualMemoryFact {
        id: "1".to_string(),
        text: "Completely unrelated content".to_string(),
        priority: ManualMemoryPriority::Normal,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let result = score_manual_memory(
        &memory,
        "xyz query",
        &["xyz".to_string(), "query".to_string()],
    );
    assert!(result.is_none());
}
#[test]
fn score_manual_memory_full_query_match_scores_higher() {
    let _guard = MemTestGuard::new();
    let memory = ManualMemoryFact {
        id: "1".to_string(),
        text: "The API key is stored in env var".to_string(),
        priority: ManualMemoryPriority::Normal,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let full_hit = score_manual_memory(&memory, "api key", &["api".to_string(), "key".to_string()]);
    let partial_hit = score_manual_memory(
        &memory,
        "api stored",
        &["api".to_string(), "stored".to_string()],
    );
    assert!(full_hit.is_some());
    assert!(partial_hit.is_some());
    // Full query match gets +100 bonus
    assert!(full_hit.unwrap().score.unwrap() > partial_hit.unwrap().score.unwrap());
}
#[test]
fn score_manual_memory_high_priority_boost() {
    let _guard = MemTestGuard::new();
    let high = ManualMemoryFact {
        id: "1".to_string(),
        text: "API key location".to_string(),
        priority: ManualMemoryPriority::High,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let normal = ManualMemoryFact {
        id: "2".to_string(),
        text: "API key location".to_string(),
        priority: ManualMemoryPriority::Normal,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let terms = vec!["api".to_string(), "key".to_string()];
    let high_hit = score_manual_memory(&high, "api key", &terms).unwrap();
    let normal_hit = score_manual_memory(&normal, "api key", &terms).unwrap();
    assert!(high_hit.score.unwrap() > normal_hit.score.unwrap());
}
#[test]
fn manual_memory_hits_filters_short_query_terms() {
    let _guard = MemTestGuard::new();
    let workspace = temp_path("short-query-terms");
    fs::create_dir_all(&workspace).unwrap();

    remember_manual_memory(
        &workspace,
        "This is a fact about the xyz service.",
        ManualMemoryPriority::High,
    )
    .unwrap();

    let paths = workspace_memory_paths(&workspace).unwrap();

    // Query "a" — single char term is filtered by len>=2, but "a" as full query
    // matches via full_query_match (the text contains "a"). The full query match
    // still applies even when all individual terms are too short.
    let hits = manual_memory_hits(&paths.manual_memories_path, "a").unwrap();
    assert!(
        !hits.is_empty(),
        "full query 'a' substring match should still score"
    );

    // Query with a single-char that does NOT appear as substring
    let hits = manual_memory_hits(&paths.manual_memories_path, "q").unwrap();
    assert!(hits.is_empty(), "no substring match means no hits");

    // Query with an empty-after-sanitize string
    let hits = manual_memory_hits(&paths.manual_memories_path, "  ").unwrap();
    assert!(hits.is_empty());

    let _ = fs::remove_dir_all(workspace);
}
#[test]
fn score_manual_memory_partial_term_match_no_full_query() {
    let _guard = MemTestGuard::new();
    let memory = ManualMemoryFact {
        id: "1".to_string(),
        text: "The deployment key is stored securely".to_string(),
        priority: ManualMemoryPriority::Normal,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    // "key" matches but full query "key rotation" does not
    let hit = score_manual_memory(
        &memory,
        "key rotation",
        &["key".to_string(), "rotation".to_string()],
    );
    assert!(hit.is_some());
    let hit = hit.unwrap();
    // Without full query match, score should be lower (no +100 bonus)
    assert!(hit.score.unwrap() < 100.0);
}
#[test]
fn manual_memory_priority_serde_round_trip() {
    for (priority, expected_json) in [
        (ManualMemoryPriority::High, "\"high\""),
        (ManualMemoryPriority::Normal, "\"normal\""),
    ] {
        let json = serde_json::to_string(&priority).unwrap();
        assert_eq!(json, expected_json);
        let decoded: ManualMemoryPriority = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, priority);
    }
}
#[test]
fn manual_memory_priority_default_is_high() {
    let priority: ManualMemoryPriority = Default::default();
    assert_eq!(priority, ManualMemoryPriority::High);
}
#[test]
fn manual_memories_state_serialization_round_trip() {
    let mut state = ManualMemoriesState::default();
    state.memories.insert(
        "id-1".to_string(),
        ManualMemoryFact {
            id: "id-1".to_string(),
            text: "Remember this".to_string(),
            priority: ManualMemoryPriority::High,
            created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
        },
    );
    let json = serde_json::to_string_pretty(&state).unwrap();
    let parsed: ManualMemoriesState = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.version, 1);
    assert_eq!(parsed.memories.len(), 1);
    assert_eq!(parsed.memories.get("id-1").unwrap().text, "Remember this");
}
#[test]
fn manual_memory_priority_labels() {
    assert_eq!(ManualMemoryPriority::High.label(), "high");
    assert_eq!(ManualMemoryPriority::Normal.label(), "normal");
}
#[test]
fn manual_memory_priority_boost_values() {
    assert!(ManualMemoryPriority::High.boost() > 0.0);
    assert_eq!(ManualMemoryPriority::Normal.boost(), 0.0);
}
#[test]
fn manual_memory_priority_ordering() {
    // Normal < High for sorting
    assert!(ManualMemoryPriority::Normal < ManualMemoryPriority::High);
}
#[test]
fn manual_memories_state_default_version_is_one() {
    let state = ManualMemoriesState::default();
    assert_eq!(state.version, 1);
    assert!(state.memories.is_empty());
}
#[test]
fn load_manual_memories_missing_file_returns_default() {
    let path = temp_path("missing-manual-mem").join("manual-memories.json");
    let state = load_manual_memories(&path).unwrap();
    assert_eq!(state, ManualMemoriesState::default());
}
#[test]
fn load_manual_memories_corrupt_json_errors() {
    let dir = temp_path("corrupt-manual");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("manual-memories.json");
    fs::write(&path, "[broken").unwrap();

    let result = load_manual_memories(&path);
    assert!(result.is_err());

    let _ = fs::remove_dir_all(dir);
}
#[test]
fn save_manual_memories_populated_round_trip() {
    let dir = temp_path("manual-pop-rt");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("manual-memories.json");

    let mut state = ManualMemoriesState::default();
    state.memories.insert(
        "a".to_string(),
        ManualMemoryFact {
            id: "a".to_string(),
            text: "Fact A".to_string(),
            priority: ManualMemoryPriority::Normal,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        },
    );
    state.memories.insert(
        "b".to_string(),
        ManualMemoryFact {
            id: "b".to_string(),
            text: "Fact B".to_string(),
            priority: ManualMemoryPriority::High,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        },
    );

    save_manual_memories(&path, &state).unwrap();
    let loaded = load_manual_memories(&path).unwrap();
    assert_eq!(loaded.memories.len(), 2);
    assert_eq!(loaded.memories.get("a").unwrap().text, "Fact A");
    assert_eq!(
        loaded.memories.get("b").unwrap().priority,
        ManualMemoryPriority::High
    );

    let _ = fs::remove_dir_all(dir);
}
#[test]
fn invalidate_memory_doc_key_normalizes_reason() {
    let _guard = MemTestGuard::new();
    let workspace = temp_path("invalidate-reason-ws");
    fs::create_dir_all(&workspace).unwrap();

    invalidate_memory_doc_key(
        &workspace,
        "chat-1:0:user",
        Some("  extra   whitespace  ".to_string()),
    )
    .unwrap();

    let paths = workspace_memory_paths(&workspace).unwrap();
    let state = load_ingest_state(&paths.ingest_state_path).unwrap();
    assert_eq!(
        state
            .invalidated_doc_keys
            .get("chat-1:0:user")
            .unwrap()
            .reason
            .as_deref(),
        Some("extra whitespace")
    );

    let _ = fs::remove_dir_all(workspace);
}
#[test]
fn invalidate_memory_doc_key_empty_reason_becomes_none() {
    let _guard = MemTestGuard::new();
    let workspace = temp_path("invalidate-empty-reason");
    fs::create_dir_all(&workspace).unwrap();

    invalidate_memory_doc_key(&workspace, "chat-1:0:user", Some("   ".to_string())).unwrap();

    let paths = workspace_memory_paths(&workspace).unwrap();
    let state = load_ingest_state(&paths.ingest_state_path).unwrap();
    assert!(state
        .invalidated_doc_keys
        .get("chat-1:0:user")
        .unwrap()
        .reason
        .is_none());

    let _ = fs::remove_dir_all(workspace);
}
#[test]
fn manual_memories_state_preserves_version_on_round_trip() {
    let json = r#"{"version":1,"memories":{}}"#;
    let state: ManualMemoriesState = serde_json::from_str(json).unwrap();
    assert_eq!(state.version, 1);
    assert!(state.memories.is_empty());
}
#[test]
fn memory_invalidation_reason_skipped_when_none() {
    let _guard = MemTestGuard::new();
    let inv = MemoryInvalidation {
        invalidated_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        reason: None,
    };
    let json = serde_json::to_string(&inv).unwrap();
    assert!(!json.contains("reason"));
}
#[test]
fn list_manual_memories_sorted_by_priority_then_date() {
    let _guard = MemTestGuard::new();
    let workspace = temp_path("list-sorted");
    fs::create_dir_all(&workspace).unwrap();

    let _low = remember_manual_memory(
        &workspace,
        "Normal priority fact",
        ManualMemoryPriority::Normal,
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));
    let _high =
        remember_manual_memory(&workspace, "High priority fact", ManualMemoryPriority::High)
            .unwrap();

    let memories = list_manual_memories(&workspace).unwrap();
    assert_eq!(memories.len(), 2);
    assert_eq!(memories[0].priority, ManualMemoryPriority::High);
    assert_eq!(memories[1].priority, ManualMemoryPriority::Normal);

    let _ = fs::remove_dir_all(workspace);
}
#[test]
fn load_manual_memories_corrupted_json_errors() {
    let dir = temp_path("corrupted-manual");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(MANUAL_MEMORIES_FILE_NAME);
    fs::write(&path, "{{broken json").unwrap();

    let result = load_manual_memories(&path);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("parse"));

    let _ = fs::remove_dir_all(dir);
}
#[test]
fn save_manual_memories_creates_parent_dirs() {
    let dir = temp_path("save-manual-mkdir");
    let nested = dir.join("x").join("y").join(MANUAL_MEMORIES_FILE_NAME);

    let state = ManualMemoriesState::default();
    save_manual_memories(&nested, &state).unwrap();

    let loaded = load_manual_memories(&nested).unwrap();
    assert_eq!(loaded.version, 1);

    let _ = fs::remove_dir_all(dir);
}
#[test]
fn score_manual_memory_single_char_full_query_match() {
    let _guard = MemTestGuard::new();
    let memory = ManualMemoryFact {
        id: "1".to_string(),
        text: "Use the X flag for builds".to_string(),
        priority: ManualMemoryPriority::Normal,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    // "x" as query: no terms pass len>=2, but full query matches via contains
    let result = score_manual_memory(&memory, "x", &[]);
    assert!(result.is_some());
    assert!(result.unwrap().score.unwrap() >= 100.0);
}
#[test]
fn score_manual_memory_completely_unrelated_returns_none() {
    let _guard = MemTestGuard::new();
    let memory = ManualMemoryFact {
        id: "1".to_string(),
        text: "Deploy to staging every Tuesday".to_string(),
        priority: ManualMemoryPriority::High,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    // "xyz" doesn't match any term or full query
    let result = score_manual_memory(&memory, "xyz", &["xyz".to_string()]);
    assert!(result.is_none());
}
#[test]
fn manual_memory_fact_serialization_round_trip() {
    let fact = ManualMemoryFact {
        id: "test-id".to_string(),
        text: "Test fact".to_string(),
        priority: ManualMemoryPriority::High,
        created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        updated_at: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
    };
    let json = serde_json::to_string(&fact).unwrap();
    let parsed: ManualMemoryFact = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "test-id");
    assert_eq!(parsed.text, "Test fact");
    assert_eq!(parsed.priority, ManualMemoryPriority::High);
}
