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
fn provider_registry_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("workflow-registry")
        .join("providers.json")
}
fn default_provider() -> String {
    "codex".to_string()
}
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
fn default_enable_workspace_memory() -> bool {
    true
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
mod constants;
mod paths;
pub use constants::*;
pub use paths::*;
