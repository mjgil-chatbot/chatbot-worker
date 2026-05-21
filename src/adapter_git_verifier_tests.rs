use super::*;
use crate::adapter_worker::run_adapter;
use crate::worker::WorkerRequest;
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

#[test]
fn verifier_rejects_missing_claimed_commit_when_head_is_unchanged() {
    let tempdir = tempfile::tempdir().unwrap();
    let repo_root = init_repo_with_origin(tempdir.path());
    let guard = GitSideEffectGuard::prepare("commit and push", &repo_root)
        .unwrap()
        .expect("guard");

    let err = guard
        .verify_success("Done! committed as `deadbee` and pushed to origin/main.")
        .unwrap_err();
    let message = err.to_string();
    assert!(message.contains("HEAD stayed at"));
    assert!(message.contains("assistant referenced missing commit(s): deadbee"));
}

#[test]
fn verifier_ignores_hex_substrings_inside_plain_words() {
    let tempdir = tempfile::tempdir().unwrap();
    let repo_root = init_repo_with_origin(tempdir.path());
    let guard = GitSideEffectGuard::prepare("commit the change", &repo_root)
        .unwrap()
        .expect("guard");

    let err = guard
        .verify_success(
            "Verification passed with lightweight builds:\n\
build succeeded for chatbot-ui.",
        )
        .unwrap_err()
        .to_string();

    assert!(err.contains("HEAD stayed at"));
    assert!(!err.contains("assistant referenced missing commit(s): cceeded"));
}

#[test]
fn verifier_ignores_commit_claims_scoped_to_other_repos() {
    let tempdir = tempfile::tempdir().unwrap();
    let repo_root = init_repo_with_origin_named(tempdir.path(), "chatbot-ui", "origin.git");
    let guard = GitSideEffectGuard::prepare("commit and push", &repo_root)
        .unwrap()
        .expect("guard");

    fs::write(repo_root.join("verified.txt"), "verified\n").unwrap();
    git(&repo_root, &["add", "verified.txt"]);
    git(&repo_root, &["commit", "-m", "verify git side effects"]);
    git(&repo_root, &["push", "origin", "main"]);
    let sha = git(&repo_root, &["rev-parse", "--short", "HEAD"]);

    let output = format!(
        "Pushed both main branches.\n\
the-markdown-commons 0e13e72Fix egui markdown link hover underline\n\
chatbot-ui {}Document markdown selection and hover behavior",
        sha
    );

    guard
        .verify_success(&output)
        .expect("sibling repo commit labels should not fail current repo verification");
}

#[test]
fn verifier_ignores_local_only_commit_mentions_scoped_to_other_repos() {
    let tempdir = tempfile::tempdir().unwrap();
    let repo_root = init_repo_with_origin_named(tempdir.path(), "rust-chatbot", "origin.git");
    let guard = GitSideEffectGuard::prepare("commit and push", &repo_root)
        .unwrap()
        .expect("guard");

    fs::write(repo_root.join("verified.txt"), "verified\n").unwrap();
    git(&repo_root, &["add", "verified.txt"]);
    git(&repo_root, &["commit", "-m", "verify git side effects"]);
    git(&repo_root, &["push", "origin", "main"]);
    let sha = git(&repo_root, &["rev-parse", "--short", "HEAD"]);

    let output = format!(
        "Pushed the clean feature commits that landed on the remotes:\n\
rust-chatbot remote origin/main {sha}Document adapter transport retry logging\n\
\n\
One important note: chatbot-worker local main is now one commit ahead with an unrelated local commit, 2ce1f74 Add repo-aware worker engineering gate and I did not push that."
    );

    guard.verify_success(&output).expect(
        "local-only sibling repo commit mentions should not fail current repo verification",
    );
}

#[test]
fn verifier_rejects_missing_tag_branch_and_merge_side_effects() {
    let tempdir = tempfile::tempdir().unwrap();
    let repo_root = init_repo_with_origin(tempdir.path());

    let branch_err = GitSideEffectGuard::prepare("create a branch for this fix", &repo_root)
        .unwrap()
        .expect("branch guard")
        .verify_success("Done!")
        .unwrap_err()
        .to_string();
    assert!(branch_err.contains("no new local branch was created"));

    let tag_err = GitSideEffectGuard::prepare("tag the release", &repo_root)
        .unwrap()
        .expect("tag guard")
        .verify_success("Done!")
        .unwrap_err()
        .to_string();
    assert!(tag_err.contains("no new tag was created"));

    let merge_err = GitSideEffectGuard::prepare("merge the feature branch", &repo_root)
        .unwrap()
        .expect("merge guard")
        .verify_success("Done!")
        .unwrap_err()
        .to_string();
    assert!(merge_err.contains("merge claim not verified"));
}

#[cfg(unix)]
#[tokio::test]
async fn run_adapter_rejects_false_commit_and_push_success() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tempdir = tempfile::tempdir().unwrap();
    let repo_root = init_repo_with_origin(tempdir.path());
    let adapter_path = tempdir.path().join("fake-adapter-false-success.py");
    let previous_adapter = std::env::var_os("RUST_CHATBOT_AGENT_ADAPTER_CLI");

    fs::write(
        &adapter_path,
        r#"#!/usr/bin/env python3
import json
import sys

payload = {
    "type": "run_completed",
    "final_text": "Done! The patch was applied successfully and committed as `b9d1a1f` - pushed to origin/main.",
    "external_session": None,
}
sys.stdout.write(json.dumps(payload) + "\n")
sys.stdout.flush()
"#,
    )
    .unwrap();
    let mut perms = fs::metadata(&adapter_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&adapter_path, perms).unwrap();
    std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", &adapter_path);

    let request = WorkerRequest {
        provider: "claude".to_string(),
        query: "apply the patch, commit and push".to_string(),
        working_dir: Some(repo_root.to_string_lossy().into_owned()),
        system_prompt: None,
        resume_session_id: None,
        memory_context: None,
        parent_pid_override: None,
        runsc_dirs: vec![],
        runsc_image: None,
        model_override: None,
        command: "chat".to_string(),
        chat_id: "false-success-git-guard".to_string(),
    };

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        run_adapter(&request, Arc::new(AtomicBool::new(false))),
    )
    .await
    .expect("adapter run should finish")
    .unwrap();

    restore_env_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", previous_adapter);

    let error = result.error.expect("false success should be rejected");
    assert!(error.contains("git side-effect verification failed for requested commit"));
    assert!(error.contains("push"));
    assert!(error.contains("HEAD stayed at"));
    assert!(error.contains("assistant referenced missing commit(s): b9d1a1f"));
}

#[cfg(unix)]
#[tokio::test]
async fn run_adapter_persists_failed_verification_transcript_and_raw_captures() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tempdir = tempfile::tempdir().unwrap();
    let repo_root = init_repo_with_origin(tempdir.path());
    let adapter_path = tempdir.path().join("fake-adapter-failure-capture.py");
    let previous_adapter = std::env::var_os("RUST_CHATBOT_AGENT_ADAPTER_CLI");
    let chat_id = "failed-verification-capture";

    fs::write(
        &adapter_path,
        r#"#!/usr/bin/env python3
import json
import sys

events = [
    {
        "type": "raw_provider_event",
        "provider": "claude",
        "payload": {"type": "assistant", "message": {"content": [{"type": "custom_tool", "name": "ShellX"}]}},
    },
    {
        "type": "run_completed",
        "final_text": "Done! committed as `b9d1a1f` and pushed to origin/main.",
        "external_session": None,
    },
]

for event in events:
    sys.stdout.write(json.dumps(event) + "\n")
    sys.stdout.flush()
"#,
    )
    .unwrap();
    let mut perms = fs::metadata(&adapter_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&adapter_path, perms).unwrap();
    std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", &adapter_path);

    let request = crate::worker::WorkerRequest {
        provider: "claude".to_string(),
        query: "apply the patch, commit and push".to_string(),
        working_dir: Some(repo_root.to_string_lossy().into_owned()),
        system_prompt: None,
        resume_session_id: None,
        memory_context: None,
        parent_pid_override: None,
        runsc_dirs: vec![],
        runsc_image: None,
        model_override: None,
        command: "chat".to_string(),
        chat_id: chat_id.to_string(),
    };

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        run_adapter(&request, Arc::new(AtomicBool::new(false))),
    )
    .await
    .expect("adapter run should finish")
    .unwrap();

    restore_env_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", previous_adapter);

    let error = result.error.expect("verification should fail");
    assert!(error.contains("git side-effect verification failed"));

    let events = crate::stream::read_events(&crate::worker::events_path(chat_id));
    assert!(events.iter().any(|event| matches!(
        event,
        crate::stream::StreamEvent::LiveSummary { content, .. }
            if content.contains("git verification failed")
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        crate::stream::StreamEvent::Error { message, .. }
            if message.contains("git side-effect verification failed")
    )));

    let spool_dir = crate::worker::worker_dir(chat_id);
    let verifier_context =
        fs::read_to_string(git_verifier_context_path(&spool_dir)).expect("verifier context");
    let raw_adapter = fs::read_to_string(raw_adapter_events_path(&spool_dir)).unwrap();
    let raw_provider = fs::read_to_string(raw_provider_events_path(&spool_dir)).unwrap();
    assert!(verifier_context.contains("\"request_query\": \"apply the patch, commit and push\""));
    assert!(verifier_context.contains("\"require_commit\": true"));
    assert!(verifier_context.contains("\"require_push\": true"));
    assert!(!verifier_context.contains("Retrieved memory:"));
    assert!(raw_adapter.contains("\"type\": \"run_completed\""));
    assert!(raw_provider.contains("\"type\":\"assistant\""));
    assert!(raw_provider.contains("\"type\":\"custom_tool\""));
}

#[cfg(unix)]
#[tokio::test]
async fn run_adapter_accepts_real_commit_and_push_success() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tempdir = tempfile::tempdir().unwrap();
    let repo_root = init_repo_with_origin(tempdir.path());
    let adapter_path = tempdir.path().join("fake-adapter-real-success.py");
    let previous_adapter = std::env::var_os("RUST_CHATBOT_AGENT_ADAPTER_CLI");

    fs::write(
        &adapter_path,
        r#"#!/usr/bin/env python3
import json
import subprocess
import sys

args = sys.argv[1:]
cwd = None
for idx, arg in enumerate(args):
    if arg == "--cwd":
        cwd = args[idx + 1]
        break

if cwd is None:
    sys.exit(9)

with open(f"{cwd}/verified.txt", "w", encoding="utf-8") as handle:
    handle.write("verified\n")

subprocess.check_call(["git", "-C", cwd, "add", "verified.txt"])
subprocess.check_call(["git", "-C", cwd, "commit", "-m", "verify git side effects"])
subprocess.check_call(["git", "-C", cwd, "push", "origin", "main"])
sha = subprocess.check_output(["git", "-C", cwd, "rev-parse", "--short", "HEAD"], text=True).strip()
payload = {
    "type": "run_completed",
    "final_text": f"Done! committed as `{sha}` and pushed to origin/main.",
    "external_session": None,
}
sys.stdout.write(json.dumps(payload) + "\n")
sys.stdout.flush()
"#,
    )
    .unwrap();
    let mut perms = fs::metadata(&adapter_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&adapter_path, perms).unwrap();
    std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", &adapter_path);

    let request = WorkerRequest {
        provider: "claude".to_string(),
        query: "apply the patch, commit and push".to_string(),
        working_dir: Some(repo_root.to_string_lossy().into_owned()),
        system_prompt: None,
        resume_session_id: None,
        memory_context: None,
        parent_pid_override: None,
        runsc_dirs: vec![],
        runsc_image: None,
        model_override: None,
        command: "chat".to_string(),
        chat_id: "real-success-git-guard".to_string(),
    };

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        run_adapter(&request, Arc::new(AtomicBool::new(false))),
    )
    .await
    .expect("adapter run should finish")
    .unwrap();

    restore_env_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", previous_adapter);

    assert!(result.error.is_none(), "{:?}", result.error);
    assert!(result.output.contains("committed as"));
    assert!(repo_root.join("verified.txt").exists());
}
