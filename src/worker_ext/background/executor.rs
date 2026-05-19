pub(super) use super::failure::provider_failure_message;
#[cfg(test)]
pub(super) use super::failure::{classify_provider_output, is_provider_failure};
use super::openrouter_prompt::build_openrouter_prompt;
use super::session_ids::{
    has_resume as provider_has_resume, resume_session_id as provider_resume_session_id,
};
use super::types::*;
use crate::cgroup::wrap_command;
use crate::config;
use crate::data::ChatSession;
use crate::model::ModelProvider;
use crate::provider_bridge::{
    background_provider_overrides, claude_compat_config, codex_compat_config, gemini_cli_config,
    sync_provider_mcp_configs, to_provider_session, ProviderRequestOverrides,
};
use crate::summary::JsonSummary;
use ai_providers::claude::cli::{effective_claude_system_prompt, get_subscription_type};
#[cfg(test)]
use ai_providers::config::QwenCodeCliConfig;
use ai_providers::config::{
    add_prompt_arg, has_mcp_servers, ClaudeCliConfig, CodexCliConfig, GeminiCliConfig,
};
use ai_providers::sandbox::wrap_command_with_runsc;
use ai_providers::session::ProviderSession;
use ai_providers::workspace;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[cfg(test)]
#[path = "executor/qwen_command.rs"]
mod qwen_command;
#[cfg(test)]
pub(crate) use qwen_command::build_qwen_command;

pub fn provider_task_executor(stop_signal: Arc<AtomicBool>) -> Arc<BackgroundTaskExecutor> {
    Arc::new(move |task| {
        let stop_signal = Arc::clone(&stop_signal);
        Box::pin(async move {
            let task_id = task.id;
            match tokio::task::spawn_blocking(move || execute_provider_task(task, stop_signal))
                .await
            {
                Ok(result) => result,
                Err(err) => BackgroundResult {
                    id: task_id,
                    output: Err(format!("background task failed to join: {err}")),
                },
            }
        })
    })
}

fn execute_provider_task(task: BackgroundTask, stop_signal: Arc<AtomicBool>) -> BackgroundResult {
    let configured_default_provider = config::config().provider.default_provider;
    let provider = match resolve_task_provider(
        task.agent_profile.provider.as_deref(),
        &configured_default_provider,
    ) {
        Some(provider) => provider,
        None => {
            let provider_name = task
                .agent_profile
                .provider
                .as_deref()
                .unwrap_or(configured_default_provider.as_str());
            return BackgroundResult {
                id: task.id,
                output: Err(format!("Unknown provider: {provider_name}")),
            };
        }
    };

    let overrides = background_provider_overrides(provider);
    BackgroundResult {
        id: task.id,
        output: execute_provider_subprocess(&task, provider, &overrides, stop_signal),
    }
}

pub(super) fn resolve_task_provider(
    provider_name: Option<&str>,
    configured_default_provider: &str,
) -> Option<ModelProvider> {
    match provider_name {
        Some(provider_name) => ModelProvider::from_supported_alias(provider_name),
        None => Some(resolve_default_provider(configured_default_provider)),
    }
}

fn resolve_default_provider(provider_name: &str) -> ModelProvider {
    if let Some(provider) = ModelProvider::from_supported_alias(provider_name) {
        provider
    } else {
        eprintln!(
            "Invalid configured default provider '{}'. Falling back to codex.",
            provider_name
        );
        ModelProvider::Codex
    }
}

#[cfg(test)]
fn override_cli_for_tests(cli: &mut String) {
    if let Some(test_cli) =
        std::env::var_os("RUST_CHATBOT_TEST_PROVIDER_CLI").filter(|value| !value.is_empty())
    {
        *cli = test_cli.to_string_lossy().into_owned();
    }
}

#[cfg(not(test))]
fn override_cli_for_tests(_cli: &mut String) {}

pub(super) fn build_background_session(
    task: &BackgroundTask,
    provider: ModelProvider,
) -> ChatSession {
    let mut session = ChatSession {
        id: task.id.to_string(),
        name: task.agent_profile.role.clone(),
        system_prompt: task.system_prompt.clone(),
        json_summary: None::<JsonSummary>,
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
        launched_from: task.working_dir.clone(),
        runsc_dirs: Vec::new(),
        runsc_image: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        messages: vec![],
    };
    if let Some(session_id) = task.resume_session_id.as_ref() {
        provider.set_session_id_on_chat(&mut session, session_id.clone());
    }
    session
}

fn execute_provider_subprocess(
    task: &BackgroundTask,
    provider: ModelProvider,
    overrides: &ProviderRequestOverrides,
    stop_signal: Arc<AtomicBool>,
) -> Result<String, String> {
    let session = build_background_session(task, provider);
    sync_provider_mcp_configs(Some(&session), Some(overrides)).map_err(|err| err.to_string())?;
    let provider_session = to_provider_session(Some(&session));
    let (mut command, runsc_workspace_sync) =
        build_provider_command(task, provider, overrides, provider_session.as_ref())?;
    if let Some(staged) = runsc_workspace_sync.staged_root() {
        eprintln!(
            "[runsc] task={} provider={} cwd={}",
            task.id,
            provider.as_str(),
            staged.display()
        );
    }
    wrap_command(&mut command);
    let mut child = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("{SPAWN_FAILURE_PREFIX}provider CLI: {err}"))?;

    loop {
        if stop_signal.load(Ordering::Relaxed) {
            let _ = child.kill();
            break;
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(err) => {
                let _ = child.kill();
                return Err(format!("Failed to wait for provider CLI: {err}"));
            }
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|err| format!("Failed to collect provider output: {err}"))?;
    runsc_workspace_sync
        .sync_back()
        .map_err(|err| format!("Failed to sync runsc workspace changes: {err}"))?;

    if stop_signal.load(Ordering::Relaxed) {
        return Err("Process stopped by user.".to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() {
        return Ok(stdout);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(provider_failure_message(
        provider,
        output.status.code(),
        &stderr,
        &stdout,
    ))
}

fn forge_provider_hint(provider: ModelProvider, model: &str) -> Option<&str> {
    match provider {
        ModelProvider::OmniForge => model.split_once(':').map(|(prefix, _)| prefix),
        ModelProvider::GeminiForge => Some("gemini_forge"),
        ModelProvider::GeminiCliForge => Some("gemini_cli_forge"),
        ModelProvider::MiniMaxForge => Some("minimax_forge"),
        _ => None,
    }
}

pub(super) fn build_provider_command(
    task: &BackgroundTask,
    provider: ModelProvider,
    overrides: &ProviderRequestOverrides,
    provider_session: Option<&ai_providers::session::ProviderSession>,
) -> Result<
    (
        std::process::Command,
        ai_providers::sandbox::RunscWorkspaceSync,
    ),
    String,
> {
    let resume_session_id = provider_resume_session_id(provider, provider_session);
    let (mut command, runsc_mounts) = match provider {
        ModelProvider::Claude => {
            let mut config = claude_compat_config(provider, Some(overrides));
            if let Some(model) = overrides.model.as_ref() {
                config.model = model.clone();
            }
            override_cli_for_tests(&mut config.cli);
            validate_claude_config(&config)?;
            (
                build_claude_command(
                    task,
                    provider_session,
                    resume_session_id.as_deref(),
                    &config,
                ),
                config.runsc_mounts.clone(),
            )
        }
        ModelProvider::MiniMax => {
            let mut config = claude_compat_config(provider, Some(overrides));
            if let Some(model) = overrides.model.as_ref() {
                config.model = model.clone();
            }
            override_cli_for_tests(&mut config.cli);
            let minimax_env = config::api_key_env_for(provider)
                .expect("MiniMax catalog entry must define an api_key_env");
            if !config
                .extra_env
                .iter()
                .any(|(key, value): &(String, String)| {
                    key == "ANTHROPIC_AUTH_TOKEN" && !value.trim().is_empty()
                })
            {
                return Err(format!(
                    "{} is not set. Export it or write the key to {} before using MiniMax.",
                    minimax_env,
                    config::minimax_api_key_path()
                ));
            }
            (
                build_claude_command(
                    task,
                    provider_session,
                    resume_session_id.as_deref(),
                    &config,
                ),
                config.runsc_mounts.clone(),
            )
        }
        ModelProvider::Codex => {
            let mut config = codex_compat_config(provider, Some(overrides));
            if let Some(model) = overrides.model.as_ref() {
                config.model = Some(model.clone());
            }
            override_cli_for_tests(&mut config.cli);
            (
                build_codex_command(provider, task, provider_session, &config),
                config.runsc_mounts.clone(),
            )
        }
        ModelProvider::Nvidia => {
            let mut config = codex_compat_config(provider, Some(overrides));
            if let Some(model) = overrides.model.as_ref() {
                config.model = Some(model.clone());
            }
            override_cli_for_tests(&mut config.cli);
            let nvidia_env = config::api_key_env_for(provider)
                .expect("Nvidia catalog entry must define an api_key_env");
            if !config
                .extra_env
                .iter()
                .any(|(key, value): &(String, String)| {
                    key == "OPENAI_API_KEY" && !value.trim().is_empty()
                })
            {
                return Err(format!(
                    "{} is not set. Export it or write the key to {} before using Nvidia.",
                    nvidia_env,
                    config::nvidia_api_key_path()
                ));
            }
            (
                build_codex_command(provider, task, provider_session, &config),
                config.runsc_mounts.clone(),
            )
        }
        ModelProvider::Gemini => {
            let mut config = gemini_cli_config(
                Some(&build_background_session(task, provider)),
                Some(overrides),
            );
            if let Some(model) = overrides.model.as_ref() {
                config.model = Some(model.clone());
            }
            override_cli_for_tests(&mut config.cli);
            (
                build_gemini_command(task, provider_session, &config),
                config.runsc_mounts.clone(),
            )
        }
        ModelProvider::Kimi => {
            let mut config = crate::provider_bridge::kimi_cli_config(Some(overrides));
            if let Some(model) = overrides.model.as_ref() {
                config.model = Some(model.clone());
            }
            override_cli_for_tests(&mut config.cli);
            let mut command = std::process::Command::new(&config.cli);
            crate::provider_bridge::apply_cli_runtime_env(&mut command, &config.cli);
            workspace::apply_session_working_dir(&mut command, provider_session);
            command.arg("--print");
            command.arg("--output-format").arg(&config.output_format);
            if config.yolo {
                command.arg("--yolo");
            }
            if let Some(model) = config.model.as_deref() {
                command.arg("--model").arg(model);
            }
            if let Some(session_id) = resume_session_id.as_ref() {
                command.arg("--resume").arg(session_id);
            }
            add_prompt_arg(&mut command, &task.prompt);
            (command, Vec::new())
        }
        ModelProvider::OpenCode
        | ModelProvider::Hermes
        | ModelProvider::Forge
        | ModelProvider::Claw => {
            // These providers are migrated to adapter-backed execution — this
            // branch should never be reached when adapter-worker is enabled.
            // They fall through to the adapter path in chatbot_worker.rs.
            // This is a fallback for when the legacy path is used directly.
            return Err(format!(
                "{} provider requires adapter-worker feature",
                provider.as_str()
            ));
        }
        ModelProvider::Qwen
        | ModelProvider::OpenRouter
        | ModelProvider::OmniForge
        | ModelProvider::GeminiForge
        | ModelProvider::GeminiCliForge
        | ModelProvider::MiniMaxForge => {
            let mut config = crate::provider_bridge::openrouter_cli_config(
                provider,
                Some(&build_background_session(task, provider)),
                Some(overrides),
            );
            if let Some(model) = overrides.model.as_ref() {
                config.model = crate::provider_bridge::openrouter_cli_model_target(provider, model);
            }
            override_cli_for_tests(&mut config.cli);
            let provider_has_subscription_auth =
                crate::provider_bridge::uses_gemini_subscription_auth(
                    provider,
                    Some(&build_background_session(task, provider)),
                    overrides.model.as_deref(),
                );
            if config.api_key.trim().is_empty() && !provider_has_subscription_auth {
                let provider_label = match provider {
                    ModelProvider::Qwen => "Qwen via OpenRouter",
                    ModelProvider::OpenRouter => "OpenRouter",
                    ModelProvider::OmniForge => "OmniForge",
                    ModelProvider::GeminiForge => "GeminiForge",
                    ModelProvider::GeminiCliForge => "GeminiCliForge",
                    ModelProvider::MiniMaxForge => "MiniMaxForge",
                    _ => unreachable!("branch only covers OpenRouter-backed providers"),
                };
                if provider == ModelProvider::GeminiCliForge {
                    return Err(format!(
                        "Gemini subscription auth is not configured. Run `gemini login` or provide {} before using GeminiCliForge.",
                        config::canonical_shared_root_dir()
                            .join(".gemini")
                            .join("oauth_creds.json")
                            .display()
                    ));
                }
                return Err(format!(
                    "{} is not set. Export it or write the key to {} before using {}.",
                    crate::provider_catalog::by_id(provider).api_key_env,
                    config::provider_api_key_path(provider)
                        .unwrap_or_else(config::openrouter_api_key_path),
                    provider_label
                ));
            }
            let mut command = std::process::Command::new(&config.cli);
            crate::provider_bridge::apply_cli_runtime_env(&mut command, &config.cli);
            for (key, value) in &config.extra_env {
                command.env(key, value);
            }
            workspace::apply_session_working_dir(&mut command, provider_session);
            let working_dir = provider_session.and_then(|session| session.working_dir.as_deref());
            let runsc_enabled = overrides
                .runsc_image
                .as_deref()
                .is_some_and(|image| !image.trim().is_empty());
            if !config.api_key.trim().is_empty() {
                command.env("OPENROUTER_API_KEY", &config.api_key);
            }
            if config.base_url != "https://openrouter.ai/api/v1" {
                command.env("OPENROUTER_BASE_URL", &config.base_url);
            }
            command.arg("exec");
            if let Some(forge_provider) = forge_provider_hint(provider, &config.model) {
                command.arg("--forge-provider").arg(forge_provider);
            }
            command.arg("--model").arg(&config.model);
            if config.base_url != "https://openrouter.ai/api/v1" {
                command.arg("--base-url").arg(&config.base_url);
            }
            if let Some(session_id) = config.resume_session_id.as_ref() {
                command.arg("--resume").arg(session_id);
            }
            if let Some(workspace_root) = config.workspace_root.as_ref() {
                command.arg("--workspace-root").arg(workspace_root);
            }
            if config.context_window_size > 0 {
                command
                    .arg("--context-window-size")
                    .arg(config.context_window_size.to_string());
            }
            if let Some(mcp_config) = config.mcp_config.as_deref() {
                command.arg("--mcp-config").arg(mcp_config);
            }
            command.arg("--sandbox").arg("workspace-write");
            command.arg("--json");
            add_prompt_arg(
                &mut command,
                &build_openrouter_prompt(task, runsc_enabled, working_dir),
            );
            (command, Vec::new())
        }
    };
    let runsc_workspace_sync = wrap_command_with_runsc(
        &mut command,
        overrides.runsc_image.as_deref(),
        &overrides.runsc_workspace_includes,
        &runsc_mounts,
    )
    .map_err(|err| err.to_string())?;
    Ok((command, runsc_workspace_sync))
}

pub(super) fn validate_claude_config(config: &ClaudeCliConfig) -> Result<(), String> {
    if !config.allow_api_billing {
        let subscription_type = get_subscription_type(config);
        if !subscription_type.is_subscription() {
            let login_cmd = if let Some(config_dir) = config.config_dir.as_ref() {
                format!("CLAUDE_CONFIG_DIR={} claude login", config_dir.display())
            } else {
                "claude login".to_string()
            };
            let error_msg = match subscription_type {
                ai_providers::claude::cli::SubscriptionType::Api => {
                    format!(
                        "API billing detected. Please login with a subscription account using '{login_cmd}' or enable allow_api_billing."
                    )
                }
                ai_providers::claude::cli::SubscriptionType::Unknown => {
                    format!(
                        "Unable to detect subscription type. Please ensure you are logged in using '{login_cmd}'."
                    )
                }
                _ => unreachable!(),
            };
            return Err(error_msg);
        }
    }

    if let Some(config_dir) = config.config_dir.as_ref() {
        if !config_dir.is_dir() {
            return Err(format!(
                "Claude config directory not found: {}",
                config_dir.display()
            ));
        }
    }

    if config.require_mcp_servers && !has_mcp_servers(config.config_dir.as_deref()) {
        return Err("No MCP servers found in provider config.".to_string());
    }

    Ok(())
}

pub(super) fn build_claude_command(
    task: &BackgroundTask,
    provider_session: Option<&ai_providers::session::ProviderSession>,
    resume_session_id: Option<&str>,
    config: &ClaudeCliConfig,
) -> std::process::Command {
    let mut command = std::process::Command::new(&config.cli);
    crate::provider_bridge::apply_cli_runtime_env(&mut command, &config.cli);
    workspace::apply_session_working_dir(&mut command, provider_session);
    let working_dir = provider_session.and_then(|session| session.working_dir.as_deref());
    let runsc_enabled = config
        .runsc_image
        .as_deref()
        .is_some_and(|image| !image.trim().is_empty());
    if let Some(config_dir) = config.config_dir.as_ref() {
        command.env("CLAUDE_CONFIG_DIR", config_dir);
    }
    for (key, value) in &config.extra_env {
        command.env(key, value);
    }

    if let Some(prompt) = effective_claude_system_prompt(
        config,
        provider_session,
        task.system_prompt.as_deref(),
        resume_session_id.is_some(),
    ) {
        command
            .arg("--append-system-prompt")
            .arg(rewrite_runsc_workspace_paths(
                &prompt,
                runsc_enabled,
                working_dir,
            ));
    }
    if config.skip_permissions {
        command.arg("--dangerously-skip-permissions");
    }
    if let Some(session_id) = resume_session_id {
        command.arg("--resume").arg(session_id);
    }
    if config.verbose_output {
        command.arg("--verbose");
    }
    command.arg("--model").arg(&config.model);
    if let Some(schema) = config.json_schema.as_deref() {
        command.arg("--json-schema").arg(schema);
    }
    command.arg("--output-format").arg(&config.output_format);
    add_prompt_arg(
        &mut command,
        &rewrite_runsc_workspace_paths(&task.prompt, runsc_enabled, working_dir),
    );
    command
}

pub(super) fn build_codex_command(
    provider: ModelProvider,
    task: &BackgroundTask,
    provider_session: Option<&ProviderSession>,
    config: &CodexCliConfig,
) -> std::process::Command {
    let mut command = std::process::Command::new(&config.cli);
    crate::provider_bridge::apply_cli_runtime_env(&mut command, &config.cli);
    workspace::apply_session_working_dir(&mut command, provider_session);
    let working_dir = provider_session.and_then(|session| session.working_dir.as_deref());
    let runsc_enabled = config
        .runsc_image
        .as_deref()
        .is_some_and(|image| !image.trim().is_empty());
    for (key, value) in &config.extra_env {
        command.env(key, value);
    }
    command.arg("exec");
    if let Some(mode) = config.sandbox_mode.as_deref() {
        command.arg("--sandbox").arg(mode);
    }
    if let Some(effort) = config.reasoning_effort.as_deref() {
        command
            .arg("-c")
            .arg(format!("reasoning_effort=\"{}\"", effort));
    }
    if config.json_stream {
        command.arg("--json");
    }
    if let Some(model) = config.model.as_deref() {
        command.arg("--model").arg(model);
    }
    if let Some(schema_path) = config.output_schema.as_ref() {
        command.arg("--output-schema").arg(schema_path);
    }
    command.arg("--skip-git-repo-check");
    if let Some(session_id) = provider_resume_session_id(provider, provider_session) {
        command.arg("resume").arg(session_id);
    }
    let prompt = build_codex_prompt(task, runsc_enabled, working_dir);
    if ai_providers::config::prompt_needs_hyphen_guard(&prompt) {
        command.arg("--");
    }
    command.arg(prompt);
    command
}

pub(super) fn build_gemini_command(
    task: &BackgroundTask,
    provider_session: Option<&ProviderSession>,
    config: &GeminiCliConfig,
) -> std::process::Command {
    let mut command = std::process::Command::new(&config.cli);
    crate::provider_bridge::apply_cli_runtime_env(&mut command, &config.cli);
    workspace::apply_session_working_dir(&mut command, provider_session);
    let working_dir = provider_session.and_then(|session| session.working_dir.as_deref());
    let runsc_enabled = config
        .runsc_image
        .as_deref()
        .is_some_and(|image| !image.trim().is_empty());
    command.arg("--output-format").arg(&config.output_format);
    for directory in gemini_include_directories(provider_session, config) {
        command.arg("--include-directories").arg(directory);
    }
    if let Some(model) = config.model.as_deref() {
        command.arg("--model").arg(model);
    }
    if let Some(mode) = config.approval_mode.as_deref() {
        command.arg("--approval-mode").arg(mode);
    }
    if let Some(session_id) = provider_resume_session_id(ModelProvider::Gemini, provider_session) {
        command.arg("--resume").arg(session_id);
    }
    add_prompt_arg(
        &mut command,
        &build_gemini_prompt(task, provider_session, runsc_enabled, working_dir),
    );
    if let Some(path) = config.project_settings_path.as_ref() {
        if path.is_file() {
            command.env("GEMINI_CLI_PROJECT_SETTINGS_PATH", path);
        }
    }
    command
}

pub(super) fn build_codex_prompt(
    task: &BackgroundTask,
    runsc_enabled: bool,
    working_dir: Option<&str>,
) -> String {
    if task.resume_session_id.is_some() {
        return rewrite_runsc_workspace_paths(&task.prompt, runsc_enabled, working_dir);
    }
    match task.system_prompt.as_deref() {
        Some(prompt) if !prompt.trim().is_empty() => {
            format!(
                "System instructions:\n{}\n\nUser: {}",
                rewrite_runsc_workspace_paths(prompt, runsc_enabled, working_dir),
                rewrite_runsc_workspace_paths(&task.prompt, runsc_enabled, working_dir)
            )
        }
        _ => rewrite_runsc_workspace_paths(&task.prompt, runsc_enabled, working_dir),
    }
}

pub(super) fn build_gemini_prompt(
    task: &BackgroundTask,
    provider_session: Option<&ProviderSession>,
    runsc_enabled: bool,
    working_dir: Option<&str>,
) -> String {
    let mut sections = Vec::new();
    let has_resume = provider_has_resume(ModelProvider::Gemini, provider_session);

    if !has_resume {
        if let Some(prompt) = task.system_prompt.as_deref() {
            if !prompt.trim().is_empty() {
                sections.push(format!(
                    "System instructions:\n{}",
                    rewrite_runsc_workspace_paths(prompt, runsc_enabled, working_dir)
                ));
            }
        }
    }

    if task.prompt.contains('\n') {
        sections.push(format!(
            "User:\n{}",
            rewrite_runsc_workspace_paths(&task.prompt, runsc_enabled, working_dir).trim()
        ));
    } else {
        sections.push(format!(
            "User: {}",
            rewrite_runsc_workspace_paths(&task.prompt, runsc_enabled, working_dir).trim()
        ));
    }

    sections.join("\n\n")
}

pub(super) fn rewrite_runsc_workspace_paths(
    text: &str,
    runsc_enabled: bool,
    working_dir: Option<&str>,
) -> String {
    if !runsc_enabled {
        return text.to_string();
    }
    let Some(working_dir) = working_dir.filter(|value| !value.trim().is_empty()) else {
        return text.to_string();
    };
    let normalized_working_dir = working_dir.trim_end_matches(['/', '\\']);
    let working_dir = if normalized_working_dir.is_empty() || normalized_working_dir.ends_with(':')
    {
        working_dir
    } else {
        normalized_working_dir
    };
    let mut rewritten = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(offset) = text[cursor..].find(working_dir) {
        let start = cursor + offset;
        let end = start + working_dir.len();
        rewritten.push_str(&text[cursor..start]);
        if has_workspace_path_boundaries(text, start, end) {
            rewritten.push('.');
        } else {
            rewritten.push_str(&text[start..end]);
        }
        cursor = end;
    }
    rewritten.push_str(&text[cursor..]);
    rewritten
}

fn has_workspace_path_boundaries(text: &str, start: usize, end: usize) -> bool {
    has_workspace_path_start_boundary(text[..start].chars().next_back())
        && has_workspace_path_end_boundary(text[end..].chars().next())
}

fn has_workspace_path_start_boundary(ch: Option<char>) -> bool {
    ch.is_none_or(|value| !matches!(value, '/' | '\\') && !is_workspace_path_segment_char(value))
}

fn has_workspace_path_end_boundary(ch: Option<char>) -> bool {
    ch.is_none_or(|value| matches!(value, '/' | '\\') || !is_workspace_path_segment_char(value))
}

fn is_workspace_path_segment_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '~')
}

fn is_disallowed_gemini_include_dir(dir: &str) -> bool {
    let trimmed = dir.trim();
    trimmed.is_empty() || (!cfg!(windows) && Path::new(trimmed) == Path::new("/"))
}

fn fallback_gemini_include_directory(
    provider_session: Option<&ai_providers::session::ProviderSession>,
) -> Option<String> {
    workspace::session_working_dir(provider_session)
        .filter(|dir| !is_disallowed_gemini_include_dir(dir))
        .map(ToString::to_string)
        .or_else(|| {
            std::env::current_dir().ok().and_then(|path| {
                if !cfg!(windows) && path == Path::new("/") {
                    None
                } else {
                    Some(path.to_string_lossy().into_owned())
                }
            })
        })
}

pub(super) fn gemini_include_directories(
    provider_session: Option<&ai_providers::session::ProviderSession>,
    config: &GeminiCliConfig,
) -> Vec<String> {
    let configured = if !config.include_directories.is_empty() {
        config.include_directories.clone()
    } else {
        match std::env::var("GEMINI_INCLUDE_DIRECTORIES") {
            Ok(value) if !value.trim().is_empty() => value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect(),
            _ => Vec::new(),
        }
    };

    let filtered: Vec<String> = configured
        .into_iter()
        .filter(|dir| !is_disallowed_gemini_include_dir(dir))
        .collect();
    if !filtered.is_empty() {
        filtered
    } else {
        fallback_gemini_include_directory(provider_session)
            .into_iter()
            .collect()
    }
}
