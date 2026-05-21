use super::*;
use crate::model::ModelProvider;
use crate::provider_catalog;

// ── Catalog-backed accessor functions ────────────────────────────────

#[test]
fn interactive_model_for_all_providers() {
    assert_eq!(
        interactive_model_for(ModelProvider::Claude),
        Some("claude-opus-4-6")
    );
    assert_eq!(
        interactive_model_for(ModelProvider::MiniMax),
        Some("MiniMax-M2.7")
    );
    assert_eq!(interactive_model_for(ModelProvider::Codex), Some("gpt-5.4"));
    assert_eq!(
        interactive_model_for(ModelProvider::Gemini),
        Some("gemini-3-flash-preview")
    );
    assert_eq!(
        interactive_model_for(ModelProvider::Qwen),
        Some("qwen/qwen3-coder:free")
    );
    assert_eq!(
        interactive_model_for(ModelProvider::Nvidia),
        Some("nvidia/nemotron-3-super-120b-a12b")
    );
    assert_eq!(
        interactive_model_for(ModelProvider::OpenRouter),
        Some("google/gemma-4-31b-it:free")
    );
    assert_eq!(
        interactive_model_for(ModelProvider::OmniForge),
        Some("gemini-3-flash-preview")
    );
    assert_eq!(
        interactive_model_for(ModelProvider::GeminiForge),
        Some("gemini-3-flash-preview")
    );
    assert_eq!(
        interactive_model_for(ModelProvider::GeminiCliForge),
        Some("gemini-3-flash-preview")
    );
    assert_eq!(
        interactive_model_for(ModelProvider::MiniMaxForge),
        Some("MiniMax-M2.7")
    );
    assert_eq!(
        interactive_model_for(ModelProvider::Kimi),
        Some("kimi-code/kimi-for-coding")
    );
}

#[test]
fn background_model_for_all_providers() {
    assert_eq!(
        background_model_for(ModelProvider::Claude),
        Some("claude-opus-4-6")
    );
    assert_eq!(
        background_model_for(ModelProvider::MiniMax),
        Some("MiniMax-M2.7")
    );
    assert_eq!(background_model_for(ModelProvider::Codex), Some("gpt-5.4"));
    assert_eq!(
        background_model_for(ModelProvider::Gemini),
        Some("gemini-3-flash-preview")
    );
    assert_eq!(
        background_model_for(ModelProvider::Qwen),
        Some("qwen/qwen3-coder:free")
    );
    assert_eq!(
        background_model_for(ModelProvider::Nvidia),
        Some("nvidia/nemotron-3-super-120b-a12b")
    );
    assert_eq!(
        background_model_for(ModelProvider::OpenRouter),
        Some("google/gemma-4-31b-it:free")
    );
    assert_eq!(
        background_model_for(ModelProvider::OmniForge),
        Some("gemini-3-flash-preview")
    );
    assert_eq!(
        background_model_for(ModelProvider::GeminiForge),
        Some("gemini-3-flash-preview")
    );
    assert_eq!(
        background_model_for(ModelProvider::GeminiCliForge),
        Some("gemini-3-flash-preview")
    );
    assert_eq!(
        background_model_for(ModelProvider::MiniMaxForge),
        Some("MiniMax-M2.7")
    );
    assert_eq!(
        background_model_for(ModelProvider::Kimi),
        Some("kimi-code/kimi-for-coding")
    );
}

#[test]
fn config_dir_name_for_all_providers() {
    assert_eq!(
        config_dir_name_for(ModelProvider::Claude),
        ".claude-desktop"
    );
    assert_eq!(config_dir_name_for(ModelProvider::MiniMax), ".minimax");
    assert_eq!(config_dir_name_for(ModelProvider::Qwen), ".qwen");
    assert_eq!(config_dir_name_for(ModelProvider::Codex), ".codex");
    assert_eq!(config_dir_name_for(ModelProvider::Nvidia), ".nvidia");
    assert_eq!(config_dir_name_for(ModelProvider::Gemini), ".gemini");
    assert_eq!(
        config_dir_name_for(ModelProvider::OpenRouter),
        ".openrouter"
    );
    assert_eq!(config_dir_name_for(ModelProvider::OmniForge), ".omniforge");
    assert_eq!(
        config_dir_name_for(ModelProvider::GeminiForge),
        ".gemini-forge"
    );
    assert_eq!(
        config_dir_name_for(ModelProvider::GeminiCliForge),
        ".gemini-cli-forge"
    );
    assert_eq!(
        config_dir_name_for(ModelProvider::MiniMaxForge),
        ".minimax-forge"
    );
}

#[test]
fn api_key_env_for_delegates_to_catalog() {
    assert_eq!(api_key_env_for(ModelProvider::Claude), None);
    assert_eq!(api_key_env_for(ModelProvider::Codex), None);
    assert_eq!(api_key_env_for(ModelProvider::Gemini), None);
    assert_eq!(api_key_env_for(ModelProvider::OmniForge), None);
    assert_eq!(api_key_env_for(ModelProvider::GeminiCliForge), None);
    assert_eq!(
        api_key_env_for(ModelProvider::GeminiForge),
        Some("GEMINI_API_KEY")
    );
    assert_eq!(
        api_key_env_for(ModelProvider::MiniMax),
        Some("MINIMAX_API_KEY")
    );
    assert_eq!(
        api_key_env_for(ModelProvider::MiniMaxForge),
        Some("MINIMAX_API_KEY")
    );
    assert_eq!(
        api_key_env_for(ModelProvider::Nvidia),
        Some("OPENAI_API_KEY")
    );
    assert_eq!(
        api_key_env_for(ModelProvider::Qwen),
        Some("OPENROUTER_API_KEY")
    );
    assert_eq!(
        api_key_env_for(ModelProvider::OpenRouter),
        Some("OPENROUTER_API_KEY")
    );
}

#[test]
fn base_url_for_delegates_to_catalog() {
    assert_eq!(base_url_for(ModelProvider::Claude), None);
    assert_eq!(base_url_for(ModelProvider::Codex), None);
    assert_eq!(base_url_for(ModelProvider::Gemini), None);
    assert_eq!(
        base_url_for(ModelProvider::OmniForge),
        Some("https://openrouter.ai/api/v1")
    );
    assert_eq!(
        base_url_for(ModelProvider::GeminiCliForge),
        Some("https://openrouter.ai/api/v1")
    );
    assert_eq!(
        base_url_for(ModelProvider::GeminiForge),
        Some("https://generativelanguage.googleapis.com/v1beta")
    );
    assert_eq!(
        base_url_for(ModelProvider::MiniMax),
        Some("https://api.minimax.io/anthropic")
    );
    assert_eq!(
        base_url_for(ModelProvider::MiniMaxForge),
        Some("https://api.minimax.io/anthropic")
    );
    assert_eq!(
        base_url_for(ModelProvider::Nvidia),
        Some("https://integrate.api.nvidia.com/v1")
    );
    assert_eq!(
        base_url_for(ModelProvider::Qwen),
        Some("https://openrouter.ai/api/v1")
    );
    assert_eq!(
        base_url_for(ModelProvider::OpenRouter),
        Some("https://openrouter.ai/api/v1")
    );
}

#[test]
fn provider_api_key_paths_follow_catalog_config_dirs() {
    assert!(minimax_api_key_path().ends_with("/.minimax/api.key"));
    assert!(nvidia_api_key_path().ends_with("/.nvidia/api.key"));
    assert!(openrouter_api_key_path().ends_with("/.openrouter/api.key"));
    assert!(legacy_openrouter_api_key_path().ends_with("/.gemma/api.key"));
    assert!(provider_api_key_path(ModelProvider::GeminiForge)
        .expect("GeminiForge should define an api key path")
        .ends_with("/.gemini-forge/api.key"));
    assert!(provider_api_key_path(ModelProvider::MiniMaxForge)
        .expect("MiniMaxForge should define an api key path")
        .ends_with("/.minimax-forge/api.key"));
}

#[test]
fn get_config_dir_returns_effective_config_path() {
    for provider in ModelProvider::ALL {
        let dir = get_config_dir(provider);
        let entry = provider_catalog::by_id(provider);
        if entry.config_dir_name.is_empty() {
            assert!(dir.is_none(), "expected None for {:?}", provider);
        } else {
            let d = dir.unwrap();
            assert!(
                d.ends_with(entry.config_dir_name),
                "expected path ending with {} for {:?}, got {}",
                entry.config_dir_name,
                provider,
                d
            );
        }
    }
}

// ── Catalog-backed path helpers delegate correctly ───────────────────

#[test]
fn data_dir_name_delegates_to_catalog() {
    for provider in ModelProvider::ALL {
        let expected = provider_catalog::by_id(provider).data_dir_name;
        assert_eq!(
            data_dir_name(provider),
            expected,
            "data_dir_name mismatch for {:?}",
            provider
        );
    }
}

#[test]
fn app_title_for_delegates_to_catalog() {
    for provider in ModelProvider::ALL {
        let expected = provider_catalog::by_id(provider).app_title;
        assert_eq!(
            app_title_for(provider),
            expected,
            "app_title_for mismatch for {:?}",
            provider
        );
    }
}

#[test]
fn app_id_for_delegates_to_catalog() {
    for provider in ModelProvider::ALL {
        let expected = provider_catalog::by_id(provider).app_id;
        assert_eq!(
            app_id_for(provider),
            expected,
            "app_id_for mismatch for {:?}",
            provider
        );
    }
}
