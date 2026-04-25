//! Model provider selection and display helpers.

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
