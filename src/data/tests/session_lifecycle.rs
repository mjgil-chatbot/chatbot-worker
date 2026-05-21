use super::*;

const PREALLOCATED_PREFIX: &str = "__rust_chatbot_preallocated__:";

#[test]
fn search_session_content_matches_real_chat_history_text() {
    let temp_dir =
        std::env::temp_dir().join(format!("rust-chatbot-search-test-{}", Uuid::new_v4()));
    fs::create_dir_all(temp_dir.join("sessions")).unwrap();

    let manager = DataManager {
        data_dir: temp_dir.clone(),
        provider: ModelProvider::Codex,
    };

    let now = Utc::now();
    let matching = ChatSession {
        id: "chat-1".to_string(),
        name: "Render Investigation".to_string(),
        system_prompt: Some("Help debug the laggy search bar.".to_string()),
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
        messages: vec![
            ChatMessage {
                content: "Search the full chat history for markdown lag.".to_string(),
                is_user: true,
                timestamp: now,
                command: None,
                retrieved_memory: None,
            },
            ChatMessage {
                content: "The command output mentions blocking render work.".to_string(),
                is_user: false,
                timestamp: now,
                command: Some("rg search_filter src/app".to_string()),
                retrieved_memory: None,
            },
        ],
    };
    let non_matching = ChatSession {
        id: "chat-2".to_string(),
        name: "Other".to_string(),
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
        messages: vec![ChatMessage {
            content: "Nothing relevant here.".to_string(),
            is_user: true,
            timestamp: now,
            command: None,
            retrieved_memory: None,
        }],
    };

    fs::write(
        temp_dir.join("sessions").join("chat-1.json"),
        serde_json::to_string(&matching).unwrap(),
    )
    .unwrap();
    fs::write(
        temp_dir.join("sessions").join("chat-2.json"),
        serde_json::to_string(&non_matching).unwrap(),
    )
    .unwrap();

    let matches = manager.search_session_content("markdown lag").unwrap();
    assert!(matches.contains("chat-1"));
    assert!(!matches.contains("chat-2"));

    let matches = manager.search_session_content("rg search_filter").unwrap();
    assert!(matches.contains("chat-1"));

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn ordinary_save_does_not_revert_an_explicit_rename() {
    let temp_dir =
        std::env::temp_dir().join(format!("rust-chatbot-rename-test-{}", Uuid::new_v4()));
    fs::create_dir_all(temp_dir.join("sessions")).unwrap();

    let manager = DataManager {
        data_dir: temp_dir.clone(),
        provider: ModelProvider::Codex,
    };

    let mut session = manager
        .create_session("Chat 202".to_string(), None)
        .unwrap();
    let session_id = session.id.clone();

    manager
        .rename_session(&session_id, "pll-fixes".to_string())
        .unwrap();

    session.add_message("follow-up".to_string(), true);
    manager.save_session(&session).unwrap();

    let saved = manager.load_session(&session_id).unwrap();
    assert_eq!(saved.name, "pll-fixes");
    assert_eq!(saved.messages.len(), 1);

    let sessions = manager.list_sessions().unwrap();
    let saved_info = sessions.iter().find(|info| info.id == session_id).unwrap();
    assert_eq!(saved_info.name, "pll-fixes");

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn height_cache_roundtrip_uses_compact_json_file() {
    let temp_dir =
        std::env::temp_dir().join(format!("rust-chatbot-height-cache-test-{}", Uuid::new_v4()));
    fs::create_dir_all(temp_dir.join("height_cache")).unwrap();

    let manager = DataManager {
        data_dir: temp_dir.clone(),
        provider: ModelProvider::Codex,
    };

    let mut cache = SessionHeightCache::new("session-1".to_string());
    cache.widths.insert(
        800,
        HeightCacheEntry {
            row_heights: vec![100.0, 110.0],
            total_height: 210.0,
            message_count: 2,
            updated_at: Utc::now(),
        },
    );

    manager.save_height_cache(&cache).unwrap();
    let stored = fs::read_to_string(temp_dir.join("height_cache").join("session-1.json")).unwrap();
    assert!(
        !stored.contains("\n  "),
        "height cache should be persisted as compact JSON"
    );

    let loaded = manager
        .load_height_cache("session-1")
        .unwrap()
        .expect("height cache should reload");
    let entry = loaded.widths.get(&800).expect("width bucket");
    assert_eq!(entry.total_height, 210.0);
    assert_eq!(entry.row_heights, vec![100.0, 110.0]);

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn mutate_session_reloads_latest_disk_state_before_saving() {
    let temp_dir =
        std::env::temp_dir().join(format!("rust-chatbot-mutate-session-{}", Uuid::new_v4()));
    fs::create_dir_all(temp_dir.join("sessions")).unwrap();

    let manager = DataManager {
        data_dir: temp_dir.clone(),
        provider: ModelProvider::Codex,
    };

    let session = manager
        .create_session("Atomic Send".to_string(), None)
        .unwrap();
    let session_id = session.id.clone();
    let stale_snapshot = manager.load_session(&session_id).unwrap();

    let mut newer_snapshot = manager.load_session(&session_id).unwrap();
    newer_snapshot.add_message("assistant reply".to_string(), false);
    manager.save_session(&newer_snapshot).unwrap();

    manager
        .mutate_session(&session_id, true, |latest| {
            assert_eq!(
                latest.messages.len(),
                1,
                "must see the latest saved message"
            );
            assert_eq!(latest.messages[0].content, "assistant reply");
            latest.add_message_with_command(
                "continue".to_string(),
                true,
                Some("chatbot-ctl send".to_string()),
            );
        })
        .unwrap();

    let saved = manager.load_session(&session_id).unwrap();
    assert_ne!(
        stale_snapshot.history_fingerprint(),
        saved.history_fingerprint(),
        "stale pre-update state must not be written back over newer messages"
    );
    assert_eq!(saved.messages.len(), 2);
    assert_eq!(saved.messages[0].content, "assistant reply");
    assert_eq!(saved.messages[1].content, "continue");
    assert_eq!(
        saved.messages[1].command.as_deref(),
        Some("chatbot-ctl send")
    );

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn list_sessions_discovers_session_files_missing_from_metadata() {
    let temp_dir =
        std::env::temp_dir().join(format!("rust-chatbot-list-sessions-{}", Uuid::new_v4()));
    fs::create_dir_all(temp_dir.join("sessions")).unwrap();

    let manager = DataManager {
        data_dir: temp_dir.clone(),
        provider: ModelProvider::Codex,
    };

    let session = ChatSession {
        id: "session-from-file".to_string(),
        name: "Recovered Session".to_string(),
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
        messages: vec![ChatMessage {
            content: "hello".to_string(),
            is_user: true,
            timestamp: Utc::now(),
            command: None,
            retrieved_memory: None,
        }],
    };

    let session_path = temp_dir.join("sessions").join("session-from-file.json");
    fs::write(
        &session_path,
        serde_json::to_string_pretty(&session).unwrap(),
    )
    .unwrap();

    let sessions = manager.list_sessions().unwrap();
    let listed = sessions
        .iter()
        .find(|info| info.id == "session-from-file")
        .expect("session file should be discovered even without metadata");
    assert_eq!(listed.name, "Recovered Session");
    assert_eq!(listed.message_count, 1);

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn list_sessions_uses_metadata_index_fast_path() {
    // When metadata already contains session info, list_sessions should
    // return results from the index without needing to read session files.
    let temp_dir = std::env::temp_dir().join(format!("rust-chatbot-index-fast-{}", Uuid::new_v4()));
    fs::create_dir_all(temp_dir.join("sessions")).unwrap();

    let manager = DataManager {
        data_dir: temp_dir.clone(),
        provider: ModelProvider::Codex,
    };

    // Create two sessions via the normal path (populates metadata index).
    let s1 = manager
        .create_session("Session A".to_string(), None)
        .unwrap();
    let s2 = manager
        .create_session("Session B".to_string(), None)
        .unwrap();

    // Now delete the session files from disk — metadata should still work.
    fs::remove_file(temp_dir.join("sessions").join(format!("{}.json", s1.id))).unwrap();
    fs::remove_file(temp_dir.join("sessions").join(format!("{}.json", s2.id))).unwrap();

    let sessions = manager.list_sessions().unwrap();
    assert_eq!(
        sessions.len(),
        2,
        "should list sessions from metadata index"
    );
    let names: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Session A"));
    assert!(names.contains(&"Session B"));

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn list_sessions_discovers_orphans_alongside_indexed_sessions() {
    // When metadata has some sessions and disk has additional orphaned files,
    // list_sessions should return both.
    let temp_dir = std::env::temp_dir().join(format!("rust-chatbot-orphan-mix-{}", Uuid::new_v4()));
    fs::create_dir_all(temp_dir.join("sessions")).unwrap();

    let manager = DataManager {
        data_dir: temp_dir.clone(),
        provider: ModelProvider::Codex,
    };

    // Create one session normally (indexed in metadata).
    let s1 = manager
        .create_session("Indexed Session".to_string(), None)
        .unwrap();

    // Drop an orphaned session file on disk (not in metadata).
    let orphan = ChatSession {
        id: "orphan-session".to_string(),
        name: "Orphan Session".to_string(),
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
        messages: vec![ChatMessage {
            content: "orphan msg".to_string(),
            is_user: true,
            timestamp: Utc::now(),
            command: None,
            retrieved_memory: None,
        }],
    };
    fs::write(
        temp_dir.join("sessions").join("orphan-session.json"),
        serde_json::to_string_pretty(&orphan).unwrap(),
    )
    .unwrap();

    let sessions = manager.list_sessions().unwrap();
    assert_eq!(sessions.len(), 2, "should include indexed + orphaned");
    assert!(sessions.iter().any(|s| s.id == s1.id));
    assert!(sessions.iter().any(|s| s.id == "orphan-session"));
    let orphan_info = sessions.iter().find(|s| s.id == "orphan-session").unwrap();
    assert_eq!(orphan_info.message_count, 1);

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn list_sessions_excludes_hidden_sessions_from_metadata() {
    let temp_dir =
        std::env::temp_dir().join(format!("rust-chatbot-hidden-meta-{}", Uuid::new_v4()));
    fs::create_dir_all(temp_dir.join("sessions")).unwrap();

    let manager = DataManager {
        data_dir: temp_dir.clone(),
        provider: ModelProvider::Codex,
    };

    let s1 = manager.create_session("Visible".to_string(), None).unwrap();
    let s2 = manager.create_session("To Hide".to_string(), None).unwrap();

    // Soft-delete one session (marks hidden in metadata).
    manager.delete_session(&s2.id).unwrap();

    let sessions = manager.list_sessions().unwrap();
    assert_eq!(sessions.len(), 1, "hidden session should be excluded");
    assert_eq!(sessions[0].id, s1.id);

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn preallocated_sessions_stay_hidden_until_checkout() {
    let temp_dir =
        std::env::temp_dir().join(format!("rust-chatbot-preallocated-{}", Uuid::new_v4()));
    fs::create_dir_all(temp_dir.join("sessions")).unwrap();

    let manager = DataManager {
        data_dir: temp_dir.clone(),
        provider: ModelProvider::Codex,
    };

    manager.ensure_preallocated_sessions(2).unwrap();

    let metadata: SessionMetadata =
        serde_json::from_str(&fs::read_to_string(temp_dir.join("sessions.json")).unwrap()).unwrap();
    let hidden_pool: Vec<&SessionInfo> = metadata
        .sessions
        .values()
        .filter(|info| info.hidden && info.name.starts_with(PREALLOCATED_PREFIX))
        .collect();
    assert_eq!(hidden_pool.len(), 2);
    assert!(manager.list_sessions().unwrap().is_empty());

    let session = manager
        .checkout_preallocated_session(
            "Visible Session".to_string(),
            Some("system prompt".to_string()),
        )
        .unwrap()
        .expect("preallocated session should be available");

    assert_eq!(session.name, "Visible Session");
    assert_eq!(session.system_prompt.as_deref(), Some("system prompt"));

    let reloaded = manager.load_session(&session.id).unwrap();
    assert_eq!(reloaded.name, "Visible Session");
    assert_eq!(reloaded.system_prompt.as_deref(), Some("system prompt"));

    let visible = manager.list_sessions().unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id, session.id);

    let metadata: SessionMetadata =
        serde_json::from_str(&fs::read_to_string(temp_dir.join("sessions.json")).unwrap()).unwrap();
    let activated = metadata
        .sessions
        .get(&session.id)
        .expect("checked-out session metadata");
    assert!(!activated.hidden);
    assert_eq!(activated.name, "Visible Session");
    assert_eq!(
        metadata
            .sessions
            .values()
            .filter(|info| info.hidden && info.name.starts_with(PREALLOCATED_PREFIX))
            .count(),
        1
    );

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn isolated_data_dir_ignores_global_sqlite_session_index() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let home_dir =
        std::env::temp_dir().join(format!("rust-chatbot-sqlite-home-{}", Uuid::new_v4()));
    let data_dir =
        std::env::temp_dir().join(format!("rust-chatbot-sqlite-data-{}", Uuid::new_v4()));
    fs::create_dir_all(home_dir.join(".rust-chatbot")).unwrap();
    fs::create_dir_all(data_dir.join("sessions")).unwrap();

    let old_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &home_dir);

    let manager = DataManager {
        data_dir: data_dir.clone(),
        provider: ModelProvider::Codex,
    };

    let local = ChatSession {
        id: "local-session".to_string(),
        name: "Local".to_string(),
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
        messages: vec![ChatMessage {
            content: "local only".to_string(),
            is_user: true,
            timestamp: Utc::now(),
            command: None,
            retrieved_memory: None,
        }],
    };
    fs::write(
        data_dir.join("sessions").join("local-session.json"),
        serde_json::to_string_pretty(&local).unwrap(),
    )
    .unwrap();

    let index = crate::storage::StorageIndex::open().unwrap();
    let conn = index.connection();
    crate::storage::upsert_session(
        &conn,
        "global-session",
        "codex",
        "Global",
        &Utc::now().to_rfc3339(),
        &Utc::now().to_rfc3339(),
        1,
        Some(1),
        None,
        None,
        false,
    )
    .unwrap();
    crate::storage::upsert_session_fts(&conn, "global-session", "global only").unwrap();
    drop(conn);

    let sessions = manager.list_sessions().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "local-session");

    let matches = manager.search_session_content("local only").unwrap();
    assert!(matches.contains("local-session"));
    assert!(!matches.contains("global-session"));

    match old_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    let _ = fs::remove_dir_all(home_dir);
    let _ = fs::remove_dir_all(data_dir);
}

#[test]
fn create_session_for_omniforge_sets_default_model_id() {
    let temp_dir =
        std::env::temp_dir().join(format!("rust-chatbot-omniforge-create-{}", Uuid::new_v4()));
    fs::create_dir_all(temp_dir.join("sessions")).unwrap();

    let manager = DataManager {
        data_dir: temp_dir.clone(),
        provider: ModelProvider::OmniForge,
    };

    let session = manager
        .create_session("Omni".to_string(), None)
        .expect("create omniforge session");

    assert_eq!(
        session.omniforge_model_id.as_deref(),
        Some(crate::omniforge::DEFAULT_MODEL_ID)
    );

    let _ = fs::remove_dir_all(temp_dir);
}
