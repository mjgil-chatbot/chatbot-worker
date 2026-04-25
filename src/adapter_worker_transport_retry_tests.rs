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
