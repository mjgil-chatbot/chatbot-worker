pub fn sync_provider_mcp_configs(
    session: Option<&ChatSession>,
    overrides: Option<&ProviderRequestOverrides>,
) -> Result<()> {
    sync_mcp_configs(session, overrides)
}
pub fn background_provider_overrides(provider: ModelProvider) -> ProviderRequestOverrides {
    background_provider_overrides_with_execution(provider, None, None)
}
pub fn background_provider_overrides_with_execution(
    provider: ModelProvider,
    execution_environment: Option<AutonomousExecutionEnvironment>,
    runsc_workspace_includes: Option<Vec<String>>,
) -> ProviderRequestOverrides {
    let entry = crate::provider_catalog::by_id(provider);
    let model = if entry.background_model.is_empty() {
        None
    } else {
        Some(entry.background_model.to_string())
    };
    let execution_environment =
        execution_environment.unwrap_or_else(background_execution_environment_from_env);
    let nested_runsc = inside_autonomous_runsc_container();
    let runsc_image = (matches!(execution_environment, AutonomousExecutionEnvironment::Runsc)
        && !nested_runsc)
        .then(config::background_runsc_image);
    let runsc_workspace_includes = if matches!(execution_environment, AutonomousExecutionEnvironment::Runsc)
        && !nested_runsc
    {
        runsc_workspace_includes.unwrap_or_else(background_runsc_workspace_includes_from_env)
    } else {
        Vec::new()
    };

    ProviderRequestOverrides {
        model,
        runsc_image,
        runsc_workspace_includes,
        ..Default::default()
    }
}
fn background_execution_environment_from_env() -> AutonomousExecutionEnvironment {
    let value = std::env::var("RUST_CHATBOT_BACKGROUND_EXECUTION_ENV")
        .ok()
        .or_else(|| std::env::var("RUST_CHATBOT_AUTONOMOUS_EXECUTION_ENV").ok());
    match value.as_deref() {
        Some("runsc") => AutonomousExecutionEnvironment::Runsc,
        _ => AutonomousExecutionEnvironment::Host,
    }
}
fn background_runsc_workspace_includes_from_env() -> Vec<String> {
    std::env::var(BACKGROUND_RUNSC_WORKSPACE_INCLUDES_ENV)
        .ok()
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default()
}
fn inside_autonomous_runsc_container() -> bool {
    std::env::var("RUST_CHATBOT_AUTONOMOUS_INSIDE_RUNSC")
        .ok()
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}
