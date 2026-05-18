use crate::omniforge;

fn openrouter_context_window_size(provider: ModelProvider) -> u64 {
    match provider {
        ModelProvider::Qwen => config::QWEN_OPENROUTER_CONTEXT_WINDOW_SIZE,
        _ => config::GEMMA_CONTEXT_WINDOW_SIZE,
    }
}

fn resolved_openrouter_api_key(
    provider: ModelProvider,
    session: Option<&ChatSession>,
    requested_model: Option<&str>,
) -> Option<String> {
    match provider {
        ModelProvider::Qwen | ModelProvider::OpenRouter => provider_api_key(ModelProvider::OpenRouter),
        ModelProvider::OmniForge => {
            let resolved = omniforge::resolve_model(session, requested_model);
            provider_api_key(omniforge::target_auth_provider(resolved.target))
        }
        ModelProvider::GeminiForge | ModelProvider::GeminiCliForge | ModelProvider::MiniMaxForge => {
            provider_api_key(provider)
        }
        _ => None,
    }
}

pub(crate) fn uses_gemini_subscription_auth(
    provider: ModelProvider,
    session: Option<&ChatSession>,
    requested_model: Option<&str>,
) -> bool {
    if !gemini_subscription_auth_available() {
        return false;
    }

    match provider {
        ModelProvider::GeminiForge => true,
        ModelProvider::GeminiCliForge => true,
        ModelProvider::OmniForge => {
            let resolved = omniforge::resolve_model(session, requested_model);
            omniforge::target_auth_provider(resolved.target) == ModelProvider::GeminiForge
        }
        _ => false,
    }
}

fn forge_target_name(provider: ModelProvider) -> Option<&'static str> {
    match provider {
        ModelProvider::OmniForge => None,
        ModelProvider::GeminiForge => Some("gemini_forge"),
        // forgecode2 currently exposes the Gemini subscription-auth transport
        // under the shared `gemini_forge` provider id.
        ModelProvider::GeminiCliForge => Some("gemini_forge"),
        ModelProvider::MiniMaxForge => Some("minimax_forge"),
        _ => None,
    }
}

pub(crate) fn openrouter_cli_model_target(provider: ModelProvider, model: &str) -> String {
    if provider == ModelProvider::OmniForge {
        return omniforge::resolve_model(None, Some(model)).prefixed_model;
    }

    let Some(target) = forge_target_name(provider) else {
        return model.to_string();
    };

    if let Some((prefix, model_name)) = model.split_once(':') {
        let normalized_prefix = match provider {
            ModelProvider::GeminiCliForge if prefix == "gemini_cli_forge" => target,
            _ => prefix,
        };
        return format!("{normalized_prefix}:{model_name}");
    }

    format!("{target}:{model}")
}

pub fn openrouter_config(
    provider: ModelProvider,
    _overrides: Option<&ProviderRequestOverrides>,
) -> OpenRouterConfig {
    let entry = crate::provider_catalog::by_id(provider);
    let requested_model = _overrides.and_then(|value| value.model.as_deref());
    let api_key = resolved_openrouter_api_key(provider, None, requested_model).unwrap_or_default();
    let base_url = entry
        .base_url
        .map(str::to_string)
        .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string());
    let model = if provider == ModelProvider::OmniForge {
        omniforge::resolve_model(None, requested_model).model
    } else {
        entry.interactive_model.to_string()
    };

    OpenRouterConfig {
        base_url,
        api_key,
        model,
        max_tokens: OpenRouterConfig::default().max_tokens,
        context_window_size: openrouter_context_window_size(provider),
        default_system_prompt: None,
    }
}

pub fn openrouter_cli_config(
    provider: ModelProvider,
    session: Option<&ChatSession>,
    _overrides: Option<&ProviderRequestOverrides>,
) -> OpenRouterCliConfig {
    let entry = crate::provider_catalog::by_id(provider);
    let requested_model = _overrides.and_then(|value| value.model.as_deref());
    let api_key =
        resolved_openrouter_api_key(provider, session, requested_model).unwrap_or_default();
    let base_url = match provider {
        ModelProvider::OmniForge
        | ModelProvider::GeminiForge
        | ModelProvider::GeminiCliForge
        | ModelProvider::MiniMaxForge => "https://openrouter.ai/api/v1".to_string(),
        _ => entry
            .base_url
            .map(str::to_string)
            .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string()),
    };
    let resume_session_id = session.and_then(|s| provider.session_id_from_chat(s).cloned());
    let cli = match provider {
        ModelProvider::OmniForge
        | ModelProvider::GeminiForge
        | ModelProvider::GeminiCliForge
        | ModelProvider::MiniMaxForge => config::forge_rust_chatbot_cli_path(),
        _ => entry.cli_binary.to_string(),
    };
    let mut extra_env = Vec::new();
    if provider == ModelProvider::OmniForge {
        let resolved = omniforge::resolve_model(session, requested_model);
        let auth_provider = omniforge::target_auth_provider(resolved.target);
        if let Some(env_name) = config::api_key_env_for(auth_provider) {
            if !api_key.is_empty() {
                extra_env.push((env_name.to_string(), api_key.clone()));
            }
        }
    } else if !entry.api_key_env.is_empty() && !api_key.is_empty() {
        extra_env.push((entry.api_key_env.to_string(), api_key.clone()));
    }
    if uses_gemini_subscription_auth(provider, session, requested_model) {
        if let Ok(home) = std::env::var("HOME") {
            if !home.trim().is_empty() {
                extra_env.push(("HOME".to_string(), home));
            }
        }
    }
    let model = if provider == ModelProvider::OmniForge {
        omniforge::resolve_model(session, requested_model).prefixed_model
    } else {
        openrouter_cli_model_target(provider, entry.interactive_model)
    };

    OpenRouterCliConfig {
        cli,
        model,
        api_key,
        base_url,
        context_window_size: openrouter_context_window_size(provider),
        default_system_prompt: None,
        resume_session_id,
        workspace_root: session.and_then(|s| s.launched_from.clone()),
        extra_env,
        mcp_config: Some(config::mcp_json_path()),
    }
}
