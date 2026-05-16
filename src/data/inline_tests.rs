use super::*;
use chrono::Utc;

fn make_session() -> ChatSession {
    ChatSession {
        id: "test-1".to_string(),
        name: "Test".to_string(),
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
        created_at: Utc::now(),
        updated_at: Utc::now(),
        messages: vec![],
    }
}

// --- height_cache_width_key ---

#[test]
fn height_cache_width_key_rounds_correctly() {
    assert_eq!(height_cache_width_key(100.4), 100);
    assert_eq!(height_cache_width_key(100.5), 101);
    assert_eq!(height_cache_width_key(100.6), 101);
}

#[test]
fn height_cache_width_key_clamps_negative_to_zero() {
    assert_eq!(height_cache_width_key(-5.0), 0);
    assert_eq!(height_cache_width_key(-0.1), 0);
}

#[test]
fn session_height_cache_new_has_empty_widths() {
    let cache = SessionHeightCache::new("s1".into());
    assert_eq!(cache.session_id, "s1");
    assert_eq!(cache.version, SESSION_HEIGHT_CACHE_VERSION);
    assert!(cache.widths.is_empty());
}

// --- session_log_snapshot ---

#[test]
fn session_log_snapshot_empty_session() {
    let session = make_session();
    let snap = session_log_snapshot(&session);
    assert_eq!(snap.total_messages, 0);
    assert_eq!(snap.user_messages, 0);
    assert_eq!(snap.assistant_messages, 0);
    assert!(snap.first_message_at.is_none());
    assert!(snap.last_message_at.is_none());
}

#[test]
fn session_log_snapshot_counts_messages() {
    let mut session = make_session();
    session.add_message("hello".into(), true);
    session.add_message("hi".into(), false);
    session.add_message("bye".into(), true);
    let snap = session_log_snapshot(&session);
    assert_eq!(snap.total_messages, 3);
    assert_eq!(snap.user_messages, 2);
    assert_eq!(snap.assistant_messages, 1);
    assert!(snap.first_message_at.is_some());
    assert!(snap.last_message_at.is_some());
}

// --- update_or_add_assistant_message ---

#[test]
fn update_or_add_replaces_last_assistant_message() {
    let mut session = make_session();
    session.add_message("user msg".into(), true);
    session.add_message("first draft".into(), false);
    session.update_or_add_assistant_message("revised".into());
    assert_eq!(session.messages.len(), 2);
    assert_eq!(session.messages[1].content, "revised");
}

#[test]
fn update_or_add_appends_when_last_is_user() {
    let mut session = make_session();
    session.add_message("user msg".into(), true);
    session.update_or_add_assistant_message("reply".into());
    assert_eq!(session.messages.len(), 2);
    assert_eq!(session.messages[1].content, "reply");
    assert!(!session.messages[1].is_user);
}

#[test]
fn update_or_add_appends_to_empty_session() {
    let mut session = make_session();
    session.update_or_add_assistant_message("first".into());
    assert_eq!(session.messages.len(), 1);
    assert_eq!(session.messages[0].content, "first");
}

// --- add_message_with_command ---

#[test]
fn add_message_with_command_stores_command() {
    let mut session = make_session();
    session.add_message_with_command("output".into(), false, Some("ls -la".into()));
    assert_eq!(session.messages[0].command, Some("ls -la".to_string()));
    assert!(session.messages[0].retrieved_memory.is_none());
}

#[test]
fn update_user_message_request_metadata_stores_command_and_memory() {
    let mut session = make_session();
    session.add_message("question".into(), true);
    let timestamp = session.messages[0].timestamp.timestamp_millis();

    session
        .update_user_message_request_metadata(
            timestamp,
            Some("codex exec".into()),
            Some("- [session=chat role=assistant time=2026-05-19T12:00:00Z] prior fix".into()),
        )
        .expect("request metadata should update");

    assert_eq!(session.messages[0].command.as_deref(), Some("codex exec"));
    assert_eq!(
        session.messages[0].retrieved_memory.as_deref(),
        Some("- [session=chat role=assistant time=2026-05-19T12:00:00Z] prior fix")
    );
}

// --- history_fingerprint ---

#[test]
fn history_fingerprint_deterministic() {
    let session = make_session();
    let fp1 = session.history_fingerprint();
    let fp2 = session.history_fingerprint();
    assert_eq!(fp1, fp2, "fingerprint should be deterministic");
}

#[test]
fn history_fingerprint_changes_with_message() {
    let session1 = make_session();
    let mut session2 = make_session();
    // Use the same timestamps
    session2.id = session1.id.clone();
    session2.created_at = session1.created_at;
    session2.updated_at = session1.updated_at;
    let fp_empty = session1.history_fingerprint();

    session2.messages.push(ChatMessage {
        content: "hello".into(),
        is_user: true,
        timestamp: Utc::now(),
        command: None,
        retrieved_memory: None,
    });
    let fp_with_msg = session2.history_fingerprint();
    assert_ne!(
        fp_empty, fp_with_msg,
        "adding a message should change fingerprint"
    );
}

#[test]
fn history_fingerprint_includes_system_prompt() {
    let mut s1 = make_session();
    let mut s2 = make_session();
    s2.id = s1.id.clone();
    s2.created_at = s1.created_at;
    s2.updated_at = s1.updated_at;
    let fp_no_prompt = s1.history_fingerprint();
    s1.system_prompt = Some("system".into());
    s2.updated_at = s1.updated_at; // keep in sync
    let fp_with_prompt = s1.history_fingerprint();
    assert_ne!(fp_no_prompt, fp_with_prompt);
}

// --- fnv_hash_bytes ---

#[test]
fn fnv_hash_bytes_empty_returns_basis() {
    assert_eq!(fnv_hash_bytes(FNV_OFFSET_BASIS, &[]), FNV_OFFSET_BASIS);
}

#[test]
fn fnv_hash_bytes_different_input_different_hash() {
    let h1 = fnv_hash_bytes(FNV_OFFSET_BASIS, b"abc");
    let h2 = fnv_hash_bytes(FNV_OFFSET_BASIS, b"xyz");
    assert_ne!(h1, h2);
}

// --- MetadataLock drop ---

#[test]
fn metadata_lock_drop_does_not_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("lock");
    let file = std::fs::File::create(&path).unwrap();
    fs2::FileExt::lock_exclusive(&file).unwrap();
    let lock = MetadataLock(file);
    drop(lock); // Should release lock without panic
}

#[test]
fn set_system_prompt_updates_timestamp() {
    let mut session = make_session();
    let ts_before = session.updated_at;
    std::thread::sleep(std::time::Duration::from_millis(5));
    session.set_system_prompt(Some("new prompt".into()));
    assert_eq!(session.system_prompt.as_deref(), Some("new prompt"));
    assert!(session.updated_at >= ts_before);
}

// --- add_message direct tests ---

#[test]
fn add_message_appends_and_updates_timestamp() {
    let mut session = make_session();
    let ts_before = session.updated_at;
    std::thread::sleep(std::time::Duration::from_millis(5));
    session.add_message("hello".into(), true);
    assert_eq!(session.messages.len(), 1);
    assert_eq!(session.messages[0].content, "hello");
    assert!(session.messages[0].is_user);
    assert!(session.messages[0].command.is_none());
    assert!(session.updated_at >= ts_before);
}

#[test]
fn add_message_preserves_ordering() {
    let mut session = make_session();
    session.add_message("first".into(), true);
    session.add_message("second".into(), false);
    session.add_message("third".into(), true);
    assert_eq!(session.messages.len(), 3);
    assert!(session.messages[0].timestamp <= session.messages[1].timestamp);
    assert!(session.messages[1].timestamp <= session.messages[2].timestamp);
    assert_eq!(session.messages[2].content, "third");
}

// --- session_log_snapshot single message ---

#[test]
fn session_log_snapshot_single_assistant_message() {
    let mut session = make_session();
    session.add_message("reply".into(), false);
    let snap = session_log_snapshot(&session);
    assert_eq!(snap.total_messages, 1);
    assert_eq!(snap.user_messages, 0);
    assert_eq!(snap.assistant_messages, 1);
    assert!(snap.first_message_at.is_some());
    assert_eq!(snap.first_message_at, snap.last_message_at);
}

// --- height_cache_width_key boundary ---

#[test]
fn height_cache_width_key_zero_and_large() {
    assert_eq!(height_cache_width_key(0.0), 0);
    assert_eq!(height_cache_width_key(f32::MAX), u32::MAX);
    // Exactly at rounding boundary
    assert_eq!(height_cache_width_key(0.5), 1);
    assert_eq!(height_cache_width_key(0.49), 0);
}

#[test]
fn resolve_height_cache_entry_prefers_exact_width() {
    let mut cache = SessionHeightCache::new("s1".into());
    cache.widths.insert(
        800,
        HeightCacheEntry {
            row_heights: vec![100.0],
            total_height: 100.0,
            message_count: 1,
            updated_at: Utc::now(),
        },
    );
    cache.widths.insert(
        812,
        HeightCacheEntry {
            row_heights: vec![120.0],
            total_height: 120.0,
            message_count: 1,
            updated_at: Utc::now(),
        },
    );

    let entry = resolve_height_cache_entry(&cache, 812).expect("exact cache entry");
    assert_eq!(entry.total_height, 120.0);
}

#[test]
fn resolve_height_cache_entry_uses_nearest_width_within_tolerance() {
    let mut cache = SessionHeightCache::new("s1".into());
    cache.widths.insert(
        800,
        HeightCacheEntry {
            row_heights: vec![100.0],
            total_height: 100.0,
            message_count: 1,
            updated_at: Utc::now(),
        },
    );

    let entry = resolve_height_cache_entry(&cache, 812).expect("nearest cache entry");
    assert_eq!(entry.total_height, 100.0);
}

#[test]
fn resolve_height_cache_entry_rejects_far_widths() {
    let mut cache = SessionHeightCache::new("s1".into());
    cache.widths.insert(
        800,
        HeightCacheEntry {
            row_heights: vec![100.0],
            total_height: 100.0,
            message_count: 1,
            updated_at: Utc::now(),
        },
    );

    assert!(resolve_height_cache_entry(&cache, 900).is_none());
}

#[test]
fn resolve_height_cache_entry_rejects_legacy_cache_versions() {
    let mut cache = SessionHeightCache::new("s1".into());
    cache.version = SESSION_HEIGHT_CACHE_VERSION - 1;
    cache.widths.insert(
        800,
        HeightCacheEntry {
            row_heights: vec![100.0],
            total_height: 100.0,
            message_count: 1,
            updated_at: Utc::now(),
        },
    );

    assert!(resolve_height_cache_entry(&cache, 800).is_none());
}
