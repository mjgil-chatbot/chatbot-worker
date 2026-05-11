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
    ];}
impl ModelProvider {
    pub const SUPPORTED: [Self; 3] = [Self::MiniMax, Self::Codex, Self::Kimi];}
impl ModelProvider {
    pub const LAUNCHABLE: [Self; 6] = [
        Self::MiniMax,
        Self::Codex,
        Self::GeminiForge,
        Self::GeminiCliForge,
        Self::MiniMaxForge,
        Self::Kimi,
    ];}
impl ModelProvider {
    /// Canonical lowercase identifier, e.g. `"claude"`, `"codex"`.
    pub fn as_str(&self) -> &'static str {
        provider_catalog::by_id(*self).id
    }}
impl ModelProvider {
    /// Human-readable display name, e.g. `"Claude"`, `"Codex"`.
    pub fn display_name(&self) -> &'static str {
        provider_catalog::by_id(*self).display_name
    }}
impl ModelProvider {
    /// Parse a provider name from any known alias (case-insensitive).
    ///
    /// This is the unified provider parser backed by the catalog.
    /// It replaces all ad hoc string-match tables.
    pub fn from_alias(value: &str) -> Option<Self> {
        provider_catalog::by_alias(value).map(|e| e.provider)
    }}
impl ModelProvider {
    pub fn from_supported_alias(value: &str) -> Option<Self> {
        Self::from_alias(value).filter(|provider| provider.is_supported())
    }}
impl ModelProvider {
    pub fn from_launch_alias(value: &str) -> Option<Self> {
        Self::from_alias(value).filter(|provider| provider.is_launchable())
    }}
impl ModelProvider {
    /// Legacy alias for [`Self::from_alias`].
    pub fn from_arg(value: &str) -> Option<Self> {
        Self::from_alias(value)
    }}
impl ModelProvider {
    /// Shared-memory discriminant code for this provider.
    pub fn shm_code(self) -> u32 {
        provider_catalog::by_id(self).shm_code
    }}
impl ModelProvider {
    /// Recover a provider from its shared-memory discriminant code.
    pub fn from_shm_code(value: u32) -> Option<Self> {
        provider_catalog::by_shm_code(value).map(|e| e.provider)
    }}
impl ModelProvider {
    pub fn is_supported(self) -> bool {
        Self::SUPPORTED.contains(&self)
    }}
impl ModelProvider {
    pub fn is_launchable(self) -> bool {
        Self::LAUNCHABLE.contains(&self)
    }}
