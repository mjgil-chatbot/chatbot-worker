#[derive(Clone, Debug, Default)]
pub struct ProviderRequestOverrides {
    pub model: Option<String>,
    pub cli: Option<String>,
    pub output_json_schema: Option<String>,
    pub config_dir: Option<PathBuf>,
    pub anthropic_base_url: Option<String>,
    pub anthropic_auth_token: Option<String>,
    pub force_skip_git_repo_check: bool,
    pub mcp_json_path: Option<PathBuf>,
    pub claude_json_path: Option<PathBuf>,
    pub gemini_settings_path: Option<PathBuf>,
    pub codex_toml_path: Option<PathBuf>,
    pub qwen_code_resume_session_id: Option<String>,
    pub runsc_image: Option<String>,
    pub runsc_workspace_includes: Vec<String>,
    pub runsc_dirs: Vec<String>,
}
const TEST_MCP_JSON_PATH_ENV: &str = "RUST_CHATBOT_TEST_MCP_JSON_PATH";
const BACKGROUND_RUNSC_WORKSPACE_INCLUDES_ENV: &str =
    "RUST_CHATBOT_AUTONOMOUS_RUNSC_WORKSPACE_INCLUDES";
const TASK_REPORT_VALIDATOR_RUNSC_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/scripts/validate_autonomous_task_report.py"
);
fn env_override(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
fn optional_string_override(explicit: Option<&String>, env_name: &str) -> Option<String> {
    explicit.cloned().or_else(|| env_override(env_name))
}
fn string_list_override(explicit: Option<&Vec<String>>, env_name: &str) -> Vec<String> {
    explicit.cloned().unwrap_or_else(|| {
        std::env::var(env_name)
            .ok()
            .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
            .unwrap_or_default()
    })
}
fn path_override(
    explicit: Option<&PathBuf>,
    env_name: &str,
    default: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    explicit
        .cloned()
        .or_else(|| env_override(env_name).map(PathBuf::from))
        .or_else(default)
}
fn upsert_env_var(extra_env: &mut Vec<(String, String)>, key: &str, value: String) {
    if let Some((_, existing)) = extra_env.iter_mut().find(|(name, _)| name == key) {
        *existing = value;
    } else {
        extra_env.push((key.to_string(), value));
    }
}
