//! Shared worker/runtime library for `rust-chatbot` and `auto-runs`.
//!
//! This crate owns the provider bridge, worker spool/types, hooks, shared
//! storage helpers, and the canonical `chatbot-worker` implementation.

use std::path::Path;
const MANAGED_LOCAL_BIN_MARKER: &str = "# managed by rust-chatbot";
const SEMBLE_WRAPPER_NAME: &str = "semble";
fn shell_double_quoted(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`");
    format!("\"{escaped}\"")
}
fn managed_semble_wrapper_script() -> String {
    let semble_root = crate::config::checkout_root()
        .join("semble-rs")
        .join("semble-rs-work");
    let default_root = shell_double_quoted(&semble_root.to_string_lossy());
    format!(
        "#!/usr/bin/env bash\n\
set -euo pipefail\n\
# managed by rust-chatbot\n\
DEFAULT_SEMBLE_RS_ROOT={default_root}\n\
SEMBLE_RS_ROOT=\"${{SEMBLE_RS_ROOT:-$DEFAULT_SEMBLE_RS_ROOT}}\"\n\
LOCAL_BIN=\"$SEMBLE_RS_ROOT/target/release/semble\"\n\
MANIFEST=\"$SEMBLE_RS_ROOT/Cargo.toml\"\n\
if [[ -x \"$LOCAL_BIN\" ]]; then\n\
  exec \"$LOCAL_BIN\" \"$@\"\n\
fi\n\
exec cargo run --release --quiet --manifest-path \"$MANIFEST\" --bin semble -- \"$@\"\n"
    )
}
pub mod adapter_git_verifier;
pub mod adapter_live_summary;
pub mod adapter_tool_call_schema;
pub mod adapter_transport;
pub mod adapter_worker;
pub mod cgroup;
pub mod chatbot_worker;
pub mod chatbot_worker_adapter;
pub mod chatbot_worker_runtime;
pub mod chatbot_worker_trace;
pub mod companion_binary;
pub mod config;
pub mod data;
pub mod debug;
pub mod delta_resolution;
pub mod hooks;
pub mod kimi_bridge;
pub mod memory;
pub mod model;
pub mod omniforge;
pub mod provider_bridge;
pub mod provider_catalog;
pub mod storage;
pub mod stream;
pub mod stream_output;
pub mod summary;
pub mod worker;
pub mod worker_ext;
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AgentRole {
    Orchestrator,
    Reviewer,
    Explorer,
    Worker,
    DeepWorker,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AutonomousExecutionEnvironment {
    Host,
    Runsc,
}
/// Shared test lock for tests that mutate the `HOME` environment variable.
pub static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// Ensure the user-local bin directory has a symlink to the current executable
/// and provision managed helper wrappers used by provider runtimes.
pub fn ensure_path_symlink() {
    let Ok(exe) = std::env::current_exe().and_then(|p| p.canonicalize()) else {
        return;
    };
    let Some(name) = exe.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let bin_dir = Path::new(&home).join(".local/bin");
    let _ = std::fs::create_dir_all(&bin_dir);
    ensure_local_bin_symlink(&bin_dir, name, &exe);
    let semble_wrapper = managed_semble_wrapper_script();
    ensure_managed_local_bin_script(&bin_dir, SEMBLE_WRAPPER_NAME, &semble_wrapper);
}
fn ensure_local_bin_symlink(bin_dir: &Path, name: &str, exe: &Path) {
    let link = bin_dir.join(name);
    if link.read_link().ok().as_deref() == Some(exe) {
        return;
    }
    let is_managed_symlink = link
        .symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    if is_managed_symlink {
        let _ = std::fs::remove_file(&link);
    } else if link.exists() {
        return;
    }
    let _ = std::os::unix::fs::symlink(exe, &link);
}
fn ensure_managed_local_bin_script(bin_dir: &Path, name: &str, contents: &str) {
    let path = bin_dir.join(name);
    match std::fs::read_to_string(&path) {
        Ok(existing) if existing == contents => return,
        Ok(existing) if !existing.contains(MANAGED_LOCAL_BIN_MARKER) => return,
        Ok(_) | Err(_) => {}
    }

    if std::fs::write(&path, contents).is_err() {
        return;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if let Ok(metadata) = std::fs::metadata(&path) {
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o755);
            let _ = std::fs::set_permissions(&path, permissions);
        }
    }
}
#[cfg(test)]
mod local_bin_tests {
    use super::*;

    #[test]
    fn managed_semble_wrapper_is_created_and_executable() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let bin_dir = tmp.path().join(".local/bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let script = managed_semble_wrapper_script();

        ensure_managed_local_bin_script(&bin_dir, SEMBLE_WRAPPER_NAME, &script);

        let wrapper = bin_dir.join(SEMBLE_WRAPPER_NAME);
        let contents = std::fs::read_to_string(&wrapper).unwrap();
        assert_eq!(contents, script);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = std::fs::metadata(wrapper).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111);
        }
    }

    #[test]
    fn managed_wrapper_targets_checkout_root_semble_rs() {
        let script = managed_semble_wrapper_script();
        assert!(script.contains("/semble-rs/semble-rs-work"));
        assert!(script.contains("cargo run --release --quiet"));
        assert!(script.contains("--bin semble -- \"$@\""));
    }

    #[test]
    fn unmanaged_wrapper_is_left_untouched() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let bin_dir = tmp.path().join(".local/bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let wrapper = bin_dir.join(SEMBLE_WRAPPER_NAME);
        std::fs::write(&wrapper, "#!/bin/sh\necho custom\n").unwrap();
        let script = managed_semble_wrapper_script();

        ensure_managed_local_bin_script(&bin_dir, SEMBLE_WRAPPER_NAME, &script);

        assert_eq!(
            std::fs::read_to_string(wrapper).unwrap(),
            "#!/bin/sh\necho custom\n"
        );
    }
}
