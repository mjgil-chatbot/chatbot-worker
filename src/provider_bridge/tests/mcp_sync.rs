use super::*;

// -- sync_mcp_configs (file-system round-trip) ----------------------------

#[cfg(unix)]
fn write_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, b"#!/bin/sh\nexit 0\n").unwrap();
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).unwrap();
}

#[test]
fn sync_mcp_configs_missing_source_is_noop() {
    let overrides = ProviderRequestOverrides {
        mcp_json_path: Some(PathBuf::from("/tmp/nonexistent_mcp_source_12345.json")),
        ..Default::default()
    };
    let result = sync_mcp_configs(None, Some(&overrides));
    assert!(result.is_ok(), "missing source should silently no-op");
}

#[test]
fn sync_mcp_configs_invalid_json_is_noop() {
    let tmp = std::env::temp_dir().join("parity_bridge_test_invalid.json");
    std::fs::write(&tmp, b"not json").unwrap();
    let overrides = ProviderRequestOverrides {
        mcp_json_path: Some(tmp.clone()),
        ..Default::default()
    };
    let result = sync_mcp_configs(None, Some(&overrides));
    let _ = std::fs::remove_file(&tmp);
    assert!(result.is_ok(), "unparseable JSON should silently no-op");
}

#[cfg(unix)]
#[test]
fn filtered_profile_mcp_servers_injects_implicit_fff_server_when_binary_exists() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    write_executable(&bin_dir.join("fff-mcp"));

    let previous_path = std::env::var_os("PATH");
    std::env::set_var("PATH", &bin_dir);

    let parsed = json!({
        "mcpServers": {
            "Context7": { "command": "context7" }
        }
    });

    let servers = filtered_profile_mcp_servers(&parsed, Some("claude"), None);

    match previous_path {
        Some(value) => std::env::set_var("PATH", value),
        None => std::env::remove_var("PATH"),
    }

    assert_eq!(
        servers
            .get("fff")
            .and_then(|value| value.get("command"))
            .and_then(|value| value.as_str()),
        Some(bin_dir.join("fff-mcp").to_string_lossy().as_ref())
    );
    assert!(servers.get("Context7").is_some());
}

#[test]
fn sync_mcp_configs_does_not_clobber_non_object_existing_json() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = std::env::temp_dir().join(format!(
        "parity_bridge_non_object_target_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let src = tmp.join("source.mcp.json");
    let claude_dst = tmp.join("claude.json");
    std::fs::write(
        &src,
        serde_json::to_vec_pretty(&json!({
            "mcpServers": {
                "test-srv": {
                    "command": "node"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(&claude_dst, b"[]").unwrap();

    let before = std::fs::read(&claude_dst).unwrap();
    let overrides = ProviderRequestOverrides {
        mcp_json_path: Some(src),
        claude_json_path: Some(claude_dst.clone()),
        ..Default::default()
    };

    sync_mcp_configs(None, Some(&overrides)).unwrap();

    let after = std::fs::read(&claude_dst).unwrap();
    assert_eq!(
        after, before,
        "non-object Claude JSON should be left untouched"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn sync_mcp_configs_does_not_clobber_invalid_existing_claude_json() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();

    let src = tmp.path().join("source.mcp.json");
    let claude_dst = tmp.path().join("claude.json");
    std::fs::write(
        &src,
        serde_json::to_vec_pretty(&json!({
            "mcpServers": {
                "test-srv": {
                    "command": "node"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(&claude_dst, b"not valid json").unwrap();

    let before = std::fs::read(&claude_dst).unwrap();
    let overrides = ProviderRequestOverrides {
        mcp_json_path: Some(src),
        claude_json_path: Some(claude_dst.clone()),
        ..Default::default()
    };

    sync_mcp_configs(None, Some(&overrides)).unwrap();

    let after = std::fs::read(&claude_dst).unwrap();
    assert_eq!(
        after, before,
        "invalid existing Claude JSON should be left untouched"
    );
}

#[test]
fn sync_mcp_configs_targets_minimax_compat_json() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = std::env::temp_dir().join(format!(
        "parity_bridge_minimax_compat_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let src = tmp.join("source.mcp.json");
    let claude_default = tmp.join("claude.json");
    let minimax_config_dir = tmp.join(".minimax");
    let minimax_json = minimax_config_dir.join(".claude.json");
    std::fs::create_dir_all(&minimax_config_dir).unwrap();

    std::fs::write(
        &src,
        serde_json::to_vec_pretty(&json!({
            "mcpServers": {
                "test-srv": {
                    "command": "node"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let overrides = ProviderRequestOverrides {
        mcp_json_path: Some(src),
        claude_json_path: Some(claude_default.clone()),
        config_dir: Some(minimax_config_dir.clone()),
        ..Default::default()
    };

    sync_mcp_configs(None, Some(&overrides)).unwrap();

    let claude: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&claude_default).unwrap()).unwrap();
    assert!(claude["mcpServers"]
        .get(MCP_COORDINATOR_SERVER_NAME)
        .is_some());
    assert_eq!(
        claude["providers"]["claude"]["mcpServers"]["test-srv"]["command"].as_str(),
        Some("node")
    );

    let minimax: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&minimax_json).unwrap()).unwrap();
    assert!(minimax["mcpServers"]
        .get(MCP_COORDINATOR_SERVER_NAME)
        .is_some());
    assert_eq!(
        minimax["providers"]["claude"]["mcpServers"]["test-srv"]["command"].as_str(),
        Some("node")
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn sync_mcp_configs_targets_shared_and_legacy_codex_paths() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &home);

    let src = tmp.path().join("source.mcp.json");
    let claude_dst = tmp.path().join("claude.json");
    let gemini_dst = tmp.path().join("gemini_settings.json");
    let config_dir = tmp.path().join(".minimax-config");

    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        &src,
        serde_json::to_vec_pretty(&json!({
            "mcpServers": {
                "memory": {
                    "command": "node"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        &gemini_dst,
        serde_json::to_vec_pretty(&json!({
            "theme": "dark",
            "mcpServers": {
                "custom-safe": { "command": "uvx", "args": ["safe-server"] },
                "bash": { "command": "npx", "args": ["-y", "bash-mcp"] }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    // Ensure the canonical shared root exists so resolved_global_config_path
    // returns the temp-home path instead of falling back to the repo.
    let canonical_shared = home.join(".rust-chatbot").join(".codex");
    std::fs::create_dir_all(&canonical_shared).unwrap();

    let overrides = ProviderRequestOverrides {
        mcp_json_path: Some(src),
        claude_json_path: Some(claude_dst.clone()),
        gemini_settings_path: Some(gemini_dst.clone()),
        config_dir: Some(config_dir),
        ..Default::default()
    };

    sync_mcp_configs(None, Some(&overrides)).unwrap();

    let shared_codex = home
        .join(".rust-chatbot")
        .join(".codex")
        .join("config.toml");
    let legacy_codex = home.join(".codex").join("config.toml");

    let shared_contents = std::fs::read_to_string(&shared_codex).unwrap();
    let legacy_contents = std::fs::read_to_string(&legacy_codex).unwrap();
    let claude: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&claude_dst).unwrap()).unwrap();
    let gemini: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&gemini_dst).unwrap()).unwrap();
    let shared_sidecar = coordinator_sidecar_path(&shared_codex);
    let legacy_sidecar = coordinator_sidecar_path(&legacy_codex);

    assert!(shared_contents.contains("[mcp_servers.\"mcp-coordinator\"]"));
    assert!(legacy_contents.contains("[mcp_servers.\"mcp-coordinator\"]"));
    assert!(claude["mcpServers"]
        .get(MCP_COORDINATOR_SERVER_NAME)
        .is_some());
    assert!(gemini["mcpServers"]
        .get(MCP_COORDINATOR_SERVER_NAME)
        .is_some());
    assert_eq!(
        claude["providers"]["claude"]["mcpServers"]["memory"]["command"].as_str(),
        Some("node")
    );
    assert_eq!(
        gemini["providers"]["gemini"]["mcpServers"]["memory"]["command"].as_str(),
        Some("node")
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&shared_sidecar).unwrap())
            .unwrap()["providers"]["codex"]["mcpServers"]["memory"]["command"]
            .as_str(),
        Some("node")
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&legacy_sidecar).unwrap())
            .unwrap()["providers"]["codex"]["mcpServers"]["memory"]["command"]
            .as_str(),
        Some("node")
    );

    match original_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
}

#[test]
fn sync_mcp_configs_respects_explicit_codex_toml_override() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let original_home = std::env::var_os("HOME");
    std::env::set_var("HOME", &home);

    let src = tmp.path().join("source.mcp.json");
    let override_codex = tmp.path().join("custom_codex.toml");
    let claude_dst = tmp.path().join("claude.json");
    let gemini_dst = tmp.path().join("gemini_settings.json");

    std::fs::write(
        &src,
        serde_json::to_vec_pretty(&json!({
            "mcpServers": {
                "memory": {
                    "command": "node"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let overrides = ProviderRequestOverrides {
        mcp_json_path: Some(src),
        claude_json_path: Some(claude_dst),
        gemini_settings_path: Some(gemini_dst),
        codex_toml_path: Some(override_codex.clone()),
        ..Default::default()
    };

    sync_mcp_configs(None, Some(&overrides)).unwrap();

    assert!(override_codex.exists());
    assert!(coordinator_sidecar_path(&override_codex).exists());
    assert!(!home
        .join(".rust-chatbot")
        .join(".codex")
        .join("config.toml")
        .exists());

    match original_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
}

#[test]
fn sync_mcp_configs_writes_all_targets() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = std::env::temp_dir().join(format!("parity_bridge_sync_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let src = tmp.join("source.mcp.json");
    let claude_dst = tmp.join("claude.json");
    let gemini_dst = tmp.join("gemini_settings.json");
    let codex_dst = tmp.join("codex_config.toml");

    std::fs::write(
        &src,
        serde_json::to_vec_pretty(&json!({
            "mcpServers": {
                "test-srv": {
                    "command": "node",
                    "args": ["index.js"],
                    "env": {"PORT": "3000"}
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let overrides = ProviderRequestOverrides {
        mcp_json_path: Some(src),
        claude_json_path: Some(claude_dst.clone()),
        gemini_settings_path: Some(gemini_dst.clone()),
        codex_toml_path: Some(codex_dst.clone()),
        ..Default::default()
    };

    sync_mcp_configs(None, Some(&overrides)).unwrap();

    // Claude JSON written
    assert!(claude_dst.exists(), "Claude JSON not written");
    let claude: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&claude_dst).unwrap()).unwrap();
    assert!(claude["mcpServers"]
        .get(MCP_COORDINATOR_SERVER_NAME)
        .is_some());
    assert_eq!(
        claude["providers"]["claude"]["mcpServers"]["test-srv"]["command"].as_str(),
        Some("node")
    );

    let gemini: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&gemini_dst).unwrap()).unwrap();
    assert!(gemini["mcpServers"]
        .get(MCP_COORDINATOR_SERVER_NAME)
        .is_some());
    assert_eq!(
        gemini["providers"]["gemini"]["mcpServers"]["test-srv"]["env"]["PORT"].as_str(),
        Some("3000")
    );

    let codex_toml = std::fs::read_to_string(&codex_dst).unwrap();
    let codex_sidecar = coordinator_sidecar_path(&codex_dst);
    assert!(
        codex_toml.contains("[mcp_servers.\"mcp-coordinator\"]"),
        "missing mcp-coordinator section"
    );
    assert!(
        codex_toml.contains("MCP_COORDINATOR_PROVIDER"),
        "missing MCP_COORDINATOR_PROVIDER"
    );
    assert!(
        codex_toml.contains("MCP_COORDINATOR_UPSTREAM_CONFIG_PATH"),
        "missing upstream config path"
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&codex_sidecar).unwrap())
            .unwrap()["providers"]["codex"]["mcpServers"]["test-srv"]["args"][0]
            .as_str(),
        Some("index.js")
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn sync_mcp_configs_keeps_shell_mcps_for_host_execution() {
    let tmp = std::env::temp_dir().join(format!(
        "parity_bridge_host_shell_filter_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let src = tmp.join("source.mcp.json");
    let claude_dst = tmp.join("claude.json");
    let gemini_dst = tmp.join("gemini_settings.json");
    let codex_dst = tmp.join("codex_config.toml");

    std::fs::write(
        &src,
        serde_json::to_vec_pretty(&json!({
            "mcpServers": {
                "lightweight-builds": {
                    "command": "cargo",
                    "args": ["run", "--quiet", "--manifest-path", "/workspace/lightweight-mcp/Cargo.toml"]
                },
                "memory": { "command": "uvx", "args": ["memory-server"] },
                "mcp-filesystem": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
                },
                "bash": { "command": "npx", "args": ["-y", "bash-mcp"] },
                "mcp-server-commands": { "command": "npx", "args": ["mcp-server-commands"] }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let overrides = ProviderRequestOverrides {
        mcp_json_path: Some(src),
        claude_json_path: Some(claude_dst.clone()),
        gemini_settings_path: Some(gemini_dst.clone()),
        codex_toml_path: Some(codex_dst.clone()),
        ..Default::default()
    };

    sync_mcp_configs(None, Some(&overrides)).unwrap();

    let claude: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&claude_dst).unwrap()).unwrap();
    let gemini: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&gemini_dst).unwrap()).unwrap();
    let codex_sidecar: serde_json::Value =
        serde_json::from_slice(&std::fs::read(coordinator_sidecar_path(&codex_dst)).unwrap())
            .unwrap();

    for provider_servers in [
        &claude["providers"]["claude"]["mcpServers"],
        &gemini["providers"]["gemini"]["mcpServers"],
        &codex_sidecar["providers"]["codex"]["mcpServers"],
    ] {
        assert!(provider_servers.get("lightweight-builds").is_some());
        assert!(provider_servers.get("memory").is_some());
        assert!(provider_servers.get("mcp-filesystem").is_some());
        assert!(provider_servers.get("bash").is_some());
        assert!(provider_servers.get("mcp-server-commands").is_some());
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn sync_mcp_configs_strips_host_filesystem_and_shell_mcps_for_runsc() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = std::env::temp_dir().join(format!(
        "parity_bridge_runsc_filter_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let src = tmp.join("source.mcp.json");
    let claude_dst = tmp.join("claude.json");
    let gemini_dst = tmp.join("gemini_settings.json");
    let codex_dst = tmp.join("codex_config.toml");

    std::fs::write(
        &src,
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
                },
                "chatbot-memory": {
                    "command": "/workspace/rust-chatbot/target/release/chatbot-memory-mcp",
                    "env": {
                        "CHATBOT_MEMORY_MCP_ALLOWED_ROOTS": "/workspace"
                    }
                },
                "mcp-filesystem": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-filesystem", "/"]
                },
                "bash": { "command": "npx", "args": ["-y", "bash-mcp"] },
                "mcp-server-commands": { "command": "npx", "args": ["mcp-server-commands"] }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        &gemini_dst,
        serde_json::to_vec_pretty(&json!({
            "theme": "dark",
            "mcpServers": {
                "custom-safe": { "command": "uvx", "args": ["safe-server"] },
                "bash": { "command": "npx", "args": ["-y", "bash-mcp"] }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let overrides = ProviderRequestOverrides {
        mcp_json_path: Some(src),
        claude_json_path: Some(claude_dst.clone()),
        gemini_settings_path: Some(gemini_dst.clone()),
        codex_toml_path: Some(codex_dst.clone()),
        runsc_image: Some("runsc-image:latest".to_string()),
        ..Default::default()
    };

    sync_mcp_configs(None, Some(&overrides)).unwrap();

    let claude: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&claude_dst).unwrap()).unwrap();
    let gemini_effective_dst = resolved_gemini_settings_path(None, Some(&overrides));
    let gemini_sidecar = gemini_coordinator_sidecar_path(&gemini_effective_dst);
    let gemini: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&gemini_effective_dst).unwrap()).unwrap();
    let gemini_upstream: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&gemini_sidecar).unwrap()).unwrap();
    let codex_toml = std::fs::read_to_string(&codex_dst).unwrap();

    assert!(claude["mcpServers"]
        .get(MCP_COORDINATOR_SERVER_NAME)
        .is_some());
    assert!(claude["providers"]["claude"]["mcpServers"]
        .get("Context7")
        .is_some());
    assert!(claude["providers"]["claude"]["mcpServers"]
        .get("sequential-thinking")
        .is_some());
    assert!(claude["providers"]["claude"]["mcpServers"]
        .get("chrome-devtools")
        .is_some());
    assert!(claude["providers"]["claude"]["mcpServers"]
        .get("puppeteer")
        .is_some());
    assert!(claude["providers"]["claude"]["mcpServers"]
        .get("lightweight-builds")
        .is_some());
    assert!(claude["providers"]["claude"]["mcpServers"]
        .get("chatbot-memory")
        .is_some());
    assert!(claude["providers"]["claude"]["mcpServers"]
        .get("mcp-filesystem")
        .is_none());
    assert!(claude["providers"]["claude"]["mcpServers"]
        .get("bash")
        .is_none());
    assert!(claude["providers"]["claude"]["mcpServers"]
        .get("mcp-server-commands")
        .is_none());

    let gemini_servers = gemini_upstream["providers"]["gemini"]["mcpServers"]
        .as_object()
        .unwrap();
    assert!(gemini["mcpServers"]
        .get(MCP_COORDINATOR_SERVER_NAME)
        .is_some());
    assert!(gemini.get("providers").is_none());
    assert!(gemini_servers.get("Context7").is_some());
    assert!(gemini_servers.get("sequential-thinking").is_some());
    assert!(gemini_servers.get("chrome-devtools").is_some());
    assert!(gemini_servers.get("puppeteer").is_some());
    assert!(gemini_servers.get("lightweight-builds").is_some());
    assert!(gemini_servers.get("chatbot-memory").is_some());
    assert!(gemini_servers.get("mcp-filesystem").is_none());
    assert!(gemini_servers.get("bash").is_none());
    assert!(gemini_servers.get("mcp-server-commands").is_none());
    assert_eq!(gemini["theme"], json!("dark"));
    assert_ne!(gemini_effective_dst, gemini_dst);

    assert!(codex_toml.contains("[mcp_servers.\"mcp-coordinator\"]"));
    assert!(!codex_toml.contains("[mcp_servers.\"Context7\"]"));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn sync_mcp_configs_preserves_non_mcp_settings_and_folds_local_codex_servers_into_coordinator() {
    let tmp = std::env::temp_dir().join(format!(
        "parity_bridge_additive_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let src = tmp.join("source.mcp.json");
    let claude_dst = tmp.join("claude.json");
    let gemini_dst = tmp.join("gemini.json");
    let codex_dst = tmp.join("codex_config.toml");

    // Pre-populate codex config with non-MCP keys and local MCP servers that
    // should be routed through the coordinator instead of staying top-level.
    std::fs::write(
            &codex_dst,
            "model = \"gpt-5.4\"\nmodel_context_window = 1050000\n\n[mcp_servers.\"lightpanda\"]\ncommand = \"/tmp/lightpanda\"\nargs = [\"mcp\"]\n\n[mcp_servers.\"task-broker\"]\ncommand = \"/tmp/task-broker\"\nargs = []\n\n[mcp_servers.\"task-broker\".env]\nTASK_BROKER_ROOT = \"/tmp/root\"\n",
        )
        .unwrap();

    // Pre-populate Claude JSON with an unmanaged server
    std::fs::write(
        &claude_dst,
        serde_json::to_vec_pretty(&json!({
            "mcpServers": {
                "custom-safe": { "command": "existing" }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    std::fs::write(
        &src,
        serde_json::to_vec_pretty(&json!({
            "mcpServers": {
                "existing-srv": {
                    "command": "should-not-overwrite"
                },
                "new-srv": {
                    "command": "node",
                    "args": ["index.js"]
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let overrides = ProviderRequestOverrides {
        mcp_json_path: Some(src),
        claude_json_path: Some(claude_dst.clone()),
        gemini_settings_path: Some(gemini_dst.clone()),
        codex_toml_path: Some(codex_dst.clone()),
        ..Default::default()
    };

    sync_mcp_configs(None, Some(&overrides)).unwrap();

    // Codex: non-MCP keys preserved
    let codex_result = std::fs::read_to_string(&codex_dst).unwrap();
    assert!(
        codex_result.contains("model = \"gpt-5.4\""),
        "model key should be preserved"
    );
    assert!(
        codex_result.contains("model_context_window = 1050000"),
        "model_context_window should be preserved"
    );
    assert!(
        !codex_result.contains("[mcp_servers.\"lightpanda\"]"),
        "top-level lightpanda should be folded into coordinator"
    );
    assert!(
        !codex_result.contains("[mcp_servers.\"task-broker\"]"),
        "top-level task broker should be folded into coordinator"
    );
    assert!(
        codex_result.contains("[mcp_servers.\"mcp-coordinator\"]"),
        "coordinator MCP server should be written"
    );
    let codex_sidecar: serde_json::Value =
        serde_json::from_slice(&std::fs::read(coordinator_sidecar_path(&codex_dst)).unwrap())
            .unwrap();
    let codex_upstreams = codex_sidecar["providers"]["codex"]["mcpServers"]
        .as_object()
        .unwrap();
    assert!(codex_upstreams.get("lightpanda").is_some());
    assert_eq!(
        codex_upstreams["taskserver"]["env"]["TASK_BROKER_ROOT"].as_str(),
        Some("/tmp/root")
    );
    assert_eq!(
        codex_upstreams["taskserver"]["command"].as_str(),
        Some("/tmp/task-broker")
    );

    let claude: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&claude_dst).unwrap()).unwrap();
    assert_eq!(
        claude["mcpServers"]["custom-safe"]["command"].as_str(),
        Some("existing"),
        "Claude unmanaged server should be preserved"
    );
    assert!(claude["mcpServers"]
        .get(MCP_COORDINATOR_SERVER_NAME)
        .is_some());
    assert!(claude["mcpServers"].get("new-srv").is_none());
    assert_eq!(
        claude["providers"]["claude"]["mcpServers"]["existing-srv"]["command"].as_str(),
        Some("should-not-overwrite")
    );
    assert_eq!(
        claude["providers"]["claude"]["mcpServers"]["new-srv"]["command"].as_str(),
        Some("node")
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

// -- write_file_atomically ------------------------------------------------

#[test]
fn write_file_atomically_creates_and_overwrites() {
    let tmp = std::env::temp_dir().join(format!("parity_bridge_atomic_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let target = tmp.join("test.txt");

    write_file_atomically(&target, b"first").unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "first");

    write_file_atomically(&target, b"second").unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "second");

    let _ = std::fs::remove_dir_all(&tmp);
}
