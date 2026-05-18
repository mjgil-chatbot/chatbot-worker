use crate::data::ChatSession;
use crate::model::ModelProvider;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OmniForgeTarget {
    GeminiForge,
    MiniMaxForge,
    OpenRouter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OmniForgeModelEntry {
    pub id: &'static str,
    pub label: &'static str,
    pub target: OmniForgeTarget,
    pub model: &'static str,
}

pub const DEFAULT_MODEL_ID: &str = "gemini-3-flash-preview";

pub static MODELS: &[OmniForgeModelEntry] = &[
    OmniForgeModelEntry {
        id: "gemini-3-flash-preview",
        label: "Gemini Forge / gemini-3-flash-preview",
        target: OmniForgeTarget::GeminiForge,
        model: "gemini-3-flash-preview",
    },
    OmniForgeModelEntry {
        id: "gemini-3.1-pro-preview",
        label: "Gemini Forge / gemini-3.1-pro-preview",
        target: OmniForgeTarget::GeminiForge,
        model: "gemini-3.1-pro-preview",
    },
    OmniForgeModelEntry {
        id: "minimax-m2.7",
        label: "MiniMax Forge / MiniMax-M2.7",
        target: OmniForgeTarget::MiniMaxForge,
        model: "MiniMax-M2.7",
    },
    OmniForgeModelEntry {
        id: "minimax-m2.5-free",
        label: "MiniMax via OpenRouter / minimax-m2.5-free",
        target: OmniForgeTarget::OpenRouter,
        model: "minimax/minimax-m2.5:free",
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedOmniForgeModel {
    pub id: &'static str,
    pub label: &'static str,
    pub target: OmniForgeTarget,
    pub model: String,
    pub prefixed_model: String,
}

pub fn default_model() -> &'static OmniForgeModelEntry {
    by_id(DEFAULT_MODEL_ID).expect("default OmniForge model must exist")
}

pub fn by_id(id: &str) -> Option<&'static OmniForgeModelEntry> {
    MODELS
        .iter()
        .find(|entry| entry.id.eq_ignore_ascii_case(id))
}

pub fn by_model(model: &str) -> Option<&'static OmniForgeModelEntry> {
    MODELS.iter().find(|entry| entry.model == model)
}

pub fn selected_model(session: Option<&ChatSession>) -> &'static OmniForgeModelEntry {
    session
        .and_then(|value| value.omniforge_model_id.as_deref())
        .and_then(by_id)
        .unwrap_or_else(default_model)
}

pub fn selected_model_id(session: Option<&ChatSession>) -> &'static str {
    selected_model(session).id
}

pub fn target_prefix(target: OmniForgeTarget) -> &'static str {
    match target {
        OmniForgeTarget::GeminiForge => "gemini_forge",
        OmniForgeTarget::MiniMaxForge => "minimax_forge",
        OmniForgeTarget::OpenRouter => "openrouter",
    }
}

pub fn target_auth_provider(target: OmniForgeTarget) -> ModelProvider {
    match target {
        OmniForgeTarget::GeminiForge => ModelProvider::GeminiForge,
        OmniForgeTarget::MiniMaxForge => ModelProvider::MiniMaxForge,
        OmniForgeTarget::OpenRouter => ModelProvider::OpenRouter,
    }
}

pub fn prefixed_model(target: OmniForgeTarget, model: &str) -> String {
    if model.starts_with("gemini_forge:")
        || model.starts_with("minimax_forge:")
        || model.starts_with("openrouter:")
    {
        model.to_string()
    } else {
        format!("{}:{model}", target_prefix(target))
    }
}

pub fn resolve_model(
    session: Option<&ChatSession>,
    requested: Option<&str>,
) -> ResolvedOmniForgeModel {
    if let Some(requested) = requested {
        if let Some(stripped) = requested.strip_prefix("gemini_forge:") {
            return ResolvedOmniForgeModel {
                id: "custom",
                label: "Custom Gemini Forge model",
                target: OmniForgeTarget::GeminiForge,
                model: stripped.to_string(),
                prefixed_model: requested.to_string(),
            };
        }
        if let Some(stripped) = requested.strip_prefix("minimax_forge:") {
            return ResolvedOmniForgeModel {
                id: "custom",
                label: "Custom MiniMax Forge model",
                target: OmniForgeTarget::MiniMaxForge,
                model: stripped.to_string(),
                prefixed_model: requested.to_string(),
            };
        }
        if let Some(stripped) = requested.strip_prefix("openrouter:") {
            return ResolvedOmniForgeModel {
                id: "custom",
                label: "Custom OpenRouter model",
                target: OmniForgeTarget::OpenRouter,
                model: stripped.to_string(),
                prefixed_model: requested.to_string(),
            };
        }
        if let Some(entry) = by_id(requested).or_else(|| by_model(requested)) {
            return ResolvedOmniForgeModel {
                id: entry.id,
                label: entry.label,
                target: entry.target,
                model: entry.model.to_string(),
                prefixed_model: prefixed_model(entry.target, entry.model),
            };
        }

        let fallback = selected_model(session);
        return ResolvedOmniForgeModel {
            id: "custom",
            label: "Custom OmniForge model",
            target: fallback.target,
            model: requested.to_string(),
            prefixed_model: prefixed_model(fallback.target, requested),
        };
    }

    let entry = selected_model(session);
    ResolvedOmniForgeModel {
        id: entry.id,
        label: entry.label,
        target: entry.target,
        model: entry.model.to_string(),
        prefixed_model: prefixed_model(entry.target, entry.model),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn test_session(model_id: Option<&str>) -> ChatSession {
        ChatSession {
            id: "session-1".to_string(),
            name: "Session 1".to_string(),
            system_prompt: None,
            json_summary: None,
            claude_session_id: None,
            minimax_session_id: None,
            qwen_session_id: None,
            codex_session_id: None,
            nvidia_session_id: None,
            gemini_session_id: None,
            openrouter_session_id: None,
            omniforge_session_id: None,
            omniforge_model_id: model_id.map(str::to_string),
            kimi_session_id: None,
            opencode_session_id: None,
            hermes_session_id: None,
            forge_session_id: None,
            claw_session_id: None,
            launched_from: None,
            runsc_dirs: Vec::new(),
            runsc_image: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            messages: Vec::new(),
        }
    }

    #[test]
    fn selected_model_defaults_when_session_is_unset() {
        assert_eq!(selected_model(None).id, DEFAULT_MODEL_ID);
    }

    #[test]
    fn selected_model_uses_session_model_id() {
        let session = test_session(Some("minimax-m2.7"));
        assert_eq!(selected_model(Some(&session)).model, "MiniMax-M2.7");
    }

    #[test]
    fn selected_model_supports_openrouter_minimax_entry() {
        let session = test_session(Some("minimax-m2.5-free"));
        assert_eq!(
            selected_model(Some(&session)).model,
            "minimax/minimax-m2.5:free"
        );
    }

    #[test]
    fn resolve_model_accepts_prefixed_model_override() {
        let resolved = resolve_model(None, Some("minimax_forge:custom-model"));
        assert_eq!(resolved.target, OmniForgeTarget::MiniMaxForge);
        assert_eq!(resolved.model, "custom-model");
        assert_eq!(resolved.prefixed_model, "minimax_forge:custom-model");
    }

    #[test]
    fn resolve_model_accepts_catalog_id_override() {
        let resolved = resolve_model(None, Some("gemini-3.1-pro-preview"));
        assert_eq!(resolved.target, OmniForgeTarget::GeminiForge);
        assert_eq!(
            resolved.prefixed_model,
            "gemini_forge:gemini-3.1-pro-preview"
        );
    }

    #[test]
    fn resolve_model_accepts_openrouter_catalog_id_override() {
        let resolved = resolve_model(None, Some("minimax-m2.5-free"));
        assert_eq!(resolved.target, OmniForgeTarget::OpenRouter);
        assert_eq!(resolved.model, "minimax/minimax-m2.5:free");
        assert_eq!(
            resolved.prefixed_model,
            "openrouter:minimax/minimax-m2.5:free"
        );
    }

    #[test]
    fn resolve_model_falls_back_to_selected_target_for_custom_unprefixed_model() {
        let session = test_session(Some("minimax-m2.7"));
        let resolved = resolve_model(Some(&session), Some("custom-model"));
        assert_eq!(resolved.target, OmniForgeTarget::MiniMaxForge);
        assert_eq!(resolved.prefixed_model, "minimax_forge:custom-model");
    }

    #[test]
    fn resolve_model_falls_back_to_openrouter_for_openrouter_selected_model() {
        let session = test_session(Some("minimax-m2.5-free"));
        let resolved = resolve_model(Some(&session), Some("custom-model"));
        assert_eq!(resolved.target, OmniForgeTarget::OpenRouter);
        assert_eq!(resolved.prefixed_model, "openrouter:custom-model");
    }
}
