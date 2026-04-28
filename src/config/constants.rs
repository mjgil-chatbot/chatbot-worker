use std::path::PathBuf;

// ============================================================================
// NON-PROVIDER DEFAULTS — behavioral flags and infrastructure constants that
// are not per-provider metadata and therefore do not belong in the catalog.
// ============================================================================

// Claude CLI behavioral flags
pub const OUTPUT_FORMAT: &str = "stream-json";
pub const SKIP_PERMISSIONS: bool = true;
pub const VERBOSE_OUTPUT: bool = true;
pub const ALLOW_API_BILLING: bool = false;
pub const CLAUDE_REASONING_EFFORT: Option<&str> = Some("high");

// Codex CLI behavioral flags
pub const CODEX_JSON_STREAM: bool = true;
pub const CODEX_REASONING_EFFORT: Option<&str> = Some("high");
pub const CODEX_SANDBOX_MODE: Option<&str> = Some("danger-full-access");
pub const MINIMAX_REASONING_EFFORT: Option<&str> = Some("xhigh");
pub const RUST_CHATBOT_CODEX_CLI_ENV: &str = "RUST_CHATBOT_CODEX_CLI";
pub const RUST_CHATBOT_NVIDIA_CODEX_HOME_ENV: &str = "RUST_CHATBOT_NVIDIA_CODEX_HOME";
pub const RUST_CHATBOT_FORGE_RUST_CHATBOT_CLI_ENV: &str = "RUST_CHATBOT_FORGE_RUST_CHATBOT_CLI";
pub const TRUSTED_WORKSPACE_ROOTS_ENV: &str = "RUST_CHATBOT_TRUSTED_WORKSPACE_ROOTS";

pub fn trusted_workspace_roots() -> Vec<PathBuf> {
    if let Some(raw_paths) = std::env::var_os(TRUSTED_WORKSPACE_ROOTS_ENV) {
        let mut roots = Vec::new();
        for path in std::env::split_paths(&raw_paths).filter(|path| !path.as_os_str().is_empty()) {
            if roots.iter().all(|existing| existing != &path) {
                roots.push(path);
            }
        }
        if !roots.is_empty() {
            return roots;
        }
    }

    vec![crate::config::checkout_root()]
}

// Gemini CLI behavioral flags
pub const GEMINI_OUTPUT_FORMAT: &str = "stream-json";
pub const GEMINI_APPROVAL_MODE: Option<&str> = Some("yolo");

// Qwen CLI behavioral flags
pub const QWEN_AUTH_TYPE: &str = "qwen-oauth";
pub const QWEN_CONTEXT_WINDOW_SIZE: u64 = 1_000_000;
pub const QWEN_OPENROUTER_CONTEXT_WINDOW_SIZE: u64 = 262_000;
pub const GEMMA_CONTEXT_WINDOW_SIZE: u64 = 262_144;
pub const RUST_CHATBOT_QWEN_CODE_CLI_ENV: &str = "RUST_CHATBOT_QWEN_CODE_CLI";

pub fn qwen_code_cli_path() -> String {
    if let Some(path) = std::env::var_os(RUST_CHATBOT_QWEN_CODE_CLI_ENV).filter(|v| !v.is_empty()) {
        return path.to_string_lossy().into_owned();
    }
    let binary = crate::provider_catalog::by_id(crate::model::ModelProvider::Qwen).cli_binary;
    resolve_cli_from_path(binary)
        .or_else(|| resolve_qwen_asdf_install(binary))
        .unwrap_or_else(|| binary.to_string())
}

fn resolve_cli_from_path(binary: &str) -> Option<String> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
        .map(|candidate| candidate.to_string_lossy().into_owned())
}

fn resolve_qwen_asdf_install(binary: &str) -> Option<String> {
    let root = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .or_else(dirs::home_dir)?
        .join(".asdf")
        .join("installs")
        .join("nodejs");
    let mut newest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir(root).ok()? {
        let Ok(entry) = entry else {
            continue;
        };
        let candidate = entry.path().join("bin").join(binary);
        if !candidate.is_file() {
            continue;
        }
        let modified = candidate
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        match &newest {
            Some((best, _)) if modified <= *best => {}
            _ => newest = Some((modified, candidate)),
        }
    }
    newest.map(|(_, path)| path.to_string_lossy().into_owned())
}

pub fn forge_rust_chatbot_cli_path() -> String {
    if let Some(path) =
        std::env::var_os(RUST_CHATBOT_FORGE_RUST_CHATBOT_CLI_ENV).filter(|v| !v.is_empty())
    {
        return path.to_string_lossy().into_owned();
    }
    forgecode2_rust_chatbot_cli_path()
        .to_string_lossy()
        .into_owned()
}

fn forgecode2_rust_chatbot_cli_path() -> std::path::PathBuf {
    let repo_root = std::path::Path::new(crate::config::repo_root());
    let parent = repo_root
        .parent()
        .expect("chatbot-worker repo root should have a parent directory");
    parent.join("others/forgecode2/target/release/rust-chatbot")
}

// UI Configuration
pub const WINDOW_WIDTH: f32 = 1000.0;
pub const WINDOW_HEIGHT: f32 = 600.0;
pub const MIN_WINDOW_WIDTH: f32 = 980.0;
pub const MIN_WINDOW_HEIGHT: f32 = 300.0;
pub const SIDE_PANEL_WIDTH: f32 = 200.0;
pub const SIDE_PANEL_MIN_WIDTH: f32 = 100.0;
pub const CONTENT_MAX_WIDTH: f32 = 700.0;
pub const MESSAGE_MAX_WIDTH: f32 = 700.0;
pub const SYSTEM_PROMPT_HEIGHT: f32 = 60.0;
pub const INPUT_HEIGHT: f32 = 25.0;

// Shared Infrastructure
pub const ENABLE_WORKSPACE_MEMORY: bool = true;
pub const SHARED_INFRA_DIR: &str = ".rust-chatbot";
pub const AUTO_UI_LAUNCH_BACKGROUND_ENV: &str = "AUTO_UI_LAUNCH_BACKGROUND";
pub const LEGACY_SHARED_INFRA_DIR: &str = ".claude-desktop";

// Session Configuration
pub const DEFAULT_SESSION_NAME: &str = "Welcome Chat";
pub const NEW_SESSION_NAME: &str = "New Chat";
pub const TEMP_SESSION_MESSAGE: &str =
    "Welcome to Desktop Chat! Note: Session persistence is not available.";

// Application Metadata
pub const APP_TITLE: &str = "Rust Chatbot";
pub const UI_BINARY_NAME: &str = "rust-chatbot";

// Coordination / autonomous run defaults
pub const AUTONOMOUS_DESIGN_DOC_PATH: &str = "docs/autonomous/design-doc.md";

// ============================================================================
// CATALOG-BACKED ACCESSORS — new code should use these instead of the
// compatibility constants below.  The canonical source of truth for all
// per-provider metadata is `src/provider_catalog.rs`.
// ============================================================================

/// Interactive (foreground) model for a provider.  Returns `None` when the
/// provider defers to the CLI default (catalog entry has an empty string).
pub fn interactive_model_for(provider: crate::model::ModelProvider) -> Option<&'static str> {
    let m = crate::provider_catalog::by_id(provider).interactive_model;
    if m.is_empty() {
        None
    } else {
        Some(m)
    }
}

/// Background (autonomous) model for a provider.  Returns `None` when the
/// provider defers to the CLI default (catalog entry has an empty string).
pub fn background_model_for(provider: crate::model::ModelProvider) -> Option<&'static str> {
    let m = crate::provider_catalog::by_id(provider).background_model;
    if m.is_empty() {
        None
    } else {
        Some(m)
    }
}

/// Config directory name for a provider, from the catalog.
pub fn config_dir_name_for(provider: crate::model::ModelProvider) -> &'static str {
    crate::provider_catalog::by_id(provider).config_dir_name
}

/// API key environment variable for a provider, from the catalog.
pub fn api_key_env_for(provider: crate::model::ModelProvider) -> Option<&'static str> {
    let env = crate::provider_catalog::by_id(provider).api_key_env;
    if env.is_empty() {
        None
    } else {
        Some(env)
    }
}

/// Base URL for a provider, from the catalog.
pub fn base_url_for(provider: crate::model::ModelProvider) -> Option<&'static str> {
    crate::provider_catalog::by_id(provider).base_url
}

/// Resolved config directory path for a provider.
pub fn get_config_dir(provider: crate::model::ModelProvider) -> Option<String> {
    crate::config::resolved_provider_config_dir(provider)
        .map(|path| path.to_string_lossy().into_owned())
}

// ============================================================================
// NON-PROVIDER HELPER FUNCTIONS
// ============================================================================

pub fn auto_ui_launch_background_requested() -> bool {
    matches!(
        std::env::var(AUTO_UI_LAUNCH_BACKGROUND_ENV).as_deref(),
        Ok("1")
    )
}
