pub fn build_provider_command_string(
    provider: ModelProvider,
    system_prompt: Option<&str>,
    session: Option<&ChatSession>,
    memory_context: Option<&MemoryContext>,
    query: &str,
) -> String {
    build_provider_command_string_with_overrides(
        provider,
        system_prompt,
        session,
        memory_context,
        None,
        query,
    )
}

pub fn build_provider_command_string_with_overrides(
    provider: ModelProvider,
    system_prompt: Option<&str>,
    session: Option<&ChatSession>,
    memory_context: Option<&MemoryContext>,
    overrides: Option<&ProviderRequestOverrides>,
    query: &str,
) -> String {
    if uses_agent_adapter(provider) {
        return build_adapter_command_string_with_overrides(
            provider,
            system_prompt,
            session,
            memory_context,
            overrides,
            query,
        );
    }

    let config = provider_config_with_overrides(provider, session, overrides);
    let provider_session = to_provider_session(session);
    let provider_memory = to_provider_memory_context(memory_context);
    let effective_system_prompt =
        effective_provider_system_prompt(provider, system_prompt, query, overrides);
    ai_providers::build_provider_command_string(
        &config,
        effective_system_prompt.as_deref(),
        query,
        provider_session.as_ref(),
        provider_memory.as_ref(),
    )
}

fn uses_agent_adapter(provider: ModelProvider) -> bool {
    matches!(
        provider,
        ModelProvider::Claude
            | ModelProvider::MiniMax
            | ModelProvider::Gemini
            | ModelProvider::Codex
            | ModelProvider::Kimi
            | ModelProvider::OpenCode
            | ModelProvider::Hermes
            | ModelProvider::Forge
            | ModelProvider::Claw
    )
}

fn build_adapter_command_string_with_overrides(
    provider: ModelProvider,
    system_prompt: Option<&str>,
    session: Option<&ChatSession>,
    memory_context: Option<&MemoryContext>,
    overrides: Option<&ProviderRequestOverrides>,
    query: &str,
) -> String {
    let prompt = crate::memory::build_query_with_memory(query, memory_context);
    let working_dir = session
        .and_then(|session| session.launched_from.clone())
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|path| path.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| ".".to_string());
    let resume_session_id = session
        .and_then(|session| provider.session_id_from_chat(session))
        .cloned();
    let system_prompt = effective_adapter_system_prompt(
        provider,
        system_prompt,
        query,
        session,
        overrides,
        resume_session_id.is_some(),
    );

    let mut args = vec![
        agent_adapter_cli_program(),
        "run".to_string(),
        "--provider".to_string(),
        provider.as_str().to_string(),
        "--prompt".to_string(),
        prompt,
        "--cwd".to_string(),
        working_dir,
    ];

    if let Some(model) = overrides.and_then(|value| value.model.as_ref()) {
        args.push("--model".to_string());
        args.push(model.clone());
    }

    if let Some(system_prompt) = system_prompt {
        args.push("--system-prompt".to_string());
        args.push(system_prompt);
    }

    if let Some(session_id) = resume_session_id {
        args.push("--session-id".to_string());
        args.push(session_id);
        args.push("--resume".to_string());
    }

    let sandbox_dirs = adapter_sandbox_dirs(session);
    if !sandbox_dirs.is_empty() {
        args.push("--sandbox-dirs".to_string());
        args.push(sandbox_dirs.join(","));
    }

    args.push("--json".to_string());

    shell_join(&args)
}

fn effective_adapter_system_prompt(
    provider: ModelProvider,
    system_prompt: Option<&str>,
    query: &str,
    session: Option<&ChatSession>,
    overrides: Option<&ProviderRequestOverrides>,
    has_resume_session: bool,
) -> Option<String> {
    let base_prompt = effective_provider_system_prompt(provider, system_prompt, query, overrides);
    match provider {
        ModelProvider::Claude | ModelProvider::MiniMax => {
            let config = claude_compat_config(provider, overrides);
            let provider_session = to_provider_session(session);
            ai_providers::claude::cli::effective_claude_system_prompt(
                &config,
                provider_session.as_ref(),
                base_prompt.as_deref(),
                has_resume_session,
            )
        }
        _ => base_prompt,
    }
}

fn adapter_sandbox_dirs(session: Option<&ChatSession>) -> Vec<String> {
    let mut dirs = Vec::new();

    if let Some(working_dir) = session
        .and_then(|session| session.launched_from.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        dirs.push(working_dir.to_string());
    }

    if let Some(session) = session {
        for dir in &session.runsc_dirs {
            let dir = dir.trim();
            if dir.is_empty() || dirs.iter().any(|existing| existing == dir) {
                continue;
            }
            dirs.push(dir.to_string());
        }
    }

    dirs
}

fn agent_adapter_cli_program() -> String {
    crate::companion_binary::resolve_agent_adapter_cli()
        .to_string_lossy()
        .into_owned()
}

fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".to_string();
    }

    if arg
        .bytes()
        .all(|byte| matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'/' | b'.' | b'_' | b':' | b',' | b'-' | b'=' ))
    {
        return arg.to_string();
    }

    format!(
        "\"{}\"",
        arg.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('$', "\\$")
    )
}
