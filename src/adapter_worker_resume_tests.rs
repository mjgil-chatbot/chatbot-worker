use super::*;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn bridge_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("claude-compat-chatbot-cli-adapter.py")
}

#[cfg(unix)]
fn write_executable_script(path: &Path, contents: String) {
    std::fs::write(path, contents).unwrap();
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn claude_compat_bridge_attempts_resume_without_local_transcript() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tempdir = tempfile::tempdir().unwrap();
    let fake_chatbot = tempdir.path().join("chatbot");
    let marker_path = tempdir.path().join("methods.log");

    write_executable_script(
        &fake_chatbot,
        format!(
            r#"#!/usr/bin/env python3
import json
import sys

if any(arg in ("--version", "--help") for arg in sys.argv[1:]):
    print("chatbot-fixture 0.0.0")
    sys.exit(0)

marker_path = {marker_path:?}

def emit(payload):
    sys.stdout.write(json.dumps(payload) + "\n")
    sys.stdout.flush()

def record(method):
    with open(marker_path, "a", encoding="utf-8") as handle:
        handle.write(method + "\n")

for raw_line in sys.stdin:
    request = json.loads(raw_line)
    method = request.get("method")
    record(method)
    if method == "initialize":
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"protocolVersion": "2024-11-05"}}}})
    elif method == "session/resume":
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"sessionId": request["params"]["sessionId"]}}}})
    elif method == "prompt":
        record("prompt_id=" + request["params"]["sessionId"])
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"message_id": "msg-1", "done": False}}}})
        emit({{"jsonrpc": "2.0", "method": "text_delta", "params": {{"text": "resumed ok"}}}})
        emit({{"jsonrpc": "2.0", "method": "completion", "params": {{"finishReason": "stop", "done": True}}}})
    elif method == "session/end":
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"sessionId": "resume-session", "ended": True}}}})
        sys.exit(0)
sys.exit(5)
"#,
            marker_path = marker_path.display().to_string()
        ),
    );

    let child = Command::new("python3")
        .arg(bridge_script_path())
        .arg("-p")
        .arg("follow up")
        .arg("--model")
        .arg("MiniMax-M2.7")
        .arg("--resume")
        .arg("resume-session")
        .arg("--dangerously-skip-permissions")
        .env("RUST_CHATBOT_CHATBOT_CLI", &fake_chatbot)
        .env("CHATBOT_CLAUDE_COMPAT_PROVIDER_NAME", "MiniMax")
        .env("CHATBOT_CLAUDE_COMPAT_PROVIDER_ID", "minimax")
        .env("CHATBOT_CLAUDE_COMPAT_PROVIDER_TYPE", "anthropic")
        .env(
            "CHATBOT_CLAUDE_COMPAT_BASE_URL",
            "https://example.invalid/v1",
        )
        .env("CHATBOT_CLAUDE_COMPAT_ENV_KEY", "FAKE_MINIMAX_KEY")
        .env("CHATBOT_CLAUDE_COMPAT_AUTH_MODE", "bearer")
        .env("FAKE_MINIMAX_KEY", "test-key")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::task::spawn_blocking(move || child.wait_with_output()),
    )
    .await
    .expect("bridge should exit")
    .expect("bridge wait task should join")
    .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success(), "stdout:\n{stdout}");
    assert!(stdout.contains("Resume diagnostics: missing bridge transcript"));
    assert_eq!(
        std::fs::read_to_string(&marker_path).unwrap(),
        "initialize\nsession/resume\nprompt\nprompt_id=resume-session\nsession/end\n"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn claude_compat_bridge_falls_back_to_create_after_resume_error() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tempdir = tempfile::tempdir().unwrap();
    let fake_chatbot = tempdir.path().join("chatbot");
    let marker_path = tempdir.path().join("methods.log");

    write_executable_script(
        &fake_chatbot,
        format!(
            r#"#!/usr/bin/env python3
import json
import sys

if any(arg in ("--version", "--help") for arg in sys.argv[1:]):
    print("chatbot-fixture 0.0.0")
    sys.exit(0)

marker_path = {marker_path:?}

def emit(payload):
    sys.stdout.write(json.dumps(payload) + "\n")
    sys.stdout.flush()

def record(method):
    with open(marker_path, "a", encoding="utf-8") as handle:
        handle.write(method + "\n")

for raw_line in sys.stdin:
    request = json.loads(raw_line)
    method = request.get("method")
    record(method)
    if method == "initialize":
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"protocolVersion": "2024-11-05"}}}})
    elif method == "session/resume":
        emit({{"jsonrpc": "2.0", "id": request["id"], "error": {{"message": "unknown session"}}}})
    elif method == "session/create":
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"sessionId": "fresh-session"}}}})
    elif method == "prompt":
        record("prompt_id=" + request["params"]["sessionId"])
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"message_id": "msg-2", "done": False}}}})
        emit({{"jsonrpc": "2.0", "method": "text_delta", "params": {{"text": "fresh ok"}}}})
        emit({{"jsonrpc": "2.0", "method": "completion", "params": {{"finishReason": "stop", "done": True}}}})
    elif method == "session/end":
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"sessionId": "fresh-session", "ended": True}}}})
        sys.exit(0)
sys.exit(6)
"#,
            marker_path = marker_path.display().to_string()
        ),
    );

    let child = Command::new("python3")
        .arg(bridge_script_path())
        .arg("-p")
        .arg("follow up")
        .arg("--model")
        .arg("MiniMax-M2.7")
        .arg("--resume")
        .arg("resume-session")
        .arg("--dangerously-skip-permissions")
        .env("RUST_CHATBOT_CHATBOT_CLI", &fake_chatbot)
        .env("CHATBOT_CLAUDE_COMPAT_PROVIDER_NAME", "MiniMax")
        .env("CHATBOT_CLAUDE_COMPAT_PROVIDER_ID", "minimax")
        .env("CHATBOT_CLAUDE_COMPAT_PROVIDER_TYPE", "anthropic")
        .env(
            "CHATBOT_CLAUDE_COMPAT_BASE_URL",
            "https://example.invalid/v1",
        )
        .env("CHATBOT_CLAUDE_COMPAT_ENV_KEY", "FAKE_MINIMAX_KEY")
        .env("CHATBOT_CLAUDE_COMPAT_AUTH_MODE", "bearer")
        .env("FAKE_MINIMAX_KEY", "test-key")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::task::spawn_blocking(move || child.wait_with_output()),
    )
    .await
    .expect("bridge should exit")
    .expect("bridge wait task should join")
    .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success(), "stdout:\n{stdout}");
    assert!(stdout.contains("Resume fallback: provider resume failed"));
    assert_eq!(
        std::fs::read_to_string(&marker_path).unwrap(),
        "initialize\nsession/resume\nsession/create\nprompt\nprompt_id=fresh-session\nsession/end\n"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn claude_compat_bridge_reuses_created_session_id_on_later_resume() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tempdir = tempfile::tempdir().unwrap();
    let fake_chatbot = tempdir.path().join("chatbot");
    let marker_path = tempdir.path().join("methods.log");
    let home_dir = tempdir.path().join("home");
    std::fs::create_dir_all(&home_dir).unwrap();

    write_executable_script(
        &fake_chatbot,
        format!(
            r#"#!/usr/bin/env python3
import json
import sys

if any(arg in ("--version", "--help") for arg in sys.argv[1:]):
    print("chatbot-fixture 0.0.0")
    sys.exit(0)

marker_path = {marker_path:?}

def emit(payload):
    sys.stdout.write(json.dumps(payload) + "\n")
    sys.stdout.flush()

def record(entry):
    with open(marker_path, "a", encoding="utf-8") as handle:
        handle.write(entry + "\n")

for raw_line in sys.stdin:
    request = json.loads(raw_line)
    method = request.get("method")
    record(method)
    if method == "initialize":
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"protocolVersion": "2024-11-05"}}}})
    elif method == "session/create":
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"sessionId": "server-session-1"}}}})
    elif method == "session/resume":
        record("resume_id=" + request["params"]["sessionId"])
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"sessionId": request["params"]["sessionId"]}}}})
    elif method == "prompt":
        record("prompt_id=" + request["params"]["sessionId"])
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"message_id": "msg-3", "done": False}}}})
        emit({{"jsonrpc": "2.0", "method": "text_delta", "params": {{"text": "ok"}}}})
        emit({{"jsonrpc": "2.0", "method": "completion", "params": {{"finishReason": "stop", "done": True}}}})
    elif method == "session/end":
        record("end_id=" + request["params"]["sessionId"])
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"sessionId": request["params"]["sessionId"], "ended": True}}}})
        sys.exit(0)
sys.exit(7)
"#,
            marker_path = marker_path.display().to_string()
        ),
    );

    let first_child = Command::new("python3")
        .arg(bridge_script_path())
        .arg("-p")
        .arg("first turn")
        .arg("--model")
        .arg("MiniMax-M2.7")
        .arg("--dangerously-skip-permissions")
        .env("HOME", &home_dir)
        .env("RUST_CHATBOT_CHATBOT_CLI", &fake_chatbot)
        .env("CHATBOT_CLAUDE_COMPAT_PROVIDER_NAME", "MiniMax")
        .env("CHATBOT_CLAUDE_COMPAT_PROVIDER_ID", "minimax")
        .env("CHATBOT_CLAUDE_COMPAT_PROVIDER_TYPE", "anthropic")
        .env(
            "CHATBOT_CLAUDE_COMPAT_BASE_URL",
            "https://example.invalid/v1",
        )
        .env("CHATBOT_CLAUDE_COMPAT_ENV_KEY", "FAKE_MINIMAX_KEY")
        .env("CHATBOT_CLAUDE_COMPAT_AUTH_MODE", "bearer")
        .env("FAKE_MINIMAX_KEY", "test-key")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let first_output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::task::spawn_blocking(move || first_child.wait_with_output()),
    )
    .await
    .expect("first bridge run should exit")
    .expect("first bridge wait task should join")
    .unwrap();

    let first_stdout = String::from_utf8(first_output.stdout).unwrap();
    assert!(
        first_output.status.success(),
        "first run should succeed\nstdout:\n{first_stdout}"
    );
    assert!(first_stdout.contains("\"session_id\": \"server-session-1\""));

    let second_child = Command::new("python3")
        .arg(bridge_script_path())
        .arg("-p")
        .arg("second turn")
        .arg("--model")
        .arg("MiniMax-M2.7")
        .arg("--resume")
        .arg("server-session-1")
        .arg("--dangerously-skip-permissions")
        .env("HOME", &home_dir)
        .env("RUST_CHATBOT_CHATBOT_CLI", &fake_chatbot)
        .env("CHATBOT_CLAUDE_COMPAT_PROVIDER_NAME", "MiniMax")
        .env("CHATBOT_CLAUDE_COMPAT_PROVIDER_ID", "minimax")
        .env("CHATBOT_CLAUDE_COMPAT_PROVIDER_TYPE", "anthropic")
        .env(
            "CHATBOT_CLAUDE_COMPAT_BASE_URL",
            "https://example.invalid/v1",
        )
        .env("CHATBOT_CLAUDE_COMPAT_ENV_KEY", "FAKE_MINIMAX_KEY")
        .env("CHATBOT_CLAUDE_COMPAT_AUTH_MODE", "bearer")
        .env("FAKE_MINIMAX_KEY", "test-key")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let second_output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::task::spawn_blocking(move || second_child.wait_with_output()),
    )
    .await
    .expect("second bridge run should exit")
    .expect("second bridge wait task should join")
    .unwrap();

    let second_stdout = String::from_utf8(second_output.stdout).unwrap();
    assert!(
        second_output.status.success(),
        "second run should succeed\nstdout:\n{second_stdout}"
    );
    assert!(second_stdout.contains("\"session_id\": \"server-session-1\""));
    assert_eq!(
        std::fs::read_to_string(&marker_path).unwrap(),
        "initialize\nsession/create\nprompt\nprompt_id=server-session-1\nsession/end\nend_id=server-session-1\ninitialize\nsession/resume\nresume_id=server-session-1\nprompt\nprompt_id=server-session-1\nsession/end\nend_id=server-session-1\n"
    );
}
