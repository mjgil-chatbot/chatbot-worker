use super::*;

#[test]
fn stream_minimax_missing_auth_sends_error_message() {
    use std::sync::atomic::AtomicBool;
    use std::sync::{mpsc, Arc};

    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    if minimax_api_key().is_some() {
        eprintln!("SKIP: MiniMax API key found on disk, cannot test missing-key path");
        return;
    }

    let overrides = ProviderRequestOverrides {
        mcp_json_path: Some(PathBuf::from("/tmp/nonexistent_mcp_mm_test.json")),
        anthropic_auth_token: None,
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    let result = with_cleared_env(
        &[
            config::api_key_env_for(ModelProvider::MiniMax)
                .expect("MiniMax catalog entry must define an api_key_env"),
            TEST_ANTHROPIC_AUTH_TOKEN_ENV,
        ],
        || {
            call_provider_cli_stream_with_overrides(
                ModelProvider::MiniMax,
                "q",
                None,
                None,
                None,
                Some(&overrides),
                tx,
                stop,
            )
        },
    );

    assert!(result.is_ok());
    let messages: Vec<_> = rx.iter().collect();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].is_final);
    assert!(
        messages[0].response.contains(
            config::api_key_env_for(ModelProvider::MiniMax)
                .expect("MiniMax catalog entry must define an api_key_env"),
        ),
        "error should mention the env var, got: {}",
        messages[0].response
    );
}

#[test]
fn stream_qwen_missing_auth_sends_error_message() {
    use std::sync::atomic::AtomicBool;
    use std::sync::{mpsc, Arc};

    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    if provider_api_key(ModelProvider::OpenRouter).is_some() {
        eprintln!(
            "SKIP: OpenRouter API key found in env or key file, cannot test missing-key path"
        );
        return;
    }

    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    let result = with_cleared_env(&["OPENROUTER_API_KEY"], || {
        call_provider_cli_stream_with_overrides(
            ModelProvider::Qwen,
            "q",
            None,
            None,
            None,
            None,
            tx,
            stop,
        )
    });

    assert!(result.is_ok());
    let messages: Vec<_> = rx.iter().collect();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].is_final);
    assert!(
        messages[0].response.contains("OPENROUTER_API_KEY"),
        "error should mention the env var, got: {}",
        messages[0].response
    );
}

#[test]
fn stream_gemini_forge_missing_auth_sends_error_message() {
    use std::sync::atomic::AtomicBool;
    use std::sync::{mpsc, Arc};

    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    if provider_api_key(ModelProvider::GeminiForge).is_some() {
        eprintln!("SKIP: GeminiForge API key found in env or key file");
        return;
    }
    if gemini_subscription_auth_available() {
        eprintln!("SKIP: Gemini subscription auth is configured");
        return;
    }

    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    let result = with_cleared_env(&["GEMINI_API_KEY"], || {
        call_provider_cli_stream_with_overrides(
            ModelProvider::GeminiForge,
            "q",
            None,
            None,
            None,
            None,
            tx,
            stop,
        )
    });

    assert!(result.is_ok());
    let messages: Vec<_> = rx.iter().collect();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].is_final);
    assert!(
        messages[0].response.contains("GEMINI_API_KEY"),
        "error should mention the env var, got: {}",
        messages[0].response
    );
    assert!(
        messages[0].response.contains("GeminiForge"),
        "error should mention the provider label, got: {}",
        messages[0].response
    );
}

#[test]
fn stream_error_includes_command_string() {
    use std::sync::atomic::AtomicBool;
    use std::sync::{mpsc, Arc};

    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    if minimax_api_key().is_some() {
        eprintln!("SKIP: MiniMax API key found on disk");
        return;
    }

    let overrides = ProviderRequestOverrides {
        mcp_json_path: Some(PathBuf::from("/tmp/nonexistent_mcp_cmd_test.json")),
        anthropic_auth_token: None,
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    with_cleared_env(
        &[
            config::api_key_env_for(ModelProvider::MiniMax)
                .expect("MiniMax catalog entry must define an api_key_env"),
            TEST_ANTHROPIC_AUTH_TOKEN_ENV,
        ],
        || {
            let _ = call_provider_cli_stream_with_overrides(
                ModelProvider::MiniMax,
                "test query",
                Some("test system prompt"),
                None,
                None,
                Some(&overrides),
                tx,
                stop,
            );
        },
    );

    let messages: Vec<_> = rx.iter().collect();
    assert_eq!(messages.len(), 1);
    assert!(
        !messages[0].command.is_empty(),
        "error message should include the provider command string"
    );
}

#[test]
fn stream_error_command_uses_mcp_visibility_note_for_minimax_inventory_queries() {
    use std::sync::atomic::AtomicBool;
    use std::sync::{mpsc, Arc};

    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    if minimax_api_key().is_some() {
        eprintln!("SKIP: MiniMax API key found on disk");
        return;
    }

    let previous_home = std::env::var_os("HOME");
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", tmp.path());

    let minimax_dir = tmp.path().join(".minimax");
    std::fs::create_dir_all(&minimax_dir).unwrap();

    let mcp_json = tmp.path().join("mcp.json");
    std::fs::write(
        &mcp_json,
        serde_json::to_vec_pretty(&json!({
            "mcpServers": {
                "mcp-coordinator": {
                    "command": "cargo"
                }
            },
            "providers": {
                "claude": {
                    "mcpServers": {
                        "Context7": {
                            "command": "cargo"
                        }
                    }
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let overrides = ProviderRequestOverrides {
        config_dir: Some(minimax_dir),
        mcp_json_path: Some(mcp_json),
        anthropic_auth_token: None,
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    with_cleared_env(
        &[
            config::api_key_env_for(ModelProvider::MiniMax)
                .expect("MiniMax catalog entry must define an api_key_env"),
            TEST_ANTHROPIC_AUTH_TOKEN_ENV,
        ],
        || {
            let _ = call_provider_cli_stream_with_overrides(
                ModelProvider::MiniMax,
                "what mcp servers do you have available?",
                None,
                None,
                None,
                Some(&overrides),
                tx,
                stop,
            );
        },
    );

    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }

    let messages: Vec<_> = rx.iter().collect();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].is_final);
    assert!(messages[0].command.contains("`mcp-coordinator`"));
    assert!(messages[0].command.contains(
        "Skills, slash commands, tool names, and prompt instructions are not MCP servers."
    ));
    assert!(messages[0]
        .command
        .contains("routed upstream tools are still callable through that server's tool namespace"));
    assert!(messages[0].command.contains(
        "`tools/list`, `prompts/list`, `resources/list`, and `resources/templates/list`"
    ));
    assert!(
        !messages[0].command.contains("`Context7`"),
        "MiniMax error command should not expose routed upstream names: {}",
        messages[0].command
    );
}

#[test]
fn stream_error_session_id_matches_provider() {
    use chrono::Utc;
    use std::sync::atomic::AtomicBool;
    use std::sync::{mpsc, Arc};

    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    if minimax_api_key().is_some() {
        eprintln!("SKIP: MiniMax API key found on disk");
        return;
    }

    let session = ChatSession {
        id: "s-err".to_string(),
        name: "err test".to_string(),
        system_prompt: None,
        json_summary: None,
        claude_session_id: None,
        minimax_session_id: Some("mm-sess-42".to_string()),
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
    };

    let overrides = ProviderRequestOverrides {
        mcp_json_path: Some(PathBuf::from("/tmp/nonexistent_mcp_sid_test.json")),
        anthropic_auth_token: None,
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    with_cleared_env(
        &[
            config::api_key_env_for(ModelProvider::MiniMax)
                .expect("MiniMax catalog entry must define an api_key_env"),
            TEST_ANTHROPIC_AUTH_TOKEN_ENV,
        ],
        || {
            let _ = call_provider_cli_stream_with_overrides(
                ModelProvider::MiniMax,
                "q",
                None,
                None,
                Some(session),
                Some(&overrides),
                tx,
                stop,
            );
        },
    );

    let messages: Vec<_> = rx.iter().collect();
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0].session_id.as_deref(),
        Some("mm-sess-42"),
        "error message should carry the MiniMax session ID"
    );
}
