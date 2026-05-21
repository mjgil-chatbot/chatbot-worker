use super::*;
use agent_adapter_core::DANGEROUS_DEFAULT_ACTIVE;
use std::path::PathBuf;

#[cfg(unix)]
fn success_events_json() -> String {
    serde_json::to_string(&vec![
        serde_json::to_string(&AgentEvent::RunStarted {
            run_id: uuid::Uuid::new_v4(),
            provider: ProviderId::Claude,
            external_session: Some(ExternalSessionRef {
                id: "retry-session".to_string(),
                provider: ProviderId::Claude,
            }),
            approval_policy: DANGEROUS_DEFAULT_ACTIVE.to_string(),
            workspace: agent_adapter_core::types::WorkspaceRef::Direct {
                path: PathBuf::from("/tmp"),
            },
        })
        .unwrap(),
        serde_json::to_string(&AgentEvent::TextDelta {
            role: agent_adapter_core::types::Role::Assistant,
            text: "retry success".to_string(),
        })
        .unwrap(),
        serde_json::to_string(&AgentEvent::RunCompleted {
            final_text: Some("retry success".to_string()),
            external_session: Some(ExternalSessionRef {
                id: "retry-session".to_string(),
                provider: ProviderId::Claude,
            }),
        })
        .unwrap(),
    ])
    .unwrap()
}

#[cfg(unix)]
fn write_retry_adapter_script(
    adapter_path: &std::path::Path,
    attempts_path: &std::path::Path,
    failure_script: &str,
) {
    std::fs::write(
        adapter_path,
        format!(
            r#"#!/usr/bin/env python3
import json
import pathlib
import sys

attempts_path = pathlib.Path({attempts_path:?})
attempt = int(attempts_path.read_text() or "0") if attempts_path.exists() else 0
attempt += 1
attempts_path.write_text(str(attempt))

{failure_script}

for line in json.loads({events_json:?}):
    sys.stdout.write(line + "\n")
    sys.stdout.flush()
sys.exit(0)
"#,
            events_json = success_events_json(),
        ),
    )
    .unwrap();

    use std::os::unix::fs::PermissionsExt;

    let mut perms = std::fs::metadata(adapter_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(adapter_path, perms).unwrap();
}

#[cfg(unix)]
fn test_request(chat_id: &str) -> WorkerRequest {
    WorkerRequest {
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
        chat_id: chat_id.to_string(),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn run_adapter_retries_transport_errors_until_success() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tempdir = tempfile::tempdir().unwrap();
    let adapter_path = tempdir.path().join("fake-adapter-transport-retry.py");
    let attempts_path = tempdir.path().join("attempts.txt");
    write_retry_adapter_script(
        &adapter_path,
        &attempts_path,
        r#"if attempt <= 2:
    sys.stderr.write("provider: Transport error: HTTP error: 400 Bad Request\n")
    sys.stderr.flush()
    sys.exit(1)"#,
    );

    let previous_adapter = std::env::var_os("RUST_CHATBOT_AGENT_ADAPTER_CLI");
    let previous_home = std::env::var_os("HOME");
    std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", &adapter_path);
    std::env::set_var("HOME", tempdir.path());

    let request = test_request("test-transport-retry");

    let result = run_adapter(&request, Arc::new(AtomicBool::new(false)))
        .await
        .unwrap();
    let transport_log =
        std::fs::read_to_string(crate::worker::transport_errors_path(&request.chat_id)).unwrap();

    match previous_adapter {
        Some(value) => std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", value),
        None => std::env::remove_var("RUST_CHATBOT_AGENT_ADAPTER_CLI"),
    }
    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }

    assert_eq!(std::fs::read_to_string(&attempts_path).unwrap(), "3");
    assert_eq!(result.output, "retry success");
    assert_eq!(result.session_id.as_deref(), Some("retry-session"));
    assert_eq!(result.error, None);
    assert!(
        transport_log.contains("attempt 1"),
        "transport log missing first retry marker: {transport_log}"
    );
    assert!(
        transport_log.contains("attempt 2"),
        "transport log missing second retry marker: {transport_log}"
    );
    assert!(
        transport_log.contains("retrying"),
        "transport log missing retry action: {transport_log}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn run_adapter_retries_exit_code_8_until_success() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tempdir = tempfile::tempdir().unwrap();
    let adapter_path = tempdir.path().join("fake-adapter-exit-8-retry.py");
    let attempts_path = tempdir.path().join("attempts.txt");
    write_retry_adapter_script(
        &adapter_path,
        &attempts_path,
        r#"if attempt == 1:
    sys.stderr.write("Error: Run command failed\n")
    sys.stderr.flush()
    sys.exit(8)"#,
    );

    let previous_adapter = std::env::var_os("RUST_CHATBOT_AGENT_ADAPTER_CLI");
    let previous_home = std::env::var_os("HOME");
    std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", &adapter_path);
    std::env::set_var("HOME", tempdir.path());

    let request = test_request("test-exit-8-retry");

    let result = run_adapter(&request, Arc::new(AtomicBool::new(false)))
        .await
        .unwrap();
    let transport_log =
        std::fs::read_to_string(crate::worker::transport_errors_path(&request.chat_id)).unwrap();

    match previous_adapter {
        Some(value) => std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", value),
        None => std::env::remove_var("RUST_CHATBOT_AGENT_ADAPTER_CLI"),
    }
    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }

    assert_eq!(std::fs::read_to_string(&attempts_path).unwrap(), "2");
    assert_eq!(result.output, "retry success");
    assert_eq!(result.session_id.as_deref(), Some("retry-session"));
    assert_eq!(result.error, None);
    assert!(transport_log.contains("exit_code:8: Run command failed"));
}

#[cfg(unix)]
#[tokio::test]
async fn run_adapter_retries_raw_provider_disconnect_error_before_text_output() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tempdir = tempfile::tempdir().unwrap();
    let adapter_path = tempdir.path().join("fake-adapter-raw-provider-retry.py");
    let attempts_path = tempdir.path().join("attempts.txt");
    let success_events_json = serde_json::to_string(&vec![
        serde_json::to_string(&AgentEvent::RunStarted {
            run_id: uuid::Uuid::new_v4(),
            provider: ProviderId::Codex,
            external_session: Some(ExternalSessionRef {
                id: "retry-session".to_string(),
                provider: ProviderId::Codex,
            }),
            approval_policy: DANGEROUS_DEFAULT_ACTIVE.to_string(),
            workspace: agent_adapter_core::types::WorkspaceRef::Direct {
                path: PathBuf::from("/tmp"),
            },
        })
        .unwrap(),
        serde_json::to_string(&AgentEvent::TextDelta {
            role: agent_adapter_core::types::Role::Assistant,
            text: "test test test".to_string(),
        })
        .unwrap(),
        serde_json::to_string(&AgentEvent::RunCompleted {
            final_text: Some("test test test".to_string()),
            external_session: Some(ExternalSessionRef {
                id: "retry-session".to_string(),
                provider: ProviderId::Codex,
            }),
        })
        .unwrap(),
    ])
    .unwrap();
    let disconnect_event_json = serde_json::to_string(&AgentEvent::RawProviderEvent {
        provider: "codex".to_string(),
        payload: serde_json::json!({
            "type": "error",
            "message": "stream disconnected before completion: websocket closed by server before response.completed"
        }),
    })
    .unwrap();
    std::fs::write(
        &adapter_path,
        format!(
            r#"#!/usr/bin/env python3
import json
import pathlib
import sys

attempts_path = pathlib.Path({attempts_path:?})
attempt = int(attempts_path.read_text() or "0") if attempts_path.exists() else 0
attempt += 1
attempts_path.write_text(str(attempt))

if attempt == 1:
    sys.stdout.write({disconnect_event_json:?} + "\n")
    sys.stdout.flush()
    sys.exit(1)

for line in json.loads({success_events_json:?}):
    sys.stdout.write(line + "\n")
    sys.stdout.flush()
sys.exit(0)
"#,
        ),
    )
    .unwrap();

    use std::os::unix::fs::PermissionsExt;

    let mut perms = std::fs::metadata(&adapter_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&adapter_path, perms).unwrap();

    let previous_adapter = std::env::var_os("RUST_CHATBOT_AGENT_ADAPTER_CLI");
    let previous_home = std::env::var_os("HOME");
    std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", &adapter_path);
    std::env::set_var("HOME", tempdir.path());

    let request = WorkerRequest {
        provider: "codex".to_string(),
        query: "repeat the word test three times and nothing else".to_string(),
        working_dir: Some("/tmp".to_string()),
        system_prompt: None,
        resume_session_id: None,
        memory_context: None,
        parent_pid_override: None,
        runsc_dirs: vec![],
        runsc_image: None,
        model_override: None,
        command: "chat".to_string(),
        chat_id: "test-raw-provider-disconnect-retry".to_string(),
    };

    let result = run_adapter(&request, Arc::new(AtomicBool::new(false)))
        .await
        .unwrap();
    let transport_log =
        std::fs::read_to_string(crate::worker::transport_errors_path(&request.chat_id)).unwrap();

    match previous_adapter {
        Some(value) => std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", value),
        None => std::env::remove_var("RUST_CHATBOT_AGENT_ADAPTER_CLI"),
    }
    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }

    assert_eq!(std::fs::read_to_string(&attempts_path).unwrap(), "2");
    assert_eq!(result.output, "test test test");
    assert_eq!(result.session_id.as_deref(), Some("retry-session"));
    assert_eq!(result.error, None);
    assert!(transport_log.contains("transport error attempt 1 failed"));
    assert!(transport_log.contains(
        "stream disconnected before completion: websocket closed by server before response.completed"
    ));
}
