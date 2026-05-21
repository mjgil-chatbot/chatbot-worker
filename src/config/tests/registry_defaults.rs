use super::*;

// ── Provider profile loading errors and defaults ────────────────

#[test]
fn load_provider_profiles_invalid_json_falls_back_to_embedded() {
    let dir = temp_dir();
    let registry_dir = dir.path().join("workflow-registry");
    std::fs::create_dir_all(&registry_dir).unwrap();
    let profiles_path = registry_dir.join("providers.json");
    std::fs::write(&profiles_path, "not valid json {{{").unwrap();

    let result = AppConfig::default().load_provider_profiles_from(&profiles_path);
    assert!(result.is_err(), "invalid JSON should produce an error");
}

#[test]
fn load_provider_profiles_empty_providers_array() {
    let dir = temp_dir();
    let registry_dir = dir.path().join("workflow-registry");
    std::fs::create_dir_all(&registry_dir).unwrap();
    let profiles_path = registry_dir.join("providers.json");
    std::fs::write(&profiles_path, r#"{"providers": []}"#).unwrap();

    let loaded = AppConfig::default()
        .load_provider_profiles_from(&profiles_path)
        .unwrap();
    assert!(loaded.is_empty());
}

#[test]
fn load_provider_profiles_global_accessor_falls_back_on_error() {
    // The public load_provider_profiles() function should always succeed
    // by falling back to the embedded registry
    let profiles = load_provider_profiles();
    assert!(!profiles.is_empty());
    assert!(profiles.contains_key("codex"));
    assert!(!profiles.contains_key("claude"));
}

// ── PromptFamily and CostTier defaults ──────────────────────────

#[test]
fn prompt_family_default_is_principles() {
    assert_eq!(PromptFamily::default(), PromptFamily::Principles);
}

#[test]
fn cost_tier_default_is_medium() {
    assert_eq!(CostTier::default(), CostTier::Medium);
}

#[test]
fn cost_tier_round_trip_all_variants() {
    for (tier, expected_json) in [
        (CostTier::Low, "\"low\""),
        (CostTier::Medium, "\"medium\""),
        (CostTier::High, "\"high\""),
        (CostTier::Premium, "\"premium\""),
    ] {
        let json = serde_json::to_string(&tier).unwrap();
        assert_eq!(json, expected_json);
        let decoded: CostTier = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, tier);
    }
}

#[test]
fn task_category_round_trip_all_variants() {
    for (cat, expected_json) in [
        (TaskCategory::Survey, "\"survey\""),
        (TaskCategory::Design, "\"design\""),
        (TaskCategory::Plan, "\"plan\""),
        (TaskCategory::DeepWrite, "\"deep_write\""),
        (TaskCategory::QuickWrite, "\"quick_write\""),
        (TaskCategory::Review, "\"review\""),
        (TaskCategory::Verify, "\"verify\""),
        (TaskCategory::DocWrite, "\"doc_write\""),
        (TaskCategory::Research, "\"research\""),
    ] {
        let json = serde_json::to_string(&cat).unwrap();
        assert_eq!(json, expected_json);
        let decoded: TaskCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, cat);
    }
}

// ── window_app_id ───────────────────────────────────────────────

#[test]
fn window_app_id_without_instance() {
    let id = window_app_id(crate::model::ModelProvider::Claude, None);
    assert_eq!(id, "rust-chatbot-claude");
}

#[test]
fn window_app_id_with_instance() {
    let id = window_app_id(crate::model::ModelProvider::MiniMax, Some(3));
    assert_eq!(id, "03-rust-chatbot-minimax");
}

// ── data_dir_name ───────────────────────────────────────────────

#[test]
fn data_dir_name_all_providers() {
    assert_eq!(
        data_dir_name(crate::model::ModelProvider::Claude),
        ".claude-desktop"
    );
    assert_eq!(
        data_dir_name(crate::model::ModelProvider::MiniMax),
        ".minimax-desktop"
    );
    assert_eq!(
        data_dir_name(crate::model::ModelProvider::Qwen),
        ".qwen-desktop"
    );
    assert_eq!(
        data_dir_name(crate::model::ModelProvider::Codex),
        ".codex-desktop"
    );
    assert_eq!(
        data_dir_name(crate::model::ModelProvider::Nvidia),
        ".nvidia-desktop"
    );
    assert_eq!(
        data_dir_name(crate::model::ModelProvider::Gemini),
        ".gemini-desktop"
    );
}

// ── Provider profile with missing optional fields ───────────────

#[test]
fn provider_profile_minimal_json_uses_defaults() {
    let json = r#"{"providers": [{"provider_id": "test"}]}"#;
    let registry: ProviderCapabilityRegistry = serde_json::from_str(json).unwrap();
    assert_eq!(registry.providers.len(), 1);
    let profile = &registry.providers[0];
    assert_eq!(profile.provider_id, "test");
    assert_eq!(profile.prompt_family, PromptFamily::Principles); // default
    assert_eq!(profile.cost_tier, CostTier::Medium); // default
    assert_eq!(profile.max_concurrency, 4); // default
    assert!(profile.supported_categories.is_empty());
    assert!(profile.fallback_chain.is_empty());
    assert!(!profile.cooldown_eligible);
    assert!(profile.model_override.is_none());
}

// ── shared_infra_dir ────────────────────────────────────────────

#[test]
fn shared_infra_dir_returns_valid_path() {
    let dir = shared_infra_dir();
    let dir_str = dir.to_string_lossy();
    assert!(
        dir_str.contains(".rust-chatbot") || dir_str.contains(".claude-desktop"),
        "unexpected shared_infra_dir: {}",
        dir_str
    );
}

// ── Embedded registry is always valid ───────────────────────────

#[test]
fn embedded_provider_registry_is_valid() {
    let registry = embedded_provider_registry();
    assert!(!registry.providers.is_empty());
    for profile in &registry.providers {
        assert!(!profile.provider_id.is_empty());
    }
}

// ── Provider profile loading: I/O error path ────────────────────

#[test]
fn load_provider_registry_from_directory_errors() {
    // Passing a directory instead of a file should produce an I/O error
    let dir = temp_dir();
    let result = AppConfig::load_provider_registry_from(dir.path());
    assert!(result.is_err());
}

// ── Empty TOML table gets serde defaults ─────────────────────────

#[test]
fn empty_toml_table_applies_serde_defaults() {
    let dir = temp_dir();
    // A file with only comments results in an empty TOML table
    let path = write_toml(&dir, "config.toml", "# only comments\n");
    let cfg = AppConfig::from_toml_path(&path).unwrap();
    // serde defaults should be applied, not Rust Default::default()
    assert_eq!(cfg.provider.default_provider, "codex");
    assert_eq!(cfg.ui.window_width, 1000.0);
    assert!(cfg.memory.enable_workspace_memory);
    assert_eq!(cfg.background.task_lease_secs, 30);
}

// ── load_from skips unreadable paths and tries the next ──────────

#[test]
fn load_from_falls_through_to_second_path() {
    let dir = temp_dir();
    let missing = dir.path().join("does-not-exist.toml");
    let toml_content = r#"
[provider]
default_provider = "kimi"
"#;
    let valid = write_toml(&dir, "second.toml", toml_content);

    let (cfg, path) = AppConfig::load_from(&[missing, valid.clone()]);
    assert_eq!(cfg.provider.default_provider, "kimi");
    assert_eq!(path, Some(valid));
}

#[test]
fn load_from_with_all_missing_returns_defaults() {
    let dir = temp_dir();
    let missing1 = dir.path().join("a.toml");
    let missing2 = dir.path().join("b.toml");

    let (cfg, path) = AppConfig::load_from(&[missing1, missing2]);
    assert!(path.is_none());
    assert_eq!(cfg.provider.default_provider, "codex");
}

// ── ProviderCapabilityProfile: partial deserialization ────────────

#[test]
fn provider_profile_with_unknown_category_errors() {
    let json =
        r#"{"providers": [{"provider_id": "test", "supported_categories": ["nonexistent"]}]}"#;
    let result: Result<ProviderCapabilityRegistry, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn provider_profile_with_model_override() {
    let json = r#"{"providers": [{"provider_id": "test", "model_override": "gpt-5"}]}"#;
    let registry: ProviderCapabilityRegistry = serde_json::from_str(json).unwrap();
    assert_eq!(
        registry.providers[0].model_override,
        Some("gpt-5".to_string())
    );
}

// ── max_concurrency accessor ─────────────────────────────────────

#[test]
fn max_concurrency_defaults_to_four() {
    reset_config();
    assert_eq!(max_concurrency(), 4);
}
