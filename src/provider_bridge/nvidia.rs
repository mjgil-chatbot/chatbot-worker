fn nvidia_model_catalog() -> serde_json::Value {
    let model = config::interactive_model_for(ModelProvider::Nvidia)
        .expect("Nvidia catalog entry must define an interactive_model");
    serde_json::json!({
        "models": [{
            "slug": model,
            "display_name": model,
            "description": "Nemotron via NVIDIA AI Playground",
            "default_reasoning_level": "high",
            "supported_reasoning_levels": [
                {"effort": "low", "description": "Fast responses with lighter reasoning"},
                {"effort": "medium", "description": "Balances speed and reasoning depth for everyday tasks"},
                {"effort": "high", "description": "Greater reasoning depth for complex problems"}
            ],
            "shell_type": "shell_command",
            "visibility": "list",
            "supported_in_api": true,
            "priority": 0,
            "availability_nux": serde_json::Value::Null,
            "upgrade": serde_json::Value::Null,
            "base_instructions": "You are Codex, a coding agent based on GPT-5. You and the user share the same workspace and collaborate to achieve the user's goals.",
            "model_messages": serde_json::Value::Null,
            "supports_reasoning_summaries": false,
            "default_reasoning_summary": "none",
            "support_verbosity": false,
            "default_verbosity": serde_json::Value::Null,
            "apply_patch_tool_type": "freeform",
            "web_search_tool_type": "text",
            "truncation_policy": {"mode": "tokens", "limit": 10000},
            "supports_parallel_tool_calls": false,
            "supports_image_detail_original": false,
            "context_window": 128000,
            "experimental_supported_tools": [],
            "input_modalities": ["text"],
            "supports_search_tool": false
        }]
    })
}

fn nvidia_mcp_config_toml(
    overrides: Option<&ProviderRequestOverrides>,
    codex_home: &Path,
) -> String {
    let src = resolved_mcp_json_path(overrides);
    let Some(parsed) = load_mcp_json_value(&src) else {
        return String::new();
    };
    let servers =
        filtered_profile_mcp_servers(&parsed, Some(MCP_PROVIDER_PROFILE_CODEX), overrides);
    let sidecar_path = codex_home.join("mcp-coordinator-upstream.json");
    if servers.is_empty() {
        let _ = std::fs::remove_file(&sidecar_path);
        return String::new();
    }
    let out = match serde_json::to_vec_pretty(&provider_upstream_json(
        MCP_PROVIDER_PROFILE_CODEX,
        &servers,
    )) {
        Ok(out) => out,
        Err(_) => return String::new(),
    };
    if write_file_atomically(&sidecar_path, &out).is_err() {
        return String::new();
    }
    render_coordinator_codex_toml(&sidecar_path).unwrap_or_default()
}

fn ensure_nvidia_codex_home(overrides: Option<&ProviderRequestOverrides>) -> Option<PathBuf> {
    let codex_home = PathBuf::from(config::nvidia_codex_home_path());
    let model_catalog_path = codex_home.join("model-catalog.json");
    let model_catalog = serde_json::to_vec_pretty(&nvidia_model_catalog()).ok()?;
    write_file_atomically(&model_catalog_path, &model_catalog).ok()?;
    let base_url = config::base_url_for(ModelProvider::Nvidia)
        .expect("Nvidia catalog entry must define a base_url");
    let api_key_env = config::api_key_env_for(ModelProvider::Nvidia)
        .expect("Nvidia catalog entry must define an api_key_env");

    let mut config_toml = format!(
        concat!(
            "model_provider = \"nvidia\"\n",
            "model_catalog_json = \"model-catalog.json\"\n\n",
            "[model_providers.nvidia]\n",
            "name = \"Nvidia\"\n",
            "base_url = \"{}\"\n",
            "env_key = \"{}\"\n",
            "wire_api = \"chat\"\n",
            "requires_openai_auth = false\n",
            "supports_websockets = false\n"
        ),
        base_url, api_key_env,
    );
    let mcp_toml = nvidia_mcp_config_toml(overrides, &codex_home);
    if !mcp_toml.is_empty() {
        config_toml.push('\n');
        config_toml.push_str(&mcp_toml);
    }
    write_file_atomically(&codex_home.join("config.toml"), config_toml.as_bytes()).ok()?;
    Some(codex_home)
}
