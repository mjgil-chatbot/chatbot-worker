//! Shared workspace and session directory helpers.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config;
use crate::data::ChatSession;

pub fn session_working_dir(session: Option<&ChatSession>) -> Option<&str> {
    session
        .and_then(|session| session.launched_from.as_deref())
        .filter(|dir| !dir.trim().is_empty())
}

pub fn current_project_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| Path::new(config::repo_root()).to_path_buf())
}

pub fn project_root_for_working_dir(working_dir: Option<&str>) -> PathBuf {
    working_dir
        .map(PathBuf::from)
        .unwrap_or_else(current_project_root)
}

pub fn project_root_for_session(session: Option<&ChatSession>) -> PathBuf {
    project_root_for_working_dir(session_working_dir(session))
}

pub fn apply_session_working_dir(cmd: &mut Command, session: Option<&ChatSession>) {
    if let Some(dir) = session_working_dir(session) {
        cmd.current_dir(dir);
    }
}

pub fn gemini_project_settings_path(session: Option<&ChatSession>) -> PathBuf {
    let settings_dir = gemini_project_settings_dir(session);
    let local = settings_dir.join("settings.local.json");
    if local.exists() {
        local
    } else {
        settings_dir.join("settings.json")
    }
}

fn gemini_project_settings_dir(session: Option<&ChatSession>) -> PathBuf {
    let project_root = project_root_for_session(session);
    let project_settings_dir = project_root.join(".gemini");
    if project_settings_dir.exists() {
        project_settings_dir
    } else {
        config::get_config_dir(crate::model::ModelProvider::Gemini)
            .map(PathBuf::from)
            .unwrap_or_else(|| config::canonical_shared_root_dir().join(".gemini"))
    }
}

pub fn should_skip_codex_git_repo_check(working_dir: Option<&str>) -> bool {
    let Some(dir) = working_dir else {
        return false;
    };
    let Ok(canonical_dir) = Path::new(dir).canonicalize() else {
        return false;
    };

    config::trusted_workspace_roots()
        .iter()
        .filter_map(|prefix| prefix.canonicalize().ok())
        .any(|prefix| canonical_dir.starts_with(&prefix))
        && !is_inside_git_repo(&canonical_dir)
}

fn is_inside_git_repo(path: &Path) -> bool {
    path.ancestors()
        .any(|ancestor| ancestor.join(".git").exists())
}

#[cfg(test)]
mod tests {
    use super::{gemini_project_settings_path, should_skip_codex_git_repo_check};
    use crate::config;
    use crate::data::ChatSession;
    use crate::TEST_ENV_LOCK;
    use chrono::Utc;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "rust-chatbot-workspace-tests-{}-{}-{}",
            label,
            std::process::id(),
            nanos
        ))
    }

    fn with_trusted_workspace_root<T>(root: &Path, run: impl FnOnce() -> T) -> T {
        let _lock = TEST_ENV_LOCK.lock().unwrap_or_else(|err| err.into_inner());
        let previous = std::env::var_os(config::TRUSTED_WORKSPACE_ROOTS_ENV);
        std::env::set_var(config::TRUSTED_WORKSPACE_ROOTS_ENV, root);
        let result = run();
        match previous {
            Some(value) => std::env::set_var(config::TRUSTED_WORKSPACE_ROOTS_ENV, value),
            None => std::env::remove_var(config::TRUSTED_WORKSPACE_ROOTS_ENV),
        }
        result
    }

    #[test]
    fn skips_git_repo_check_for_non_repo_under_trusted_root() {
        let root = unique_temp_dir("trusted-root");
        let workspace = root.join("non-repo");
        fs::create_dir_all(&workspace).unwrap();

        let result = with_trusted_workspace_root(&root, || {
            should_skip_codex_git_repo_check(Some(workspace.to_string_lossy().as_ref()))
        });

        fs::remove_dir_all(&root).unwrap();
        assert!(result);
    }

    #[test]
    fn does_not_skip_git_repo_check_inside_repo() {
        let root = unique_temp_dir("trusted-root");
        let repo = root.join("repo");
        let nested = repo.join("src");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(&nested).unwrap();

        let result = with_trusted_workspace_root(&root, || {
            should_skip_codex_git_repo_check(Some(nested.to_string_lossy().as_ref()))
        });

        fs::remove_dir_all(&root).unwrap();
        assert!(!result);
    }

    fn test_session(launched_from: PathBuf) -> ChatSession {
        ChatSession {
            id: "workspace-test".to_string(),
            name: "Workspace Test".to_string(),
            system_prompt: None,
            json_summary: None,
            claude_session_id: None,
            minimax_session_id: None,
            qwen_session_id: None,
            codex_session_id: None,
            nvidia_session_id: None,
            gemini_session_id: None,
            openrouter_session_id: None,
            omniforge_session_id: None,
            omniforge_model_id: None,
            kimi_session_id: None,
            opencode_session_id: None,
            hermes_session_id: None,
            forge_session_id: None,
            claw_session_id: None,
            launched_from: Some(launched_from.to_string_lossy().into_owned()),
            runsc_dirs: Vec::new(),
            runsc_image: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            messages: vec![],
        }
    }

    #[test]
    fn gemini_project_settings_falls_back_to_global_config_for_external_workspace() {
        let workspace = unique_temp_dir("external-gemini");
        fs::create_dir_all(&workspace).unwrap();
        let session = test_session(workspace.clone());

        let result = gemini_project_settings_path(Some(&session));

        fs::remove_dir_all(&workspace).unwrap();
        let global_gemini_dir = config::get_config_dir(crate::model::ModelProvider::Gemini)
            .map(PathBuf::from)
            .unwrap_or_else(|| config::canonical_shared_root_dir().join(".gemini"));
        let expected = if global_gemini_dir.join("settings.local.json").exists() {
            global_gemini_dir.join("settings.local.json")
        } else {
            global_gemini_dir.join("settings.json")
        };
        assert_eq!(result, expected);
    }

    #[test]
    fn gemini_project_settings_uses_session_root_when_local_override_exists() {
        let repo_nested = unique_temp_dir("gemini-project-settings");
        let gemini_dir = repo_nested.join(".gemini");
        let _ = fs::remove_dir_all(&repo_nested);
        fs::create_dir_all(&gemini_dir).unwrap();
        fs::write(gemini_dir.join("settings.json"), "{}\n").unwrap();
        let session = test_session(repo_nested.clone());

        let result = gemini_project_settings_path(Some(&session));

        assert_eq!(result, gemini_dir.join("settings.json"));
        let _ = fs::remove_dir_all(&repo_nested);
    }

    #[test]
    fn gemini_project_settings_prefers_repo_local_override_when_present() {
        let repo_nested = unique_temp_dir("gemini-local-settings");
        let gemini_dir = repo_nested.join(".gemini");
        let _ = fs::remove_dir_all(&repo_nested);
        fs::create_dir_all(&gemini_dir).unwrap();
        fs::write(gemini_dir.join("settings.json"), "{}\n").unwrap();
        fs::write(gemini_dir.join("settings.local.json"), "{}\n").unwrap();
        let session = test_session(repo_nested.clone());

        let result = gemini_project_settings_path(Some(&session));

        assert_eq!(result, gemini_dir.join("settings.local.json"));
        let _ = fs::remove_dir_all(&repo_nested);
    }
}
