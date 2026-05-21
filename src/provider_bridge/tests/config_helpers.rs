use super::*;

const TEST_TRUSTED_WORKSPACE_ROOT: &str = "/workspace";

fn with_trusted_workspace_root<T>(run: impl FnOnce() -> T) -> T {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|err| err.into_inner());
    let previous = std::env::var_os(crate::config::TRUSTED_WORKSPACE_ROOTS_ENV);
    std::env::set_var(
        crate::config::TRUSTED_WORKSPACE_ROOTS_ENV,
        TEST_TRUSTED_WORKSPACE_ROOT,
    );
    let result = run();
    match previous {
        Some(value) => std::env::set_var(crate::config::TRUSTED_WORKSPACE_ROOTS_ENV, value),
        None => std::env::remove_var(crate::config::TRUSTED_WORKSPACE_ROOTS_ENV),
    }
    result
}

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

// -- render_codex_mcp_toml -------------------------------------------------

#[test]
fn codex_toml_single_server() {
    let servers: serde_json::Map<String, serde_json::Value> = serde_json::from_value(json!({
        "memory": {
            "command": "echo",
            "args": ["hello", "world"],
            "env": {"KEY": "val"}
        }
    }))
    .unwrap();
    let toml = render_codex_mcp_toml(&servers).unwrap();
    assert!(
        toml.contains("[mcp_servers.\"memory\"]"),
        "missing section header"
    );
    assert!(toml.contains("command = \"echo\""), "missing command");
    assert!(
        toml.contains("args = [\"hello\", \"world\"]"),
        "missing args"
    );
    assert!(toml.contains("\"KEY\" = \"val\""), "missing env entry");
}

#[test]
fn codex_toml_empty_servers() {
    let servers = serde_json::Map::new();
    let toml = render_codex_mcp_toml(&servers).unwrap();
    assert!(toml.is_empty(), "empty servers should produce empty TOML");
}

#[test]
fn codex_toml_multiple_servers_ordering() {
    let servers: serde_json::Map<String, serde_json::Value> = serde_json::from_value(json!({
        "alpha": {"command": "a"},
        "beta": {"command": "b"},
    }))
    .unwrap();
    let toml = render_codex_mcp_toml(&servers).unwrap();
    let alpha_pos = toml.find("\"alpha\"").expect("alpha missing");
    let beta_pos = toml.find("\"beta\"").expect("beta missing");
    assert!(
        alpha_pos < beta_pos,
        "servers should appear in iteration order (sorted for BTreeMap-backed JSON)"
    );
}

// -- string_override / path_override / optional_string_override -----------

#[test]
fn string_override_explicit_wins() {
    let explicit = Some("explicit".to_string());
    let result = string_override(explicit.as_ref(), "NONEXISTENT_ENV_VAR_XYZ", || {
        "default".to_string()
    });
    assert_eq!(result, "explicit");
}

#[test]
fn env_override_trims_values_and_rejects_empty_strings() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    std::env::set_var(TEST_PROVIDER_CLI_ENV, "  /tmp/provider-cli  ");
    assert_eq!(
        env_override(TEST_PROVIDER_CLI_ENV).as_deref(),
        Some("/tmp/provider-cli")
    );

    std::env::set_var(TEST_PROVIDER_CLI_ENV, "   ");
    assert!(env_override(TEST_PROVIDER_CLI_ENV).is_none());
    std::env::remove_var(TEST_PROVIDER_CLI_ENV);
}

#[test]
fn string_override_falls_to_default() {
    let result = string_override(None, "NONEXISTENT_ENV_VAR_XYZ_2", || "default".to_string());
    assert_eq!(result, "default");
}

#[test]
fn string_override_uses_environment_when_explicit_is_missing() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var(TEST_PROVIDER_CLI_ENV, "  env-cli  ");

    let result = string_override(None, TEST_PROVIDER_CLI_ENV, || "default".to_string());

    assert_eq!(result, "env-cli");
    std::env::remove_var(TEST_PROVIDER_CLI_ENV);
}

#[test]
fn optional_string_override_none_when_absent() {
    let result = optional_string_override(None, "NONEXISTENT_ENV_VAR_XYZ_3");
    assert!(result.is_none());
}

#[test]
fn optional_string_override_uses_environment_when_explicit_is_missing() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var(TEST_ANTHROPIC_AUTH_TOKEN_ENV, "  env-token  ");

    let result = optional_string_override(None, TEST_ANTHROPIC_AUTH_TOKEN_ENV);

    assert_eq!(result.as_deref(), Some("env-token"));
    std::env::remove_var(TEST_ANTHROPIC_AUTH_TOKEN_ENV);
}

#[test]
fn path_override_explicit_wins() {
    let explicit = Some(PathBuf::from("/explicit"));
    let result = path_override(explicit.as_ref(), "NONEXISTENT_ENV_VAR_XYZ_4", || {
        Some(PathBuf::from("/default"))
    });
    assert_eq!(result, Some(PathBuf::from("/explicit")));
}

#[test]
fn path_override_uses_environment_when_explicit_is_missing() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var(TEST_PROVIDER_CONFIG_DIR_ENV, "/tmp/provider-config");

    let result = path_override(None, TEST_PROVIDER_CONFIG_DIR_ENV, || {
        Some(PathBuf::from("/default"))
    });

    assert_eq!(result, Some(PathBuf::from("/tmp/provider-config")));
    std::env::remove_var(TEST_PROVIDER_CONFIG_DIR_ENV);
}

#[test]
fn ensure_codex_home_dir_uses_codex_config_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let codex_toml = tmp.path().join(".codex-home").join("config.toml");
    let overrides = ProviderRequestOverrides {
        codex_toml_path: Some(codex_toml.clone()),
        ..Default::default()
    };

    let codex_home = ensure_codex_home_dir(Some(&overrides));

    assert_eq!(codex_home, codex_toml.parent().unwrap());
    assert!(
        codex_home.exists(),
        "Codex home directory should be created"
    );
}

// -- upsert_env_var -------------------------------------------------------

#[test]
fn upsert_env_var_inserts_new() {
    let mut env = vec![("A".to_string(), "1".to_string())];
    upsert_env_var(&mut env, "B", "2".to_string());
    assert_eq!(env.len(), 2);
    assert_eq!(env[1], ("B".to_string(), "2".to_string()));
}

#[test]
fn upsert_env_var_updates_existing() {
    let mut env = vec![("A".to_string(), "1".to_string())];
    upsert_env_var(&mut env, "A", "99".to_string());
    assert_eq!(env.len(), 1);
    assert_eq!(env[0], ("A".to_string(), "99".to_string()));
}

// -- apply_anthropic_transport_overrides -----------------------------------

#[test]
fn anthropic_transport_overrides_explicit() {
    let overrides = ProviderRequestOverrides {
        anthropic_base_url: Some("http://test:1234".to_string()),
        anthropic_auth_token: Some("tok-123".to_string()),
        ..Default::default()
    };
    let mut env = Vec::new();
    apply_anthropic_transport_overrides(&mut env, Some(&overrides), None, None);
    assert!(env
        .iter()
        .any(|(k, v)| k == "ANTHROPIC_BASE_URL" && v == "http://test:1234"));
    assert!(env
        .iter()
        .any(|(k, v)| k == "ANTHROPIC_AUTH_TOKEN" && v == "tok-123"));
}

#[test]
fn anthropic_transport_overrides_defaults() {
    let mut env = Vec::new();
    apply_anthropic_transport_overrides(
        &mut env,
        None,
        Some("http://default:80".to_string()),
        Some("default-tok".to_string()),
    );
    assert!(env
        .iter()
        .any(|(k, v)| k == "ANTHROPIC_BASE_URL" && v == "http://default:80"));
    assert!(env
        .iter()
        .any(|(k, v)| k == "ANTHROPIC_AUTH_TOKEN" && v == "default-tok"));
}

#[test]
fn anthropic_transport_overrides_explicit_beats_default() {
    let overrides = ProviderRequestOverrides {
        anthropic_base_url: Some("http://explicit".to_string()),
        ..Default::default()
    };
    let mut env = Vec::new();
    apply_anthropic_transport_overrides(
        &mut env,
        Some(&overrides),
        Some("http://default".to_string()),
        None,
    );
    assert!(env
        .iter()
        .any(|(k, v)| k == "ANTHROPIC_BASE_URL" && v == "http://explicit"));
}

#[test]
fn anthropic_transport_overrides_use_environment_when_explicit_missing() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var(TEST_ANTHROPIC_BASE_URL_ENV, "http://env-base");
    std::env::set_var(TEST_ANTHROPIC_AUTH_TOKEN_ENV, "env-token");

    let mut env = Vec::new();
    apply_anthropic_transport_overrides(
        &mut env,
        None,
        Some("http://default".to_string()),
        Some("default-token".to_string()),
    );

    assert!(env
        .iter()
        .any(|(k, v)| k == "ANTHROPIC_BASE_URL" && v == "http://env-base"));
    assert!(env
        .iter()
        .any(|(k, v)| k == "ANTHROPIC_AUTH_TOKEN" && v == "env-token"));

    std::env::remove_var(TEST_ANTHROPIC_BASE_URL_ENV);
    std::env::remove_var(TEST_ANTHROPIC_AUTH_TOKEN_ENV);
}

#[test]
fn claude_config_honors_test_environment_overrides() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var(TEST_PROVIDER_CLI_ENV, "/tmp/claude-cli");
    std::env::set_var(TEST_PROVIDER_CONFIG_DIR_ENV, "/tmp/claude-config");
    std::env::set_var(TEST_ANTHROPIC_BASE_URL_ENV, "http://claude-base");
    std::env::set_var(TEST_ANTHROPIC_AUTH_TOKEN_ENV, "claude-token");

    let config = claude_compat_config(crate::model::ModelProvider::Claude, None);

    assert_eq!(config.cli, "/tmp/claude-cli");
    assert_eq!(config.config_dir, Some(PathBuf::from("/tmp/claude-config")));
    assert!(config.require_mcp_servers);
    assert!(config
        .extra_env
        .iter()
        .any(|(k, v)| k == "ANTHROPIC_BASE_URL" && v == "http://claude-base"));
    assert!(config
        .extra_env
        .iter()
        .any(|(k, v)| k == "ANTHROPIC_AUTH_TOKEN" && v == "claude-token"));

    std::env::remove_var(TEST_PROVIDER_CLI_ENV);
    std::env::remove_var(TEST_PROVIDER_CONFIG_DIR_ENV);
    std::env::remove_var(TEST_ANTHROPIC_BASE_URL_ENV);
    std::env::remove_var(TEST_ANTHROPIC_AUTH_TOKEN_ENV);
}

#[test]
fn minimax_config_uses_default_transport_and_layout() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let config_dir = tempfile::tempdir().unwrap();
    let minimax_env = config::api_key_env_for(crate::model::ModelProvider::MiniMax)
        .expect("MiniMax catalog entry must define an api_key_env");
    std::env::set_var(minimax_env, "minimax-token");
    let overrides = ProviderRequestOverrides {
        config_dir: Some(config_dir.path().to_path_buf()),
        ..Default::default()
    };

    let config = claude_compat_config(crate::model::ModelProvider::MiniMax, Some(&overrides));

    assert_eq!(config.config_dir, Some(config_dir.path().to_path_buf()));
    assert!(!config.require_mcp_servers);
    assert!(config.allow_api_billing);
    assert!(config.extra_env.iter().any(|(k, v)| {
        k == "ANTHROPIC_BASE_URL"
            && Some(v.as_str()) == config::base_url_for(crate::model::ModelProvider::MiniMax)
    }));
    assert!(config
        .extra_env
        .iter()
        .any(|(k, v)| k == "ANTHROPIC_AUTH_TOKEN" && v == "minimax-token"));
    assert!(config_dir.path().exists());

    std::env::remove_var(minimax_env);
}

#[test]
fn resolved_paths_use_test_environment_overrides() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var(TEST_MCP_JSON_PATH_ENV, "/tmp/env.mcp.json");
    std::env::set_var(TEST_CLAUDE_JSON_PATH_ENV, "/tmp/env.claude.json");
    std::env::set_var(TEST_GEMINI_SETTINGS_PATH_ENV, "/tmp/env.gemini.json");
    std::env::set_var(TEST_CODEX_TOML_PATH_ENV, "/tmp/env.codex.toml");

    assert_eq!(
        resolved_mcp_json_path(None),
        PathBuf::from("/tmp/env.mcp.json")
    );
    assert_eq!(
        resolved_claude_json_path(None),
        Some(PathBuf::from("/tmp/env.claude.json"))
    );
    assert_eq!(
        resolved_gemini_settings_path(None, None),
        PathBuf::from("/tmp/env.gemini.json")
    );
    assert_eq!(
        resolved_codex_toml_path(None),
        PathBuf::from("/tmp/env.codex.toml")
    );

    std::env::remove_var(TEST_MCP_JSON_PATH_ENV);
    std::env::remove_var(TEST_CLAUDE_JSON_PATH_ENV);
    std::env::remove_var(TEST_GEMINI_SETTINGS_PATH_ENV);
    std::env::remove_var(TEST_CODEX_TOML_PATH_ENV);
}

#[test]
fn mcp_server_names_extracts_only_mcp_server_keys() {
    with_trusted_workspace_root(|| {
        with_isolated_path(|| {
            let tmp = tempfile::tempdir().unwrap();
            let mcp_json = tmp.path().join("mcp.json");
            std::fs::write(
                &mcp_json,
                serde_json::to_vec_pretty(&json!({
                    "mcpServers": {
                        "memory": {
                            "command": "uvx",
                            "args": ["memory-server"]
                        },
                        "git": { "command": "uvx", "args": ["git-server"] },
                    },
                    "notMcpServers": {
                        "ignored": true
                    }
                }))
                .unwrap(),
            )
            .unwrap();

            let overrides = ProviderRequestOverrides {
                mcp_json_path: Some(mcp_json),
                ..Default::default()
            };
            let mut names = mcp_server_names(Some(&overrides));
            names.sort_unstable();

            assert_eq!(names, vec!["git".to_string(), "memory".to_string()]);
        })
    });
}

#[test]
fn mcp_server_names_returns_empty_for_invalid_json() {
    let tmp = tempfile::tempdir().unwrap();
    let mcp_json = tmp.path().join("invalid.mcp.json");
    std::fs::write(&mcp_json, b"not json").unwrap();

    let overrides = ProviderRequestOverrides {
        mcp_json_path: Some(mcp_json),
        ..Default::default()
    };
    let names = mcp_server_names(Some(&overrides));

    assert!(names.is_empty());
}

#[test]
fn mcp_server_names_keeps_shell_execution_servers_outside_runsc() {
    with_trusted_workspace_root(|| {
        with_isolated_path(|| {
            let tmp = tempfile::tempdir().unwrap();
            let mcp_json = tmp.path().join("mcp.json");
            std::fs::write(
            &mcp_json,
            serde_json::to_vec_pretty(&json!({
                "mcpServers": {
                    "memory": { "command": "uvx", "args": ["memory-server"] },
                    "lightweight-builds": {
                        "command": "cargo",
                        "args": ["run", "--quiet", "--manifest-path", "/workspace/lightweight-mcp/Cargo.toml"]
                    },
                    "bash": { "command": "npx", "args": ["-y", "bash-mcp"] },
                    "mcp-server-commands": { "command": "npx", "args": ["mcp-server-commands"] }
                }
            }))
            .unwrap(),
        )
        .unwrap();

            let overrides = ProviderRequestOverrides {
                mcp_json_path: Some(mcp_json),
                ..Default::default()
            };
            let mut names = mcp_server_names(Some(&overrides));
            names.sort_unstable();

            assert_eq!(
                names,
                vec![
                    "bash".to_string(),
                    "lightweight-builds".to_string(),
                    "mcp-server-commands".to_string(),
                    "memory".to_string()
                ]
            );
        })
    });
}

#[test]
fn effective_mcp_server_names_keeps_routed_rust_servers_for_gemini_runsc() {
    with_trusted_workspace_root(|| {
        with_isolated_path(|| {
            let tmp = tempfile::tempdir().unwrap();
            let mcp_json = tmp.path().join("mcp.json");
            std::fs::write(
            &mcp_json,
            serde_json::to_vec_pretty(&json!({
                "mcpServers": {
                    "Context7": {
                        "command": "cargo",
                        "args": [
                            "run",
                            "--quiet",
                            "--manifest-path",
                            "/workspace/mcp-servers/context7-mcp/apps/stdio-server/Cargo.toml"
                        ]
                    },
                    "sequential-thinking": {
                        "command": "cargo",
                        "args": [
                            "run",
                            "--quiet",
                            "--manifest-path",
                            "/workspace/mcp-servers/mcp-server-sequential-thinking/apps/stdio-server/Cargo.toml"
                        ]
                    },
                    "chrome-devtools": {
                        "command": "cargo",
                        "args": [
                            "run",
                            "--quiet",
                            "--manifest-path",
                            "/workspace/mcp-servers/chrome-devtools-mcp/apps/stdio-server/Cargo.toml"
                        ]
                    },
                    "puppeteer": {
                        "command": "cargo",
                        "args": [
                            "run",
                            "--quiet",
                            "--manifest-path",
                            "/workspace/mcp-servers/mcp-server-puppeteer/apps/stdio-server/Cargo.toml"
                        ]
                    },
                    "lightweight-builds": {
                        "command": "cargo",
                        "args": [
                            "run",
                            "--quiet",
                            "--manifest-path",
                            "/workspace/lightweight-mcp/Cargo.toml"
                        ]
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

            let overrides = ProviderRequestOverrides {
                mcp_json_path: Some(mcp_json),
                runsc_image: Some("runsc-image:latest".to_string()),
                ..Default::default()
            };

            let mut gemini = effective_mcp_server_names(ModelProvider::Gemini, Some(&overrides));
            gemini.sort();
            assert_eq!(
                gemini,
                vec![
                    "Context7".to_string(),
                    "chrome-devtools".to_string(),
                    "lightweight-builds".to_string(),
                    "puppeteer".to_string(),
                    "sequential-thinking".to_string()
                ]
            );

            let mut claude = effective_mcp_server_names(ModelProvider::Claude, Some(&overrides));
            claude.sort();
            assert_eq!(
                claude,
                vec![
                    "Context7".to_string(),
                    "chrome-devtools".to_string(),
                    "lightweight-builds".to_string(),
                    "puppeteer".to_string(),
                    "sequential-thinking".to_string()
                ]
            );
        })
    });
}

#[test]
fn effective_mcp_server_names_keeps_shell_execution_servers_outside_runsc() {
    with_trusted_workspace_root(|| {
        with_isolated_path(|| {
            let tmp = tempfile::tempdir().unwrap();
            let mcp_json = tmp.path().join("mcp.json");
            std::fs::write(
            &mcp_json,
            serde_json::to_vec_pretty(&json!({
                "providers": {
                    "claude": {
                        "mcpServers": {
                            "memory": { "command": "uvx", "args": ["memory-server"] },
                            "lightweight-builds": {
                                "command": "cargo",
                                "args": ["run", "--quiet", "--manifest-path", "/workspace/lightweight-mcp/Cargo.toml"]
                            },
                            "bash": { "command": "npx", "args": ["-y", "bash-mcp"] },
                            "mcp-server-commands": { "command": "npx", "args": ["mcp-server-commands"] }
                        }
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

            let overrides = ProviderRequestOverrides {
                mcp_json_path: Some(mcp_json),
                ..Default::default()
            };
            let mut names = effective_mcp_server_names(ModelProvider::Claude, Some(&overrides));
            names.sort_unstable();

            assert_eq!(
                names,
                vec![
                    "bash".to_string(),
                    "lightweight-builds".to_string(),
                    "mcp-server-commands".to_string(),
                    "memory".to_string()
                ]
            );
        })
    });
}

#[test]
fn effective_mcp_server_names_prefers_provider_scoped_servers() {
    with_trusted_workspace_root(|| {
        with_isolated_path(|| {
            let tmp = tempfile::tempdir().unwrap();
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
                                "memory": { "command": "uvx", "args": ["memory-server"] }
                            }
                        },
                        "codex": {
                            "mcpServers": {
                                "builds": { "command": "cargo", "args": ["run"] }
                            }
                        }
                    }
                }))
                .unwrap(),
            )
            .unwrap();

            let overrides = ProviderRequestOverrides {
                mcp_json_path: Some(mcp_json),
                ..Default::default()
            };

            assert_eq!(
                effective_mcp_server_names(ModelProvider::Claude, Some(&overrides)),
                vec!["memory".to_string()]
            );
            assert_eq!(
                effective_mcp_server_names(ModelProvider::Codex, Some(&overrides)),
                vec!["builds".to_string()]
            );
        })
    });
}

#[test]
fn effective_mcp_server_names_applies_default_gemini_runsc_restrictions() {
    with_trusted_workspace_root(|| {
        with_isolated_path(|| {
            let tmp = tempfile::tempdir().unwrap();
            let mcp_json = tmp.path().join("mcp.json");
            std::fs::write(
            &mcp_json,
            serde_json::to_vec_pretty(&json!({
                "providers": {
                    "gemini": {
                        "mcpServers": {
                            "memory": { "command": "cargo", "args": ["run", "--quiet"] },
                            "mcp-filesystem": {
                                "command": "npx",
                                "args": ["-y", "@modelcontextprotocol/server-filesystem", "/workspace"]
                            },
                            "bash": { "command": "npx", "args": ["-y", "bash-mcp"] },
                            "mcp-server-commands": { "command": "npx", "args": ["mcp-server-commands"] }
                        }
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

            let overrides = ProviderRequestOverrides {
                mcp_json_path: Some(mcp_json),
                ..Default::default()
            };

            assert_eq!(
                effective_mcp_server_names(ModelProvider::Gemini, Some(&overrides)),
                vec!["memory".to_string()]
            );
        })
    });
}

#[test]
fn resolve_cli_launch_path_falls_back_to_asdf_node_install() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let bin_dir = home.path().join(".asdf/installs/nodejs/99.0.0/bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let cli_path = bin_dir.join("claude");
    std::fs::write(&cli_path, "#!/usr/bin/env node\n").unwrap();

    let original_home = std::env::var_os("HOME");
    let original_path = std::env::var_os("PATH");
    std::env::set_var("HOME", home.path());
    std::env::set_var("PATH", "/usr/bin:/bin");

    assert_eq!(
        resolve_cli_launch_path("claude"),
        cli_path.to_string_lossy().as_ref()
    );

    match original_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    match original_path {
        Some(value) => std::env::set_var("PATH", value),
        None => std::env::remove_var("PATH"),
    }
}

#[test]
fn claude_config_sets_runtime_path_for_resolved_cli() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let home = tempfile::tempdir().unwrap();
    let bin_dir = home.path().join(".asdf/installs/nodejs/99.0.0/bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let cli_path = bin_dir.join("claude");
    std::fs::write(&cli_path, "#!/usr/bin/env node\n").unwrap();

    let original_home = std::env::var_os("HOME");
    let original_path = std::env::var_os("PATH");
    std::env::set_var("HOME", home.path());
    std::env::set_var("PATH", "/usr/bin:/bin");

    let config = claude_compat_config(crate::model::ModelProvider::Claude, None);
    assert_eq!(config.cli, cli_path.to_string_lossy().as_ref());
    assert!(config
        .extra_env
        .iter()
        .any(|(k, v)| k == "PATH" && v.starts_with(&format!("{}:", bin_dir.display()))));

    match original_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    match original_path {
        Some(value) => std::env::set_var("PATH", value),
        None => std::env::remove_var("PATH"),
    }
}
