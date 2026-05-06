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
