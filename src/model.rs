//! Model provider selection and display helpers.

use crate::provider_catalog;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelProvider {
    Claude,
    MiniMax,
    Qwen,
    Codex,
    Nvidia,
    Gemini,
    OpenRouter,
    OmniForge,
    GeminiForge,
    GeminiCliForge,
    MiniMaxForge,
    Kimi,
    OpenCode,
    Hermes,
    Forge,
    Claw,
}

impl ModelProvider {
    pub const ALL: [Self; 16] = [
        Self::Claude,
        Self::MiniMax,
        Self::Qwen,
        Self::Codex,
        Self::Nvidia,
        Self::Gemini,
        Self::OpenRouter,
        Self::OmniForge,
        Self::GeminiForge,
        Self::GeminiCliForge,
        Self::MiniMaxForge,
        Self::Kimi,
        Self::OpenCode,
        Self::Hermes,
        Self::Forge,
        Self::Claw,
    ];

    pub const SUPPORTED: [Self; 3] = [Self::MiniMax, Self::Codex, Self::Kimi];

    pub const LAUNCHABLE: [Self; 6] = [
        Self::MiniMax,
        Self::Codex,
        Self::GeminiForge,
        Self::GeminiCliForge,
        Self::MiniMaxForge,
        Self::Kimi,
    ];

    /// Canonical lowercase identifier, e.g. `"claude"`, `"codex"`.
    pub fn as_str(&self) -> &'static str {
        provider_catalog::by_id(*self).id
    }

    /// Human-readable display name, e.g. `"Claude"`, `"Codex"`.
    pub fn display_name(&self) -> &'static str {
        provider_catalog::by_id(*self).display_name
    }

    /// Parse a provider name from any known alias (case-insensitive).
    ///
    /// This is the unified provider parser backed by the catalog.
    /// It replaces all ad hoc string-match tables.
    pub fn from_alias(value: &str) -> Option<Self> {
        provider_catalog::by_alias(value).map(|e| e.provider)
    }

    pub fn from_supported_alias(value: &str) -> Option<Self> {
        Self::from_alias(value).filter(|provider| provider.is_supported())
    }

    pub fn from_launch_alias(value: &str) -> Option<Self> {
        Self::from_alias(value).filter(|provider| provider.is_launchable())
    }

    /// Legacy alias for [`Self::from_alias`].
    pub fn from_arg(value: &str) -> Option<Self> {
        Self::from_alias(value)
    }

    /// Shared-memory discriminant code for this provider.
    pub fn shm_code(self) -> u32 {
        provider_catalog::by_id(self).shm_code
    }

    /// Recover a provider from its shared-memory discriminant code.
    pub fn from_shm_code(value: u32) -> Option<Self> {
        provider_catalog::by_shm_code(value).map(|e| e.provider)
    }

    pub fn is_supported(self) -> bool {
        Self::SUPPORTED.contains(&self)
    }

    pub fn is_launchable(self) -> bool {
        Self::LAUNCHABLE.contains(&self)
    }

    // ── centralized session ID accessors ───────────────────────────────

    /// Read the provider-specific session ID from a [`ChatSession`].
    pub fn session_id_from_chat<'a>(
        &self,
        session: &'a crate::data::ChatSession,
    ) -> Option<&'a String> {
        match self {
            Self::Claude => session.claude_session_id.as_ref(),
            Self::MiniMax => session.minimax_session_id.as_ref(),
            Self::Qwen => session.qwen_session_id.as_ref(),
            Self::Codex => session.codex_session_id.as_ref(),
            Self::Nvidia => session.nvidia_session_id.as_ref(),
            Self::Gemini => session.gemini_session_id.as_ref(),
            Self::OpenRouter => session.openrouter_session_id.as_ref(),
            Self::OmniForge => session.omniforge_session_id.as_ref(),
            Self::GeminiForge => session.gemini_session_id.as_ref(),
            Self::GeminiCliForge => session.gemini_session_id.as_ref(),
            Self::MiniMaxForge => session.minimax_session_id.as_ref(),
            Self::Kimi => session.kimi_session_id.as_ref(),
            Self::OpenCode => None, // OpenCode does not maintain a session ID
            Self::Hermes => session.hermes_session_id.as_ref(),
            Self::Forge => None, // Forge uses oneshot transport, no session ID
            Self::Claw => None,  // Claw does not maintain a session ID
        }
    }

    /// Write the provider-specific session ID on a [`ChatSession`].
    pub fn set_session_id_on_chat(&self, session: &mut crate::data::ChatSession, id: String) {
        match self {
            Self::Claude => session.claude_session_id = Some(id),
            Self::MiniMax => session.minimax_session_id = Some(id),
            Self::Qwen => session.qwen_session_id = Some(id),
            Self::Codex => session.codex_session_id = Some(id),
            Self::Nvidia => session.nvidia_session_id = Some(id),
            Self::Gemini => session.gemini_session_id = Some(id),
            Self::OpenRouter => session.openrouter_session_id = Some(id),
            Self::OmniForge => session.omniforge_session_id = Some(id),
            Self::GeminiForge => session.gemini_session_id = Some(id),
            Self::GeminiCliForge => session.gemini_session_id = Some(id),
            Self::MiniMaxForge => session.minimax_session_id = Some(id),
            Self::Kimi => session.kimi_session_id = Some(id),
            Self::OpenCode => {} // OpenCode does not maintain a session ID
            Self::Hermes => session.hermes_session_id = Some(id),
            Self::Forge => {} // Forge uses oneshot transport, no session ID
            Self::Claw => {}  // Claw does not maintain a session ID
        }
    }

    /// Clear the provider-specific session ID on a [`ChatSession`].
    pub fn clear_session_id_on_chat(&self, session: &mut crate::data::ChatSession) {
        match self {
            Self::Claude => session.claude_session_id = None,
            Self::MiniMax => session.minimax_session_id = None,
            Self::Qwen => session.qwen_session_id = None,
            Self::Codex => session.codex_session_id = None,
            Self::Nvidia => session.nvidia_session_id = None,
            Self::Gemini => session.gemini_session_id = None,
            Self::OpenRouter => session.openrouter_session_id = None,
            Self::OmniForge => session.omniforge_session_id = None,
            Self::GeminiForge => session.gemini_session_id = None,
            Self::GeminiCliForge => session.gemini_session_id = None,
            Self::MiniMaxForge => session.minimax_session_id = None,
            Self::Kimi => session.kimi_session_id = None,
            Self::OpenCode => {} // OpenCode does not maintain a session ID
            Self::Hermes => session.hermes_session_id = None,
            Self::Forge => {} // Forge uses oneshot transport, no session ID
            Self::Claw => {}  // Claw does not maintain a session ID
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_contains_every_variant() {
        assert_eq!(ModelProvider::ALL.len(), 16);
    }

    #[test]
    fn supported_contains_only_adapter_backed_providers() {
        assert_eq!(
            ModelProvider::SUPPORTED,
            [
                ModelProvider::MiniMax,
                ModelProvider::Codex,
                ModelProvider::Kimi
            ]
        );
    }

    #[test]
    fn launchable_contains_forge_variants() {
        assert_eq!(
            ModelProvider::LAUNCHABLE,
            [
                ModelProvider::MiniMax,
                ModelProvider::Codex,
                ModelProvider::GeminiForge,
                ModelProvider::GeminiCliForge,
                ModelProvider::MiniMaxForge,
                ModelProvider::Kimi,
            ]
        );
    }

    #[test]
    fn as_str_roundtrips_through_from_arg() {
        for provider in ModelProvider::ALL {
            let parsed = ModelProvider::from_arg(provider.as_str());
            assert_eq!(parsed, Some(provider));
        }
    }

    #[test]
    fn as_str_roundtrips_through_from_alias() {
        for provider in ModelProvider::ALL {
            let parsed = ModelProvider::from_alias(provider.as_str());
            assert_eq!(parsed, Some(provider));
        }
    }

    #[test]
    fn from_supported_alias_rejects_non_adapter_backed_providers() {
        assert_eq!(
            ModelProvider::from_supported_alias("codex"),
            Some(ModelProvider::Codex)
        );
        assert_eq!(ModelProvider::from_supported_alias("claude"), None);
        assert_eq!(ModelProvider::from_supported_alias("gemini"), None);
        assert_eq!(ModelProvider::from_supported_alias("geminiforge"), None);
        assert_eq!(ModelProvider::from_supported_alias("geminicliforge"), None);
        assert_eq!(ModelProvider::from_supported_alias("minimaxforge"), None);
    }

    #[test]
    fn from_launch_alias_accepts_forge_variants() {
        assert_eq!(
            ModelProvider::from_launch_alias("geminiforge"),
            Some(ModelProvider::GeminiForge)
        );
        assert_eq!(
            ModelProvider::from_launch_alias("geminicliforge"),
            Some(ModelProvider::GeminiCliForge)
        );
        assert_eq!(
            ModelProvider::from_launch_alias("minimaxforge"),
            Some(ModelProvider::MiniMaxForge)
        );
    }

    #[test]
    fn from_arg_case_insensitive() {
        assert_eq!(
            ModelProvider::from_arg("CLAUDE"),
            Some(ModelProvider::Claude)
        );
        assert_eq!(
            ModelProvider::from_arg("Gemini"),
            Some(ModelProvider::Gemini)
        );
        assert_eq!(
            ModelProvider::from_arg("MiniMax"),
            Some(ModelProvider::MiniMax)
        );
        assert_eq!(
            ModelProvider::from_arg("nvidia"),
            Some(ModelProvider::Nvidia)
        );
        assert_eq!(
            ModelProvider::from_arg("OmniForge"),
            Some(ModelProvider::OmniForge)
        );
        assert_eq!(
            ModelProvider::from_arg("GeminiForge"),
            Some(ModelProvider::GeminiForge)
        );
        assert_eq!(
            ModelProvider::from_arg("GeminiCliForge"),
            Some(ModelProvider::GeminiCliForge)
        );
        assert_eq!(
            ModelProvider::from_arg("MinimaxForge"),
            Some(ModelProvider::MiniMaxForge)
        );
    }

    #[test]
    fn from_arg_unknown_returns_none() {
        assert_eq!(ModelProvider::from_arg("gpt4"), None);
        assert_eq!(ModelProvider::from_arg(""), None);
        assert_eq!(ModelProvider::from_arg("glm"), None);
    }

    #[test]
    fn from_alias_accepts_display_names() {
        assert_eq!(
            ModelProvider::from_alias("MiniMax"),
            Some(ModelProvider::MiniMax)
        );
        assert_eq!(
            ModelProvider::from_alias("Nvidia"),
            Some(ModelProvider::Nvidia)
        );
        assert_eq!(
            ModelProvider::from_alias("OmniForge"),
            Some(ModelProvider::OmniForge)
        );
        assert_eq!(
            ModelProvider::from_alias("GeminiForge"),
            Some(ModelProvider::GeminiForge)
        );
        assert_eq!(
            ModelProvider::from_alias("GeminiCliForge"),
            Some(ModelProvider::GeminiCliForge)
        );
        assert_eq!(
            ModelProvider::from_alias("MinimaxForge"),
            Some(ModelProvider::MiniMaxForge)
        );
    }

    #[test]
    fn shm_code_roundtrips() {
        for provider in ModelProvider::ALL {
            let code = provider.shm_code();
            let recovered = ModelProvider::from_shm_code(code);
            assert_eq!(recovered, Some(provider));
        }
    }

    #[test]
    fn from_shm_code_out_of_range_returns_none() {
        assert_eq!(ModelProvider::from_shm_code(99), None);
        assert_eq!(ModelProvider::from_shm_code(1), None); // gap left by removed GLM provider
        assert_eq!(ModelProvider::from_shm_code(7), None); // gap between Nvidia(6) and OpenRouter(8)
        assert_eq!(ModelProvider::from_shm_code(18), None); // gaps after last provider (17 = Claw)
    }

    #[test]
    fn display_name_is_capitalized() {
        for provider in ModelProvider::ALL {
            let name = provider.display_name();
            assert!(name.chars().next().unwrap().is_uppercase());
        }
    }

    #[test]
    fn shm_codes_are_unique() {
        let codes: Vec<u32> = ModelProvider::ALL.iter().map(|p| p.shm_code()).collect();
        let mut deduped = codes.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(codes.len(), deduped.len());
    }
}
