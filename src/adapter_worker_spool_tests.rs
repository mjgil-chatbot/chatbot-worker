use super::*;
use crate::chatbot_worker_adapter::finalize_adapter_result;
use crate::stream::read_events;
use crate::worker::{self, WorkerJobSnapshot, WorkerState};
use serde_json::Value;
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

fn restore_env_var(key: &str, previous: Option<OsString>) {
    match previous {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn adapter_spool_bounds_large_tool_payloads_and_persists_terminal_error_state() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tempdir = tempfile::tempdir().unwrap();
    let adapter_path = tempdir.path().join("fake-adapter-tool-overflow.py");
    let previous_home = std::env::var_os("HOME");
    let previous_config = std::env::var_os("RUST_CHATBOT_CONFIG_PATH");
    let previous_adapter = std::env::var_os("RUST_CHATBOT_AGENT_ADAPTER_CLI");

    std::env::set_var("HOME", tempdir.path());
    std::env::remove_var("RUST_CHATBOT_CONFIG_PATH");
    std::fs::write(
        &adapter_path,
        r#"#!/usr/bin/env python3
import json
import sys

large_output = "x" * 200000
events = [
    {
        "type": "tool_call_started",
        "id": "call-listdir-1",
        "name": "ListDir",
        "input": {"path": "/workspace/home", "recursive": True},
    },
    {
        "type": "tool_call_finished",
        "id": "call-listdir-1",
        "output": large_output,
        "error": None,
    },
    {
        "type": "run_failed",
        "code": 7,
        "message": "tool replay overflow",
    },
]

for event in events:
    sys.stdout.write(json.dumps(event) + "\n")
    sys.stdout.flush()

sys.exit(7)
"#,
    )
    .unwrap();
    let mut perms = std::fs::metadata(&adapter_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&adapter_path, perms).unwrap();
    std::env::set_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", &adapter_path);

    let chat_id = "adapter-spool-large-tool-error";
    let request = WorkerRequest {
        provider: "codex".to_string(),
        query: "show the directory".to_string(),
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
    };

    let adapter_result = tokio::time::timeout(
        Duration::from_secs(5),
        run_adapter(&request, Arc::new(AtomicBool::new(false))),
    )
    .await
    .expect("adapter run should finish")
    .unwrap();
    let adapter_error = adapter_result.error.clone();

    let mut job = WorkerJobSnapshot::new(chat_id, "codex", 123, 456);
    finalize_adapter_result(chat_id, &mut job, adapter_result).unwrap();

    assert_eq!(
        adapter_error.as_deref(),
        Some("exit_code:7: tool replay overflow")
    );

    let events_path = worker::events_path(chat_id);
    let events_bytes = std::fs::metadata(&events_path).unwrap().len();
    assert!(
        events_bytes < 128 * 1024,
        "bounded spool should stay compact, got {events_bytes} bytes"
    );

    let events = read_events(&events_path);
    let tool_end_output = events
        .iter()
        .find_map(|event| match event {
            StreamEvent::ToolEnd { output, .. } => Some(output),
            _ => None,
        })
        .expect("tool end event should be persisted");

    assert_eq!(tool_end_output["truncated"], Value::Bool(true));
    assert!(tool_end_output["original_bytes"].as_u64().unwrap() > 32 * 1024);
    assert!(tool_end_output["preview"]
        .as_str()
        .unwrap()
        .contains("\"output\""));

    let tool_start_live_update = events
        .iter()
        .find_map(|event| match event {
            StreamEvent::ToolStart { live_update, .. } => live_update.as_ref(),
            _ => None,
        })
        .expect("tool start should persist canonical live update schema");
    assert_eq!(tool_start_live_update.schema_version, 1);
    assert_eq!(tool_start_live_update.label, "List Dir");

    let persisted = worker::read_job_state(chat_id).unwrap().unwrap();
    let result_json = std::fs::read_to_string(worker::result_path(chat_id)).unwrap();

    restore_env_var("RUST_CHATBOT_AGENT_ADAPTER_CLI", previous_adapter);
    restore_env_var("RUST_CHATBOT_CONFIG_PATH", previous_config);
    restore_env_var("HOME", previous_home);

    assert_eq!(persisted.state, WorkerState::Error);
    assert!(result_json.contains("tool replay overflow"));
}
