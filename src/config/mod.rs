//! Schema-backed runtime configuration for rust-chatbot.
//!
//! Search order (first hit wins):
//!   1. `$RUST_CHATBOT_CONFIG_PATH` if set — exclusive, no fallback. Lets test
//!      harnesses pin the config file regardless of the repo root or `$HOME`.
//!   2. the shared runtime root `config.toml`.
//!   3. Legacy repo-local `chatbot-config.toml` files from the split repos.
//!
//! All fields are optional; missing fields fall back to the hardcoded defaults
//! in this module so existing callers are never forced to change.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::RwLock;

const EMBEDDED_PROVIDER_CAPABILITY_REGISTRY_JSON: &str =
    include_str!("../../workflow-registry/providers.json");

/// Env var that pins the config path. When set, it is the exclusive search
/// entry — the default shared-runtime-root `config.toml` and legacy repo-local paths are
/// not consulted. Intended for the auto-ui test harness; production leaves it
/// unset.
pub const CONFIG_PATH_OVERRIDE_ENV: &str = "RUST_CHATBOT_CONFIG_PATH";

// ─── Lazy config singleton ──────────────────────────────────────────────────

static APP_CONFIG: once_cell::sync::Lazy<RwLock<AppConfig>> =
    once_cell::sync::Lazy::new(|| RwLock::new(AppConfig::load().0));

fn load_config() -> AppConfig {
    APP_CONFIG.read().unwrap().clone()
}

// ─── Public entry point ─────────────────────────────────────────────────────

/// Returns the fully-merged runtime application config, loaded from the first
/// available TOML path.  Individual knobs are
/// accessible as `CONFIG.provider.xxx`, `CONFIG.ui.xxx`, etc.
pub fn config() -> AppConfig {
    load_config()
}

/// Reset the global config (useful in tests to exercise loading path logic).
#[cfg(test)]
pub(crate) fn reset_config() {
    *APP_CONFIG.write().unwrap() = AppConfig::load().0;
}

// ─── Top-level config ───────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub provider: ProviderConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    #[serde(alias = "coordination")]
    pub background: BackgroundTaskConfig,
}

impl AppConfig {
    /// Attempts to load config from the first available TOML path.
    /// Returns `(config, path_used)` — path is `None` if only defaults were used.
    fn load() -> (Self, Option<PathBuf>) {
        Self::load_from(Self::config_search_path().as_slice())
    }

    fn load_from(paths: &[PathBuf]) -> (Self, Option<PathBuf>) {
        for path in paths {
            if let Ok(loaded) = Self::from_toml_path(path) {
                return (loaded, Some(path.clone()));
            }
        }
        // No file found — deserialize empty TOML so serde defaults are applied
        // (derived Default gives zero-value Rust defaults, not serde defaults)
        let default_config: Self = toml::from_str("").unwrap();
        (default_config, None)
    }

    fn from_toml_path(path: &PathBuf) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let raw: toml::Value = toml::from_str(&content)?;
        // If the TOML table is empty or only contains comments, deserialize an empty
        // document so serde defaults (#[serde(default)]) are applied rather than the
        // derived Default impl which gives zero-value Rust defaults.
        if raw.as_table().map(|t| t.is_empty()).unwrap_or(false) {
            let empty: Self = toml::from_str("")?;
            return Ok(empty);
        }
        let config: Self = raw.try_into()?;
        config.validate()?;
        Ok(config)
    }

    /// Returns the ordered list of paths to search for config files.
    /// `RUST_CHATBOT_CONFIG_PATH`, when set, is the exclusive search path —
    /// callers (especially test harnesses) opt out of the default lookup
    /// entirely so a legacy repo-local `chatbot-config.toml` cannot preempt it.
    fn config_search_path() -> Vec<PathBuf> {
        if let Some(override_path) = std::env::var_os(CONFIG_PATH_OVERRIDE_ENV) {
            return vec![PathBuf::from(override_path)];
        }
        let mut paths = vec![canonical_shared_root_dir().join("config.toml")];
        paths.extend(legacy_repo_config_paths("chatbot-config.toml"));
        paths
    }

    /// Validate loaded config fields; return an error with context on failure.
    fn validate(&self) -> anyhow::Result<()> {
        // Ensure providers.json is loadable if referenced (non-fatal — warn only)
        if let Err(e) = self.load_provider_registry() {
            eprintln!("[config] warning: could not load provider profiles: {}", e);
        }
        Ok(())
    }

    fn load_provider_registry(&self) -> anyhow::Result<ProviderCapabilityRegistry> {
        Self::load_provider_registry_from(&provider_registry_path())
    }

    fn load_provider_registry_from(
        path: &std::path::Path,
    ) -> anyhow::Result<ProviderCapabilityRegistry> {
        match std::fs::read_to_string(path) {
            Ok(content) => Ok(serde_json::from_str(&content)?),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(embedded_provider_registry())
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Load provider capability profiles from the checked-in provider registry.
    /// Falls back to the embedded checked-in registry when the file is missing.
    pub fn load_provider_profiles(
        &self,
    ) -> anyhow::Result<BTreeMap<String, ProviderCapabilityProfile>> {
        Ok(supported_provider_profiles(self.load_provider_registry()?))
    }
}

fn provider_registry_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("workflow-registry")
        .join("providers.json")
}

fn embedded_provider_registry() -> ProviderCapabilityRegistry {
    serde_json::from_str(EMBEDDED_PROVIDER_CAPABILITY_REGISTRY_JSON)
        .expect("embedded provider capability registry should be valid")
}

fn supported_provider_profiles(
    registry: ProviderCapabilityRegistry,
) -> BTreeMap<String, ProviderCapabilityProfile> {
    registry
        .providers
        .into_iter()
        .filter_map(|mut profile| {
            let provider = crate::model::ModelProvider::from_supported_alias(&profile.provider_id)?;
            profile.provider_id = provider.as_str().to_string();
            profile.fallback_chain.retain(|target| {
                crate::model::ModelProvider::from_supported_alias(target).is_some()
            });
            Some((profile.provider_id.clone(), profile))
        })
        .collect()
}

// ─── Provider config ────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize)]
pub struct ProviderConfig {
    /// Claude reasoning effort (e.g. "low", "medium", "high"). Defaults to "high".
    #[serde(default)]
    pub claude_reasoning_effort: Option<String>,

    /// Codex reasoning effort (e.g. "low", "medium", "high").  Defaults to "high".
    #[serde(default)]
    pub codex_reasoning_effort: Option<String>,
    /// MiniMax reasoning effort (e.g. "low", "medium", "high", "xhigh"). Defaults to "xhigh".
    #[serde(default)]
    pub minimax_reasoning_effort: Option<String>,

    /// Codex sandbox mode (read-only, workspace-write, danger-full-access).
    /// Defaults to "danger-full-access".
    #[serde(default)]
    pub codex_sandbox_mode: Option<String>,

    /// Gemini approval mode (default, auto_edit, plan, yolo).
    /// Defaults to "yolo".
    #[serde(default)]
    pub gemini_approval_mode: Option<String>,

    /// Default provider name used when none is specified.
    /// Defaults to "codex".
    #[serde(default = "default_provider")]
    pub default_provider: String,

    /// Whether to allow API-key billing (vs subscription-only).
    /// Defaults to false.
    #[serde(default)]
    pub allow_api_billing: Option<bool>,

    /// Maximum concurrent provider calls.  Defaults to 4.
    #[serde(default)]
    pub max_concurrency: Option<usize>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            claude_reasoning_effort: None,
            codex_reasoning_effort: None,
            minimax_reasoning_effort: None,
            codex_sandbox_mode: None,
            gemini_approval_mode: None,
            default_provider: default_provider(),
            allow_api_billing: None,
            max_concurrency: None,
        }
    }
}

fn default_provider() -> String {
    "codex".to_string()
}

// ─── UI config ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_window_width")]
    pub window_width: f32,

    #[serde(default = "default_window_height")]
    pub window_height: f32,

    #[serde(default = "default_min_window_width")]
    pub min_window_width: f32,

    #[serde(default = "default_min_window_height")]
    pub min_window_height: f32,

    #[serde(default = "default_side_panel_width")]
    pub side_panel_width: f32,

    #[serde(default = "default_side_panel_min_width")]
    pub side_panel_min_width: f32,

    #[serde(default = "default_message_max_width")]
    pub message_max_width: f32,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            window_width: default_window_width(),
            window_height: default_window_height(),
            min_window_width: default_min_window_width(),
            min_window_height: default_min_window_height(),
            side_panel_width: default_side_panel_width(),
            side_panel_min_width: default_side_panel_min_width(),
            message_max_width: default_message_max_width(),
        }
    }
}

fn default_window_width() -> f32 {
    1000.0
}
fn default_window_height() -> f32 {
    600.0
}
fn default_min_window_width() -> f32 {
    980.0
}
fn default_min_window_height() -> f32 {
    300.0
}
fn default_side_panel_width() -> f32 {
    200.0
}
fn default_side_panel_min_width() -> f32 {
    100.0
}
fn default_message_max_width() -> f32 {
    700.0
}

// ─── Memory config ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize)]
pub struct MemoryConfig {
    /// Whether to enable workspace-scoped MemMachine retrieval.
    /// Defaults to true.
    #[serde(default = "default_enable_workspace_memory")]
    pub enable_workspace_memory: bool,

    /// Base URL for the MemMachine server.
    /// Defaults to http://127.0.0.1:8080 when omitted.
    #[serde(default)]
    pub memmachine_base_url: Option<String>,

    /// Organization identifier used for workspace memory projects.
    /// Defaults to rust-chatbot when omitted.
    #[serde(default)]
    pub memmachine_org_id: Option<String>,

    /// Number of top memories to retrieve in prompt injection.
    /// Defaults to 8.
    #[serde(default)]
    pub memory_top_k: Option<usize>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enable_workspace_memory: default_enable_workspace_memory(),
            memmachine_base_url: None,
            memmachine_org_id: None,
            memory_top_k: None,
        }
    }
}

fn default_enable_workspace_memory() -> bool {
    true
}

// ─── Background task config ─────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize)]
pub struct BackgroundTaskConfig {
    /// Task lease duration in seconds.  Defaults to 30.
    #[serde(default = "default_task_lease_secs")]
    pub task_lease_secs: i64,

    /// Task heartbeat interval in seconds.  Defaults to 5.
    #[serde(default = "default_task_heartbeat_secs")]
    pub task_heartbeat_secs: u64,

    /// Default maximum task attempts before declaring failure.
    /// Defaults to 3.
    #[serde(default = "default_task_max_attempts")]
    pub task_max_attempts: u32,

    /// Default maximum agents per background task batch.
    /// Defaults to 5.
    #[serde(default = "default_max_agents")]
    pub max_agents: u32,

    /// Default maximum iterations per background task batch.
    /// Defaults to 10.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,

    /// Whether background write tasks are allowed by default.
    /// Defaults to true.
    #[serde(default = "default_allow_write_tasks")]
    pub allow_write_tasks: bool,

    /// Whether to require review before applying edits.
    /// Defaults to true.
    #[serde(default = "default_require_review_before_apply")]
    pub require_review_before_apply: bool,

    /// Default design doc path for background workflows.
    #[serde(default)]
    pub design_doc_path: Option<String>,

    /// Default canonical design doc path.
    #[serde(default = "default_background_design_doc_path")]
    pub background_design_doc_path: String,

    /// Container image used when a background task opts into runsc isolation.
    #[serde(default)]
    pub background_runsc_image: Option<String>,

    /// Background dispatch max concurrent tasks.
    /// Defaults to 4.
    #[serde(default = "default_background_max_concurrent")]
    pub background_max_concurrent: usize,

    /// Auto-prune background task runs older than N days.  When set to `Some(N)`,
    /// old completed/failed/canceled runs and their artifacts are deleted on
    /// app startup (via the `maybe_auto_prune` hook).  When `None` (the
    /// default), auto-prune is disabled and normal coordination runs are
    /// unaffected.
    #[serde(default)]
    pub auto_prune_age_days: Option<u32>,
}

impl Default for BackgroundTaskConfig {
    fn default() -> Self {
        Self {
            task_lease_secs: default_task_lease_secs(),
            task_heartbeat_secs: default_task_heartbeat_secs(),
            task_max_attempts: default_task_max_attempts(),
            max_agents: default_max_agents(),
            max_iterations: default_max_iterations(),
            allow_write_tasks: default_allow_write_tasks(),
            require_review_before_apply: default_require_review_before_apply(),
            design_doc_path: None,
            background_design_doc_path: default_background_design_doc_path(),
            background_runsc_image: None,
            background_max_concurrent: default_background_max_concurrent(),
            auto_prune_age_days: None,
        }
    }
}

fn default_task_lease_secs() -> i64 {
    30
}
fn default_task_heartbeat_secs() -> u64 {
    5
}
fn default_task_max_attempts() -> u32 {
    3
}
fn default_max_agents() -> u32 {
    5
}
fn default_max_iterations() -> u32 {
    u32::MAX
}
fn default_allow_write_tasks() -> bool {
    true
}
fn default_require_review_before_apply() -> bool {
    true
}
fn default_background_design_doc_path() -> String {
    "docs/autonomous/design-doc.md".to_string()
}
fn default_background_max_concurrent() -> usize {
    4
}

fn default_background_runsc_image() -> String {
    "rust-chatbot-autonomous-runsc:latest".to_string()
}

// ─── Provider capability profile (loaded from workflow-registry/providers.json)

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProviderCapabilityRegistry {
    #[serde(default)]
    pub providers: Vec<ProviderCapabilityProfile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderCapabilityProfile {
    pub provider_id: String,
    #[serde(default)]
    pub model_override: Option<String>,
    #[serde(default)]
    pub prompt_family: PromptFamily,
    #[serde(default)]
    pub supported_categories: Vec<TaskCategory>,
    #[serde(default)]
    pub preferred_categories: Vec<TaskCategory>,
    #[serde(default)]
    pub fallback_chain: Vec<String>,
    #[serde(default)]
    pub cooldown_eligible: bool,
    #[serde(default = "default_profile_max_concurrency")]
    pub max_concurrency: usize,
    #[serde(default)]
    pub cost_tier: CostTier,
    #[serde(default)]
    pub notes: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PromptFamily {
    Mechanics,
    Principles,
}

impl Default for PromptFamily {
    fn default() -> Self {
        PromptFamily::Principles
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum CostTier {
    Low,
    Medium,
    High,
    Premium,
}

impl Default for CostTier {
    fn default() -> Self {
        CostTier::Medium
    }
}

fn default_profile_max_concurrency() -> usize {
    4
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskCategory {
    Survey,
    Design,
    Plan,
    DeepWrite,
    QuickWrite,
    Review,
    Verify,
    DocWrite,
    Research,
}

// ─── Compatibility accessor helpers ──────────────────────────────────────────
// These delegate to the loaded runtime config, falling back to the original
// hardcoded constant if no TOML file was found.

/// Returns the loaded AppConfig, or defaults if no config file exists.
pub fn app_config() -> AppConfig {
    load_config()
}

/// Provider knob: Claude reasoning effort.
pub fn claude_reasoning_effort() -> Option<String> {
    load_config().provider.claude_reasoning_effort.clone()
}

/// Provider knob: Codex reasoning effort.
pub fn codex_reasoning_effort() -> Option<String> {
    load_config().provider.codex_reasoning_effort.clone()
}

/// Provider knob: MiniMax reasoning effort.
pub fn minimax_reasoning_effort() -> Option<String> {
    load_config().provider.minimax_reasoning_effort.clone()
}

/// Returns the effective Codex reasoning effort, falling back to the default
/// constant when not explicitly configured.
pub fn effective_codex_reasoning_effort() -> Option<String> {
    codex_reasoning_effort().or_else(|| CODEX_REASONING_EFFORT.map(str::to_string))
}

/// Returns the effective Claude reasoning effort, falling back to the default
/// constant when not explicitly configured.
pub fn effective_claude_reasoning_effort() -> Option<String> {
    claude_reasoning_effort().or_else(|| CLAUDE_REASONING_EFFORT.map(str::to_string))
}

/// Returns the effective MiniMax reasoning effort, falling back to the default
/// constant when not explicitly configured.
pub fn effective_minimax_reasoning_effort() -> Option<String> {
    minimax_reasoning_effort().or_else(|| MINIMAX_REASONING_EFFORT.map(str::to_string))
}

/// Returns the effective Codex-compat reasoning effort for a provider.
pub fn effective_reasoning_effort_for_provider(
    provider: crate::model::ModelProvider,
) -> Option<String> {
    match provider {
        crate::model::ModelProvider::Claude => effective_claude_reasoning_effort(),
        crate::model::ModelProvider::MiniMax | crate::model::ModelProvider::MiniMaxForge => {
            effective_minimax_reasoning_effort()
        }
        crate::model::ModelProvider::Codex | crate::model::ModelProvider::Nvidia => {
            effective_codex_reasoning_effort()
        }
        _ => None,
    }
}

/// Provider knob: Codex sandbox mode.
pub fn codex_sandbox_mode() -> Option<String> {
    load_config().provider.codex_sandbox_mode.clone()
}

/// Provider knob: Gemini approval mode.
pub fn gemini_approval_mode() -> Option<String> {
    load_config().provider.gemini_approval_mode.clone()
}

/// Returns the effective Gemini approval mode, with a compile-time fallback to
/// `GEMINI_APPROVAL_MODE`.
pub fn effective_gemini_approval_mode() -> Option<String> {
    gemini_approval_mode().or_else(|| constants::GEMINI_APPROVAL_MODE.map(str::to_string))
}

/// Allow API billing (vs subscription-only).
pub fn allow_api_billing() -> bool {
    load_config()
        .provider
        .allow_api_billing
        .unwrap_or(ALLOW_API_BILLING)
}

/// Maximum concurrent provider calls.
pub fn max_concurrency() -> usize {
    load_config().provider.max_concurrency.unwrap_or(4)
}

/// Whether workspace memory is enabled.
pub fn enable_workspace_memory() -> bool {
    load_config().memory.enable_workspace_memory
}

pub fn memmachine_base_url() -> String {
    load_config()
        .memory
        .memmachine_base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_end_matches('/').to_string())
        .unwrap_or_else(|| "http://127.0.0.1:8080".to_string())
}

pub fn memmachine_org_id() -> String {
    load_config()
        .memory
        .memmachine_org_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "rust-chatbot".to_string())
}

pub fn workspace_memory_top_k() -> usize {
    load_config().memory.memory_top_k.unwrap_or(8)
}

/// Coordination: default task lease duration in seconds.
pub fn task_lease_secs() -> i64 {
    load_config().background.task_lease_secs
}

/// Coordination: default max task attempts.
pub fn task_max_attempts() -> u32 {
    load_config().background.task_max_attempts
}

/// Default max agents per background task batch.
pub fn background_max_agents() -> u32 {
    load_config().background.max_agents
}

/// Default max iterations per background task batch.
pub fn background_max_iterations() -> u32 {
    load_config().background.max_iterations
}

/// Returns the auto-prune age threshold from config, or `None` when
/// auto-prune is disabled (the default).  When `Some(N)`, background task runs
/// older than N days are automatically pruned on startup.
pub fn auto_prune_age_days() -> Option<u32> {
    load_config().background.auto_prune_age_days
}

/// Whether write tasks are allowed by default for background workflows.
pub fn background_allow_write_tasks() -> bool {
    load_config().background.allow_write_tasks
}

/// Whether review is required before applying edits.
pub fn background_require_review_before_apply() -> bool {
    load_config().background.require_review_before_apply
}

/// Background dispatcher max concurrent tasks.
pub fn background_max_concurrent() -> usize {
    load_config().background.background_max_concurrent
}

/// Default runsc container image for background provider calls.
pub fn background_runsc_image() -> String {
    load_config()
        .background
        .background_runsc_image
        .clone()
        .unwrap_or_else(default_background_runsc_image)
}

/// Load provider capability profiles from the checked-in provider registry.
/// Falls back to the embedded checked-in registry when the file is missing.
pub fn load_provider_profiles() -> BTreeMap<String, ProviderCapabilityProfile> {
    load_config()
        .load_provider_profiles()
        .unwrap_or_else(|_| supported_provider_profiles(embedded_provider_registry()))
}

mod constants;
mod paths;
#[cfg(test)]
mod tests;

pub use constants::*;
pub use paths::*;

// Extra impl for testing — loads profiles from an arbitrary path
impl AppConfig {
    #[cfg(test)]
    fn load_provider_profiles_from(
        &self,
        path: &std::path::Path,
    ) -> anyhow::Result<BTreeMap<String, ProviderCapabilityProfile>> {
        Ok(supported_provider_profiles(
            Self::load_provider_registry_from(path)?,
        ))
    }
}
