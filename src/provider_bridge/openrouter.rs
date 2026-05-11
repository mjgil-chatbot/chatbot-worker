fn openrouter_context_window_size(provider: ModelProvider) -> u64 {
    match provider {
        ModelProvider::Qwen => config::QWEN_OPENROUTER_CONTEXT_WINDOW_SIZE,
        _ => config::GEMMA_CONTEXT_WINDOW_SIZE,
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
