pub fn call_provider_cli_stream(
    provider: ModelProvider,
    query: &str,
    system_prompt: Option<&str>,
    memory_context: Option<&MemoryContext>,
    session: Option<ChatSession>,
    tx: mpsc::Sender<StreamMessage>,
    stop_signal: Arc<AtomicBool>,
) -> Result<()> {
    call_provider_cli_stream_with_overrides(
        provider,
        query,
        system_prompt,
        memory_context,
        session,
        None,
        tx,
        stop_signal,
    )
}

pub fn call_provider_cli_stream_with_overrides(
    provider: ModelProvider,
    query: &str,
    system_prompt: Option<&str>,
    memory_context: Option<&MemoryContext>,
    session: Option<ChatSession>,
    overrides: Option<&ProviderRequestOverrides>,
    tx: mpsc::Sender<StreamMessage>,
    stop_signal: Arc<AtomicBool>,
) -> Result<()> {
    sync_mcp_configs(session.as_ref(), overrides)?;
    let minimax_auth_token = optional_string_override(
        overrides.and_then(|overrides| overrides.anthropic_auth_token.as_ref()),
        TEST_ANTHROPIC_AUTH_TOKEN_ENV,
    )
    .or_else(minimax_api_key);
    let minimax_env = config::api_key_env_for(ModelProvider::MiniMax)
        .expect("MiniMax catalog entry must define an api_key_env");
    if provider == ModelProvider::MiniMax && minimax_auth_token.is_none() {
        let _ = tx.send(StreamMessage {
            response: format!(
                "{} is not set. Export it or write the key to {} before using MiniMax.",
                minimax_env,
                config::minimax_api_key_path()
            ),
            command: build_provider_command_string_with_overrides(
                provider,
                system_prompt,
                session.as_ref(),
                memory_context,
                overrides,
                query,
            ),
            session_id: session
                .as_ref()
                .and_then(|s| provider.session_id_from_chat(s))
                .cloned(),
            summary: None,
            is_final: true,
        });
        return Ok(());
    }
    // Nvidia requires the NVIDIA API key
    if provider == ModelProvider::Nvidia {
        let nvidia_key = provider_api_key(provider);
        let nvidia_env = config::api_key_env_for(provider)
            .expect("Nvidia catalog entry must define an api_key_env");
        if nvidia_key.is_none() {
            let _ = tx.send(StreamMessage {
                response: format!(
                    "{} is not set. Export it or write the key to {} before using Nvidia.",
                    nvidia_env,
                    config::nvidia_api_key_path()
                ),
                command: build_provider_command_string_with_overrides(
                    provider,
                    system_prompt,
                    session.as_ref(),
                    memory_context,
                    overrides,
                    query,
                ),
                session_id: session
                    .as_ref()
                    .and_then(|s| provider.session_id_from_chat(s))
                    .cloned(),
                summary: None,
                is_final: true,
            });
            return Ok(());
        }
    }

    // OpenRouter-backed providers require an API key.
    if matches!(
        provider,
        ModelProvider::Qwen
            | ModelProvider::OpenRouter
            | ModelProvider::OmniForge
            | ModelProvider::GeminiForge
            | ModelProvider::GeminiCliForge
            | ModelProvider::MiniMaxForge
    ) {
        let entry = crate::provider_catalog::by_id(provider);
        let provider_key = match provider {
            ModelProvider::Qwen | ModelProvider::OpenRouter => provider_api_key(ModelProvider::OpenRouter),
            ModelProvider::OmniForge => {
                let requested = overrides.and_then(|value| value.model.as_deref());
                let resolved = crate::omniforge::resolve_model(session.as_ref(), requested);
                provider_api_key(crate::omniforge::target_auth_provider(resolved.target))
            }
            ModelProvider::GeminiForge | ModelProvider::GeminiCliForge | ModelProvider::MiniMaxForge => {
                provider_api_key(provider)
            }
            _ => None,
        };
        let provider_has_subscription_auth =
            uses_gemini_subscription_auth(provider, session.as_ref(), overrides.and_then(|value| value.model.as_deref()));
        if provider_key.is_none() && !provider_has_subscription_auth {
            let provider_label = match provider {
                ModelProvider::Qwen => "Qwen via OpenRouter",
                ModelProvider::OpenRouter => "OpenRouter",
                ModelProvider::OmniForge => "OmniForge",
                ModelProvider::GeminiForge => "GeminiForge",
                ModelProvider::GeminiCliForge => "GeminiCliForge",
                ModelProvider::MiniMaxForge => "MiniMaxForge",
                _ => unreachable!("guard only covers OpenRouter-backed providers"),
            };
            if provider == ModelProvider::GeminiCliForge {
                let _ = tx.send(StreamMessage {
                    response: format!(
                        "Gemini subscription auth is not configured. Run `gemini login` or provide {} before using GeminiCliForge.",
                        config::canonical_shared_root_dir()
                            .join(".gemini")
                            .join("oauth_creds.json")
                            .display()
                    ),
                    command: build_provider_command_string_with_overrides(
                        provider,
                        system_prompt,
                        session.as_ref(),
                        memory_context,
                        overrides,
                        query,
                    ),
                    session_id: session
                        .as_ref()
                        .and_then(|s| provider.session_id_from_chat(s))
                        .cloned(),
                    summary: None,
                    is_final: true,
                });
                return Ok(());
            }
            let key_path = match provider {
                ModelProvider::OmniForge => {
                    let requested = overrides.and_then(|value| value.model.as_deref());
                    let resolved = crate::omniforge::resolve_model(session.as_ref(), requested);
                    let auth_provider = crate::omniforge::target_auth_provider(resolved.target);
                    config::provider_api_key_path(auth_provider)
                        .unwrap_or_else(config::openrouter_api_key_path)
                }
                _ => config::provider_api_key_path(provider)
                    .unwrap_or_else(config::openrouter_api_key_path),
            };
            let env_name = match provider {
                ModelProvider::OmniForge => {
                    let requested = overrides.and_then(|value| value.model.as_deref());
                    let resolved = crate::omniforge::resolve_model(session.as_ref(), requested);
                    config::api_key_env_for(crate::omniforge::target_auth_provider(resolved.target))
                        .unwrap_or(entry.api_key_env)
                }
                _ => entry.api_key_env,
            };
            let _ = tx.send(StreamMessage {
                response: format!(
                    "{} is not set. Export it or write the key to {} before using {}.",
                    env_name,
                    key_path,
                    provider_label
                ),
                command: build_provider_command_string_with_overrides(
                    provider,
                    system_prompt,
                    session.as_ref(),
                    memory_context,
                    overrides,
                    query,
                ),
                session_id: session
                    .as_ref()
                    .and_then(|s| provider.session_id_from_chat(s))
                    .cloned(),
                summary: None,
                is_final: true,
            });
            return Ok(());
        }
    }

    let config = provider_config_with_overrides(provider, session.as_ref(), overrides);
    let provider_session = to_provider_session(session.as_ref());
    let provider_memory = to_provider_memory_context(memory_context);
    let effective_system_prompt =
        effective_provider_system_prompt(provider, system_prompt, query, overrides);
    ai_providers::call_provider_stream(
        &config,
        query,
        effective_system_prompt.as_deref(),
        provider_memory.as_ref(),
        provider_session,
        tx,
        stop_signal,
    )
}
