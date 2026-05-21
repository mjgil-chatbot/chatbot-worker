use super::*;

// ---- Defaults ----------------------------------------------------------

#[test]
fn app_config_defaults_when_no_file() {
    reset_config();
    let cfg = AppConfig::load().0;
    // All fields fall back to defaults
    assert_eq!(cfg.provider.default_provider, "codex");
    assert_eq!(cfg.provider.claude_reasoning_effort, None);
    assert_eq!(cfg.provider.codex_reasoning_effort, None);
    assert_eq!(cfg.provider.minimax_reasoning_effort, None);
    assert_eq!(cfg.provider.codex_sandbox_mode, None);
    assert_eq!(cfg.provider.gemini_approval_mode, None);
    assert_eq!(cfg.ui.window_width, 1000.0);
    assert_eq!(cfg.ui.window_height, 600.0);
    assert!(cfg.memory.enable_workspace_memory);
    assert_eq!(cfg.background.task_lease_secs, 30);
    assert_eq!(cfg.background.task_heartbeat_secs, 5);
    assert_eq!(cfg.background.task_max_attempts, 3);
    assert_eq!(cfg.background.max_agents, 5);
    assert_eq!(cfg.background.max_iterations, u32::MAX);
    assert!(cfg.background.allow_write_tasks);
    assert!(cfg.background.require_review_before_apply);
    assert_eq!(cfg.background.background_max_concurrent, 4);
}

#[test]
fn app_config_load_returns_none_path_when_no_file() {
    reset_config();
    let (_cfg, path) = AppConfig::load();
    assert!(path.is_none());
}

#[test]
fn app_config_load_returns_some_path_when_file_exists() {
    let dir = temp_dir();
    let toml_content = r#"
[provider]
default_provider = "minimax"
"#;
    write_toml(&dir, "chatbot-config.toml", toml_content);
    // Patch the search path so we only look in the temp dir
    let path = dir.path().join("chatbot-config.toml");
    let (cfg, loaded_path) = AppConfig::load_from(&[path.clone()]);
    assert!(loaded_path.is_some());
    assert_eq!(loaded_path.unwrap(), path);
    assert_eq!(cfg.provider.default_provider, "minimax");
}

// ---- Partial override --------------------------------------------------

#[test]
fn partial_override_merges_with_defaults() {
    let dir = temp_dir();
    let toml_content = r#"
[provider]
default_provider = "codex"

[coordination]
max_iterations = 20
allow_write_tasks = false
"#;
    let path = write_toml(&dir, "chatbot-config.toml", toml_content);
    let (cfg, _) = AppConfig::load_from(&[path]);

    // Overridden
    assert_eq!(cfg.provider.default_provider, "codex");
    assert_eq!(cfg.background.max_iterations, 20);
    assert!(!cfg.background.allow_write_tasks);
    // Preserved defaults
    assert_eq!(cfg.ui.window_width, 1000.0);
    assert_eq!(cfg.ui.window_height, 600.0);
    assert!(cfg.memory.enable_workspace_memory);
    assert_eq!(cfg.background.task_lease_secs, 30);
    assert_eq!(cfg.background.task_heartbeat_secs, 5);
}

#[test]
fn partial_override_ui_section() {
    let dir = temp_dir();
    let toml_content = r#"
[ui]
window_width = 1200.0
message_max_width = 800.0
"#;
    let path = write_toml(&dir, "chatbot-config.toml", toml_content);
    let (cfg, _) = AppConfig::load_from(&[path]);

    assert_eq!(cfg.ui.window_width, 1200.0);
    assert_eq!(cfg.ui.message_max_width, 800.0);
    // Preserved
    assert_eq!(cfg.ui.window_height, 600.0);
    assert_eq!(cfg.ui.min_window_width, 980.0);
}

#[test]
fn partial_override_memory_section() {
    let dir = temp_dir();
    let toml_content = r#"
[memory]
enable_workspace_memory = false
memory_top_k = 16
"#;
    let path = write_toml(&dir, "chatbot-config.toml", toml_content);
    let (cfg, _) = AppConfig::load_from(&[path]);

    assert!(!cfg.memory.enable_workspace_memory);
    assert_eq!(cfg.memory.memory_top_k, Some(16));
}

#[test]
fn partial_override_provider_knobs() {
    let dir = temp_dir();
    let toml_content = r#"
[provider]
claude_reasoning_effort = "high"
codex_reasoning_effort = "low"
minimax_reasoning_effort = "xhigh"
codex_sandbox_mode = "read-only"
gemini_approval_mode = "plan"
allow_api_billing = true
max_concurrency = 8
"#;
    let path = write_toml(&dir, "chatbot-config.toml", toml_content);
    let (cfg, _) = AppConfig::load_from(&[path]);

    assert_eq!(
        cfg.provider.claude_reasoning_effort,
        Some("high".to_string())
    );
    assert_eq!(cfg.provider.codex_reasoning_effort, Some("low".to_string()));
    assert_eq!(
        cfg.provider.minimax_reasoning_effort,
        Some("xhigh".to_string())
    );
    assert_eq!(
        cfg.provider.codex_sandbox_mode,
        Some("read-only".to_string())
    );
    assert_eq!(cfg.provider.gemini_approval_mode, Some("plan".to_string()));
    assert_eq!(cfg.provider.allow_api_billing, Some(true));
    assert_eq!(cfg.provider.max_concurrency, Some(8));
}

#[test]
fn partial_override_coord_section() {
    let dir = temp_dir();
    let toml_content = r#"
[coordination]
task_lease_secs = 60
task_max_attempts = 5
max_agents = 10
design_doc_path = "docs/my-design.md"
background_max_concurrent = 8
"#;
    let path = write_toml(&dir, "chatbot-config.toml", toml_content);
    let (cfg, _) = AppConfig::load_from(&[path]);

    assert_eq!(cfg.background.task_lease_secs, 60);
    assert_eq!(cfg.background.task_max_attempts, 5);
    assert_eq!(cfg.background.max_agents, 10);
    assert_eq!(
        cfg.background.design_doc_path,
        Some("docs/my-design.md".to_string())
    );
    assert_eq!(cfg.background.background_max_concurrent, 8);
}

// ---- Invalid config errors ----------------------------------------------

#[test]
fn invalid_toml_produces_error() {
    let dir = temp_dir();
    let toml_content = "not valid toml = [}";
    let path = dir.path().join("chatbot-config.toml");
    std::fs::write(&path, toml_content).unwrap();
    let result: Result<AppConfig, _> = AppConfig::from_toml_path(&path);
    assert!(result.is_err());
}

#[test]
fn unknown_field_in_toml_is_ignored() {
    // serde ignores unknown fields by default
    let dir = temp_dir();
    let toml_content = r#"
unknown_section = "ignored"
[provider]
default_provider = "kimi"
"#;
    let path = write_toml(&dir, "chatbot-config.toml", toml_content);
    let (cfg, _) = AppConfig::load_from(&[path]);
    assert_eq!(cfg.provider.default_provider, "kimi");
}

#[test]
fn config_path_env_override_is_exclusive_search_entry() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_dir();
    let override_path = write_toml(
        &dir,
        "override.toml",
        r#"
[provider]
default_provider = "minimax"
max_concurrency = 17
"#,
    );

    let prev = std::env::var_os(CONFIG_PATH_OVERRIDE_ENV);
    std::env::set_var(CONFIG_PATH_OVERRIDE_ENV, &override_path);

    let paths = AppConfig::config_search_path();
    assert_eq!(
        paths,
        vec![override_path.clone()],
        "env override must be the exclusive search entry — repo-root and home paths must not be consulted",
    );

    let (cfg, used) = AppConfig::load();
    assert_eq!(used.as_deref(), Some(override_path.as_path()));
    assert_eq!(cfg.provider.default_provider, "minimax");
    assert_eq!(cfg.provider.max_concurrency, Some(17));

    match prev {
        Some(v) => std::env::set_var(CONFIG_PATH_OVERRIDE_ENV, v),
        None => std::env::remove_var(CONFIG_PATH_OVERRIDE_ENV),
    }
}

#[test]
fn config_path_env_override_pointing_at_missing_file_falls_back_to_defaults() {
    // When the override is set but the file is missing, we do NOT silently
    // fall back to the default home or legacy repo config — the explicit pin is honored
    // strictly. The result is the all-defaults config.
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_dir();
    let missing = dir.path().join("does-not-exist.toml");

    let prev = std::env::var_os(CONFIG_PATH_OVERRIDE_ENV);
    std::env::set_var(CONFIG_PATH_OVERRIDE_ENV, &missing);

    let (cfg, used) = AppConfig::load();
    assert!(
        used.is_none(),
        "missing override file must produce a None loaded-path, not silently fall through"
    );
    assert_eq!(cfg.provider.default_provider, "codex");

    match prev {
        Some(v) => std::env::set_var(CONFIG_PATH_OVERRIDE_ENV, v),
        None => std::env::remove_var(CONFIG_PATH_OVERRIDE_ENV),
    }
}

#[test]
fn completely_empty_toml_file_loads_defaults() {
    let dir = temp_dir();
    let toml_content = "";
    let path = dir.path().join("chatbot-config.toml");
    std::fs::write(&path, toml_content).unwrap();
    let result: Result<AppConfig, _> = AppConfig::from_toml_path(&path);
    // Empty file should be treated as defaults
    assert!(result.is_ok());
}
