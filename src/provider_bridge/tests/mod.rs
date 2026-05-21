use super::*;
use ai_providers::sandbox::RunscMountTarget;
use serde_json::json;
use std::path::PathBuf;
mod cli_stream;
mod config_helpers;
mod gemini_runsc_home;
mod mcp_sync;
mod provider_configs;
/// Shared crate-wide HOME/env mutex. Re-exported under the historic local
/// name so existing call sites compile unchanged. A single mutex across all
/// test modules is required: any module-local mutex would race with HOME
/// mutations elsewhere (config tests, hooks, session_runtime_tests).
use crate::TEST_ENV_LOCK as ENV_LOCK;
/// Helper: temporarily removes env vars, runs the closure, restores them.
fn with_cleared_env<F, R>(vars: &[&str], f: F) -> R
where
    F: FnOnce() -> R,
{
    let saved: Vec<(&str, Option<String>)> =
        vars.iter().map(|v| (*v, std::env::var(v).ok())).collect();
    for v in vars {
        std::env::remove_var(v);
    }
    let result = f();
    for (v, val) in &saved {
        match val {
            Some(val) => std::env::set_var(v, val),
            None => std::env::remove_var(v),
        }
    }
    result
}
