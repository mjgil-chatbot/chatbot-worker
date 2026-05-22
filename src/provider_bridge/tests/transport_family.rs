use super::*;

#[test]
fn transport_family_dispatch_covers_all_providers() {
    for provider in ModelProvider::ALL {
        let config = provider_config_with_overrides(provider, None, None);
        let variant_name = format!("{:?}", config);
        assert!(
            !variant_name.is_empty(),
            "dispatch must produce a config for {:?}",
            provider,
        );
    }
}

#[test]
fn autonomous_overrides_use_catalog_background_model() {
    for provider in ModelProvider::ALL {
        let entry = crate::provider_catalog::by_id(provider);
        let overrides = autonomous_provider_overrides(provider);
        if entry.background_model.is_empty() {
            assert!(
                overrides.model.is_none(),
                "empty catalog background_model should map to None for {:?}",
                provider,
            );
        } else {
            assert_eq!(
                overrides.model.as_deref(),
                Some(entry.background_model),
                "catalog background_model mismatch for {:?}",
                provider,
            );
        }
    }
}

#[test]
fn claude_compat_config_uses_catalog_base_url() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let config = claude_compat_config(ModelProvider::MiniMax, None);
    let entry = crate::provider_catalog::by_id(ModelProvider::MiniMax);
    if let Some(expected_url) = entry.base_url {
        assert!(
            config
                .extra_env
                .iter()
                .any(|(k, v)| k == "ANTHROPIC_BASE_URL" && v == expected_url),
            "ClaudeCompat should set base_url from catalog for MiniMax",
        );
    }
}

#[test]
fn claude_compat_config_sets_anthropic_reasoning_effort_env() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let claude_config = claude_compat_config(ModelProvider::Claude, None);
    let minimax_config = claude_compat_config(ModelProvider::MiniMax, None);

    assert!(claude_config
        .extra_env
        .iter()
        .any(|(k, v)| k == "ANTHROPIC_REASONING_EFFORT" && v == "high"));
    assert!(minimax_config
        .extra_env
        .iter()
        .any(|(k, v)| k == "ANTHROPIC_REASONING_EFFORT" && v == "xhigh"));
}

#[test]
fn codex_compat_config_uses_system_codex_command_and_catalog_model() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original_codex_cli = std::env::var_os(crate::config::RUST_CHATBOT_CODEX_CLI_ENV);
    std::env::remove_var(crate::config::RUST_CHATBOT_CODEX_CLI_ENV);

    let config = codex_compat_config(ModelProvider::Codex, None);
    let entry = crate::provider_catalog::by_id(ModelProvider::Codex);
    assert_eq!(config.cli, "codex");
    assert_eq!(
        config.model.as_deref(),
        Some(entry.interactive_model),
        "CodexCompat should use catalog interactive_model",
    );

    match original_codex_cli {
        Some(value) => std::env::set_var(crate::config::RUST_CHATBOT_CODEX_CLI_ENV, value),
        None => std::env::remove_var(crate::config::RUST_CHATBOT_CODEX_CLI_ENV),
    }
}

#[test]
fn provider_default_reasoning_effort_matches_catalog_expectations() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let codex_config = codex_compat_config(ModelProvider::Codex, None);
    let minimax_config = claude_compat_config(ModelProvider::MiniMax, None);
    assert_eq!(codex_config.reasoning_effort.as_deref(), Some("high"));
    assert!(minimax_config
        .extra_env
        .iter()
        .any(|(k, v)| k == "ANTHROPIC_REASONING_EFFORT" && v == "xhigh"));
}

#[test]
fn forge_providers_use_openrouter_cli_transport_with_prefixed_model_targets() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original_gemini_api_key = std::env::var_os("GEMINI_API_KEY");
    let original_minimax_api_key = std::env::var_os("MINIMAX_API_KEY");
    std::env::set_var("GEMINI_API_KEY", "gemini-forge-test-key");
    std::env::set_var("MINIMAX_API_KEY", "minimax-forge-test-key");
    let gemini = provider_config_with_overrides(ModelProvider::GeminiForge, None, None);
    let minimax = provider_config_with_overrides(ModelProvider::MiniMaxForge, None, None);

    match gemini {
        ProviderConfig::OpenRouterCli(config) => {
            assert!(config
                .cli
                .ends_with("others/forgecode2/target/release/rust-chatbot"));
            assert_eq!(config.model, "gemini_forge:gemini-3-flash-preview");
            assert!(config
                .extra_env
                .iter()
                .any(|(k, v)| k == "GEMINI_API_KEY" && v == "gemini-forge-test-key"));
        }
        other => panic!("expected OpenRouterCli config, got {:?}", other),
    }

    match minimax {
        ProviderConfig::OpenRouterCli(config) => {
            assert!(config
                .cli
                .ends_with("others/forgecode2/target/release/rust-chatbot"));
            assert_eq!(config.model, "minimax_forge:MiniMax-M2.7");
            assert!(config
                .extra_env
                .iter()
                .any(|(k, v)| k == "MINIMAX_API_KEY" && v == "minimax-forge-test-key"));
        }
        other => panic!("expected OpenRouterCli config, got {:?}", other),
    }

    match original_gemini_api_key {
        Some(value) => std::env::set_var("GEMINI_API_KEY", value),
        None => std::env::remove_var("GEMINI_API_KEY"),
    }
    match original_minimax_api_key {
        Some(value) => std::env::set_var("MINIMAX_API_KEY", value),
        None => std::env::remove_var("MINIMAX_API_KEY"),
    }
}

#[test]
fn gemini_cli_forge_uses_gemini_forge_target_for_forgecode2_compat() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous_home = std::env::var_os("HOME");
    let original_gemini_api_key = std::env::var_os("GEMINI_API_KEY");
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", tmp.path());
    std::env::remove_var("GEMINI_API_KEY");

    let gemini_dir = tmp.path().join(".rust-chatbot").join(".gemini");
    std::fs::create_dir_all(&gemini_dir).unwrap();
    std::fs::write(gemini_dir.join("oauth_creds.json"), "{}\n").unwrap();

    let config = provider_config_with_overrides(ModelProvider::GeminiCliForge, None, None);
    match config {
        ProviderConfig::OpenRouterCli(config) => {
            assert!(config
                .cli
                .ends_with("others/forgecode2/target/release/rust-chatbot"));
            assert_eq!(config.model, "gemini_forge:gemini-3-flash-preview");
            assert!(config.api_key.is_empty());
            assert!(config
                .extra_env
                .iter()
                .any(|(k, v)| k == "HOME" && v == tmp.path().to_string_lossy().as_ref()));
            assert!(!config.extra_env.iter().any(|(k, _)| k == "GEMINI_API_KEY"));
        }
        other => panic!("expected OpenRouterCli config, got {:?}", other),
    }

    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    match original_gemini_api_key {
        Some(value) => std::env::set_var("GEMINI_API_KEY", value),
        None => std::env::remove_var("GEMINI_API_KEY"),
    }
}

#[test]
fn gemini_cli_forge_normalizes_legacy_prefixed_model_override() {
    let config = provider_config_with_overrides(
        ModelProvider::GeminiCliForge,
        None,
        Some(&ProviderRequestOverrides {
            model: Some("gemini_cli_forge:gemini-3-flash-preview".to_string()),
            ..ProviderRequestOverrides::default()
        }),
    );

    match config {
        ProviderConfig::OpenRouterCli(config) => {
            assert_eq!(config.model, "gemini_forge:gemini-3-flash-preview");
        }
        other => panic!("expected OpenRouterCli config, got {:?}", other),
    }
}

#[test]
fn gemini_forge_subscription_auth_passes_through_ambient_home_without_api_key_env() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous_home = std::env::var_os("HOME");
    let original_gemini_api_key = std::env::var_os("GEMINI_API_KEY");
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", tmp.path());
    std::env::remove_var("GEMINI_API_KEY");

    let gemini_dir = tmp.path().join(".rust-chatbot").join(".gemini");
    std::fs::create_dir_all(&gemini_dir).unwrap();
    std::fs::write(gemini_dir.join("oauth_creds.json"), "{}\n").unwrap();

    let gemini = provider_config_with_overrides(ModelProvider::GeminiForge, None, None);
    match gemini {
        ProviderConfig::OpenRouterCli(config) => {
            assert!(config
                .cli
                .ends_with("others/forgecode2/target/release/rust-chatbot"));
            assert_eq!(config.model, "gemini_forge:gemini-3-flash-preview");
            assert!(config.api_key.is_empty());
            assert!(config
                .extra_env
                .iter()
                .any(|(k, v)| k == "HOME" && v == tmp.path().to_string_lossy().as_ref()));
            assert!(!config.extra_env.iter().any(|(k, _)| k == "GEMINI_API_KEY"));
        }
        other => panic!("expected OpenRouterCli config, got {:?}", other),
    }

    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    match original_gemini_api_key {
        Some(value) => std::env::set_var("GEMINI_API_KEY", value),
        None => std::env::remove_var("GEMINI_API_KEY"),
    }
}

#[test]
fn omniforge_gemini_model_passes_through_ambient_home_without_api_key_env() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous_home = std::env::var_os("HOME");
    let original_gemini_api_key = std::env::var_os("GEMINI_API_KEY");
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", tmp.path());
    std::env::remove_var("GEMINI_API_KEY");

    let gemini_dir = tmp.path().join(".rust-chatbot").join(".gemini");
    std::fs::create_dir_all(&gemini_dir).unwrap();
    std::fs::write(gemini_dir.join("oauth_creds.json"), "{}\n").unwrap();

    let session = ChatSession {
        id: "omniforge-gemini-sub-auth".to_string(),
        name: "OmniForge".to_string(),
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
        omniforge_model_id: Some("gemini-3-flash-preview".to_string()),
        kimi_session_id: None,
        opencode_session_id: None,
        hermes_session_id: None,
        forge_session_id: None,
        claw_session_id: None,
        launched_from: None,
        runsc_dirs: Vec::new(),
        runsc_image: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        messages: Vec::new(),
    };

    let config = provider_config_with_overrides(ModelProvider::OmniForge, Some(&session), None);
    match config {
        ProviderConfig::OpenRouterCli(config) => {
            assert_eq!(config.model, "gemini_forge:gemini-3-flash-preview");
            assert!(config.api_key.is_empty());
            assert!(config
                .extra_env
                .iter()
                .any(|(k, v)| k == "HOME" && v == tmp.path().to_string_lossy().as_ref()));
            assert!(!config.extra_env.iter().any(|(k, _)| k == "GEMINI_API_KEY"));
        }
        other => panic!("expected OpenRouterCli config, got {:?}", other),
    }

    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    match original_gemini_api_key {
        Some(value) => std::env::set_var("GEMINI_API_KEY", value),
        None => std::env::remove_var("GEMINI_API_KEY"),
    }
}

#[test]
fn omniforge_uses_selected_model_target_and_matching_auth_env() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original_gemini_api_key = std::env::var_os("GEMINI_API_KEY");
    let original_minimax_api_key = std::env::var_os("MINIMAX_API_KEY");
    let original_openrouter_api_key = std::env::var_os("OPENROUTER_API_KEY");
    std::env::set_var("GEMINI_API_KEY", "gemini-omniforge-key");
    std::env::set_var("MINIMAX_API_KEY", "minimax-omniforge-key");
    std::env::set_var("OPENROUTER_API_KEY", "openrouter-omniforge-key");

    let session = ChatSession {
        id: "omniforge-session".to_string(),
        name: "OmniForge".to_string(),
        system_prompt: None,
        json_summary: None,
        claude_session_id: None,
        minimax_session_id: None,
        qwen_session_id: None,
        codex_session_id: None,
        nvidia_session_id: None,
        gemini_session_id: None,
        openrouter_session_id: None,
        omniforge_session_id: Some("omni-resume-1".to_string()),
        omniforge_model_id: Some("minimax-m2.7".to_string()),
        kimi_session_id: None,
        opencode_session_id: None,
        hermes_session_id: None,
        forge_session_id: None,
        claw_session_id: None,
        launched_from: Some("/tmp/project".to_string()),
        runsc_dirs: Vec::new(),
        runsc_image: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        messages: Vec::new(),
    };

    let config = provider_config_with_overrides(ModelProvider::OmniForge, Some(&session), None);
    match config {
        ProviderConfig::OpenRouterCli(config) => {
            assert!(config
                .cli
                .ends_with("others/forgecode2/target/release/rust-chatbot"));
            assert_eq!(config.model, "minimax_forge:MiniMax-M2.7");
            assert_eq!(config.resume_session_id.as_deref(), Some("omni-resume-1"));
            assert!(config
                .extra_env
                .iter()
                .any(|(k, v)| k == "MINIMAX_API_KEY" && v == "minimax-omniforge-key"));
            assert!(!config.extra_env.iter().any(|(k, _)| k == "GEMINI_API_KEY"));
        }
        other => panic!("expected OpenRouterCli config, got {:?}", other),
    }

    let gemini_override = ProviderRequestOverrides {
        model: Some("gemini-3.1-pro-preview".to_string()),
        ..Default::default()
    };
    let gemini_config = provider_config_with_overrides(
        ModelProvider::OmniForge,
        Some(&session),
        Some(&gemini_override),
    );
    match gemini_config {
        ProviderConfig::OpenRouterCli(config) => {
            assert_eq!(config.model, "gemini_forge:gemini-3.1-pro-preview");
            assert!(config
                .extra_env
                .iter()
                .any(|(k, v)| k == "GEMINI_API_KEY" && v == "gemini-omniforge-key"));
            assert!(!config.extra_env.iter().any(|(k, _)| k == "MINIMAX_API_KEY"));
        }
        other => panic!("expected OpenRouterCli config, got {:?}", other),
    }

    match original_gemini_api_key {
        Some(value) => std::env::set_var("GEMINI_API_KEY", value),
        None => std::env::remove_var("GEMINI_API_KEY"),
    }
    match original_minimax_api_key {
        Some(value) => std::env::set_var("MINIMAX_API_KEY", value),
        None => std::env::remove_var("MINIMAX_API_KEY"),
    }
    match original_openrouter_api_key {
        Some(value) => std::env::set_var("OPENROUTER_API_KEY", value),
        None => std::env::remove_var("OPENROUTER_API_KEY"),
    }
}

#[test]
fn omniforge_openrouter_minimax_model_uses_openrouter_auth_env() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original_openrouter_api_key = std::env::var_os("OPENROUTER_API_KEY");
    let original_minimax_api_key = std::env::var_os("MINIMAX_API_KEY");
    std::env::set_var("OPENROUTER_API_KEY", "openrouter-omniforge-key");
    std::env::set_var("MINIMAX_API_KEY", "minimax-should-not-be-used");

    let session = ChatSession {
        id: "omniforge-openrouter-minimax".to_string(),
        name: "OmniForge".to_string(),
        system_prompt: None,
        json_summary: None,
        claude_session_id: None,
        minimax_session_id: None,
        qwen_session_id: None,
        codex_session_id: None,
        nvidia_session_id: None,
        gemini_session_id: None,
        openrouter_session_id: None,
        omniforge_session_id: Some("omni-resume-openrouter".to_string()),
        omniforge_model_id: Some("minimax-m2.5-free".to_string()),
        kimi_session_id: None,
        opencode_session_id: None,
        hermes_session_id: None,
        forge_session_id: None,
        claw_session_id: None,
        launched_from: Some("/tmp/project".to_string()),
        runsc_dirs: Vec::new(),
        runsc_image: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        messages: Vec::new(),
    };

    let config = provider_config_with_overrides(ModelProvider::OmniForge, Some(&session), None);
    match config {
        ProviderConfig::OpenRouterCli(config) => {
            assert!(config
                .cli
                .ends_with("others/forgecode2/target/release/rust-chatbot"));
            assert_eq!(config.model, "openrouter:minimax/minimax-m2.5:free");
            assert_eq!(
                config.resume_session_id.as_deref(),
                Some("omni-resume-openrouter")
            );
            assert!(config
                .extra_env
                .iter()
                .any(|(k, v)| k == "OPENROUTER_API_KEY" && v == "openrouter-omniforge-key"));
            assert!(!config.extra_env.iter().any(|(k, _)| k == "MINIMAX_API_KEY"));
        }
        other => panic!("expected OpenRouterCli config, got {:?}", other),
    }

    match original_openrouter_api_key {
        Some(value) => std::env::set_var("OPENROUTER_API_KEY", value),
        None => std::env::remove_var("OPENROUTER_API_KEY"),
    }
    match original_minimax_api_key {
        Some(value) => std::env::set_var("MINIMAX_API_KEY", value),
        None => std::env::remove_var("MINIMAX_API_KEY"),
    }
}

#[test]
fn minimax_forge_reuses_native_minimax_key_file_when_forge_key_file_is_missing() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original_home = std::env::var_os("HOME");
    let original_minimax_api_key = std::env::var_os("MINIMAX_API_KEY");
    let tmp = tempfile::tempdir().unwrap();
    let shared_root = tmp.path().join(".rust-chatbot");
    let minimax_dir = shared_root.join(".minimax");
    std::fs::create_dir_all(&minimax_dir).unwrap();
    std::fs::write(minimax_dir.join("api.key"), "minimax-native-file-key\n").unwrap();
    std::env::set_var("HOME", tmp.path());
    std::env::remove_var("MINIMAX_API_KEY");

    let minimax = provider_config_with_overrides(ModelProvider::MiniMaxForge, None, None);

    match minimax {
        ProviderConfig::OpenRouterCli(config) => {
            assert_eq!(config.model, "minimax_forge:MiniMax-M2.7");
            assert_eq!(config.api_key, "minimax-native-file-key");
            assert!(config
                .extra_env
                .iter()
                .any(|(k, v)| k == "MINIMAX_API_KEY" && v == "minimax-native-file-key"));
        }
        other => panic!("expected OpenRouterCli config, got {:?}", other),
    }

    match original_minimax_api_key {
        Some(value) => std::env::set_var("MINIMAX_API_KEY", value),
        None => std::env::remove_var("MINIMAX_API_KEY"),
    }
    match original_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
}

#[test]
fn runsc_dirs_mount_user_dirs_in_container() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let user_dir1 = tmp.path().join("my-project");
    let user_dir2 = tmp.path().join("shared-libs");
    std::fs::create_dir_all(&user_dir1).unwrap();
    std::fs::create_dir_all(&user_dir2).unwrap();

    let overrides = ProviderRequestOverrides {
        runsc_dirs: vec![
            user_dir1.to_string_lossy().into_owned(),
            user_dir2.to_string_lossy().into_owned(),
        ],
        ..Default::default()
    };

    let mounts = claude_compat_runsc_mounts(ModelProvider::Claude, Some(&overrides));
    let user_mounts: Vec<_> = mounts
        .iter()
        .filter(|m| m.host == user_dir1 || m.host == user_dir2)
        .collect();
    assert_eq!(user_mounts.len(), 2, "both user dirs should be mounted");
    assert!(user_mounts
        .iter()
        .all(|m| m.target == RunscMountTarget::SamePath));
}

#[test]
fn runsc_dirs_skipped_when_empty() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let config_dir = tmp.path().join(".claude-desktop");
    std::fs::create_dir_all(&config_dir).unwrap();
    let claude_json = config_dir.join(".claude.json");
    std::fs::write(&claude_json, "{}\n").unwrap();

    let overrides = ProviderRequestOverrides {
        config_dir: Some(config_dir.clone()),
        claude_json_path: Some(claude_json.clone()),
        runsc_dirs: Vec::new(),
        ..Default::default()
    };

    let mounts = claude_compat_runsc_mounts(ModelProvider::Claude, Some(&overrides));
    // Auth sidecar mounts are disabled for all providers except gemini,
    // so when runsc_dirs is empty there should be no mounts at all.
    assert!(
        mounts.is_empty(),
        "expected no mounts when runsc_dirs is empty and auth sidecars are disabled"
    );
}
