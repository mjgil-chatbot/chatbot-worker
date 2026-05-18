use super::{
    add_prompt_arg, provider_has_resume, provider_resume_session_id, rewrite_runsc_workspace_paths,
    workspace, BackgroundTask, ModelProvider, ProviderSession, QwenCodeCliConfig,
};

pub(crate) fn build_qwen_command(
    task: &BackgroundTask,
    provider_session: Option<&ProviderSession>,
    config: &QwenCodeCliConfig,
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

    command.arg("--output-format").arg(&config.output_format);

    if config.yolo {
        command.arg("--yolo");
    }

    if let Some(model) = config.model.as_ref() {
        command.arg("--model").arg(model);
    }

    let resume_session_id = provider_resume_session_id(ModelProvider::Qwen, provider_session);
    if let Some(session_id) = resume_session_id
        .as_deref()
        .or(config.resume_session_id.as_deref())
    {
        command.arg("--resume").arg(session_id);
    }

    if let Some(auth_type) = config.auth_type.as_ref() {
        command.arg("--auth-type").arg(auth_type);
    }

    // System prompt via --append-system-prompt on new sessions only.
    let has_resume = provider_has_resume(ModelProvider::Qwen, provider_session);
    if !has_resume {
        let effective_system_prompt = match (
            config.append_system_prompt.as_deref(),
            task.system_prompt.as_deref(),
        ) {
            (Some(default), Some(session))
                if !default.trim().is_empty() && !session.trim().is_empty() =>
            {
                Some(format!(
                    "{}\n\n{}",
                    default.trim(),
                    rewrite_runsc_workspace_paths(session.trim(), runsc_enabled, working_dir)
                ))
            }
            (Some(default), _) if !default.trim().is_empty() => Some(default.trim().to_string()),
            (_, Some(session)) if !session.trim().is_empty() => Some(
                rewrite_runsc_workspace_paths(session.trim(), runsc_enabled, working_dir),
            ),
            _ => None,
        };
        if let Some(sys_prompt) = effective_system_prompt.as_deref() {
            command.arg("--append-system-prompt").arg(sys_prompt);
        }
    }

    for dir in &config.include_directories {
        command.arg("--include-directories").arg(dir);
    }

    if !config.allowed_mcp_server_names.is_empty() {
        command
            .arg("--allowed-mcp-server-names")
            .arg(config.allowed_mcp_server_names.join(","));
    }

    add_prompt_arg(
        &mut command,
        &rewrite_runsc_workspace_paths(&task.prompt, runsc_enabled, working_dir),
    );
    command
}
