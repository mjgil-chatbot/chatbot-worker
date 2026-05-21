use super::*;
use crate::model::ModelProvider;

// ---- Provider profiles --------------------------------------------------

#[test]
fn load_provider_profiles_fall_back_to_embedded_registry_when_file_missing() {
    let missing = temp_dir().path().join("missing-providers.json");
    let loaded = AppConfig::default()
        .load_provider_profiles_from(&missing)
        .unwrap();

    assert!(loaded.len() >= 2);
    assert!(loaded.contains_key("codex"));
    assert!(loaded.contains_key("minimax"));
}

#[test]
fn load_provider_profiles_loads_valid_json() {
    let dir = temp_dir();
    let json_content = serde_json::json!({
        "providers": [
            {
                "provider_id": "codex",
                "model_override": "gpt-5",
                "prompt_family": "principles",
                "supported_categories": ["design", "review", "verify"],
                "preferred_categories": ["design"],
                "fallback_chain": ["minimax", "claude"],
                "cooldown_eligible": false,
                "max_concurrency": 4,
                "cost_tier": "high",
                "notes": "Primary frontier model"
            },
            {
                "provider_id": "minimax",
                "prompt_family": "mechanics",
                "supported_categories": ["deep_write", "quick_write", "research"],
                "preferred_categories": ["deep_write"],
                "fallback_chain": ["qwen", "codex"],
                "cooldown_eligible": true,
                "max_concurrency": 8,
                "cost_tier": "medium",
                "notes": "Fast and affordable"
            }
        ]
    })
    .to_string();
    let registry_dir = dir.path().join("workflow-registry");
    std::fs::create_dir_all(&registry_dir).unwrap();
    let profiles_path = registry_dir.join("providers.json");
    std::fs::write(&profiles_path, json_content).unwrap();

    let loaded = AppConfig::default()
        .load_provider_profiles_from(&profiles_path)
        .unwrap();

    assert_eq!(loaded.len(), 2);
    let codex = loaded.get("codex").unwrap();
    assert_eq!(codex.prompt_family, PromptFamily::Principles);
    assert!(codex.supported_categories.contains(&TaskCategory::Design));
    assert_eq!(codex.fallback_chain, vec!["minimax".to_string()]);
    assert_eq!(codex.cost_tier, CostTier::High);

    let minimax = loaded.get("minimax").unwrap();
    assert_eq!(minimax.prompt_family, PromptFamily::Mechanics);
    assert!(minimax.cooldown_eligible);
    assert_eq!(minimax.max_concurrency, 8);
    assert_eq!(minimax.fallback_chain, vec!["codex".to_string()]);
}

#[test]
fn load_provider_profiles_supports_checked_in_registry_object_format() {
    let loaded = AppConfig::default()
        .load_provider_profiles_from(&provider_registry_path())
        .unwrap();

    assert!(loaded.len() >= 2);
    let codex = loaded.get("codex").unwrap();
    assert_eq!(codex.prompt_family, PromptFamily::Mechanics);
    assert!(codex
        .supported_categories
        .contains(&TaskCategory::DeepWrite));
}

#[test]
fn prompt_family_and_task_category_round_trip() {
    let prompt_family = PromptFamily::Mechanics;
    let prompt_family_json = serde_json::to_string(&prompt_family).unwrap();
    assert_eq!(prompt_family_json, "\"mechanics\"");
    let decoded_prompt_family: PromptFamily = serde_json::from_str(&prompt_family_json).unwrap();
    assert_eq!(decoded_prompt_family, PromptFamily::Mechanics);

    let task_category = TaskCategory::DocWrite;
    let task_category_json = serde_json::to_string(&task_category).unwrap();
    assert_eq!(task_category_json, "\"doc_write\"");
    let decoded_task_category: TaskCategory = serde_json::from_str(&task_category_json).unwrap();
    assert_eq!(decoded_task_category, TaskCategory::DocWrite);
}

// ---- Autonomous policy catalog validation -------------------------------

#[test]
fn every_provider_id_in_registry_resolves_to_catalog_entry() {
    let profiles = AppConfig::default()
        .load_provider_profiles_from(&provider_registry_path())
        .unwrap();

    for provider_id in profiles.keys() {
        assert!(
            ModelProvider::from_alias(provider_id).is_some(),
            "provider_id {:?} in workflow-registry/providers.json does not resolve to a known ModelProvider",
            provider_id
        );
    }
}

#[test]
fn every_fallback_target_resolves_to_catalog_entry() {
    let profiles = AppConfig::default()
        .load_provider_profiles_from(&provider_registry_path())
        .unwrap();

    for (provider_id, profile) in &profiles {
        for target in &profile.fallback_chain {
            assert!(
                ModelProvider::from_alias(target).is_some(),
                "fallback target {:?} in fallback_chain for {:?} does not resolve to a known ModelProvider",
                target,
                provider_id
            );
            assert!(
                profiles.contains_key(target.as_str()),
                "fallback target {:?} in fallback_chain for {:?} is not present in the autonomous policy file",
                target,
                provider_id
            );
        }
    }
}

#[test]
fn autonomous_policy_is_subset_of_runtime_catalog() {
    let profiles = AppConfig::default()
        .load_provider_profiles_from(&provider_registry_path())
        .unwrap();

    for (_, profile) in &profiles {
        let provider_id = profile.provider_id.as_str();
        assert!(
            ModelProvider::from_alias(provider_id).is_some(),
            "autonomous policy contains unknown provider: {}",
            provider_id
        );
    }
}

// ---- Compatibility accessors --------------------------------------------

#[test]
fn compatibility_accessors_return_loaded_or_default() {
    reset_config();
    // With no config file, all fall back to the compile-time defaults
    assert_eq!(claude_reasoning_effort(), None); // TOML default is None
    assert_eq!(codex_reasoning_effort(), None); // TOML default is None
    assert_eq!(minimax_reasoning_effort(), None);
    assert_eq!(codex_sandbox_mode(), None);
    assert_eq!(gemini_approval_mode(), None);
    assert_eq!(allow_api_billing(), ALLOW_API_BILLING);
    assert!(enable_workspace_memory());
    assert_eq!(task_lease_secs(), 30);
    assert_eq!(task_max_attempts(), 3);
    assert_eq!(background_max_agents(), 5);
    assert_eq!(background_max_iterations(), u32::MAX);
    assert!(background_allow_write_tasks());
    assert!(background_require_review_before_apply());
    assert_eq!(background_max_concurrent(), 4);
}

#[test]
fn compatibility_accessor_reflects_loaded_config() {
    let dir = temp_dir();
    let toml_content = r#"
[provider]
claude_reasoning_effort = "low"
codex_reasoning_effort = "low"
minimax_reasoning_effort = "xhigh"
allow_api_billing = true

[coordination]
task_lease_secs = 45
max_agents = 8
allow_write_tasks = false
background_max_concurrent = 6
"#;
    let path = write_toml(&dir, "chatbot-config.toml", toml_content);
    // We can't easily patch the search path, so test the struct method directly
    let (cfg, _) = AppConfig::load_from(&[path]);
    assert_eq!(
        cfg.provider.claude_reasoning_effort,
        Some("low".to_string())
    );
    assert_eq!(cfg.provider.codex_reasoning_effort, Some("low".to_string()));
    assert_eq!(
        cfg.provider.minimax_reasoning_effort,
        Some("xhigh".to_string())
    );
    assert_eq!(cfg.provider.allow_api_billing, Some(true));
    assert_eq!(cfg.background.task_lease_secs, 45);
    assert_eq!(cfg.background.max_agents, 8);
    assert!(!cfg.background.allow_write_tasks);
    assert_eq!(cfg.background.background_max_concurrent, 6);
}

#[test]
fn effective_gemini_approval_mode_uses_config_or_default() {
    let original_override = std::env::var(CONFIG_PATH_OVERRIDE_ENV).ok();
    reset_config();
    assert_eq!(effective_gemini_approval_mode(), Some("yolo".to_string()));

    let dir = temp_dir();
    let toml_content = r#"
[provider]
gemini_approval_mode = "plan"
"#;
    let path = write_toml(&dir, "chatbot-config.toml", toml_content);
    std::env::set_var(CONFIG_PATH_OVERRIDE_ENV, &path);
    reset_config();
    assert_eq!(effective_gemini_approval_mode(), Some("plan".to_string()));

    match original_override {
        Some(value) => std::env::set_var(CONFIG_PATH_OVERRIDE_ENV, value),
        None => std::env::remove_var(CONFIG_PATH_OVERRIDE_ENV),
    }
    reset_config();
}

// ---- Original constants still work -------------------------------------

#[test]
fn original_constants_preserved() {
    assert_eq!(CODEX_SANDBOX_MODE, Some("danger-full-access"));
    assert_eq!(GEMINI_APPROVAL_MODE, Some("yolo"));
    assert_eq!(
        BackgroundTaskConfig::default().background_design_doc_path,
        "docs/autonomous/design-doc.md"
    );
    assert_eq!(WINDOW_WIDTH, 1000.0);
    assert_eq!(MIN_WINDOW_WIDTH, 980.0);
    assert!(ENABLE_WORKSPACE_MEMORY);
    assert_eq!(SHARED_INFRA_DIR, ".rust-chatbot");
}

#[test]
fn original_helper_functions_preserved() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original_codex_cli = std::env::var_os(RUST_CHATBOT_CODEX_CLI_ENV);
    std::env::remove_var(RUST_CHATBOT_CODEX_CLI_ENV);

    // shared_infra_dir returns canonical when it exists, legacy when only legacy exists
    let infra = shared_infra_dir().to_string_lossy().to_string();
    assert!(
        infra.contains(".rust-chatbot") || infra.contains(".claude-desktop"),
        "shared_infra_dir() returned unexpected path: {}",
        infra
    );
    assert!(repo_root().ends_with("chatbot-worker"));
    assert_eq!(codex_cli_path(), "codex");
    assert_eq!(
        app_title_for(crate::model::ModelProvider::Claude),
        "Claude Rust Chatbot"
    );
    assert_eq!(
        app_id_for(crate::model::ModelProvider::Claude),
        "rust-chatbot-claude"
    );
    let title = window_title(crate::model::ModelProvider::Claude, Some(1));
    assert!(title.contains("Claude"));

    match original_codex_cli {
        Some(value) => std::env::set_var(RUST_CHATBOT_CODEX_CLI_ENV, value),
        None => std::env::remove_var(RUST_CHATBOT_CODEX_CLI_ENV),
    }
}
