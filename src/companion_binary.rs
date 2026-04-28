use std::env;
use std::path::{Path, PathBuf};

const AGENT_ADAPTER_CLI: &str = "agent-adapter-cli";
const AGENT_ADAPTER_CLI_ENV: &str = "RUST_CHATBOT_AGENT_ADAPTER_CLI";
const MINIMAX_CHATBOT_ADAPTER: &str = "minimax-chatbot-cli-adapter.py";
const MINIMAX_CHATBOT_ADAPTER_ENV: &str = "RUST_CHATBOT_MINIMAX_CHATBOT_ADAPTER";

pub fn resolve_agent_adapter_cli() -> PathBuf {
    resolve_agent_adapter_cli_with_current_exe(current_exe_dir())
        .unwrap_or_else(|| PathBuf::from(AGENT_ADAPTER_CLI))
}

pub fn resolve_minimax_chatbot_adapter() -> PathBuf {
    resolve_minimax_chatbot_adapter_with_current_exe(current_exe_dir()).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scripts")
            .join(MINIMAX_CHATBOT_ADAPTER)
    })
}

fn resolve_agent_adapter_cli_with_current_exe(current_exe_dir: Option<PathBuf>) -> Option<PathBuf> {
    resolve_agent_adapter_cli_candidates(
        env_override(AGENT_ADAPTER_CLI_ENV),
        dev_repo_fallback(),
        current_exe_dir.and_then(|dir| existing_file(dir.join(AGENT_ADAPTER_CLI))),
        find_on_path(AGENT_ADAPTER_CLI),
    )
}

fn resolve_minimax_chatbot_adapter_with_current_exe(
    current_exe_dir: Option<PathBuf>,
) -> Option<PathBuf> {
    env_override(MINIMAX_CHATBOT_ADAPTER_ENV)
        .or_else(|| {
            current_exe_dir.and_then(|dir| existing_file(dir.join(MINIMAX_CHATBOT_ADAPTER)))
        })
        .or_else(minimax_chatbot_adapter_repo_fallback)
}

fn resolve_agent_adapter_cli_candidates(
    env_override_path: Option<PathBuf>,
    dev_repo_path: Option<PathBuf>,
    current_exe_path: Option<PathBuf>,
    path_binary: Option<PathBuf>,
) -> Option<PathBuf> {
    env_override_path
        .or(dev_repo_path)
        .or(current_exe_path)
        .or(path_binary)
}

fn env_override(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn current_exe_dir() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok().or(Some(path)))
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

fn find_on_path(binary_name: &str) -> Option<PathBuf> {
    let path_env = env::var_os("PATH")?;
    env::split_paths(&path_env).find_map(|dir| existing_file(dir.join(binary_name)))
}

fn dev_repo_fallback() -> Option<PathBuf> {
    dev_repo_candidates().into_iter().find_map(existing_file)
}

fn minimax_chatbot_adapter_repo_fallback() -> Option<PathBuf> {
    minimax_chatbot_adapter_candidates()
        .into_iter()
        .find_map(existing_file)
}

fn dev_repo_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(parent) = manifest_dir.parent() {
        candidates.extend(adapter_target_candidates(parent.join("chatbot-adapter")));
    }

    if let Some(home) = env::var_os("HOME") {
        candidates.extend(adapter_target_candidates(
            Path::new(&home).join("git/chatbot-adapter"),
        ));
    }

    candidates
}

fn minimax_chatbot_adapter_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    candidates.push(manifest_dir.join("scripts").join(MINIMAX_CHATBOT_ADAPTER));

    if let Some(home) = env::var_os("HOME") {
        candidates.push(
            Path::new(&home)
                .join("git/chatbot-worker/scripts")
                .join(MINIMAX_CHATBOT_ADAPTER),
        );
    }

    candidates
}

fn adapter_target_candidates(repo_root: PathBuf) -> Vec<PathBuf> {
    let target = repo_root.join("target");
    vec![
        target.join("release/agent-adapter-cli"),
        target.join("release/release/agent-adapter-cli"),
        target.join("debug/agent-adapter-cli"),
    ]
}

fn existing_file(path: PathBuf) -> Option<PathBuf> {
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TEST_ENV_LOCK;

    fn restore_env_var(name: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => env::set_var(name, value),
            None => env::remove_var(name),
        }
    }

    #[test]
    fn resolve_agent_adapter_cli_prefers_env_override() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = env::var_os(AGENT_ADAPTER_CLI_ENV);
        env::set_var(AGENT_ADAPTER_CLI_ENV, "/tmp/custom-agent-adapter-cli");

        let resolved = resolve_agent_adapter_cli_with_current_exe(None).unwrap();

        restore_env_var(AGENT_ADAPTER_CLI_ENV, previous);
        assert_eq!(resolved, PathBuf::from("/tmp/custom-agent-adapter-cli"));
    }

    #[test]
    fn resolve_agent_adapter_cli_uses_path_when_available() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous_path = env::var_os("PATH");
        let previous_override = env::var_os(AGENT_ADAPTER_CLI_ENV);
        env::remove_var(AGENT_ADAPTER_CLI_ENV);

        let tmp = tempfile::tempdir().unwrap();
        let binary = tmp.path().join(AGENT_ADAPTER_CLI);
        std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&binary).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&binary, perms).unwrap();
        }

        env::set_var("PATH", tmp.path());
        let resolved =
            resolve_agent_adapter_cli_candidates(None, None, None, Some(binary.clone())).unwrap();

        restore_env_var("PATH", previous_path);
        restore_env_var(AGENT_ADAPTER_CLI_ENV, previous_override);
        assert_eq!(resolved, binary);
    }

    #[test]
    fn resolve_agent_adapter_cli_prefers_dev_repo_over_path() {
        let dev_repo_binary = PathBuf::from("/tmp/dev-repo/agent-adapter-cli");
        let path_binary = PathBuf::from("/tmp/path/agent-adapter-cli");

        let resolved = resolve_agent_adapter_cli_candidates(
            None,
            Some(dev_repo_binary.clone()),
            None,
            Some(path_binary),
        )
        .unwrap();

        assert_eq!(resolved, dev_repo_binary);
    }

    #[test]
    fn resolve_agent_adapter_cli_prefers_dev_repo_over_current_exe_dir() {
        let current_exe_binary = PathBuf::from("/tmp/current-exe/agent-adapter-cli");
        let dev_repo_binary = PathBuf::from("/tmp/dev-repo/agent-adapter-cli");

        let resolved = resolve_agent_adapter_cli_candidates(
            None,
            Some(dev_repo_binary.clone()),
            Some(current_exe_binary),
            None,
        )
        .unwrap();

        assert_eq!(resolved, dev_repo_binary);
    }

    #[test]
    fn adapter_target_candidates_prefer_canonical_release_before_nested_release() {
        let repo_root = PathBuf::from("/tmp/chatbot-adapter");
        let candidates = adapter_target_candidates(repo_root);

        assert_eq!(
            candidates[0],
            PathBuf::from("/tmp/chatbot-adapter/target/release/agent-adapter-cli")
        );
        assert_eq!(
            candidates[1],
            PathBuf::from("/tmp/chatbot-adapter/target/release/release/agent-adapter-cli")
        );
    }

    #[test]
    fn resolve_minimax_chatbot_adapter_prefers_env_override() {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = env::var_os(MINIMAX_CHATBOT_ADAPTER_ENV);
        env::set_var(
            MINIMAX_CHATBOT_ADAPTER_ENV,
            "/tmp/custom-minimax-chatbot-cli-adapter.py",
        );

        let resolved = resolve_minimax_chatbot_adapter_with_current_exe(None).unwrap();

        restore_env_var(MINIMAX_CHATBOT_ADAPTER_ENV, previous);
        assert_eq!(
            resolved,
            PathBuf::from("/tmp/custom-minimax-chatbot-cli-adapter.py")
        );
    }

    #[test]
    fn resolve_minimax_chatbot_adapter_prefers_current_exe_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let adapter_binary = tmp.path().join(MINIMAX_CHATBOT_ADAPTER);
        std::fs::write(&adapter_binary, "#!/usr/bin/env python3\n").unwrap();
        let resolved =
            resolve_minimax_chatbot_adapter_with_current_exe(Some(tmp.path().to_path_buf()))
                .unwrap();

        assert_eq!(resolved, adapter_binary);
    }
}
