use super::*;
use crate::companion_binary::resolve_minimax_chatbot_adapter;
use agent_adapter_core::DANGEROUS_DEFAULT_ACTIVE;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[test]
fn provider_string_round_trips() {
    let providers = vec![
        (ProviderId::Claude, "claude"),
        (ProviderId::MiniMax, "minimax"),
        (ProviderId::Codex, "codex"),
        (ProviderId::Gemini, "gemini"),
        (ProviderId::Kimi, "kimi"),
        (ProviderId::OpenCode, "opencode"),
        (ProviderId::Hermes, "hermes"),
        (ProviderId::Forge, "forge"),
        (ProviderId::Claw, "claw"),
        (ProviderId::Custom("custom".to_string()), "custom"),
    ];
    for (provider, expected) in providers {
        assert_eq!(provider_string(&provider), expected);
    }
}

#[test]
fn claude_compat_providers_skip_sandbox_dirs() {
    assert!(!provider_supports_sandbox_dirs(&ProviderId::Claude));
    assert!(!provider_supports_sandbox_dirs(&ProviderId::MiniMax));
    assert!(provider_supports_sandbox_dirs(&ProviderId::Codex));
}

#[test]
fn shared_tool_call_live_update_schema_exempts_only_passthrough_providers() {
    for provider in [
        ModelProvider::Claude,
        ModelProvider::MiniMax,
        ModelProvider::Codex,
        ModelProvider::Gemini,
        ModelProvider::Kimi,
        ModelProvider::OpenCode,
        ModelProvider::Claw,
    ] {
        assert!(
            provider_uses_shared_tool_call_live_update_schema(Some(provider)),
            "{} should use the shared tool-call live update schema",
            provider.as_str()
        );
    }

    for provider in [ModelProvider::Forge, ModelProvider::Hermes] {
        assert!(
            !provider_uses_shared_tool_call_live_update_schema(Some(provider)),
            "{} should stay explicitly exempt from the shared tool-call live update schema",
            provider.as_str()
        );
    }
}

#[test]
fn worker_request_to_chat_request_minimal() {
    let req = WorkerRequest {
        provider: "claude".to_string(),
        query: "hello".to_string(),
        working_dir: Some("/tmp".to_string()),
        system_prompt: None,
        resume_session_id: None,
        memory_context: None,
        parent_pid_override: None,
        runsc_dirs: vec![],
        runsc_image: None,
        model_override: None,
        command: "chat".to_string(),
        chat_id: "test-123".to_string(),
    };
    let chat = worker_request_to_chat_request(&req).unwrap();
    assert_eq!(chat.provider, ProviderId::Claude);
    assert_eq!(chat.prompt, "hello");
    assert_eq!(chat.cwd, PathBuf::from("/tmp"));
    assert!(matches!(chat.resume, ResumeMode::None));
}

#[test]
fn worker_request_to_chat_request_with_resume() {
    let req = WorkerRequest {
        provider: "claude".to_string(),
        query: "continue".to_string(),
        working_dir: None,
        system_prompt: None,
        resume_session_id: Some("sess-abc".to_string()),
        memory_context: None,
        parent_pid_override: None,
        runsc_dirs: vec![],
        runsc_image: None,
        model_override: None,
        command: "chat".to_string(),
        chat_id: "test-456".to_string(),
    };
    let chat = worker_request_to_chat_request(&req).unwrap();
    let resume = match &chat.resume {
        ResumeMode::External(ext) => ext.id.clone(),
        _ => panic!("expected external resume"),
    };
    assert_eq!(resume, "sess-abc");
}

#[test]
fn worker_request_to_chat_request_minimax_ports_native_contract() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let original_home = std::env::var_os("HOME");
    let original_minimax_api_key = std::env::var_os("MINIMAX_API_KEY");
    let original_config_path = std::env::var_os(config::CONFIG_PATH_OVERRIDE_ENV);
    let temp_home = tempfile::tempdir().unwrap();
    let config_dir = temp_home.path().join(".rust-chatbot").join(".minimax");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join(".minimax.json"), "{}").unwrap();
    std::env::set_var("HOME", temp_home.path());
    std::env::remove_var(config::CONFIG_PATH_OVERRIDE_ENV);
    std::env::set_var("MINIMAX_API_KEY", "test-minimax-key");

    let req = WorkerRequest {
        provider: "minimax".to_string(),
        query: "hello".to_string(),
        working_dir: Some("/tmp".to_string()),
        system_prompt: None,
        resume_session_id: None,
        memory_context: None,
        parent_pid_override: None,
        runsc_dirs: vec![],
        runsc_image: None,
        model_override: None,
        command: "chat".to_string(),
        chat_id: "test-minimax".to_string(),
    };

    let chat = worker_request_to_chat_request(&req).unwrap();
    let expected_config_dir = config_dir.to_string_lossy().into_owned();

    assert_eq!(chat.model.as_deref(), Some("MiniMax-M2.7"));
    assert_eq!(
        chat.env.get("ANTHROPIC_BASE_URL").map(String::as_str),
        config::base_url_for(ModelProvider::MiniMax)
    );
    assert_eq!(
        chat.env.get("ANTHROPIC_AUTH_TOKEN").map(String::as_str),
        Some("test-minimax-key")
    );
    assert_eq!(
        chat.env.get("MINIMAX_API_KEY").map(String::as_str),
        Some("test-minimax-key")
    );
    assert_eq!(
        chat.env
            .get("ANTHROPIC_REASONING_EFFORT")
            .map(String::as_str),
        config::effective_reasoning_effort_for_provider(ModelProvider::MiniMax).as_deref()
    );
    assert_eq!(
        chat.env.get("CLAUDE_CONFIG_DIR").map(String::as_str),
        Some(expected_config_dir.as_str())
    );
    assert_eq!(
        chat.env
            .get("AGENT_ADAPTER_MINIMAX_EXE")
            .map(String::as_str),
        Some(resolve_minimax_chatbot_adapter().to_string_lossy().as_ref())
    );
    assert!(std::fs::symlink_metadata(config_dir.join(".claude.json")).is_ok());

    match original_minimax_api_key {
        Some(value) => std::env::set_var("MINIMAX_API_KEY", value),
        None => std::env::remove_var("MINIMAX_API_KEY"),
    }
    match original_config_path {
        Some(value) => std::env::set_var(config::CONFIG_PATH_OVERRIDE_ENV, value),
        None => std::env::remove_var(config::CONFIG_PATH_OVERRIDE_ENV),
    }
    match original_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
}

#[test]
fn worker_request_to_chat_request_minimax_requires_key() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let original_home = std::env::var_os("HOME");
    let original_minimax_api_key = std::env::var_os("MINIMAX_API_KEY");
    let original_config_path = std::env::var_os(config::CONFIG_PATH_OVERRIDE_ENV);
    let temp_home = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", temp_home.path());
    std::env::remove_var("MINIMAX_API_KEY");
    std::env::remove_var(config::CONFIG_PATH_OVERRIDE_ENV);

    let req = WorkerRequest {
        provider: "minimax".to_string(),
        query: "hello".to_string(),
        working_dir: Some("/tmp".to_string()),
        system_prompt: None,
        resume_session_id: None,
        memory_context: None,
        parent_pid_override: None,
        runsc_dirs: vec![],
        runsc_image: None,
        model_override: None,
        command: "chat".to_string(),
        chat_id: "test-minimax-missing-key".to_string(),
    };

    let err = worker_request_to_chat_request(&req).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "MINIMAX_API_KEY is not set. Export it or write the key to {} before using MiniMax.",
            config::minimax_api_key_path()
        )
    );

    match original_minimax_api_key {
        Some(value) => std::env::set_var("MINIMAX_API_KEY", value),
        None => std::env::remove_var("MINIMAX_API_KEY"),
    }
    match original_config_path {
        Some(value) => std::env::set_var(config::CONFIG_PATH_OVERRIDE_ENV, value),
        None => std::env::remove_var(config::CONFIG_PATH_OVERRIDE_ENV),
    }
    match original_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
}

#[test]
fn agent_event_text_delta_to_stream_event() {
    let agent = AgentEvent::TextDelta {
        role: agent_adapter_core::types::Role::Assistant,
        text: "hello world".to_string(),
    };
    let stream = agent_event_to_stream_event(&agent, None);
    assert!(stream.is_some());
    if let Some(StreamEvent::TextDelta { content, .. }) = stream {
        assert_eq!(content, "hello world");
    } else {
        panic!("expected TextDelta");
    }
}

#[test]
fn agent_event_error_to_stream_event() {
    let agent = AgentEvent::RunFailed {
        code: Some(1),
        message: "something went wrong".to_string(),
    };
    let stream = agent_event_to_stream_event(&agent, None);
    assert!(stream.is_some());
    if let Some(StreamEvent::Error { message, .. }) = stream {
        assert_eq!(message, "something went wrong");
    } else {
        panic!("expected Error");
    }
}

#[test]
fn agent_event_run_started_no_stream_event() {
    let agent = AgentEvent::RunStarted {
        run_id: uuid::Uuid::new_v4(),
        provider: ProviderId::Claude,
        external_session: None,
        approval_policy: DANGEROUS_DEFAULT_ACTIVE.to_string(),
        workspace: agent_adapter_core::types::WorkspaceRef::Direct {
            path: PathBuf::from("/tmp"),
        },
    };
    assert!(agent_event_to_stream_event(&agent, None).is_none());
}

#[tokio::test]
async fn run_adapter_returns_terminal_error_when_binary_is_missing() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let previous = std::env::var_os("RUST_CHATBOT_AGENT_ADAPTER_CLI");
    std::env::set_var(
        "RUST_CHATBOT_AGENT_ADAPTER_CLI",
        "/definitely/missing/agent-adapter-cli",
    );

    let request = WorkerRequest {
        provider: "claude".to_string(),
        query: "hello".to_string(),
        working_dir: Some("/tmp".to_string()),
        system_prompt: None,
        resume_session_id: None,
        memory_context: None,
        parent_pid_override: None,
        runsc_dirs: vec![],
        runsc_image: None,
        model_override: None,
        command: "chat".to_string(),
        chat_id: "test-missing-binary".to_string(),
    };

    let result = run_adapter(&request, Arc::new(AtomicBool::new(false)))
        .await
        .unwrap();

    match previous {
        Some(value) => std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", value),
        None => std::env::remove_var("RUST_CHATBOT_AGENT_ADAPTER_CLI"),
    }

    assert!(result.output.is_empty());
    assert!(result.session_id.is_none());
    assert!(result
        .error
        .as_deref()
        .is_some_and(|message| message.contains("failed to spawn")));
}

#[cfg(unix)]
#[tokio::test]
async fn run_adapter_observes_stop_signal_while_stdout_is_idle() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tempdir = tempfile::tempdir().unwrap();
    let adapter_path = tempdir.path().join("fake-adapter.sh");
    std::fs::write(&adapter_path, "#!/bin/sh\ntrap 'exit 0' TERM\nsleep 30\n").unwrap();
    let mut perms = std::fs::metadata(&adapter_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&adapter_path, perms).unwrap();

    let previous = std::env::var_os("RUST_CHATBOT_AGENT_ADAPTER_CLI");
    std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", &adapter_path);

    let request = WorkerRequest {
        provider: "claude".to_string(),
        query: "hello".to_string(),
        working_dir: Some("/tmp".to_string()),
        system_prompt: None,
        resume_session_id: None,
        memory_context: None,
        parent_pid_override: None,
        runsc_dirs: vec![],
        runsc_image: None,
        model_override: None,
        command: "chat".to_string(),
        chat_id: "test-stop-idle".to_string(),
    };
    let stop_signal = Arc::new(AtomicBool::new(false));
    let task_stop_signal = Arc::clone(&stop_signal);

    let handle = tokio::spawn(async move { run_adapter(&request, task_stop_signal).await });
    tokio::time::sleep(Duration::from_millis(150)).await;
    stop_signal.store(true, Ordering::Relaxed);

    let result = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("stop should not hang")
        .expect("adapter task should join")
        .unwrap();

    match previous {
        Some(value) => std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", value),
        None => std::env::remove_var("RUST_CHATBOT_AGENT_ADAPTER_CLI"),
    }

    assert_eq!(result.error.as_deref(), Some("Stopped by user"));
}

#[cfg(unix)]
#[tokio::test]
async fn run_adapter_escalates_to_sigkill_when_adapter_ignores_sigterm() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tempdir = tempfile::tempdir().unwrap();
    let adapter_path = tempdir.path().join("fake-adapter-ignore-term.sh");
    std::fs::write(
        &adapter_path,
        "#!/bin/sh\ntrap '' TERM\nwhile :; do sleep 30; done\n",
    )
    .unwrap();
    let mut perms = std::fs::metadata(&adapter_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&adapter_path, perms).unwrap();

    let previous = std::env::var_os("RUST_CHATBOT_AGENT_ADAPTER_CLI");
    std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", &adapter_path);

    let request = WorkerRequest {
        provider: "claude".to_string(),
        query: "hello".to_string(),
        working_dir: Some("/tmp".to_string()),
        system_prompt: None,
        resume_session_id: None,
        memory_context: None,
        parent_pid_override: None,
        runsc_dirs: vec![],
        runsc_image: None,
        model_override: None,
        command: "chat".to_string(),
        chat_id: "test-stop-ignore-term".to_string(),
    };
    let stop_signal = Arc::new(AtomicBool::new(false));
    let task_stop_signal = Arc::clone(&stop_signal);

    let handle = tokio::spawn(async move { run_adapter(&request, task_stop_signal).await });
    tokio::time::sleep(Duration::from_millis(150)).await;
    stop_signal.store(true, Ordering::Relaxed);

    let result = tokio::time::timeout(Duration::from_secs(6), handle)
        .await
        .expect("stop escalation should not hang")
        .expect("adapter task should join")
        .unwrap();

    match previous {
        Some(value) => std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", value),
        None => std::env::remove_var("RUST_CHATBOT_AGENT_ADAPTER_CLI"),
    }

    assert_eq!(result.error.as_deref(), Some("Stopped by user"));
}

#[cfg(unix)]
#[tokio::test]
async fn claude_compat_bridge_ends_acp_session_after_completion() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tempdir = tempfile::tempdir().unwrap();
    let fake_chatbot = tempdir.path().join("chatbot");
    let marker_path = tempdir.path().join("session-end.log");
    let bridge_script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("claude-compat-chatbot-cli-adapter.py");

    std::fs::write(
        &fake_chatbot,
        format!(
            r#"#!/usr/bin/env python3
import json
import os
import sys

if any(arg in ("--version", "--help") for arg in sys.argv[1:]):
    print("chatbot-fixture 0.0.0")
    sys.exit(0)

marker_path = {marker_path:?}

def emit(payload):
    sys.stdout.write(json.dumps(payload) + "\n")
    sys.stdout.flush()

for raw_line in sys.stdin:
    request = json.loads(raw_line)
    method = request.get("method")
    if method == "initialize":
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"protocolVersion": "2024-11-05"}}}})
    elif method == "session/create":
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"sessionId": "fake-session"}}}})
    elif method == "prompt":
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"message_id": "msg-1", "done": False}}}})
        emit({{"jsonrpc": "2.0", "method": "text_delta", "params": {{"text": "hello "}}}})
        emit({{"jsonrpc": "2.0", "method": "text_delta", "params": {{"text": "world"}}}})
        emit({{"jsonrpc": "2.0", "method": "completion", "params": {{"finishReason": "stop", "done": True, "summary": "hello world"}}}})
    elif method == "session/end":
        with open(marker_path, "a", encoding="utf-8") as handle:
            handle.write("session_end\n")
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"sessionId": "fake-session", "ended": True}}}})
        sys.exit(0)

with open(marker_path, "a", encoding="utf-8") as handle:
    handle.write("eof\n")
sys.exit(3)
"#,
            marker_path = marker_path.display().to_string()
        ),
    )
    .unwrap();
    let mut perms = std::fs::metadata(&fake_chatbot).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fake_chatbot, perms).unwrap();

    let child = Command::new("python3")
        .arg(&bridge_script)
        .arg("-p")
        .arg("hello world")
        .arg("--model")
        .arg("MiniMax-M2.7")
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
    .expect("bridge should not hang after completion")
    .expect("bridge wait task should join")
    .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        output.status.success(),
        "bridge should exit cleanly\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("\"type\": \"session_meta\""));
    assert!(stdout.contains("\"session_id\": \"fake-session\""));
    assert!(stdout.contains("\"text\": \"hello \""));
    assert!(stdout.contains("\"text\": \"world\""));
    assert_eq!(
        std::fs::read_to_string(&marker_path).unwrap(),
        "session_end\n"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn claude_compat_bridge_ends_acp_session_after_error_completion() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tempdir = tempfile::tempdir().unwrap();
    let fake_chatbot = tempdir.path().join("chatbot");
    let marker_path = tempdir.path().join("session-end.log");
    let bridge_script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join("claude-compat-chatbot-cli-adapter.py");

    std::fs::write(
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

for raw_line in sys.stdin:
    request = json.loads(raw_line)
    method = request.get("method")
    if method == "initialize":
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"protocolVersion": "2024-11-05"}}}})
    elif method == "session/create":
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"sessionId": "fake-session"}}}})
    elif method == "prompt":
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"message_id": "msg-1", "done": False}}}})
        emit({{"jsonrpc": "2.0", "method": "completion", "params": {{"finishReason": "error", "done": True, "error": {{"type": "provider_error", "message": "boom"}}}}}})
    elif method == "session/end":
        with open(marker_path, "a", encoding="utf-8") as handle:
            handle.write("session_end\n")
        emit({{"jsonrpc": "2.0", "id": request["id"], "result": {{"sessionId": "fake-session", "ended": True}}}})
        sys.exit(0)

with open(marker_path, "a", encoding="utf-8") as handle:
    handle.write("eof\n")
sys.exit(4)
"#,
            marker_path = marker_path.display().to_string()
        ),
    )
    .unwrap();
    let mut perms = std::fs::metadata(&fake_chatbot).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fake_chatbot, perms).unwrap();

    let child = Command::new("python3")
        .arg(&bridge_script)
        .arg("-p")
        .arg("hello world")
        .arg("--model")
        .arg("MiniMax-M2.7")
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
    .expect("bridge should not hang after error completion")
    .expect("bridge wait task should join")
    .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !output.status.success(),
        "bridge should surface error completion as failure\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("\"type\": \"session_meta\""));
    assert!(stdout.contains("\"session_id\": \"fake-session\""));
    assert!(stdout.contains("\"text\": \"Error: boom\""));
    assert_eq!(
        std::fs::read_to_string(&marker_path).unwrap(),
        "session_end\n"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn run_adapter_drains_large_stderr_without_hanging() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tempdir = tempfile::tempdir().unwrap();
    let adapter_path = tempdir.path().join("fake-adapter-stderr-flood.sh");
    std::fs::write(
        &adapter_path,
        "#!/bin/sh\ni=0\nwhile [ \"$i\" -lt 20000 ]; do\n  echo flood-line >&2\n  i=$((i + 1))\ndone\nexit 7\n",
    )
    .unwrap();
    let mut perms = std::fs::metadata(&adapter_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&adapter_path, perms).unwrap();

    let previous = std::env::var_os("RUST_CHATBOT_AGENT_ADAPTER_CLI");
    std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", &adapter_path);

    let request = WorkerRequest {
        provider: "claude".to_string(),
        query: "hello".to_string(),
        working_dir: Some("/tmp".to_string()),
        system_prompt: None,
        resume_session_id: None,
        memory_context: None,
        parent_pid_override: None,
        runsc_dirs: vec![],
        runsc_image: None,
        model_override: None,
        command: "chat".to_string(),
        chat_id: "test-stderr-flood".to_string(),
    };

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        run_adapter(&request, Arc::new(AtomicBool::new(false))),
    )
    .await
    .expect("adapter stderr flood should not hang")
    .unwrap();

    match previous {
        Some(value) => std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", value),
        None => std::env::remove_var("RUST_CHATBOT_AGENT_ADAPTER_CLI"),
    }

    assert_eq!(result.session_id, None);
    assert!(result.output.is_empty());
    assert!(result
        .error
        .as_deref()
        .is_some_and(|message| message.contains("flood-line")));
}

#[cfg(unix)]
#[tokio::test]
async fn run_adapter_prefers_raw_provider_result_errors_over_generic_run_failed() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tempdir = tempfile::tempdir().unwrap();
    let adapter_path = tempdir.path().join("fake-adapter-result-error.py");
    std::fs::write(
        &adapter_path,
        r#"#!/usr/bin/env python3
import json
import sys
import uuid

events = [
    {
        "type": "run_started",
        "run_id": str(uuid.uuid4()),
        "provider": "minimax",
        "external_session": None,
        "approval_policy": "DANGEROUS_DEFAULT_ACTIVE",
        "workspace": {"kind": "direct", "path": "/tmp"},
    },
    {
        "type": "raw_provider_event",
        "provider": "minimax",
        "payload": {
            "type": "result",
            "is_error": True,
            "errors": ["No conversation found with session ID: 799b06e2-0cd1-456d-af38-425093b97a68"],
        },
    },
    {"type": "run_failed", "code": 1, "message": "claude exited with code Some(1)"},
]
for event in events:
    sys.stdout.write(json.dumps(event) + "\n")
    sys.stdout.flush()
sys.stderr.write("Error: Run command failed\n")
sys.stderr.flush()
sys.exit(1)
"#,
    )
    .unwrap();
    let mut perms = std::fs::metadata(&adapter_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&adapter_path, perms).unwrap();

    let previous = std::env::var_os("RUST_CHATBOT_AGENT_ADAPTER_CLI");
    std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", &adapter_path);
    let previous_key = std::env::var_os("MINIMAX_API_KEY");
    std::env::set_var("MINIMAX_API_KEY", "synthetic-test-key");

    let request = WorkerRequest {
        provider: "minimax".to_string(),
        query: "hello".to_string(),
        working_dir: Some("/tmp".to_string()),
        system_prompt: None,
        resume_session_id: None,
        memory_context: None,
        parent_pid_override: None,
        runsc_dirs: vec![],
        runsc_image: None,
        model_override: None,
        command: "chat".to_string(),
        chat_id: "test-result-error-preference".to_string(),
    };

    let result = run_adapter(&request, Arc::new(AtomicBool::new(false)))
        .await
        .unwrap();

    match previous {
        Some(value) => std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", value),
        None => std::env::remove_var("RUST_CHATBOT_AGENT_ADAPTER_CLI"),
    }

    match previous_key {
        Some(value) => std::env::set_var("MINIMAX_API_KEY", value),
        None => std::env::remove_var("MINIMAX_API_KEY"),
    }

    assert!(result.output.is_empty());
    assert_eq!(
        result.error.as_deref(),
        Some(
            "exit_code:1: No conversation found with session ID: 799b06e2-0cd1-456d-af38-425093b97a68"
        )
    );
}

#[test]
fn terminal_adapter_error_message_prefers_output_text() {
    assert_eq!(
        terminal_adapter_error_message(
            Some(1),
            "Error: Your organization does not have access to Claude.",
            Some("claude exited with code Some(1)"),
        ),
        "exit_code:1: Your organization does not have access to Claude."
    );
}
