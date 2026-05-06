//! Typed runtime provider catalog.
//!
//! Single source of truth for per-provider metadata. The [`ModelProvider`] enum
//! remains the compile-time identity type; this catalog supplies the data behind
//! its accessor methods and replaces scattered constant tables.

use crate::model::ModelProvider;
/// Transport-level grouping that determines which config builder family
/// handles a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportFamily {
    /// Claude API wire format (Claude, MiniMax).
    ClaudeCompat,
    /// Codex CLI wire format (Codex, Nvidia).
    CodexCompat,
    /// Gemini CLI wire format.
    GeminiCli,
    /// Qwen CLI wire format.
    QwenCli,
    /// OpenRouter OpenAI-compatible wire format (direct HTTP via curl).
    OpenRouterCompat,
    /// OpenRouter-style subprocess contract (JSONL stream from a companion CLI).
    OpenRouterCliCompat,
    /// Kimi CLI wire format.
    KimiCli,
}
/// Output normalization strategy applied to provider responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizationKind {
    Claude,
    Codex,
    Gemini,
}
/// All static metadata for one runtime provider.
pub struct ProviderEntry {
    pub provider: ModelProvider,
    /// Canonical lowercase identifier (matches [`ModelProvider::as_str`]).
    pub id: &'static str,
    /// Human-readable display name.
    pub display_name: &'static str,
    /// Shared-memory discriminant — must never change for a released provider.
    pub shm_code: u32,
    pub transport_family: TransportFamily,
    pub normalization: NormalizationKind,
    /// Default model for interactive use (empty string = let CLI decide).
    pub interactive_model: &'static str,
    /// Default model for background/autonomous tasks (empty string = let CLI decide).
    pub background_model: &'static str,
    /// CLI binary name.
    pub cli_binary: &'static str,
    /// Environment variable holding the API key (empty string = provider uses its own auth).
    pub api_key_env: &'static str,
    /// Optional base URL override for API-compatible providers.
    pub base_url: Option<&'static str>,
    /// Config directory name under the shared config root (empty string = none).
    pub config_dir_name: &'static str,
    /// Data directory name under the shared runtime root for session storage.
    pub data_dir_name: &'static str,
    /// Window / app title.
    pub app_title: &'static str,
    /// Desktop app-id for window matching.
    pub app_id: &'static str,
    /// Case-insensitive aliases accepted by [`by_alias`] (includes the canonical id).
    pub aliases: &'static [&'static str],
}
/// All runtime providers.  Order is not significant but is kept
/// stable for readability.
pub static PROVIDERS: &[ProviderEntry] = &[
    // 0 — Claude
    ProviderEntry {
        provider: ModelProvider::Claude,
        id: "claude",
        display_name: "Claude",
        shm_code: 0,
        transport_family: TransportFamily::ClaudeCompat,
        normalization: NormalizationKind::Claude,
        interactive_model: "claude-opus-4-6",
        background_model: "claude-opus-4-6",
        cli_binary: "claude",
        api_key_env: "",
        base_url: None,
        config_dir_name: ".claude-desktop",
        data_dir_name: ".claude-desktop",
        app_title: "Claude Rust Chatbot",
        app_id: "rust-chatbot-claude",
        aliases: &["claude"],
    },
    // 2 — MiniMax
    ProviderEntry {
        provider: ModelProvider::MiniMax,
        id: "minimax",
        display_name: "MiniMax",
        shm_code: 2,
        transport_family: TransportFamily::ClaudeCompat,
        normalization: NormalizationKind::Claude,
        interactive_model: "MiniMax-M2.7",
        background_model: "MiniMax-M2.7",
        cli_binary: "claude",
        api_key_env: "MINIMAX_API_KEY",
        base_url: Some("https://api.minimax.io/anthropic"),
        config_dir_name: ".minimax",
        data_dir_name: ".minimax-desktop",
        app_title: "MiniMax Rust Chatbot",
        app_id: "rust-chatbot-minimax",
        aliases: &["minimax"],
    },
    // 3 — Codex
    ProviderEntry {
        provider: ModelProvider::Codex,
        id: "codex",
        display_name: "Codex",
        shm_code: 3,
        transport_family: TransportFamily::CodexCompat,
        normalization: NormalizationKind::Codex,
        interactive_model: "gpt-5.4",
        background_model: "gpt-5.4",
        cli_binary: "codex",
        api_key_env: "",
        base_url: None,
        config_dir_name: ".codex",
        data_dir_name: ".codex-desktop",
        app_title: "Codex Rust Chatbot",
        app_id: "rust-chatbot-codex",
        aliases: &["codex"],
    },
    // 4 — Gemini
    ProviderEntry {
        provider: ModelProvider::Gemini,
        id: "gemini",
        display_name: "Gemini",
        shm_code: 4,
        transport_family: TransportFamily::GeminiCli,
        normalization: NormalizationKind::Gemini,
        interactive_model: "gemini-3-flash-preview",
        background_model: "gemini-3-flash-preview",
        cli_binary: "gemini",
        api_key_env: "",
        base_url: None,
        config_dir_name: ".gemini",
        data_dir_name: ".gemini-desktop",
        app_title: "Gemini Rust Chatbot",
        app_id: "rust-chatbot-gemini",
        aliases: &["gemini"],
    },
    // 5 — Qwen (OpenRouter-backed)
    ProviderEntry {
        provider: ModelProvider::Qwen,
        id: "qwen",
        display_name: "Qwen",
        shm_code: 5,
        transport_family: TransportFamily::OpenRouterCompat,
        normalization: NormalizationKind::Claude,
        interactive_model: "qwen/qwen3-coder:free",
        background_model: "qwen/qwen3-coder:free",
        cli_binary: "openrouter-cli",
        api_key_env: "OPENROUTER_API_KEY",
        base_url: Some("https://openrouter.ai/api/v1"),
        config_dir_name: ".qwen",
        data_dir_name: ".qwen-desktop",
        app_title: "Qwen Rust Chatbot",
        app_id: "rust-chatbot-qwen",
        aliases: &["qwen"],
    },
    // 6 — Nvidia (Nemotron via NVIDIA AI Playground)
    ProviderEntry {
        provider: ModelProvider::Nvidia,
        id: "nvidia",
        display_name: "Nvidia",
        shm_code: 6,
        transport_family: TransportFamily::CodexCompat,
        normalization: NormalizationKind::Codex,
        interactive_model: "nvidia/nemotron-3-super-120b-a12b",
        background_model: "nvidia/nemotron-3-super-120b-a12b",
        cli_binary: "codex",
        api_key_env: "OPENAI_API_KEY",
        base_url: Some("https://integrate.api.nvidia.com/v1"),
        config_dir_name: ".nvidia",
        data_dir_name: ".nvidia-desktop",
        app_title: "Nvidia Rust Chatbot",
        app_id: "rust-chatbot-nvidia",
        aliases: &["nvidia"],
    },
    // 8 — OpenRouter
    ProviderEntry {
        provider: ModelProvider::OpenRouter,
        id: "openrouter",
        display_name: "OpenRouter",
        shm_code: 8,
        transport_family: TransportFamily::OpenRouterCompat,
        normalization: NormalizationKind::Claude,
        interactive_model: "google/gemma-4-31b-it:free",
        background_model: "google/gemma-4-31b-it:free",
        cli_binary: "openrouter-cli",
        api_key_env: "OPENROUTER_API_KEY",
        base_url: Some("https://openrouter.ai/api/v1"),
        config_dir_name: ".openrouter",
        data_dir_name: ".openrouter-desktop",
        app_title: "OpenRouter Rust Chatbot",
        app_id: "rust-chatbot-openrouter",
        aliases: &["openrouter", "gemma"],
    },
    // 9 — OmniForge
    ProviderEntry {
        provider: ModelProvider::OmniForge,
        id: "omniforge",
        display_name: "OmniForge",
        shm_code: 9,
        transport_family: TransportFamily::OpenRouterCliCompat,
        normalization: NormalizationKind::Gemini,
        interactive_model: "gemini-3-flash-preview",
        background_model: "gemini-3-flash-preview",
        cli_binary: "rust-chatbot",
        api_key_env: "",
        base_url: Some("https://openrouter.ai/api/v1"),
        config_dir_name: ".omniforge",
        data_dir_name: ".omniforge-desktop",
        app_title: "OmniForge Rust Chatbot",
        app_id: "rust-chatbot-omniforge",
        aliases: &["omniforge", "omni-forge", "omni_forge"],
    },
    // 10 — GeminiForge
    ProviderEntry {
        provider: ModelProvider::GeminiForge,
        id: "geminiforge",
        display_name: "GeminiForge",
        shm_code: 10,
        transport_family: TransportFamily::OpenRouterCliCompat,
        normalization: NormalizationKind::Gemini,
        interactive_model: "gemini-3-flash-preview",
        background_model: "gemini-3-flash-preview",
        cli_binary: "rust-chatbot",
        api_key_env: "GEMINI_API_KEY",
        base_url: Some("https://generativelanguage.googleapis.com/v1beta"),
        config_dir_name: ".gemini-forge",
        data_dir_name: ".gemini-forge-desktop",
        app_title: "GeminiForge Rust Chatbot",
        app_id: "rust-chatbot-geminiforge",
        aliases: &["geminiforge", "gemini-forge", "gemini_forge"],
    },
    // 12 — GeminiCliForge
    ProviderEntry {
        provider: ModelProvider::GeminiCliForge,
        id: "geminicliforge",
        display_name: "GeminiCliForge",
        shm_code: 12,
        transport_family: TransportFamily::OpenRouterCliCompat,
        normalization: NormalizationKind::Gemini,
        interactive_model: "gemini-3-flash-preview",
        background_model: "gemini-3-flash-preview",
        cli_binary: "rust-chatbot",
        api_key_env: "",
        base_url: Some("https://openrouter.ai/api/v1"),
        config_dir_name: ".gemini-cli-forge",
        data_dir_name: ".gemini-cli-forge-desktop",
        app_title: "GeminiCliForge Rust Chatbot",
        app_id: "rust-chatbot-geminicliforge",
        aliases: &["geminicliforge", "gemini-cli-forge", "gemini_cli_forge"],
    },
    // 11 — MiniMaxForge
    ProviderEntry {
        provider: ModelProvider::MiniMaxForge,
        id: "minimaxforge",
        display_name: "MiniMaxForge",
        shm_code: 11,
        transport_family: TransportFamily::OpenRouterCliCompat,
        normalization: NormalizationKind::Claude,
        interactive_model: "MiniMax-M2.7",
        background_model: "MiniMax-M2.7",
        cli_binary: "rust-chatbot",
        api_key_env: "MINIMAX_API_KEY",
        base_url: Some("https://api.minimax.io/anthropic"),
        config_dir_name: ".minimax-forge",
        data_dir_name: ".minimax-forge-desktop",
        app_title: "MiniMaxForge Rust Chatbot",
        app_id: "rust-chatbot-minimaxforge",
        aliases: &["minimaxforge", "minimax-forge", "minimax_forge"],
    },
    // 13 — Kimi
    ProviderEntry {
        provider: ModelProvider::Kimi,
        id: "kimi",
        display_name: "Kimi",
        shm_code: 13,
        transport_family: TransportFamily::KimiCli,
        normalization: NormalizationKind::Claude,
        interactive_model: "kimi-code/kimi-for-coding",
        background_model: "kimi-code/kimi-for-coding",
        cli_binary: "kimi",
        api_key_env: "",
        base_url: None,
        config_dir_name: ".kimi",
        data_dir_name: ".kimi-desktop",
        app_title: "Kimi Rust Chatbot",
        app_id: "rust-chatbot-kimi",
        aliases: &["kimi"],
    },
    // 14 — OpenCode
    ProviderEntry {
        provider: ModelProvider::OpenCode,
        id: "opencode",
        display_name: "OpenCode",
        shm_code: 14,
        transport_family: TransportFamily::KimiCli,
        normalization: NormalizationKind::Claude,
        interactive_model: "opencode",
        background_model: "opencode",
        cli_binary: "opencode",
        api_key_env: "",
        base_url: None,
        config_dir_name: ".opencode",
        data_dir_name: ".opencode-desktop",
        app_title: "OpenCode Rust Chatbot",
        app_id: "rust-chatbot-opencode",
        aliases: &["opencode"],
    },
    // 15 — Hermes
    ProviderEntry {
        provider: ModelProvider::Hermes,
        id: "hermes",
        display_name: "Hermes",
        shm_code: 15,
        transport_family: TransportFamily::KimiCli,
        normalization: NormalizationKind::Claude,
        interactive_model: "hermes",
        background_model: "hermes",
        cli_binary: "hermes-agent",
        api_key_env: "",
        base_url: None,
        config_dir_name: ".hermes",
        data_dir_name: ".hermes-desktop",
        app_title: "Hermes Rust Chatbot",
        app_id: "rust-chatbot-hermes",
        aliases: &["hermes"],
    },
    // 16 — Forge
    ProviderEntry {
        provider: ModelProvider::Forge,
        id: "forge",
        display_name: "Forge",
        shm_code: 16,
        transport_family: TransportFamily::KimiCli,
        normalization: NormalizationKind::Claude,
        interactive_model: "forge",
        background_model: "forge",
        cli_binary: "forge",
        api_key_env: "",
        base_url: None,
        config_dir_name: ".forge",
        data_dir_name: ".forge-desktop",
        app_title: "Forge Rust Chatbot",
        app_id: "rust-chatbot-forge",
        aliases: &["forge"],
    },
    // 17 — Claw
    ProviderEntry {
        provider: ModelProvider::Claw,
        id: "claw",
        display_name: "Claw",
        shm_code: 17,
        transport_family: TransportFamily::KimiCli,
        normalization: NormalizationKind::Claude,
        interactive_model: "claw",
        background_model: "claw",
        cli_binary: "claw",
        api_key_env: "",
        base_url: None,
        config_dir_name: ".claw",
        data_dir_name: ".claw-desktop",
        app_title: "Claw Rust Chatbot",
        app_id: "rust-chatbot-claw",
        aliases: &["claw"],
    },
];
/// Look up a catalog entry by its [`ModelProvider`] variant.
///
/// # Panics
///
/// Panics if the catalog does not contain an entry for the given provider.
/// This is a programming error — the catalog must cover every variant.
pub fn by_id(provider: ModelProvider) -> &'static ProviderEntry {
    PROVIDERS
        .iter()
        .find(|e| e.provider == provider)
        .unwrap_or_else(|| {
            panic!(
                "provider_catalog: no entry for {:?} — catalog is incomplete",
                provider
            )
        })
}
/// Look up a catalog entry by its shared-memory discriminant code.
pub fn by_shm_code(code: u32) -> Option<&'static ProviderEntry> {
    PROVIDERS.iter().find(|e| e.shm_code == code)
}
