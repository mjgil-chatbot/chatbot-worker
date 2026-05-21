use super::*;

// ---- Round-trip --------------------------------------------------------

#[test]
fn app_config_serialize_deserialize_roundtrip() {
    let dir = temp_dir();
    let toml_content = r#"
[provider]
default_provider = "qwen"
claude_reasoning_effort = "high"
codex_reasoning_effort = "medium"
minimax_reasoning_effort = "xhigh"
codex_sandbox_mode = "workspace-write"
gemini_approval_mode = "auto_edit"
allow_api_billing = true
max_concurrency = 6

[ui]
window_width = 1400.0
window_height = 900.0
min_window_width = 1024.0
side_panel_width = 250.0
message_max_width = 800.0

[memory]
enable_workspace_memory = false
memmachine_base_url = "http://127.0.0.1:8080"
memmachine_org_id = "serialization-tests"
memory_top_k = 20

[coordination]
task_lease_secs = 90
task_heartbeat_secs = 10
task_max_attempts = 5
max_agents = 15
max_iterations = 25
allow_write_tasks = false
require_review_before_apply = false
design_doc_path = "docs/custom-design.md"
background_design_doc_path = "docs/custom-autonomous.md"
background_max_concurrent = 10
"#;
    let path = write_toml(&dir, "chatbot-config.toml", toml_content);
    let (cfg, _) = AppConfig::load_from(&[path]);

    assert_eq!(cfg.provider.default_provider, "qwen");
    assert_eq!(
        cfg.provider.codex_reasoning_effort,
        Some("medium".to_string())
    );
    assert_eq!(
        cfg.provider.claude_reasoning_effort,
        Some("high".to_string())
    );
    assert_eq!(
        cfg.provider.minimax_reasoning_effort,
        Some("xhigh".to_string())
    );
    assert_eq!(
        cfg.provider.codex_sandbox_mode,
        Some("workspace-write".to_string())
    );
    assert_eq!(
        cfg.provider.gemini_approval_mode,
        Some("auto_edit".to_string())
    );
    assert_eq!(cfg.provider.allow_api_billing, Some(true));
    assert_eq!(cfg.provider.max_concurrency, Some(6));
    assert_eq!(cfg.ui.window_width, 1400.0);
    assert_eq!(cfg.ui.window_height, 900.0);
    assert_eq!(cfg.ui.min_window_width, 1024.0);
    assert_eq!(cfg.ui.side_panel_width, 250.0);
    assert_eq!(cfg.ui.message_max_width, 800.0);
    assert!(!cfg.memory.enable_workspace_memory);
    assert_eq!(
        cfg.memory.memmachine_base_url,
        Some("http://127.0.0.1:8080".to_string())
    );
    assert_eq!(
        cfg.memory.memmachine_org_id,
        Some("serialization-tests".to_string())
    );
    assert_eq!(cfg.memory.memory_top_k, Some(20));
    assert_eq!(cfg.background.task_lease_secs, 90);
    assert_eq!(cfg.background.task_heartbeat_secs, 10);
    assert_eq!(cfg.background.task_max_attempts, 5);
    assert_eq!(cfg.background.max_agents, 15);
    assert_eq!(cfg.background.max_iterations, 25); // This test uses explicit config value
    assert!(!cfg.background.allow_write_tasks);
    assert!(!cfg.background.require_review_before_apply);
    assert_eq!(
        cfg.background.design_doc_path,
        Some("docs/custom-design.md".to_string())
    );
    assert_eq!(
        cfg.background.background_design_doc_path,
        "docs/custom-autonomous.md"
    );
    assert_eq!(cfg.background.background_max_concurrent, 10);
}

// ---- Config search path ------------------------------------------------

#[test]
fn config_search_path_prefers_home_then_checks_legacy_repo_paths() {
    let search = AppConfig::config_search_path();
    assert!(!search.is_empty());
    assert!(search[0]
        .to_string_lossy()
        .contains(".rust-chatbot/config.toml"));
    assert!(search
        .iter()
        .skip(1)
        .any(|path| path.ends_with("chatbot-config.toml")));
}

// ---- Prompt / command helpers -----------------------------------------

#[test]
fn test_auto_ui_launch_background_requested() {
    std::env::remove_var(AUTO_UI_LAUNCH_BACKGROUND_ENV);
    assert!(!auto_ui_launch_background_requested());

    std::env::set_var(AUTO_UI_LAUNCH_BACKGROUND_ENV, "1");
    assert!(auto_ui_launch_background_requested());

    std::env::set_var(AUTO_UI_LAUNCH_BACKGROUND_ENV, "0");
    assert!(!auto_ui_launch_background_requested());

    std::env::remove_var(AUTO_UI_LAUNCH_BACKGROUND_ENV);
}

// ---- qwen_code_cli_path resolution ------------------------------------

#[test]
fn test_qwen_code_cli_path_env_override() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original = std::env::var_os(RUST_CHATBOT_QWEN_CODE_CLI_ENV);
    std::env::set_var(
        RUST_CHATBOT_QWEN_CODE_CLI_ENV,
        "/custom/path/openrouter-cli",
    );
    reset_config();
    assert_eq!(qwen_code_cli_path(), "/custom/path/openrouter-cli");
    match original {
        Some(v) => std::env::set_var(RUST_CHATBOT_QWEN_CODE_CLI_ENV, v),
        None => std::env::remove_var(RUST_CHATBOT_QWEN_CODE_CLI_ENV),
    }
    reset_config();
}

#[test]
fn test_qwen_code_cli_path_default_is_openrouter_cli() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original_home = std::env::var_os("HOME");
    let original_path = std::env::var_os("PATH");
    std::env::remove_var(RUST_CHATBOT_QWEN_CODE_CLI_ENV);
    let tmp = temp_dir();
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("PATH", tmp.path().join("bin"));
    reset_config();
    let path = qwen_code_cli_path();
    assert_eq!(path, "openrouter-cli");
    match original_home {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
    }
    match original_path {
        Some(v) => std::env::set_var("PATH", v),
        None => std::env::remove_var("PATH"),
    }
    reset_config();
}

#[test]
fn test_qwen_code_cli_path_empty_env_gives_default() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original_home = std::env::var_os("HOME");
    let original_path = std::env::var_os("PATH");
    std::env::set_var(RUST_CHATBOT_QWEN_CODE_CLI_ENV, "");
    let tmp = temp_dir();
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("PATH", tmp.path().join("bin"));
    reset_config();
    let path = qwen_code_cli_path();
    assert_eq!(path, "openrouter-cli");
    std::env::remove_var(RUST_CHATBOT_QWEN_CODE_CLI_ENV);
    match original_home {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
    }
    match original_path {
        Some(v) => std::env::set_var("PATH", v),
        None => std::env::remove_var("PATH"),
    }
    reset_config();
}

#[test]
fn test_qwen_code_cli_path_resolves_absolute_binary_from_path() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original_home = std::env::var_os("HOME");
    let original_path = std::env::var_os("PATH");
    std::env::remove_var(RUST_CHATBOT_QWEN_CODE_CLI_ENV);
    let tmp = temp_dir();
    let bin_dir = tmp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let openrouter_cli = bin_dir.join("openrouter-cli");
    std::fs::write(&openrouter_cli, "#!/bin/sh\nexit 0\n").unwrap();
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("PATH", &bin_dir);
    reset_config();
    assert_eq!(
        qwen_code_cli_path(),
        openrouter_cli.to_string_lossy().to_string()
    );
    match original_home {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
    }
    match original_path {
        Some(v) => std::env::set_var("PATH", v),
        None => std::env::remove_var("PATH"),
    }
    reset_config();
}

#[test]
fn test_qwen_code_cli_path_falls_back_to_asdf_install() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original_home = std::env::var_os("HOME");
    let original_path = std::env::var_os("PATH");
    std::env::remove_var(RUST_CHATBOT_QWEN_CODE_CLI_ENV);
    let tmp = temp_dir();
    let openrouter_cli = tmp
        .path()
        .join(".asdf")
        .join("installs")
        .join("nodejs")
        .join("22.15.0")
        .join("bin")
        .join("openrouter-cli");
    std::fs::create_dir_all(openrouter_cli.parent().unwrap()).unwrap();
    std::fs::write(&openrouter_cli, "#!/bin/sh\nexit 0\n").unwrap();
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("PATH", tmp.path().join("missing-bin"));
    reset_config();
    assert_eq!(
        qwen_code_cli_path(),
        openrouter_cli.to_string_lossy().to_string()
    );
    match original_home {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
    }
    match original_path {
        Some(v) => std::env::set_var("PATH", v),
        None => std::env::remove_var("PATH"),
    }
    reset_config();
}
