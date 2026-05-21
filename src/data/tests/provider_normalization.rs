use super::*;

#[test]
fn normalize_loaded_codex_session_converts_cumulative_turns_to_deltas() {
    let mut session = ChatSession {
        id: "session-1".to_string(),
        name: "Codex".to_string(),
        system_prompt: None,
        json_summary: Some(JsonSummary {
            model: Some("gpt-5.4".to_string()),
            total_input_tokens: 250,
            total_output_tokens: 30,
            total_cost_usd: 1.3,
            cache_read_tokens: 120,
            context_window_tokens: 210,
            turn_history: vec![
                TurnUsage {
                    input_tokens: 100,
                    output_tokens: 10,
                    cache_read_tokens: 40,
                    cost_usd: 0.5,
                    context_window_tokens: 140,
                    tool_calls: vec![ToolCallRecord {
                        name: "command_execution".to_string(),
                        input_preview: "ls".to_string(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                TurnUsage {
                    input_tokens: 150,
                    output_tokens: 20,
                    cache_read_tokens: 60,
                    cost_usd: 0.8,
                    context_window_tokens: 210,
                    tool_calls: vec![ToolCallRecord {
                        name: "command_execution".to_string(),
                        input_preview: "pwd".to_string(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        claude_session_id: None,
        minimax_session_id: None,
        qwen_session_id: None,
        codex_session_id: Some("thread-1".to_string()),
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
        messages: Vec::new(),
    };

    ModelProvider::Codex.normalize_loaded_session(&mut session);

    let summary = session.json_summary.unwrap();
    assert_eq!(summary.context_window_size, 0);
    assert_eq!(summary.context_window_tokens, 0);
    assert_eq!(summary.total_input_tokens, 150);
    assert_eq!(summary.total_output_tokens, 20);
    assert_eq!(summary.cache_read_tokens, 60);
    assert!((summary.total_cost_usd - 0.8).abs() < 1e-10);
    assert_eq!(summary.response_count, 2);

    assert_eq!(summary.turn_history[0].input_tokens, 100);
    assert_eq!(summary.turn_history[0].cache_read_tokens, 40);
    assert!((summary.turn_history[0].cost_usd - 0.5).abs() < 1e-10);
    assert_eq!(summary.turn_history[0].context_window_tokens, 0);
    assert_eq!(summary.turn_history[0].tool_calls[0].input_tokens, 100);
    assert_eq!(summary.turn_history[0].tool_calls[0].cache_read_tokens, 40);

    assert_eq!(summary.turn_history[1].input_tokens, 50);
    assert_eq!(summary.turn_history[1].output_tokens, 10);
    assert_eq!(summary.turn_history[1].cache_read_tokens, 20);
    assert!((summary.turn_history[1].cost_usd - 0.3).abs() < 1e-10);
    assert_eq!(summary.turn_history[1].context_window_tokens, 0);
    assert_eq!(summary.turn_history[1].tool_calls[0].input_tokens, 50);
    assert_eq!(summary.turn_history[1].tool_calls[0].cache_read_tokens, 20);
}

#[test]
fn normalize_loaded_codex_session_recovers_mixed_history() {
    let mut session = ChatSession {
        id: "session-mixed".to_string(),
        name: "Codex".to_string(),
        system_prompt: None,
        json_summary: Some(JsonSummary {
            model: Some("gpt-5.4".to_string()),
            total_input_tokens: 22_038_476,
            total_output_tokens: 89_672,
            total_cost_usd: 29.596784,
            cache_read_tokens: 17_392_256,
            context_window_size: 1_050_000,
            response_count: 11,
            turn_history: vec![
                TurnUsage {
                    input_tokens: 378_162,
                    output_tokens: 5_815,
                    cache_read_tokens: 228_736,
                    cost_usd: 0.9351515,
                    ..Default::default()
                },
                TurnUsage::default(),
                TurnUsage {
                    input_tokens: 8_345_898,
                    output_tokens: 26_708,
                    cache_read_tokens: 6_847_744,
                    cost_usd: 9.803636,
                    ..Default::default()
                },
                TurnUsage::default(),
                TurnUsage {
                    input_tokens: 15_889_406,
                    output_tokens: 49_503,
                    cache_read_tokens: 12_466_560,
                    cost_usd: 21.3446875,
                    ..Default::default()
                },
                TurnUsage::default(),
                TurnUsage {
                    input_tokens: 17_068_161,
                    output_tokens: 58_373,
                    cache_read_tokens: 13_267_456,
                    cost_usd: 23.6337815,
                    ..Default::default()
                },
                TurnUsage {
                    input_tokens: 4_385_216,
                    output_tokens: 26_210,
                    cache_read_tokens: 3_646_464,
                    cost_usd: 5.195101,
                    ..Default::default()
                },
                TurnUsage {
                    input_tokens: 281_124,
                    output_tokens: 1_549,
                    cache_read_tokens: 274_304,
                    cost_usd: 0.1375285,
                    ..Default::default()
                },
                TurnUsage {
                    input_tokens: 198_073,
                    output_tokens: 2_824,
                    cache_read_tokens: 101_248,
                    cost_usd: 0.572977,
                    ..Default::default()
                },
                TurnUsage {
                    input_tokens: 105_902,
                    output_tokens: 716,
                    cache_read_tokens: 102_784,
                    cost_usd: 0.057396,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        claude_session_id: None,
        minimax_session_id: None,
        qwen_session_id: None,
        nvidia_session_id: None,
        codex_session_id: Some("thread-mixed".to_string()),
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
        messages: Vec::new(),
    };

    ModelProvider::Codex.normalize_loaded_session(&mut session);

    let summary = session.json_summary.unwrap();
    assert_eq!(summary.response_count, 11);
    assert_eq!(summary.total_input_tokens, 22_038_476);
    assert_eq!(summary.total_output_tokens, 89_672);
    assert_eq!(summary.cache_read_tokens, 17_392_256);
    assert!((summary.total_cost_usd - 29.596784).abs() < 1e-10);
    assert_eq!(summary.context_window_tokens, 0);
    assert_eq!(summary.turn_history.len(), 8);

    let input_totals: u64 = summary
        .turn_history
        .iter()
        .map(|turn| turn.input_tokens)
        .sum();
    assert_eq!(input_totals, 22_038_476);
    let cache_totals: u64 = summary
        .turn_history
        .iter()
        .map(|turn| turn.cache_read_tokens)
        .sum();
    assert_eq!(cache_totals, 17_392_256);
    let cost_totals: f64 = summary.turn_history.iter().map(|turn| turn.cost_usd).sum();
    assert!((cost_totals - 29.596784).abs() < 1e-10);
}

#[test]
fn normalize_loaded_codex_session_rolls_up_unrecoverable_history() {
    let mut session = ChatSession {
        id: "session-collapse".to_string(),
        name: "Codex".to_string(),
        system_prompt: None,
        json_summary: Some(JsonSummary {
            model: Some("gpt-5.4".to_string()),
            total_input_tokens: 100,
            total_output_tokens: 12,
            total_cost_usd: 1.0,
            cache_read_tokens: 80,
            context_window_size: 1_050_000,
            response_count: 3,
            turn_history: vec![
                TurnUsage {
                    input_tokens: 10,
                    output_tokens: 1,
                    cache_read_tokens: 8,
                    cost_usd: 0.1,
                    context_window_tokens: 1,
                    ..Default::default()
                },
                TurnUsage {
                    input_tokens: 90,
                    output_tokens: 10,
                    cache_read_tokens: 72,
                    cost_usd: 0.9,
                    ..Default::default()
                },
                TurnUsage {
                    input_tokens: 60,
                    output_tokens: 8,
                    cache_read_tokens: 48,
                    cost_usd: 0.6,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        claude_session_id: None,
        minimax_session_id: None,
        qwen_session_id: None,
        codex_session_id: Some("thread-collapse".to_string()),
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
        messages: Vec::new(),
    };

    ModelProvider::Codex.normalize_loaded_session(&mut session);

    let summary = session.json_summary.unwrap();
    assert_eq!(summary.turn_history.len(), 1);
    assert_eq!(summary.turn_history[0].input_tokens, 100);
    assert_eq!(summary.turn_history[0].output_tokens, 12);
    assert_eq!(summary.turn_history[0].cache_read_tokens, 80);
    assert!((summary.turn_history[0].cost_usd - 1.0).abs() < 1e-10);
    assert_eq!(summary.response_count, 3);
    assert_eq!(summary.total_input_tokens, 100);
    assert_eq!(summary.total_output_tokens, 12);
    assert_eq!(summary.cache_read_tokens, 80);
    assert!((summary.total_cost_usd - 1.0).abs() < 1e-10);
}

#[test]
fn normalize_loaded_gemini_session_sets_cost_and_context_metadata() {
    let mut session = ChatSession {
        id: "session-2".to_string(),
        name: "Gemini".to_string(),
        system_prompt: None,
        json_summary: Some(JsonSummary {
            model: Some("gemini-3.1-pro-preview".to_string()),
            total_input_tokens: 100_000,
            total_output_tokens: 50_000,
            cache_read_tokens: 25_000,
            turn_history: vec![TurnUsage {
                input_tokens: 100_000,
                output_tokens: 50_000,
                cache_read_tokens: 25_000,
                ..Default::default()
            }],
            ..Default::default()
        }),
        claude_session_id: None,
        minimax_session_id: None,
        qwen_session_id: None,
        codex_session_id: None,
        nvidia_session_id: None,
        gemini_session_id: Some("gem-session-1".to_string()),
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
        messages: Vec::new(),
    };

    ModelProvider::Gemini.normalize_loaded_session(&mut session);

    let summary = session.json_summary.unwrap();
    assert_eq!(summary.context_window_size, 1_048_576);
    assert_eq!(summary.context_window_tokens, 100_000);
    assert!((summary.total_cost_usd - 0.755).abs() < 1e-10);
}

#[test]
fn load_codex_rollout_metadata_reads_model_and_context_window() {
    let root = std::env::temp_dir().join(format!("rust-chatbot-codex-rollout-{}", Uuid::new_v4()));
    let rollout_dir = root.join("2026").join("04").join("02");
    fs::create_dir_all(&rollout_dir).unwrap();
    let rollout_path = rollout_dir.join("rollout-2026-04-02T23-00-00-thread-123.jsonl");
    fs::write(
        &rollout_path,
        concat!(
            "{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5.4\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"model_context_window\":258400}}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"model_context_window\":258400,\"estimated_token_count\":6931}}}\n"
        ),
    )
    .unwrap();

    let metadata = load_codex_rollout_metadata_from_root(&root, "thread-123").unwrap();
    assert_eq!(
        metadata,
        CodexRolloutMetadata {
            model: Some("gpt-5.4".to_string()),
            context_window_size: 258_400,
            context_window_tokens: 6_931,
        }
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn normalize_loaded_codex_session_prefers_rollout_context_window_size() {
    let root =
        std::env::temp_dir().join(format!("rust-chatbot-codex-normalize-{}", Uuid::new_v4()));
    let rollout_dir = root.join("2026").join("04").join("02");
    fs::create_dir_all(&rollout_dir).unwrap();
    let rollout_path = rollout_dir.join("rollout-2026-04-02T23-00-00-thread-rollout.jsonl");
    fs::write(
        &rollout_path,
        concat!(
            "{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5.4\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"model_context_window\":258400,\"estimated_token_count\":6931}}}\n"
        ),
    )
    .unwrap();

    let mut session = ChatSession {
        id: "session-rollout".to_string(),
        name: "Codex".to_string(),
        system_prompt: None,
        json_summary: Some(JsonSummary {
            model: Some("gpt-5.4".to_string()),
            context_window_size: 1_050_000,
            ..Default::default()
        }),
        claude_session_id: None,
        nvidia_session_id: None,
        minimax_session_id: None,
        qwen_session_id: None,
        codex_session_id: Some("thread-rollout".to_string()),
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
        messages: Vec::new(),
    };

    let rollout_metadata = load_codex_rollout_metadata_from_root(&root, "thread-rollout").unwrap();
    apply_codex_rollout_metadata(session.json_summary.as_mut().unwrap(), &rollout_metadata);

    let summary = session.json_summary.unwrap();
    assert_eq!(summary.context_window_size, 258_400);
    assert_eq!(summary.context_window_tokens, 6_931);
    assert_eq!(summary.model.as_deref(), Some("gpt-5.4"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn apply_codex_rollout_metadata_preserves_existing_current_context_tokens() {
    let mut summary = JsonSummary {
        model: Some("gpt-5.4".to_string()),
        context_window_tokens: 12_345,
        ..Default::default()
    };

    apply_codex_rollout_metadata(
        &mut summary,
        &CodexRolloutMetadata {
            model: Some("gpt-5.4".to_string()),
            context_window_size: 258_400,
            context_window_tokens: 6_931,
        },
    );

    assert_eq!(summary.context_window_size, 258_400);
    assert_eq!(summary.context_window_tokens, 12_345);
}
