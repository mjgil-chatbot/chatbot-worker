use super::*;
use crate::model::ModelProvider;

#[test]
fn provider_capability_registry_default_is_empty() {
    let registry = ProviderCapabilityRegistry::default();
    assert!(registry.providers.is_empty());
}

// ── ProviderConfig default ────────────────────────────────────────

#[test]
fn provider_config_default_values() {
    let cfg = ProviderConfig::default();
    assert_eq!(cfg.default_provider, "codex");
    assert!(cfg.claude_reasoning_effort.is_none());
    assert!(cfg.codex_reasoning_effort.is_none());
    assert!(cfg.minimax_reasoning_effort.is_none());
    assert!(cfg.codex_sandbox_mode.is_none());
    assert!(cfg.gemini_approval_mode.is_none());
    assert!(cfg.allow_api_billing.is_none());
    assert!(cfg.max_concurrency.is_none());
}

#[test]
fn effective_codex_reasoning_effort_falls_back_to_default() {
    // When no config file sets codex_reasoning_effort, the effective value
    // should fall back to the constant default (high).
    reset_config();
    let effort = effective_codex_reasoning_effort();
    assert_eq!(effort, Some("high".to_string()));
}

#[test]
fn effective_minimax_reasoning_effort_falls_back_to_default() {
    // When no config file sets minimax_reasoning_effort, the effective value
    // should fall back to the constant default (xhigh).
    reset_config();
    let effort = effective_minimax_reasoning_effort();
    assert_eq!(effort, Some("xhigh".to_string()));
}

#[test]
fn effective_claude_reasoning_effort_falls_back_to_default() {
    // When no config file sets claude_reasoning_effort, the effective value
    // should fall back to the constant default (high).
    reset_config();
    let effort = effective_claude_reasoning_effort();
    assert_eq!(effort, Some("high".to_string()));
}

#[test]
fn effective_reasoning_effort_for_provider_dispatches() {
    reset_config();
    assert_eq!(
        effective_reasoning_effort_for_provider(ModelProvider::Claude),
        Some("high".to_string())
    );
    assert_eq!(
        effective_reasoning_effort_for_provider(ModelProvider::Codex),
        Some("high".to_string())
    );
    assert_eq!(
        effective_reasoning_effort_for_provider(ModelProvider::MiniMax),
        Some("xhigh".to_string())
    );
    assert_eq!(
        effective_reasoning_effort_for_provider(ModelProvider::Qwen),
        None
    );
}

// ── BackgroundTaskConfig default ───────────────────────────────────

#[test]
fn coordination_config_background_design_doc_path_default() {
    let cfg = BackgroundTaskConfig::default();
    assert_eq!(
        cfg.background_design_doc_path,
        "docs/autonomous/design-doc.md"
    );
    assert!(cfg.design_doc_path.is_none());
}

// ── MemoryConfig default ─────────────────────────────────────────

#[test]
fn memory_config_default_values() {
    let cfg = MemoryConfig::default();
    assert!(cfg.enable_workspace_memory);
    assert!(cfg.memmachine_base_url.is_none());
    assert!(cfg.memmachine_org_id.is_none());
    assert!(cfg.memory_top_k.is_none());
}

// ── Provider profile: full round-trip with all fields ────────────

#[test]
fn provider_profile_full_serialization_round_trip() {
    let profile = ProviderCapabilityProfile {
        provider_id: "test-rt".to_string(),
        model_override: Some("test-model".to_string()),
        prompt_family: PromptFamily::Mechanics,
        supported_categories: vec![TaskCategory::DeepWrite, TaskCategory::Review],
        preferred_categories: vec![TaskCategory::DeepWrite],
        fallback_chain: vec!["codex".to_string(), "minimax".to_string()],
        cooldown_eligible: true,
        max_concurrency: 12,
        cost_tier: CostTier::Premium,
        notes: "Test profile for round-trip".to_string(),
    };
    let json = serde_json::to_string(&profile).unwrap();
    let parsed: ProviderCapabilityProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.provider_id, "test-rt");
    assert_eq!(parsed.model_override, Some("test-model".to_string()));
    assert_eq!(parsed.prompt_family, PromptFamily::Mechanics);
    assert!(parsed.cooldown_eligible);
    assert_eq!(parsed.max_concurrency, 12);
    assert_eq!(parsed.cost_tier, CostTier::Premium);
    assert_eq!(parsed.notes, "Test profile for round-trip");
    assert_eq!(parsed.supported_categories.len(), 2);
    assert_eq!(parsed.fallback_chain.len(), 2);
}

// ── Provider profile: duplicate provider_ids last wins ───────────

#[test]
fn provider_profiles_duplicate_ids_last_wins() {
    let json = r#"{"providers": [
            {"provider_id": "dup", "cost_tier": "low"},
            {"provider_id": "dup", "cost_tier": "high"}
        ]}"#;
    let registry: ProviderCapabilityRegistry = serde_json::from_str(json).unwrap();
    let map: BTreeMap<String, ProviderCapabilityProfile> = registry
        .providers
        .into_iter()
        .map(|p| (p.provider_id.clone(), p))
        .collect();
    // BTreeMap insert: last value wins for same key
    assert_eq!(map.get("dup").unwrap().cost_tier, CostTier::High);
}

// ── Provider profile: notes and cooldown fields ──────────────────

#[test]
fn provider_profile_notes_and_cooldown_from_json() {
    let json = r#"{"providers": [{
            "provider_id": "test",
            "cooldown_eligible": true,
            "notes": "Important note"
        }]}"#;
    let registry: ProviderCapabilityRegistry = serde_json::from_str(json).unwrap();
    assert!(registry.providers[0].cooldown_eligible);
    assert_eq!(registry.providers[0].notes, "Important note");
}

// ── Provider profile: unknown prompt_family errors ────────────────

#[test]
fn provider_profile_unknown_prompt_family_errors() {
    let json = r#"{"providers": [{"provider_id": "test", "prompt_family": "unknown_family"}]}"#;
    let result: Result<ProviderCapabilityRegistry, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

// ── Provider profile: unknown cost_tier errors ────────────────────

#[test]
fn provider_profile_unknown_cost_tier_errors() {
    let json = r#"{"providers": [{"provider_id": "test", "cost_tier": "ultra"}]}"#;
    let result: Result<ProviderCapabilityRegistry, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

// ── Config validation: validate() always returns Ok ──────────────

#[test]
fn validate_always_returns_ok_even_with_missing_registry() {
    let cfg = AppConfig::default();
    // validate() warns on stderr but returns Ok
    assert!(cfg.validate().is_ok());
}

// ── Config: type mismatch in TOML produces error ─────────────────

#[test]
fn type_mismatch_in_toml_produces_error() {
    let dir = temp_dir();
    let toml_content = r#"
[ui]
window_width = "not_a_number"
"#;
    let path = write_toml(&dir, "chatbot-config.toml", toml_content);
    let result = AppConfig::from_toml_path(&path);
    assert!(result.is_err());
}

// ── Config: both design doc paths set ────────────────────────────

#[test]
fn both_design_doc_paths_override() {
    let dir = temp_dir();
    let toml_content = r#"
[coordination]
design_doc_path = "docs/custom.md"
background_design_doc_path = "docs/auto-custom.md"
"#;
    let path = write_toml(&dir, "chatbot-config.toml", toml_content);
    let (cfg, _) = AppConfig::load_from(&[path]);
    assert_eq!(
        cfg.background.design_doc_path,
        Some("docs/custom.md".to_string())
    );
    assert_eq!(
        cfg.background.background_design_doc_path,
        "docs/auto-custom.md"
    );
}

// ── Config: MemMachine overrides ────────────────────────────────

#[test]
fn memmachine_config_overrides() {
    let dir = temp_dir();
    let toml_content = r#"
[memory]
memmachine_base_url = "http://memmachine.internal:8080/"
memmachine_org_id = "workspace-tests"
"#;
    let path = write_toml(&dir, "chatbot-config.toml", toml_content);
    let (cfg, _) = AppConfig::load_from(&[path]);
    assert_eq!(
        cfg.memory.memmachine_base_url,
        Some("http://memmachine.internal:8080/".to_string())
    );
    assert_eq!(
        cfg.memory.memmachine_org_id,
        Some("workspace-tests".to_string())
    );
}

// ── PromptFamily round-trip all variants ─────────────────────────

#[test]
fn prompt_family_round_trip_all_variants() {
    for (family, expected_json) in [
        (PromptFamily::Mechanics, "\"mechanics\""),
        (PromptFamily::Principles, "\"principles\""),
    ] {
        let json = serde_json::to_string(&family).unwrap();
        assert_eq!(json, expected_json);
        let decoded: PromptFamily = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, family);
    }
}

// ── load_provider_registry_from: not-found returns embedded ──────

#[test]
fn load_provider_registry_from_not_found_returns_embedded() {
    let dir = temp_dir();
    let missing = dir.path().join("nonexistent-dir").join("providers.json");
    let registry = AppConfig::load_provider_registry_from(&missing).unwrap();
    assert!(!registry.providers.is_empty());
    assert!(registry.providers.iter().any(|p| p.provider_id == "claude"));
}

// ── has_mcp_servers: no JSON file ────────────────────────────────

#[test]
fn has_mcp_servers_returns_bool() {
    // This just exercises the function; the result depends on the local
    // checkout's .claude-desktop/.claude.json state.
    let _result = has_mcp_servers();
}

// ── config: load_from with empty search list ─────────────────────

#[test]
fn load_from_empty_search_list_returns_defaults() {
    let (cfg, path) = AppConfig::load_from(&[]);
    assert!(path.is_none());
    assert_eq!(cfg.provider.default_provider, "codex");
}

// ── config: from_toml_path with non-table content ────────────────

#[test]
fn from_toml_path_with_non_table_toml_errors() {
    let dir = temp_dir();
    // A TOML file with just a bare string is not a table
    let path = write_toml(&dir, "bare.toml", "\"just a string\"");
    let result = AppConfig::from_toml_path(&path);
    assert!(result.is_err());
}

// ── config: load_provider_registry_from corrupt binary file ──────

#[test]
fn load_provider_registry_from_corrupt_file_errors() {
    let dir = temp_dir();
    let path = dir.path().join("corrupt.json");
    std::fs::write(&path, &[0xFF, 0xFE, 0x00, 0x01]).unwrap();
    let result = AppConfig::load_provider_registry_from(&path);
    assert!(result.is_err());
}

// ── config: provider profile with all categories ─────────────────

#[test]
fn provider_profile_all_categories_round_trip() {
    let json = r#"{"providers": [{
            "provider_id": "full",
            "supported_categories": ["survey", "design", "plan", "deep_write", "quick_write", "review", "verify", "doc_write", "research"],
            "preferred_categories": ["design", "review"]
        }]}"#;
    let registry: ProviderCapabilityRegistry = serde_json::from_str(json).unwrap();
    assert_eq!(registry.providers[0].supported_categories.len(), 9);
}

// ── app_title_for and app_id_for all providers ──────────────────

#[test]
fn app_title_and_id_cover_all_providers() {
    use crate::model::ModelProvider;
    for provider in [
        ModelProvider::Claude,
        ModelProvider::MiniMax,
        ModelProvider::Qwen,
        ModelProvider::Codex,
        ModelProvider::Nvidia,
        ModelProvider::Gemini,
    ] {
        let title = app_title_for(provider);
        assert!(!title.is_empty());
        let id = app_id_for(provider);
        assert!(id.starts_with("rust-chatbot-"));
        let dir = data_dir_name(provider);
        assert!(dir.starts_with('.'));
    }
}

// ── window_title with no instance ────────────────────────────────

#[test]
fn window_title_without_instance() {
    let title = window_title(crate::model::ModelProvider::Gemini, None);
    assert!(title.contains("Gemini"));
}

// ── codex_cli_path env override is test-only ─────────────────────

#[test]
fn codex_cli_path_env_override_for_tests() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original = std::env::var_os(RUST_CHATBOT_CODEX_CLI_ENV);
    std::env::set_var(RUST_CHATBOT_CODEX_CLI_ENV, "/custom/codex");
    let path = codex_cli_path();
    assert_eq!(path, "/custom/codex");
    match original {
        Some(v) => std::env::set_var(RUST_CHATBOT_CODEX_CLI_ENV, v),
        None => std::env::remove_var(RUST_CHATBOT_CODEX_CLI_ENV),
    }
}

// ── codex_cli_path empty env falls back to the system codex command ─────

#[test]
fn codex_cli_path_empty_env_falls_back_to_system_codex_command() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original = std::env::var_os(RUST_CHATBOT_CODEX_CLI_ENV);
    std::env::set_var(RUST_CHATBOT_CODEX_CLI_ENV, "");
    let path = codex_cli_path();
    assert_eq!(path, "codex");
    match original {
        Some(v) => std::env::set_var(RUST_CHATBOT_CODEX_CLI_ENV, v),
        None => std::env::remove_var(RUST_CHATBOT_CODEX_CLI_ENV),
    }
}

#[test]
fn codex_cli_path_uses_system_command_even_when_path_contains_codex() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_dir();
    let codex = dir.path().join("codex");
    std::fs::write(&codex, "#!/bin/sh\nexit 0\n").unwrap();

    let original_override = std::env::var_os(RUST_CHATBOT_CODEX_CLI_ENV);
    let original_path = std::env::var_os("PATH");
    std::env::remove_var(RUST_CHATBOT_CODEX_CLI_ENV);
    std::env::set_var("PATH", dir.path());

    let path = codex_cli_path();
    assert_eq!(path, "codex");

    match original_path {
        Some(v) => std::env::set_var("PATH", v),
        None => std::env::remove_var("PATH"),
    }
    match original_override {
        Some(v) => std::env::set_var(RUST_CHATBOT_CODEX_CLI_ENV, v),
        None => std::env::remove_var(RUST_CHATBOT_CODEX_CLI_ENV),
    }
}

#[test]
fn codex_cli_path_uses_system_command_even_with_home_asdf_shim() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let home = temp_dir();
    let shim = home.path().join(".asdf/shims/codex");
    std::fs::create_dir_all(shim.parent().unwrap()).unwrap();
    std::fs::write(&shim, "#!/bin/sh\nexit 0\n").unwrap();

    let original_home = std::env::var_os("HOME");
    let original_path = std::env::var_os("PATH");
    let original_override = std::env::var_os(RUST_CHATBOT_CODEX_CLI_ENV);
    std::env::remove_var(RUST_CHATBOT_CODEX_CLI_ENV);
    std::env::set_var("HOME", home.path());
    std::env::set_var("PATH", "");

    let path = codex_cli_path();
    assert_eq!(path, "codex");

    match original_override {
        Some(v) => std::env::set_var(RUST_CHATBOT_CODEX_CLI_ENV, v),
        None => std::env::remove_var(RUST_CHATBOT_CODEX_CLI_ENV),
    }
    match original_path {
        Some(v) => std::env::set_var("PATH", v),
        None => std::env::remove_var("PATH"),
    }
    match original_home {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
    }
}

#[test]
fn forge_rust_chatbot_cli_path_env_override_for_tests() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original = std::env::var_os(RUST_CHATBOT_FORGE_RUST_CHATBOT_CLI_ENV);
    std::env::set_var(
        RUST_CHATBOT_FORGE_RUST_CHATBOT_CLI_ENV,
        "/custom/forge-rust-chatbot",
    );
    let path = forge_rust_chatbot_cli_path();
    assert_eq!(path, "/custom/forge-rust-chatbot");
    match original {
        Some(v) => std::env::set_var(RUST_CHATBOT_FORGE_RUST_CHATBOT_CLI_ENV, v),
        None => std::env::remove_var(RUST_CHATBOT_FORGE_RUST_CHATBOT_CLI_ENV),
    }
}

#[test]
fn forge_rust_chatbot_cli_path_defaults_to_sibling_forgecode2_binary() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original = std::env::var_os(RUST_CHATBOT_FORGE_RUST_CHATBOT_CLI_ENV);
    std::env::remove_var(RUST_CHATBOT_FORGE_RUST_CHATBOT_CLI_ENV);
    let path = forge_rust_chatbot_cli_path();
    assert!(path.ends_with("others/forgecode2/target/release/rust-chatbot"));
    match original {
        Some(v) => std::env::set_var(RUST_CHATBOT_FORGE_RUST_CHATBOT_CLI_ENV, v),
        None => std::env::remove_var(RUST_CHATBOT_FORGE_RUST_CHATBOT_CLI_ENV),
    }
}

// ── claude_json_path returns Some ────────────────────────────────

#[test]
fn claude_json_path_returns_some() {
    let path = claude_json_path();
    assert!(path.is_some());
    assert!(path.unwrap().contains(".claude.json"));
}

// ── config path helpers return non-empty strings ─────────────────

#[test]
fn config_path_helpers_return_expected_values() {
    assert!(gemini_project_settings_path().contains("settings.json"));
    assert!(codex_toml_path().contains("config.toml"));
    assert!(mcp_json_path().contains(".mcp.json"));
    assert!(qwen_api_key_path().contains("api.key"));
    assert!(qwen_json_path().contains(".qwen.json"));
    assert!(minimax_api_key_path().contains("api.key"));
    assert!(minimax_json_path().contains(".minimax.json"));
}

// ── get_*_config_dir cover all providers ─────────────────────────

#[test]
fn get_config_dir_helpers() {
    assert!(get_claude_config_dir().is_some());
    assert!(get_minimax_config_dir().is_some());
    assert!(get_qwen_config_dir().is_some());
    assert!(get_nvidia_config_dir().is_some());
}

// ── Provider profile: minimal deserialization uses serde defaults ─

#[test]
fn provider_profile_minimal_deserialization_defaults() {
    let json = r#"{"provider_id": "bare"}"#;
    let profile: ProviderCapabilityProfile = serde_json::from_str(json).unwrap();
    assert_eq!(profile.provider_id, "bare");
    assert!(profile.model_override.is_none());
    assert_eq!(profile.prompt_family, PromptFamily::Principles);
    assert!(profile.supported_categories.is_empty());
    assert!(profile.preferred_categories.is_empty());
    assert!(profile.fallback_chain.is_empty());
    assert!(!profile.cooldown_eligible);
    assert_eq!(profile.max_concurrency, 4);
    assert_eq!(profile.cost_tier, CostTier::Medium);
    assert!(profile.notes.is_empty());
}

// ── TOML: negative task_lease_secs accepted ─────────────────────

#[test]
fn negative_task_lease_secs_accepted() {
    let dir = temp_dir();
    let toml_content = r#"
[coordination]
task_lease_secs = -1
"#;
    let path = write_toml(&dir, "chatbot-config.toml", toml_content);
    let (cfg, _) = AppConfig::load_from(&[path]);
    assert_eq!(cfg.background.task_lease_secs, -1);
}

// ── auto_ui_launch_background_requested edge case ────────────────

#[test]
fn auto_ui_launch_background_empty_string_is_not_requested() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var(AUTO_UI_LAUNCH_BACKGROUND_ENV, "");
    assert!(!auto_ui_launch_background_requested());
    std::env::remove_var(AUTO_UI_LAUNCH_BACKGROUND_ENV);
}
