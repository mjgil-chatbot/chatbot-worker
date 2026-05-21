use super::*;

fn with_isolated_path<T>(run: impl FnOnce() -> T) -> T {
    let temp = tempfile::tempdir().unwrap();
    let previous = std::env::var_os("PATH");
    std::env::set_var("PATH", temp.path());
    let result = run();
    match previous {
        Some(value) => std::env::set_var("PATH", value),
        None => std::env::remove_var("PATH"),
    }
    result
}

fn with_trusted_workspace_root<T>(root: &str, run: impl FnOnce() -> T) -> T {
    let previous = std::env::var_os(crate::config::TRUSTED_WORKSPACE_ROOTS_ENV);
    std::env::set_var(crate::config::TRUSTED_WORKSPACE_ROOTS_ENV, root);
    let result = run();
    match previous {
        Some(value) => std::env::set_var(crate::config::TRUSTED_WORKSPACE_ROOTS_ENV, value),
        None => std::env::remove_var(crate::config::TRUSTED_WORKSPACE_ROOTS_ENV),
    }
    result
}

// -- provider_config_with_overrides model injection -----------------------

#[test]
fn provider_config_model_override_claude() {
    let overrides = ProviderRequestOverrides {
        model: Some("custom-claude-model".to_string()),
        ..Default::default()
    };
    let config = provider_config_with_overrides(ModelProvider::Claude, None, Some(&overrides));
    match config {
        ProviderConfig::Claude(c) => assert_eq!(c.model, "custom-claude-model"),
        other => panic!("expected Claude config, got {:?}", other),
    }
}

#[test]
fn minimax_command_string_uses_adapter_model_override() {
    let overrides = ProviderRequestOverrides {
        model: Some("custom-minimax".to_string()),
        ..Default::default()
    };
    let command = build_provider_command_string_with_overrides(
        ModelProvider::MiniMax,
        Some("base system prompt"),
        None,
        None,
        Some(&overrides),
        "hello from minimax",
    );
    assert!(command.contains("agent-adapter-cli run --provider minimax"));
    assert!(command.contains("--model custom-minimax"));
    assert!(command.contains("--system-prompt"));
}

#[test]
fn gemini_forge_api_key_falls_back_to_native_gemini_key_file() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous_home = std::env::var_os("HOME");
    let previous_gemini = std::env::var_os("GEMINI_API_KEY");
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", tmp.path());
    std::env::remove_var("GEMINI_API_KEY");

    let gemini_dir = tmp.path().join(".rust-chatbot").join(".gemini");
    std::fs::create_dir_all(&gemini_dir).unwrap();
    std::fs::write(gemini_dir.join("api.key"), "native-gemini-key\n").unwrap();

    assert_eq!(
        provider_api_key(ModelProvider::GeminiForge).as_deref(),
        Some("native-gemini-key")
    );

    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    match previous_gemini {
        Some(value) => std::env::set_var("GEMINI_API_KEY", value),
        None => std::env::remove_var("GEMINI_API_KEY"),
    }
}

#[test]
fn minimax_mcp_inventory_queries_use_provider_facing_server_names() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
                        },
                        "chrome-devtools": {
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
        anthropic_auth_token: Some("test-token".to_string()),
        ..Default::default()
    };

    let command = build_provider_command_string_with_overrides(
        ModelProvider::MiniMax,
        Some("base system prompt"),
        None,
        None,
        Some(&overrides),
        "what mcp servers do you have available?",
    );

    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }

    assert!(command.contains("base system prompt"));
    assert!(command.contains(
        "Skills, slash commands, tool names, and prompt instructions are not MCP servers."
    ));
    assert!(command.contains(
        "The only configured MCP server visible to you in this session is `mcp-coordinator`."
    ));
    assert!(command
        .contains("routed upstream tools are still callable through that server's tool namespace"));
    assert!(command.contains(
        "`tools/list`, `prompts/list`, `resources/list`, and `resources/templates/list`"
    ));
    assert!(
        !command.contains("`Context7`"),
        "MiniMax MCP inventory note should not expose routed upstream names: {command}"
    );
}

#[test]
fn provider_config_claude_preserves_output_json_schema_override() {
    let schema =
        "{\"type\":\"object\",\"properties\":{\"summary\":{\"type\":\"string\"}}}".to_string();
    let overrides = ProviderRequestOverrides {
        output_json_schema: Some(schema.clone()),
        ..Default::default()
    };
    let config = provider_config_with_overrides(ModelProvider::Claude, None, Some(&overrides));
    match config {
        ProviderConfig::Claude(c) => assert_eq!(c.json_schema.as_deref(), Some(schema.as_str())),
        other => panic!("expected Claude config, got {:?}", other),
    }
}

#[test]
fn build_queries_append_lightweight_builds_tooling_instruction() {
    let command = build_provider_command_string_with_overrides(
        ModelProvider::Claude,
        Some("base system prompt"),
        None,
        None,
        None,
        "please run cargo test --release for this workspace",
    );

    assert!(command.contains("base system prompt"));
    assert!(command.contains("`lightweight-builds` and `lightweight-tests`"));
    assert!(command.contains("`run_build` through `lightweight-builds`"));
    assert!(command.contains("`run_test` through `lightweight-tests`"));
    assert!(command.contains("`bash` or `mcp-server-commands`"));
}

#[test]
fn non_build_queries_do_not_append_lightweight_builds_tooling_instruction() {
    let command = build_provider_command_string_with_overrides(
        ModelProvider::Claude,
        Some("base system prompt"),
        None,
        None,
        None,
        "list the MCP servers available in this session",
    );

    assert!(command.contains("base system prompt"));
    assert!(!command.contains("`lightweight-builds` and `lightweight-tests`"));
}

// -- autonomous_provider_overrides ----------------------------------------

#[test]
fn autonomous_overrides_sets_model_for_all_providers() {
    for provider in [
        ModelProvider::Claude,
        ModelProvider::MiniMax,
        ModelProvider::Qwen,
        ModelProvider::OpenRouter,
    ] {
        let overrides = autonomous_provider_overrides(provider);
        assert!(
            overrides.model.is_some(),
            "autonomous overrides should set model for {:?}",
            provider
        );
    }
    let qwen_overrides = autonomous_provider_overrides(ModelProvider::Qwen);
    assert_eq!(
        qwen_overrides.model.as_deref(),
        Some("qwen/qwen3-coder:free")
    );
}

#[test]
fn autonomous_overrides_enable_runsc_when_requested() {
    let expected = config::background_runsc_image();
    let overrides = autonomous_provider_overrides_with_execution(
        ModelProvider::Gemini,
        Some(AutonomousExecutionEnvironment::Runsc),
        None,
    );
    assert_eq!(overrides.runsc_image.as_deref(), Some(expected.as_str()));
}

#[test]
fn autonomous_overrides_include_runsc_workspace_paths_when_requested() {
    let overrides = autonomous_provider_overrides_with_execution(
        ModelProvider::Codex,
        Some(AutonomousExecutionEnvironment::Runsc),
        Some(vec!["src".to_string(), "Cargo.toml".to_string()]),
    );
    assert_eq!(
        overrides.runsc_workspace_includes,
        vec!["src".to_string(), "Cargo.toml".to_string()]
    );
}

#[test]
fn autonomous_overrides_skip_nested_runsc_inside_worker_container() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::var_os("RUST_CHATBOT_AUTONOMOUS_INSIDE_RUNSC");
    std::env::set_var("RUST_CHATBOT_AUTONOMOUS_INSIDE_RUNSC", "1");

    let overrides = autonomous_provider_overrides_with_execution(
        ModelProvider::Gemini,
        Some(AutonomousExecutionEnvironment::Runsc),
        Some(vec!["instructions/".to_string(), "plan.txt".to_string()]),
    );

    match previous {
        Some(value) => std::env::set_var("RUST_CHATBOT_AUTONOMOUS_INSIDE_RUNSC", value),
        None => std::env::remove_var("RUST_CHATBOT_AUTONOMOUS_INSIDE_RUNSC"),
    }

    assert!(overrides.runsc_image.is_none());
    assert!(overrides.runsc_workspace_includes.is_empty());
}

#[test]
fn autonomous_overrides_do_not_force_skip_git_repo_check() {
    assert!(!autonomous_provider_overrides(ModelProvider::Codex).force_skip_git_repo_check);
    assert!(!autonomous_provider_overrides(ModelProvider::Nvidia).force_skip_git_repo_check);
    assert!(!autonomous_provider_overrides(ModelProvider::Claude).force_skip_git_repo_check);
}

// ── Slice 3: provider bridge dispatch & routing tests ───────────

#[test]
fn provider_config_dispatch_returns_correct_variant() {
    // Each ModelProvider maps to the correct ProviderConfig discriminant
    let cases: Vec<(ModelProvider, &str)> = vec![
        (ModelProvider::Claude, "Claude"),
        (ModelProvider::Codex, "Codex"),
        (ModelProvider::Nvidia, "Nemotron"),
        (ModelProvider::Gemini, "Gemini"),
        (ModelProvider::Qwen, "OpenRouter"),
        (ModelProvider::OpenRouter, "OpenRouter"),
    ];
    for (provider, label) in cases {
        let config = provider_config_with_overrides(provider, None, None);
        let variant_name = format!("{:?}", config);
        assert!(
            variant_name.starts_with(label),
            "expected {} variant for {:?}, got {}",
            label,
            provider,
            variant_name
        );
    }
}

#[test]
fn provider_config_model_override_codex() {
    let overrides = ProviderRequestOverrides {
        model: Some("custom-codex".to_string()),
        runsc_image: Some("runsc-image:latest".to_string()),
        runsc_workspace_includes: vec!["src".to_string(), "Cargo.toml".to_string()],
        ..Default::default()
    };
    let config = provider_config_with_overrides(ModelProvider::Codex, None, Some(&overrides));
    match config {
        ProviderConfig::Codex(c) => {
            assert_eq!(c.model, Some("custom-codex".to_string()));
            assert_eq!(c.runsc_image.as_deref(), Some("runsc-image:latest"));
            assert_eq!(
                c.runsc_workspace_includes,
                vec!["src".to_string(), "Cargo.toml".to_string()]
            );
        }
        other => panic!("expected Codex config, got {:?}", other),
    }
}

#[test]
fn provider_config_codex_persists_output_schema_and_mounts_it() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous_home = std::env::var_os("HOME");
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", tmp.path());

    let codex_dir = tmp.path().join(".codex");
    std::fs::create_dir_all(&codex_dir).unwrap();
    let config_path = codex_dir.join("config.toml");
    std::fs::write(&config_path, "model = \"gpt-5.4\"\n").unwrap();

    let schema = "{\"type\":\"object\",\"properties\":{\"ok\":{\"type\":\"boolean\"}}}".to_string();
    let overrides = ProviderRequestOverrides {
        codex_toml_path: Some(config_path),
        output_json_schema: Some(schema.clone()),
        ..Default::default()
    };
    let config = provider_config_with_overrides(ModelProvider::Codex, None, Some(&overrides));

    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }

    match config {
        ProviderConfig::Codex(c) => {
            let schema_path = c
                .output_schema
                .as_ref()
                .expect("codex config should persist output schema");
            assert_eq!(std::fs::read_to_string(schema_path).unwrap(), schema);
            // Output schema is mounted via codex_runsc_mounts when runsc is enabled.
            assert!(c.runsc_mounts.iter().any(|mount| {
                mount.host == *schema_path && mount.target == RunscMountTarget::SamePath
            }));
        }
        other => panic!("expected Codex config, got {:?}", other),
    }
}

#[test]
fn codex_runsc_mounts_include_repo_local_auth_sidecar() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let codex_dir = tmp.path().join(".codex");
    std::fs::create_dir_all(&codex_dir).unwrap();
    let config_path = codex_dir.join("config.toml");
    let auth_path = codex_dir.join("auth.json");
    std::fs::write(&config_path, "model = \"gpt-5.4\"\n").unwrap();
    std::fs::write(&auth_path, "{}\n").unwrap();

    let overrides = ProviderRequestOverrides {
        codex_toml_path: Some(config_path.clone()),
        ..Default::default()
    };
    let mounts = codex_runsc_mounts(Some(&overrides));

    // Auth sidecar mounts are disabled for all providers except gemini.
    // Only user-specified runsc_dirs and output schemas are mounted.
    assert!(
        mounts.iter().any(|mount| mount.host == config_path),
        "codex config.toml should be mounted"
    );
}

#[test]
fn claude_runsc_mounts_include_container_home_auth_sidecars() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let config_dir = tmp.path().join(".claude-desktop");
    std::fs::create_dir_all(&config_dir).unwrap();
    let claude_json = config_dir.join(".claude.json");
    let credentials = config_dir.join(".credentials.json");
    let settings = config_dir.join("settings.json");
    std::fs::write(&claude_json, "{}\n").unwrap();
    std::fs::write(&credentials, "{}\n").unwrap();
    std::fs::write(&settings, "{}\n").unwrap();

    let overrides = ProviderRequestOverrides {
        config_dir: Some(config_dir.clone()),
        claude_json_path: Some(claude_json.clone()),
        ..Default::default()
    };
    let mounts = claude_compat_runsc_mounts(ModelProvider::Claude, Some(&overrides));

    // Auth sidecar mounts are disabled for all providers except gemini.
    assert!(
        mounts.is_empty(),
        "auth sidecar mounts are disabled; got {:?}",
        mounts
    );
}

#[test]
fn minimax_runsc_mounts_include_container_home_sidecars() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let config_dir = tmp.path().join(".minimax");
    std::fs::create_dir_all(&config_dir).unwrap();
    let claude_json = config_dir.join(".claude.json");
    let settings = config_dir.join("settings.json");
    std::fs::write(&claude_json, "{}\n").unwrap();
    std::fs::write(&settings, "{}\n").unwrap();

    let overrides = ProviderRequestOverrides {
        config_dir: Some(config_dir),
        ..Default::default()
    };
    let mounts = claude_compat_runsc_mounts(ModelProvider::MiniMax, Some(&overrides));

    // Auth sidecar mounts are disabled for all providers except gemini.
    assert!(
        mounts.is_empty(),
        "auth sidecar mounts are disabled; got {:?}",
        mounts
    );
}

#[test]
fn qwen_runsc_mounts_include_oauth_sidecars_from_resolved_config_dir() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let qwen_dir = home.path().join(".rust-chatbot").join(".qwen");
    std::fs::create_dir_all(&qwen_dir).unwrap();
    let settings = qwen_dir.join("settings.json");
    let oauth = qwen_dir.join("oauth_creds.json");
    let install = qwen_dir.join("installation_id");
    std::fs::write(&settings, "{}\n").unwrap();
    std::fs::write(&oauth, "{}\n").unwrap();
    std::fs::write(&install, "id\n").unwrap();

    let previous_home = std::env::var_os("HOME");
    std::env::set_var("HOME", home.path());
    let mounts = qwen_runsc_mounts(None);
    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }

    // Auth sidecar mounts are disabled for all providers except gemini.
    assert!(
        mounts.is_empty(),
        "auth sidecar mounts are disabled; got {:?}",
        mounts
    );
}

#[test]
fn qwen_cli_config_strips_repo_filesystem_and_shell_mcps_for_runsc() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    with_trusted_workspace_root("/workspace", || {
        with_isolated_path(|| {
            let tmp = tempfile::tempdir().unwrap();
            let mcp_json = tmp.path().join("mcp.json");
            std::fs::write(
                &mcp_json,
                serde_json::to_vec_pretty(&json!({
                    "mcpServers": {
                        "memory": { "command": "uvx", "args": ["memory-server"] },
                        "mcp-filesystem": {
                            "command": "npx",
                            "args": ["-y", "@modelcontextprotocol/server-filesystem", "/workspace"]
                        },
                        "bash": { "command": "npx", "args": ["-y", "bash-mcp"] },
                        "mcp-server-commands": { "command": "npx", "args": ["mcp-server-commands"] }
                    }
                }))
                .unwrap(),
            )
            .unwrap();

            let config = qwen_cli_config(Some(&ProviderRequestOverrides {
                mcp_json_path: Some(mcp_json),
                runsc_image: Some("runsc-image:latest".to_string()),
                ..Default::default()
            }));

            assert_eq!(config.allowed_mcp_server_names, vec!["memory".to_string()]);
        })
    });
}

#[test]
fn provider_config_model_override_gemini() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let settings_path = tmp.path().join("settings.local.json");
    std::fs::write(&settings_path, "{}\n").unwrap();

    let overrides = ProviderRequestOverrides {
        model: Some("custom-gemini".to_string()),
        gemini_settings_path: Some(settings_path.clone()),
        runsc_image: Some("runsc-image:latest".to_string()),
        ..Default::default()
    };
    let config = provider_config_with_overrides(ModelProvider::Gemini, None, Some(&overrides));
    match config {
        ProviderConfig::Gemini(c) => {
            assert_eq!(c.model, Some("custom-gemini".to_string()));
            assert_eq!(c.runsc_image.as_deref(), Some("runsc-image:latest"));
            assert_ne!(
                c.project_settings_path.as_deref(),
                Some(settings_path.as_path())
            );
        }
        other => panic!("expected Gemini config, got {:?}", other),
    }
}

#[test]
fn provider_config_model_gemini_includes_gemini_api_key() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original_api_key = std::env::var_os("GEMINI_API_KEY");
    let original_home = std::env::var_os("HOME");
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("GEMINI_API_KEY", "env-gemini-key");

    let config = provider_config_with_overrides(ModelProvider::Gemini, None, None);
    match config {
        ProviderConfig::Gemini(config) => {
            assert!(config
                .extra_env
                .iter()
                .any(|(k, v)| k == "GEMINI_API_KEY" && v == "env-gemini-key"));
        }
        other => panic!("expected Gemini config, got {:?}", other),
    }

    match original_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    match original_api_key {
        Some(value) => std::env::set_var("GEMINI_API_KEY", value),
        None => std::env::remove_var("GEMINI_API_KEY"),
    }
}

#[test]
fn gemini_runsc_mounts_include_repo_local_auth_sidecars() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let gemini_dir = tmp.path().join(".gemini");
    std::fs::create_dir_all(&gemini_dir).unwrap();
    let settings_path = gemini_dir.join("settings.local.json");
    std::fs::write(&settings_path, "{}\n").unwrap();
    std::fs::write(gemini_dir.join("projects.json"), "{\"projects\":{}}\n").unwrap();
    std::fs::write(gemini_dir.join("oauth_creds.json"), "{}\n").unwrap();
    std::fs::write(gemini_dir.join("google_accounts.json"), "{}\n").unwrap();

    let overrides = ProviderRequestOverrides {
        gemini_settings_path: Some(settings_path.clone()),
        ..Default::default()
    };
    let mounts = gemini_runsc_mounts(None, Some(&overrides));

    assert!(mounts.iter().any(|mount| mount.host == settings_path));
    let home_mount = mounts
        .iter()
        .find(|mount| mount.target == RunscMountTarget::ContainerHome(PathBuf::from(".gemini")))
        .expect("expected writable .gemini home mount");
    assert_eq!(
        std::fs::read_to_string(home_mount.host.join("oauth_creds.json")).unwrap(),
        "{}\n"
    );
    assert_eq!(
        std::fs::read_to_string(home_mount.host.join("google_accounts.json")).unwrap(),
        "{}\n"
    );
}

#[test]
fn provider_config_model_override_qwen() {
    let overrides = ProviderRequestOverrides {
        model: Some("custom-qwen".to_string()),
        ..Default::default()
    };
    let config = provider_config_with_overrides(ModelProvider::Qwen, None, Some(&overrides));
    match config {
        ProviderConfig::OpenRouter(c) => assert_eq!(c.model, "custom-qwen"),
        other => panic!("expected OpenRouter config for Qwen, got {:?}", other),
    }
}

#[test]
fn nvidia_config_uses_dedicated_codex_home_with_model_catalog() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let codex_home = tmp.path().join("nvidia-codex-home");
    let mcp_json = tmp.path().join("mcp.json");
    std::fs::write(
        &mcp_json,
        r#"{"mcpServers":{"memory":{"command":"uvx","args":["memory-server"]}}}"#,
    )
    .unwrap();

    let nvidia_env = config::api_key_env_for(crate::model::ModelProvider::Nvidia)
        .expect("Nvidia catalog entry must define an api_key_env");
    let previous_home = std::env::var_os(config::RUST_CHATBOT_NVIDIA_CODEX_HOME_ENV);
    let previous_key = std::env::var_os(nvidia_env);
    std::env::set_var(config::RUST_CHATBOT_NVIDIA_CODEX_HOME_ENV, &codex_home);
    std::env::set_var(nvidia_env, "test-nvidia-key");

    let overrides = ProviderRequestOverrides {
        mcp_json_path: Some(mcp_json),
        ..Default::default()
    };
    let config = codex_compat_config(ModelProvider::Nvidia, Some(&overrides));

    match previous_home {
        Some(value) => std::env::set_var(config::RUST_CHATBOT_NVIDIA_CODEX_HOME_ENV, value),
        None => std::env::remove_var(config::RUST_CHATBOT_NVIDIA_CODEX_HOME_ENV),
    }
    match previous_key {
        Some(value) => std::env::set_var(nvidia_env, value),
        None => std::env::remove_var(nvidia_env),
    }

    assert!(config
        .extra_env
        .iter()
        .any(|(key, value)| { key == "CODEX_HOME" && value == &codex_home.to_string_lossy() }));
    assert!(config
        .extra_env
        .iter()
        .any(|(key, value)| { key == "OPENAI_API_KEY" && value == "test-nvidia-key" }));

    let config_toml = std::fs::read_to_string(codex_home.join("config.toml")).unwrap();
    assert!(config_toml.contains("model_provider = \"nvidia\""));
    assert!(config_toml.contains("model_catalog_json = \"model-catalog.json\""));
    assert!(config_toml.contains("wire_api = \"chat\""));
    assert!(config_toml.contains("supports_websockets = false"));
    assert!(config_toml.contains("[mcp_servers.\"mcp-coordinator\"]"));

    let model_catalog: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(codex_home.join("model-catalog.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        model_catalog["models"][0]["slug"].as_str(),
        config::interactive_model_for(crate::model::ModelProvider::Nvidia)
    );
}

#[test]
fn to_provider_memory_context_none_returns_none() {
    assert!(to_provider_memory_context(None).is_none());
}

#[test]
fn to_provider_session_none_returns_none() {
    assert!(to_provider_session(None).is_none());
}

#[test]
fn autonomous_overrides_default_fields_are_none() {
    let overrides = autonomous_provider_overrides(ModelProvider::Claude);
    assert!(overrides.cli.is_none());
    assert!(overrides.config_dir.is_none());
    assert!(overrides.anthropic_base_url.is_none());
    assert!(overrides.anthropic_auth_token.is_none());
    assert!(overrides.mcp_json_path.is_none());
}

#[test]
fn codex_toml_special_chars_in_server_name() {
    // Server names with special chars should be properly quoted
    let servers: serde_json::Map<String, serde_json::Value> = serde_json::from_value(json!({
        "my-server.v2": {"command": "run"}
    }))
    .unwrap();
    let toml = render_codex_mcp_toml(&servers).unwrap();
    assert!(
        toml.contains("\"my-server.v2\""),
        "special chars in name should be quoted"
    );
}

#[test]
fn toml_basic_string_escapes_quotes() {
    let result = toml_basic_string("hello \"world\"").unwrap();
    assert!(result.contains("\\\""), "inner quotes should be escaped");
}

#[test]
fn provider_request_overrides_default_is_all_none() {
    let d = ProviderRequestOverrides::default();
    assert!(d.model.is_none());
    assert!(d.cli.is_none());
    assert!(d.config_dir.is_none());
    assert!(d.anthropic_base_url.is_none());
    assert!(d.anthropic_auth_token.is_none());
    assert!(d.mcp_json_path.is_none());
    assert!(d.claude_json_path.is_none());
    assert!(d.gemini_settings_path.is_none());
    assert!(d.codex_toml_path.is_none());
    assert!(d.qwen_code_resume_session_id.is_none());
}

// ── Slice 3: response normalization with populated data ─────────

#[test]
fn to_provider_session_maps_all_fields() {
    use crate::data::ChatMessage;
    use chrono::Utc;

    let session = ChatSession {
        id: "s1".to_string(),
        name: "test".to_string(),
        system_prompt: Some("sys".to_string()),
        json_summary: None,
        claude_session_id: Some("claude-1".to_string()),
        minimax_session_id: Some("mm-1".to_string()),
        qwen_session_id: Some("qw-1".to_string()),
        codex_session_id: Some("cx-1".to_string()),
        nvidia_session_id: None,
        gemini_session_id: Some("gm-1".to_string()),
        openrouter_session_id: None,
        omniforge_session_id: None,
        omniforge_model_id: None,
        kimi_session_id: None,
        opencode_session_id: None,
        hermes_session_id: None,
        forge_session_id: None,
        claw_session_id: None,
        launched_from: Some("/tmp".to_string()),
        runsc_dirs: Vec::new(),
        runsc_image: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        messages: vec![
            ChatMessage {
                content: "hello".to_string(),
                is_user: true,
                timestamp: Utc::now(),
                command: None,
                retrieved_memory: None,
            },
            ChatMessage {
                content: "hi back".to_string(),
                is_user: false,
                timestamp: Utc::now(),
                command: None,
                retrieved_memory: None,
            },
        ],
    };

    let provider = to_provider_session(Some(&session)).expect("should produce ProviderSession");
    assert_eq!(provider.claude_session_id.as_deref(), Some("claude-1"));
    assert_eq!(provider.minimax_session_id.as_deref(), Some("mm-1"));
    assert_eq!(provider.qwen_session_id.as_deref(), Some("qw-1"));
    assert_eq!(provider.codex_session_id.as_deref(), Some("cx-1"));
    assert_eq!(provider.gemini_session_id.as_deref(), Some("gm-1"));
    assert_eq!(provider.working_dir.as_deref(), Some("/tmp"));
    assert_eq!(provider.messages.len(), 2);
    assert_eq!(provider.messages[0].content, "hello");
    assert!(provider.messages[0].is_user);
    assert!(provider.messages[0].history_content.is_none());
    assert_eq!(provider.messages[1].content, "hi back");
    assert!(!provider.messages[1].is_user);
    assert!(provider.messages[1].history_content.is_none());
}

#[test]
fn to_provider_memory_context_maps_hits() {
    use crate::memory::{MemoryContext, MemoryHit};
    use chrono::Utc;

    let ctx = MemoryContext {
        workspace_id: "ws-42".to_string(),
        query: "search term".to_string(),
        hits: vec![
            MemoryHit {
                doc_key: "dk1".to_string(),
                chat_id: "c1".to_string(),
                role: "user".to_string(),
                session_name: "s1".to_string(),
                timestamp: Utc::now(),
                snippet: "first snippet".to_string(),
                title: Some("Title A".to_string()),
                message_index: Some(1),
                turn_index: Some(0),
                score: Some(0.9),
            },
            MemoryHit {
                doc_key: "dk2".to_string(),
                chat_id: "c2".to_string(),
                role: "assistant".to_string(),
                session_name: "s2".to_string(),
                timestamp: Utc::now(),
                snippet: "second snippet".to_string(),
                title: None,
                message_index: None,
                turn_index: None,
                score: None,
            },
        ],
    };

    let provider = to_provider_memory_context(Some(&ctx)).expect("should map");
    assert_eq!(provider.workspace_id, "ws-42");
    assert_eq!(provider.query, "search term");
    assert_eq!(provider.hits.len(), 2);
    assert_eq!(provider.hits[0].doc_key, "dk1");
    assert_eq!(provider.hits[0].snippet, "first snippet");
    assert_eq!(provider.hits[0].title.as_deref(), Some("Title A"));
    assert_eq!(provider.hits[0].score, Some(0.9));
    assert_eq!(provider.hits[1].doc_key, "dk2");
    assert!(provider.hits[1].title.is_none());
    assert!(provider.hits[1].score.is_none());
}
