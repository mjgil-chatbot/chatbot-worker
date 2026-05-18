pub(super) use super::failure::provider_failure_message;
#[cfg(test)]
pub(super) use super::failure::{classify_provider_output, is_provider_failure};
use super::session_ids::{
    has_resume as provider_has_resume, resume_session_id as provider_resume_session_id,
};
use super::types::*;
use crate::config;
use crate::data::ChatSession;
use crate::model::ModelProvider;
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
