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
