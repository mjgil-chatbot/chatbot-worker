use super::*;
use crate::model::ModelProvider;
use crate::provider_bridge::{claude_compat_config, to_provider_session, ProviderRequestOverrides};
use ai_providers::config::{CodexCliConfig, GeminiCliConfig, QwenCodeCliConfig};
use std::fs;
use tempfile::tempdir;

use super::super::executor;

fn assert_only_provider_resume_id(
    session: &crate::data::ChatSession,
    provider: ModelProvider,
    expected: &str,
) {
    assert_eq!(
        provider.session_id_from_chat(session).map(String::as_str),
        Some(expected)
    );
    for other in ModelProvider::ALL {
        if other != provider {
            // Some providers share the same underlying session_id field
            // (e.g. GeminiForge/GeminiCliForge both use gemini_session_id),
            // so they will legitimately see the same value.
            let shares_field = provider.session_id_from_chat(session).is_some()
                && other.session_id_from_chat(session).is_some()
                && provider.session_id_from_chat(session) == other.session_id_from_chat(session);
            if shares_field {
                continue;
            }
            assert!(
                other.session_id_from_chat(session).is_none(),
                "expected {} to have no resume id, but found {:?}",
                other.as_str(),
                other.session_id_from_chat(session)
            );
        }
    }
}

fn assert_no_provider_resume_ids(session: &crate::data::ChatSession) {
    for provider in ModelProvider::ALL {
        assert!(provider.session_id_from_chat(session).is_none());
    }
}

// --- build_background_session resume paths ---

#[test]
fn build_background_session_sets_codex_resume_id() {
    let mut task = make_task("resume_test");
    task.resume_session_id = Some("codex-session-42".into());
    task.system_prompt = Some("be helpful".into());
    task.working_dir = Some("/tmp/work".into());

    let session = executor::build_background_session(&task, ModelProvider::Codex);
    assert_only_provider_resume_id(&session, ModelProvider::Codex, "codex-session-42");
    assert_eq!(session.system_prompt.as_deref(), Some("be helpful"));
    assert_eq!(session.launched_from.as_deref(), Some("/tmp/work"));
}

#[test]
fn build_background_session_sets_claude_resume_id() {
    let mut task = make_task("resume_claude");
    task.resume_session_id = Some("claude-sess-7".into());

    let session = executor::build_background_session(&task, ModelProvider::Claude);
    assert_only_provider_resume_id(&session, ModelProvider::Claude, "claude-sess-7");
}

#[test]
fn build_background_session_sets_gemini_resume_id() {
    let mut task = make_task("resume_gemini");
    task.resume_session_id = Some("gem-sess-3".into());

    let session = executor::build_background_session(&task, ModelProvider::Gemini);
    assert_only_provider_resume_id(&session, ModelProvider::Gemini, "gem-sess-3");
}

#[test]
fn build_background_session_sets_minimax_resume_id() {
    let mut task = make_task("resume_minimax");
    task.resume_session_id = Some("minimax-sess-5".into());

    let session = executor::build_background_session(&task, ModelProvider::MiniMax);
    assert_only_provider_resume_id(&session, ModelProvider::MiniMax, "minimax-sess-5");
}

#[test]
fn build_background_session_sets_qwen_resume_id() {
    let mut task = make_task("resume_qwen");
    task.resume_session_id = Some("qwen-sess-4".into());

    let session = executor::build_background_session(&task, ModelProvider::Qwen);
    assert_only_provider_resume_id(&session, ModelProvider::Qwen, "qwen-sess-4");
}

#[test]
fn build_background_session_sets_nvidia_resume_id() {
    let mut task = make_task("resume_nvidia");
    task.resume_session_id = Some("nvidia-sess-8".into());

    let session = executor::build_background_session(&task, ModelProvider::Nvidia);
    assert_only_provider_resume_id(&session, ModelProvider::Nvidia, "nvidia-sess-8");
}

#[test]
fn build_background_session_no_resume_leaves_all_ids_none() {
    let task = make_task("no_resume");
    let session = executor::build_background_session(&task, ModelProvider::Codex);
    assert_no_provider_resume_ids(&session);
}

#[test]
fn validate_claude_config_rejects_missing_directory() {
    let mut config = claude_compat_config(ModelProvider::Claude, None);
    config.allow_api_billing = true;
    config.config_dir = Some(tempdir().unwrap().path().join("missing"));
    config.require_mcp_servers = false;

    let err = executor::validate_claude_config(&config).expect_err("missing dir should fail");

    assert!(err.contains("Claude config directory not found"));
}

#[test]
fn validate_claude_config_rejects_missing_mcp_servers() {
    let config_dir = tempdir().unwrap();
    let mut config = claude_compat_config(ModelProvider::Claude, None);
    config.allow_api_billing = true;
    config.config_dir = Some(config_dir.path().to_path_buf());
    config.require_mcp_servers = true;

    let err =
        executor::validate_claude_config(&config).expect_err("missing provider config should fail");

    assert_eq!(err, "No MCP servers found in provider config.");
}

#[test]
fn build_claude_command_adds_prompt_flags_and_environment() {
    let config_dir = tempdir().unwrap();
    let mut task = make_task("finish the work");
    task.system_prompt = Some("system guidance".into());
    task.working_dir = Some(config_dir.path().display().to_string());
    let session = executor::build_background_session(&task, ModelProvider::Claude);
    let provider_session = to_provider_session(Some(&session));
    let mut config = claude_compat_config(ModelProvider::Claude, None);
    config.cli = "claude".to_string();
    config.config_dir = Some(config_dir.path().to_path_buf());
    config.extra_env = vec![("EXTRA".to_string(), "1".to_string())];
    config.skip_permissions = true;
    config.verbose_output = true;
    config.output_format = "stream-json".to_string();
    config.model = "claude-model".to_string();
    config.extra_env.push((
        "ANTHROPIC_REASONING_EFFORT".to_string(),
        "xhigh".to_string(),
    ));

    let command = executor::build_claude_command(&task, provider_session.as_ref(), None, &config);
    let args = command_args(&command);

    assert_eq!(command.get_program().to_string_lossy(), "claude");
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--append-system-prompt", "system guidance"]));
    assert!(args
        .iter()
        .any(|arg| arg == "--dangerously-skip-permissions"));
    assert!(args.iter().any(|arg| arg == "--verbose"));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--model", "claude-model"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--output-format", "stream-json"]));
    assert!(args.iter().any(|arg| arg == "finish the work"));
    assert_eq!(
        command_env_value(&command, "CLAUDE_CONFIG_DIR").as_deref(),
        Some(config_dir.path().to_string_lossy().as_ref())
    );
    assert_eq!(
        command_env_value(&command, "ANTHROPIC_REASONING_EFFORT").as_deref(),
        Some("xhigh")
    );
    assert_eq!(command_env_value(&command, "EXTRA").as_deref(), Some("1"));
}

#[test]
fn build_claude_command_uses_reasoning_effort_for_claude_provider() {
    let config_dir = tempdir().unwrap();
    let task = make_task("reasoning path check");
    let session = executor::build_background_session(&task, ModelProvider::Claude);
    let provider_session = to_provider_session(Some(&session));
    let mut config = claude_compat_config(ModelProvider::Claude, None);
    config.cli = "claude".to_string();
    config.config_dir = Some(config_dir.path().to_path_buf());

    let command = executor::build_claude_command(&task, provider_session.as_ref(), None, &config);

    assert_eq!(
        command_env_value(&command, "ANTHROPIC_REASONING_EFFORT").as_deref(),
        Some("high")
    );
}

#[test]
fn build_claude_command_uses_reasoning_effort_for_minimax_provider() {
    let config_dir = tempdir().unwrap();
    let task = make_task("reasoning path check");
    let session = executor::build_background_session(&task, ModelProvider::MiniMax);
    let provider_session = to_provider_session(Some(&session));
    let mut config = claude_compat_config(ModelProvider::MiniMax, None);
    config.cli = "claude".to_string();
    config.config_dir = Some(config_dir.path().to_path_buf());

    let command = executor::build_claude_command(&task, provider_session.as_ref(), None, &config);

    assert_eq!(
        command_env_value(&command, "ANTHROPIC_REASONING_EFFORT").as_deref(),
        Some("xhigh")
    );
}

#[test]
fn build_claude_command_resume_omits_append_system_prompt() {
    let mut task = make_task("resume prompt");
    task.system_prompt = Some("system guidance".into());
    let mut config = claude_compat_config(ModelProvider::Claude, None);
    config.cli = "claude".to_string();
    config.output_format = "json".to_string();
    config.model = "claude-model".to_string();
    config.require_mcp_servers = false;

    let command = executor::build_claude_command(&task, None, Some("claude-session"), &config);
    let args = command_args(&command);

    assert!(args
        .windows(2)
        .any(|pair| pair == ["--resume", "claude-session"]));
    assert!(!args.iter().any(|arg| arg == "--append-system-prompt"));
}

#[test]
fn build_claude_command_resume_keeps_claude_md_for_minimax() {
    let _test_env_lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let temp_home = tempdir().unwrap();
    let claude_dir = temp_home.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(claude_dir.join("CLAUDE.md"), "global minimax rule").unwrap();

    let previous_home = std::env::var_os("HOME");
    std::env::set_var("HOME", temp_home.path());

    let mut task = make_task("resume prompt");
    task.system_prompt = Some("session guidance".into());
    let mut config = claude_compat_config(ModelProvider::MiniMax, None);
    config.cli = "claude".to_string();
    config.output_format = "json".to_string();
    config.model = "MiniMax-M2.7".to_string();

    let command = executor::build_claude_command(&task, None, Some("mm-session"), &config);
    let args = command_args(&command);

    match previous_home {
        Some(home) => std::env::set_var("HOME", home),
        None => std::env::remove_var("HOME"),
    }

    assert!(args
        .windows(2)
        .any(|pair| pair == ["--resume", "mm-session"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--append-system-prompt", "global minimax rule"]));
    assert!(!args.iter().any(|arg| arg == "session guidance"));
}

#[test]
fn build_claude_command_includes_json_schema_flag() {
    let task = make_task("return structured output");
    let mut config = claude_compat_config(ModelProvider::Claude, None);
    config.cli = "claude".to_string();
    config.model = "claude-model".to_string();
    config.output_format = "stream-json".to_string();
    config.json_schema =
        Some("{\"type\":\"object\",\"properties\":{\"ok\":{\"type\":\"boolean\"}}}".to_string());

    let command = executor::build_claude_command(&task, None, None, &config);
    let args = command_args(&command);

    assert!(args.windows(2).any(|pair| {
        pair == [
            "--json-schema",
            "{\"type\":\"object\",\"properties\":{\"ok\":{\"type\":\"boolean\"}}}",
        ]
    }));
}

#[test]
fn build_provider_command_covers_minimax_path() {
    let task = make_task("provider branch");
    let session = executor::build_background_session(&task, ModelProvider::MiniMax);
    let provider_session = to_provider_session(Some(&session));
    let minimax_overrides = ProviderRequestOverrides {
        model: Some("minimax-custom".to_string()),
        anthropic_auth_token: Some("minimax-token".to_string()),
        config_dir: Some(tempdir().unwrap().path().to_path_buf()),
        ..Default::default()
    };

    let (minimax, _) = executor::build_provider_command(
        &task,
        ModelProvider::MiniMax,
        &minimax_overrides,
        provider_session.as_ref(),
    )
    .expect("minimax command should build");
    let minimax_args = command_args(&minimax);
    assert!(minimax_args
        .windows(2)
        .any(|pair| pair == ["--model", "minimax-custom"]));
}

#[test]
fn build_provider_command_rejects_missing_minimax_auth_token() {
    let task = make_task("provider branch");
    let overrides = ProviderRequestOverrides {
        anthropic_auth_token: Some(String::new()),
        ..Default::default()
    };

    let minimax_err =
        executor::build_provider_command(&task, ModelProvider::MiniMax, &overrides, None)
            .expect_err("minimax should require auth");
    assert!(minimax_err.contains(
        crate::config::api_key_env_for(ModelProvider::MiniMax)
            .expect("MiniMax catalog entry must define an api_key_env")
    ));
}

#[test]
fn build_provider_command_uses_nvidia_resume_id() {
    let _test_env_lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _env_lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    let nvidia_home = tempdir().unwrap();
    let _home = EnvVarGuard::set(
        crate::config::RUST_CHATBOT_NVIDIA_CODEX_HOME_ENV,
        nvidia_home.path(),
    );
    let _key = EnvVarGuard::set(
        crate::config::api_key_env_for(ModelProvider::Nvidia)
            .expect("Nvidia catalog entry must define an api_key_env"),
        "nvidia-token",
    );
    let mut nvidia_task = make_task("nvidia prompt");
    nvidia_task.resume_session_id = Some("nvidia-resume".into());
    let nvidia_session = executor::build_background_session(&nvidia_task, ModelProvider::Nvidia);
    let nvidia_provider_session = to_provider_session(Some(&nvidia_session));

    let (nvidia, _) = executor::build_provider_command(
        &nvidia_task,
        ModelProvider::Nvidia,
        &ProviderRequestOverrides::default(),
        nvidia_provider_session.as_ref(),
    )
    .expect("nvidia command should build");
    let nvidia_args = command_args(&nvidia);
    assert!(nvidia_args
        .windows(2)
        .any(|pair| pair == ["resume", "nvidia-resume"]));
}

#[test]
fn build_provider_command_covers_qwen_and_gemini_paths() {
    let _test_env_lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _env_lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let previous_openrouter = std::env::var_os("OPENROUTER_API_KEY");
    std::env::set_var("OPENROUTER_API_KEY", "test-openrouter-key");

    let settings_dir = tempdir().unwrap();
    let settings_path = settings_dir.path().join("settings.json");
    fs::write(&settings_path, "{}").unwrap();

    let mut qwen_task = make_task("qwen prompt");
    qwen_task.system_prompt = Some("session system".into());
    qwen_task.resume_session_id = Some("qwen-resume".into());
    let qwen_session = executor::build_background_session(&qwen_task, ModelProvider::Qwen);
    let qwen_provider_session = to_provider_session(Some(&qwen_session));
    let qwen_overrides = ProviderRequestOverrides {
        model: Some("qwen-custom".to_string()),
        ..Default::default()
    };
    let (qwen, _) = executor::build_provider_command(
        &qwen_task,
        ModelProvider::Qwen,
        &qwen_overrides,
        qwen_provider_session.as_ref(),
    )
    .expect("qwen command should build");
    let qwen_args = command_args(&qwen);
    assert!(qwen_args.iter().any(|arg| arg == "exec"));
    assert!(qwen_args
        .windows(2)
        .any(|pair| pair == ["--model", "qwen-custom"]));
    assert!(qwen_args
        .windows(2)
        .any(|pair| pair == ["--resume", "qwen-resume"]));

    let mut gemini_task = make_task("gemini prompt");
    gemini_task.system_prompt = Some("gemini system".into());
    gemini_task.resume_session_id = Some("gem-resume".into());
    let gemini_session = executor::build_background_session(&gemini_task, ModelProvider::Gemini);
    let gemini_provider_session = to_provider_session(Some(&gemini_session));
    let gemini_overrides = ProviderRequestOverrides {
        model: Some("gemini-custom".to_string()),
        gemini_settings_path: Some(settings_path.clone()),
        ..Default::default()
    };
    let (gemini, _) = executor::build_provider_command(
        &gemini_task,
        ModelProvider::Gemini,
        &gemini_overrides,
        gemini_provider_session.as_ref(),
    )
    .expect("gemini command should build");
    let gemini_args = command_args(&gemini);
    assert!(gemini_args
        .windows(2)
        .any(|pair| pair == ["--model", "gemini-custom"]));
    assert!(gemini_args
        .windows(2)
        .any(|pair| pair == ["--resume", "gem-resume"]));

    match previous_openrouter {
        Some(value) => std::env::set_var("OPENROUTER_API_KEY", value),
        None => std::env::remove_var("OPENROUTER_API_KEY"),
    }
}

#[test]
fn build_provider_command_passes_forge_provider_for_omniforge_openrouter_models() {
    let _test_env_lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _env_lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let _key = EnvVarGuard::set("OPENROUTER_API_KEY", "openrouter-token");

    let mut task = make_task("omniforge prompt");
    task.resume_session_id = Some("omni-resume".into());
    task.working_dir = Some("/tmp/omniforge".into());
    let session = executor::build_background_session(&task, ModelProvider::OmniForge);
    let provider_session = to_provider_session(Some(&session));
    let overrides = ProviderRequestOverrides {
        model: Some("minimax-m2.5-free".to_string()),
        ..Default::default()
    };

    let (command, _) = executor::build_provider_command(
        &task,
        ModelProvider::OmniForge,
        &overrides,
        provider_session.as_ref(),
    )
    .expect("omniforge command should build");
    let args = command_args(&command);

    assert!(args
        .windows(2)
        .any(|pair| pair == ["--forge-provider", "openrouter"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--model", "openrouter:minimax/minimax-m2.5:free"]));
}

#[test]
fn build_gemini_prompt_and_include_directories_cover_new_and_resume_sessions() {
    let _test_env_lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _env_lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let mut task = make_task("first line\nsecond line");
    task.system_prompt = Some("system guidance".into());

    let no_resume = executor::build_gemini_prompt(&task, None, false, None);
    assert!(no_resume.contains("System instructions:\nsystem guidance"));
    assert!(no_resume.contains("User:\nfirst line\nsecond line"));

    let mut resumed_task = task.clone();
    resumed_task.resume_session_id = Some("gem-resume".into());
    resumed_task.working_dir = Some("/tmp/gemini-session".into());
    let resumed_session = executor::build_background_session(&resumed_task, ModelProvider::Gemini);
    let resumed_provider_session = to_provider_session(Some(&resumed_session));
    let resumed_prompt = executor::build_gemini_prompt(
        &resumed_task,
        resumed_provider_session.as_ref(),
        false,
        None,
    );
    assert!(!resumed_prompt.contains("System instructions:"));
    assert!(resumed_prompt.contains("User:\nfirst line\nsecond line"));

    std::env::set_var("GEMINI_INCLUDE_DIRECTORIES", " /tmp/a , /tmp/b ,, ");
    let config = GeminiCliConfig {
        cli: "gemini".to_string(),
        model: None,
        output_format: "text".to_string(),
        approval_mode: None,
        include_directories: Vec::new(),
        project_settings_path: None,
        extra_env: Vec::new(),
        runsc_image: None,
        runsc_workspace_includes: Vec::new(),
        runsc_mounts: Vec::new(),
    };
    assert_eq!(
        executor::gemini_include_directories(resumed_provider_session.as_ref(), &config),
        vec!["/tmp/a".to_string(), "/tmp/b".to_string()]
    );
    std::env::remove_var("GEMINI_INCLUDE_DIRECTORIES");

    let root_only_config = GeminiCliConfig {
        include_directories: vec!["/".to_string()],
        ..config
    };
    assert_eq!(
        executor::gemini_include_directories(resumed_provider_session.as_ref(), &root_only_config),
        vec!["/tmp/gemini-session".to_string()]
    );
}

#[test]
fn build_gemini_command_sets_project_settings_env_and_resume() {
    let settings_dir = tempdir().unwrap();
    let settings_path = settings_dir.path().join("settings.json");
    fs::write(&settings_path, "{}").unwrap();
    let mut task = make_task("gemini body");
    task.system_prompt = Some("gemini system".into());
    task.resume_session_id = Some("gem-session".into());
    let session = executor::build_background_session(&task, ModelProvider::Gemini);
    let provider_session = to_provider_session(Some(&session));
    let config = GeminiCliConfig {
        cli: "gemini".to_string(),
        model: Some("gem-model".to_string()),
        output_format: "stream-json".to_string(),
        approval_mode: Some("yolo".to_string()),
        include_directories: vec!["/tmp/include".to_string()],
        project_settings_path: Some(settings_path.clone()),
        extra_env: Vec::new(),
        runsc_image: None,
        runsc_workspace_includes: Vec::new(),
        runsc_mounts: Vec::new(),
    };

    let command = executor::build_gemini_command(&task, provider_session.as_ref(), &config);
    let args = command_args(&command);

    assert!(args
        .windows(2)
        .any(|pair| pair == ["--output-format", "stream-json"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--include-directories", "/tmp/include"]));
    assert!(args.windows(2).any(|pair| pair == ["--model", "gem-model"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--approval-mode", "yolo"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--resume", "gem-session"]));
    assert_eq!(
        command_env_value(&command, "GEMINI_CLI_PROJECT_SETTINGS_PATH").as_deref(),
        Some(settings_path.to_string_lossy().as_ref())
    );
}

#[test]
fn build_qwen_command_covers_system_prompt_and_resume_precedence() {
    let mut task = make_task("qwen body");
    task.system_prompt = Some("session system".into());
    let config = QwenCodeCliConfig {
        cli: "qwen".to_string(),
        model: Some("qwen-model".to_string()),
        output_format: "stream-json".to_string(),
        yolo: true,
        append_system_prompt: Some("default system".to_string()),
        resume_session_id: Some("config-resume".to_string()),
        include_directories: vec!["/tmp/include".to_string()],
        allowed_mcp_server_names: vec!["memory".to_string(), "filesystem".to_string()],
        auth_type: Some("oauth".to_string()),
        extra_env: vec![("EXTRA".to_string(), "1".to_string())],
        runsc_image: None,
        runsc_workspace_includes: Vec::new(),
        runsc_mounts: Vec::new(),
    };

    let command = executor::build_qwen_command(&task, None, &config);
    let args = command_args(&command);
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--output-format", "stream-json"]));
    assert!(args.iter().any(|arg| arg == "--yolo"));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--model", "qwen-model"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--resume", "config-resume"]));
    assert!(args.windows(2).any(|pair| pair == ["--auth-type", "oauth"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--include-directories", "/tmp/include"]));
    assert!(args
        .windows(2)
        .any(|pair| { pair == ["--allowed-mcp-server-names", "memory,filesystem"] }));
    assert!(args
        .windows(2)
        .any(|pair| { pair == ["--append-system-prompt", "default system\n\nsession system"] }));
    assert_eq!(command_env_value(&command, "EXTRA").as_deref(), Some("1"));

    let mut resumed_task = task.clone();
    resumed_task.resume_session_id = Some("session-resume".into());
    let resumed_session = executor::build_background_session(&resumed_task, ModelProvider::Qwen);
    let resumed_provider_session = to_provider_session(Some(&resumed_session));
    let resumed =
        executor::build_qwen_command(&resumed_task, resumed_provider_session.as_ref(), &config);
    let resumed_args = command_args(&resumed);
    assert!(resumed_args
        .windows(2)
        .any(|pair| pair == ["--resume", "session-resume"]));
    assert!(!resumed_args
        .iter()
        .any(|arg| arg == "--append-system-prompt"));
}

#[test]
fn build_qwen_command_prepends_cli_directory_to_path() {
    let _test_env_lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _env_lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    let original_path = std::env::var_os("PATH");
    std::env::set_var("PATH", "/usr/bin:/bin");

    let cli_root = tempdir().unwrap();
    let cli_dir = cli_root.path().join("bin");
    fs::create_dir_all(&cli_dir).unwrap();
    let cli_path = cli_dir.join("qwen");
    fs::write(&cli_path, "#!/usr/bin/env node\n").unwrap();

    let config = QwenCodeCliConfig {
        cli: cli_path.to_string_lossy().into_owned(),
        model: None,
        output_format: "stream-json".to_string(),
        yolo: true,
        append_system_prompt: None,
        resume_session_id: None,
        include_directories: Vec::new(),
        allowed_mcp_server_names: Vec::new(),
        auth_type: None,
        extra_env: Vec::new(),
        runsc_image: None,
        runsc_workspace_includes: Vec::new(),
        runsc_mounts: Vec::new(),
    };

    let command = executor::build_qwen_command(&make_task("qwen body"), None, &config);
    let path = command_env_value(&command, "PATH").expect("PATH should be set");
    assert!(path.starts_with(&format!("{}:", cli_dir.display())));

    match original_path {
        Some(value) => std::env::set_var("PATH", value),
        None => std::env::remove_var("PATH"),
    }
}

#[test]
fn build_codex_command_prepends_cli_directory_to_path() {
    let _test_env_lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _env_lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    let original_path = std::env::var_os("PATH");
    std::env::set_var("PATH", "/usr/bin:/bin");

    let cli_root = tempdir().unwrap();
    let cli_dir = cli_root.path().join("bin");
    fs::create_dir_all(&cli_dir).unwrap();
    let cli_path = cli_dir.join("codex");
    fs::write(&cli_path, "#!/usr/bin/env node\n").unwrap();

    let config = CodexCliConfig {
        cli: cli_path.to_string_lossy().into_owned(),
        model: None,
        output_schema: None,
        json_stream: true,
        reasoning_effort: None,
        sandbox_mode: None,
        force_skip_git_repo_check: false,
        skip_git_repo_check_prefixes: Vec::new(),
        extra_env: Vec::new(),
        runsc_image: None,
        runsc_workspace_includes: Vec::new(),
        runsc_mounts: Vec::new(),
    };

    let command = executor::build_codex_command(
        ModelProvider::Codex,
        &make_task("codex body"),
        None,
        &config,
    );
    let path = command_env_value(&command, "PATH").expect("PATH should be set");
    assert!(path.starts_with(&format!("{}:", cli_dir.display())));

    match original_path {
        Some(value) => std::env::set_var("PATH", value),
        None => std::env::remove_var("PATH"),
    }
}

#[test]
fn build_codex_prompt_covers_resume_and_system_prompt_branches() {
    let mut task = make_task("user body");
    task.system_prompt = Some("system guidance".into());

    let prompt = executor::build_codex_prompt(&task, false, None);
    assert!(prompt.contains("System instructions:\nsystem guidance\n\nUser: user body"));

    task.resume_session_id = Some("resume".into());
    assert_eq!(
        executor::build_codex_prompt(&task, false, None),
        "user body"
    );
}

#[test]
fn rewrite_runsc_workspace_paths_rewrites_absolute_workspace_mentions() {
    let text =
        "Inspect /tmp/workspace and /tmp/workspace/instructions plus /tmp/workspace/plan.txt";
    let rewritten = executor::rewrite_runsc_workspace_paths(text, true, Some("/tmp/workspace"));
    assert_eq!(rewritten, "Inspect . and ./instructions plus ./plan.txt");
}

#[test]
fn rewrite_runsc_workspace_paths_preserves_sibling_and_embedded_paths() {
    let text =
        "Keep /tmp/workspace-other and prefix/tmp/workspace, but inspect /tmp/workspace/file.txt";
    let rewritten = executor::rewrite_runsc_workspace_paths(text, true, Some("/tmp/workspace"));
    assert_eq!(
        rewritten,
        "Keep /tmp/workspace-other and prefix/tmp/workspace, but inspect ./file.txt"
    );
}

#[test]
fn rewrite_runsc_workspace_paths_rewrites_trailing_separator_working_dir() {
    let text = "Inspect /tmp/workspace/file.txt from /tmp/workspace";
    let rewritten = executor::rewrite_runsc_workspace_paths(text, true, Some("/tmp/workspace/"));
    assert_eq!(rewritten, "Inspect ./file.txt from .");
}

#[test]
fn build_codex_command_skips_git_repo_check_in_runsc_mode() {
    let task = make_task("user body");
    let config = CodexCliConfig {
        cli: "codex".to_string(),
        model: Some("gpt-5.4".to_string()),
        output_schema: None,
        json_stream: true,
        reasoning_effort: None,
        sandbox_mode: None,
        force_skip_git_repo_check: false,
        skip_git_repo_check_prefixes: Vec::new(),
        extra_env: Vec::new(),
        runsc_image: Some("runsc-image:latest".to_string()),
        runsc_workspace_includes: Vec::new(),
        runsc_mounts: Vec::new(),
    };

    let command = executor::build_codex_command(ModelProvider::Codex, &task, None, &config);
    let args = command_args(&command);
    assert!(args.iter().any(|arg| arg == "--skip-git-repo-check"));
}

#[test]
fn build_codex_command_skips_git_repo_check_by_default() {
    let task = make_task("user body");
    let config = CodexCliConfig {
        cli: "codex".to_string(),
        model: Some("gpt-5.4".to_string()),
        output_schema: None,
        json_stream: true,
        reasoning_effort: None,
        sandbox_mode: None,
        force_skip_git_repo_check: false,
        skip_git_repo_check_prefixes: Vec::new(),
        extra_env: Vec::new(),
        runsc_image: None,
        runsc_workspace_includes: Vec::new(),
        runsc_mounts: Vec::new(),
    };

    let command = executor::build_codex_command(ModelProvider::Codex, &task, None, &config);
    let args = command_args(&command);
    assert!(args.iter().any(|arg| arg == "--skip-git-repo-check"));
}

#[test]
fn build_codex_command_includes_output_schema_flag() {
    let task = make_task("return structured output");
    let config = CodexCliConfig {
        cli: "codex".to_string(),
        model: Some("gpt-5.4".to_string()),
        output_schema: Some(std::path::PathBuf::from("/tmp/contract-schema.json")),
        json_stream: true,
        reasoning_effort: None,
        sandbox_mode: None,
        force_skip_git_repo_check: false,
        skip_git_repo_check_prefixes: Vec::new(),
        extra_env: Vec::new(),
        runsc_image: None,
        runsc_workspace_includes: Vec::new(),
        runsc_mounts: Vec::new(),
    };

    let command = executor::build_codex_command(ModelProvider::Codex, &task, None, &config);
    let args = command_args(&command);

    assert!(args
        .windows(2)
        .any(|pair| pair == ["--output-schema", "/tmp/contract-schema.json"]));
}
