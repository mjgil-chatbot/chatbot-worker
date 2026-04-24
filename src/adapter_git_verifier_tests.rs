use super::*;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
fn restore_env_var(key: &str, previous: Option<OsString>) {
    match previous {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}
fn git(repo_root: &Path, args: &[&str]) -> String {
    git_stdout(repo_root, args).unwrap()
}
fn init_repo_with_origin(root: &Path) -> PathBuf {
    init_repo_with_origin_named(root, "repo", "remote.git")
}
fn init_repo_with_origin_named(root: &Path, repo_name: &str, remote_name: &str) -> PathBuf {
    let remote_root = root.join(remote_name);
    let repo_root = root.join(repo_name);
    fs::create_dir_all(&remote_root).unwrap();
    fs::create_dir_all(&repo_root).unwrap();

    git(
        root,
        &["init", "--bare", remote_root.to_string_lossy().as_ref()],
    );
    git(
        root,
        &["init", "-b", "main", repo_root.to_string_lossy().as_ref()],
    );
    git(&repo_root, &["config", "user.email", "test@example.com"]);
    git(&repo_root, &["config", "user.name", "Test User"]);
    fs::write(repo_root.join("README.md"), "seed\n").unwrap();
    git(&repo_root, &["add", "README.md"]);
    git(&repo_root, &["commit", "-m", "init"]);
    git(
        &repo_root,
        &[
            "remote",
            "add",
            "origin",
            remote_root.to_string_lossy().as_ref(),
        ],
    );
    git(&repo_root, &["push", "-u", "origin", "main"]);

    repo_root
}
#[test]
fn guard_only_activates_for_commit_or_push_requests() {
    assert!(
        GitSideEffectGuard::prepare("explain the code", Path::new("/tmp"))
            .unwrap()
            .is_none()
    );
    assert!(
        GitSideEffectGuard::prepare("commit the change", Path::new("/tmp"))
            .unwrap()
            .is_some()
    );
    assert!(
        GitSideEffectGuard::prepare("please commit", Path::new("/tmp"))
            .unwrap()
            .is_some()
    );
    assert!(
        GitSideEffectGuard::prepare("push it upstream", Path::new("/tmp"))
            .unwrap()
            .is_some()
    );
    assert!(GitSideEffectGuard::prepare("push", Path::new("/tmp"))
        .unwrap()
        .is_some());
    assert!(
        GitSideEffectGuard::prepare("create a branch for the fix", Path::new("/tmp"))
            .unwrap()
            .is_some()
    );
    assert!(
        GitSideEffectGuard::prepare("tag the release", Path::new("/tmp"))
            .unwrap()
            .is_some()
    );
    assert!(
        GitSideEffectGuard::prepare("create a git tag for the release", Path::new("/tmp"))
            .unwrap()
            .is_some()
    );
    assert!(
        GitSideEffectGuard::prepare(
            "can you make selection work on the actual selected text --- sometimes it captures the entire paragraph even though only a single line is selected",
            Path::new("/tmp")
        )
        .unwrap()
        .is_none()
    );
    assert!(
        GitSideEffectGuard::prepare("merge remote state from disk", Path::new("/tmp"))
            .unwrap()
            .is_none()
    );
    assert!(
        GitSideEffectGuard::prepare("merge the feature branch", Path::new("/tmp"))
            .unwrap()
            .is_some()
    );
    assert!(
        GitSideEffectGuard::prepare("show the commit history", Path::new("/tmp"))
            .unwrap()
            .is_none()
    );
    assert!(
        GitSideEffectGuard::prepare("fix push notification rendering", Path::new("/tmp"))
            .unwrap()
            .is_none()
    );
    assert!(
        GitSideEffectGuard::prepare("fix html tag rendering", Path::new("/tmp"))
            .unwrap()
            .is_none()
    );
}
#[test]
fn guard_ignores_git_examples_inside_pasted_transcript_blocks() {
    let query = "what was the issue --- chatbot-ui (7a98d139)\n\n\
Workspace\n\
/workspace/chatbot-ui I will abide by AGENTS.md\n\
Next Steps:\n\
- If you want, I can also harden commit and push intent detection the same way.\n\
- I can also prove merge the feature branch still triggers the guard.\n\n\
🤖 Assistant: May 21 10:04am\n\
Error: git side-effect verification failed for requested commit, merge in /workspace/chatbot-ui";

    assert!(GitSideEffectGuard::prepare(query, Path::new("/tmp"))
        .unwrap()
        .is_none());
}
#[test]
fn guard_ignores_inline_assistant_error_suffix_after_fix_request() {
    let query = "fix this --- 🤖 Assistant: May 21 12:04pm\n\n\
Error: git side-effect verification failed for requested commit in /workspace/chatbot/rust-chatbot: HEAD stayed at e41565b";

    assert!(GitSideEffectGuard::prepare(query, Path::new("/tmp"))
        .unwrap()
        .is_none());
}
