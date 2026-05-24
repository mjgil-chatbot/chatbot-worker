use super::executor;
use super::types::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use tempfile::tempdir;
use uuid::Uuid;
mod command_building;
static ENV_MUTEX: StdMutex<()> = StdMutex::new(());
pub(super) fn make_task(prompt: &str) -> BackgroundTask {
    BackgroundTask {
        id: Uuid::new_v4(),
        agent_profile: AgentProfile {
            role: "Worker".to_string(),
            provider: Some("codex".to_string()),
        },
        prompt: prompt.to_string(),
        context_budget: 1000,
        system_prompt: None,
        working_dir: None,
        resume_session_id: None,
    }
}
pub(super) struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}
impl EnvVarGuard {
    pub(super) fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }}
pub(super) fn command_args(command: &std::process::Command) -> Vec<String> {
    command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}
pub(super) fn command_env_value(command: &std::process::Command, key: &str) -> Option<String> {
    command.get_envs().find_map(|(name, value)| {
        (name.to_string_lossy() == key)
            .then(|| value.map(|value| value.to_string_lossy().into_owned()))
            .flatten()
    })
}
