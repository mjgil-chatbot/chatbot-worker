//! Adapter-based worker that routes CLI requests through agent-adapter-cli.
//!
//! This module replaces direct CLI spawning (`call_provider_cli_stream`) with
//! the `agent-adapter-cli` unified adapter layer, providing:
//! - Protocol-first design: ACP > JSONL > PTY fallback hierarchy
//! - Secret redaction: Automatic redaction of API keys/tokens from logs
//! - Process group cleanup: Proper child process termination on cancel/timeout
//! - Session resumption: For providers that support it

use std::collections::HashMap;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use agent_adapter_core::{
    AgentEvent, ApprovalPolicy, ChatRequest, ExternalSessionRef, ProviderId, ResumeMode,
};
use ai_providers::claude::cli::effective_claude_system_prompt;
use ai_providers::session::ProviderSession;
use anyhow::Result;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::time::{sleep, Duration};
use crate::model::ModelProvider;
use crate::stream::StreamEvent;
fn raw_provider_event_error_message(payload: &serde_json::Value) -> Option<String> {
    let event_type = payload.get("type").and_then(|value| value.as_str());
    if event_type == Some("error") {
        let message = payload.get("message")?.as_str()?.trim();
        if !message.is_empty() {
            return Some(message.to_string());
        }
    }

    if payload.get("is_error").and_then(|value| value.as_bool()) != Some(true) {
        return None;
    }

    let errors = payload
        .get("errors")
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !errors.is_empty() {
        return Some(errors.join("; "));
    }

    let message = payload.get("message")?.as_str()?.trim();
    if message.is_empty() {
        return None;
    }

    Some(message.to_string())
}
pub(crate) async fn wait_for_stop_signal(stop_signal: &AtomicBool) {
    while !stop_signal.load(Ordering::Relaxed) {
        sleep(Duration::from_millis(50)).await;
    }
}
fn ensure_minimax_adapter_layout(path: &Path) {
    let _ = std::fs::create_dir_all(path);

    let claude_json = path.join(".claude.json");
    if claude_json.exists() {
        return;
    }

    #[cfg(unix)]
    {
        let _ = std::os::unix::fs::symlink(".minimax.json", &claude_json);
    }

    #[cfg(not(unix))]
    {
        let minimax_json = path.join(".minimax.json");
        if minimax_json.exists() {
            let _ = std::fs::copy(&minimax_json, &claude_json);
        }
    }
}
/// Get provider string for CLI argument.
fn provider_string(provider: &ProviderId) -> String {
    match provider {
        ProviderId::Claude => "claude".to_string(),
        ProviderId::MiniMax => "minimax".to_string(),
        ProviderId::Codex => "codex".to_string(),
        ProviderId::Gemini => "gemini".to_string(),
        ProviderId::Kimi => "kimi".to_string(),
        ProviderId::OpenCode => "opencode".to_string(),
        ProviderId::Hermes => "hermes".to_string(),
        ProviderId::Forge => "forge".to_string(),
        ProviderId::Claw => "claw".to_string(),
        ProviderId::Custom(name) => name.clone(),
    }
}
fn provider_supports_sandbox_dirs(provider: &ProviderId) -> bool {
    !matches!(provider, ProviderId::Claude | ProviderId::MiniMax)
}
fn provider_uses_shared_tool_call_live_update_schema(provider: Option<ModelProvider>) -> bool {
    !matches!(provider, Some(ModelProvider::Forge | ModelProvider::Hermes))
}
/// Extract session ID from ResumeMode for --session-id flag.
fn external_session_id(resume: &ResumeMode) -> Option<String> {
    match resume {
        ResumeMode::External(ext) => Some(ext.id.clone()),
        _ => None,
    }
}
/// Map AgentEvent to StreamEvent for spool persistence.
///
/// Not all AgentEvent variants have a corresponding StreamEvent — some are
/// used only internally (RunStarted, internal tool events) and are dropped.
fn agent_event_to_stream_event(
    event: &AgentEvent,
    live_update: Option<crate::adapter_tool_call_schema::ToolCallLiveUpdate>,
) -> Option<StreamEvent> {
    match event {
        AgentEvent::TextDelta { text, role: _, .. } => Some(StreamEvent::TextDelta {
            content: text.clone(),
            timestamp_ms: timestamp_now(),
        }),
        AgentEvent::ToolCallStarted { name, input, .. } => Some(StreamEvent::ToolStart {
            tool: name.clone(),
            input: input.clone(),
            live_update,
            timestamp_ms: timestamp_now(),
        }),
        AgentEvent::ToolCallDelta { id, delta } => {
            // ToolCallDelta is a streaming intermediate event — emit as tool update
            Some(StreamEvent::ToolStart {
                tool: format!("{}.delta", id),
                input: delta.clone(),
                live_update: None,
                timestamp_ms: timestamp_now(),
            })
        }
        AgentEvent::ToolCallFinished {
            id, output, error, ..
        } => Some(StreamEvent::ToolEnd {
            tool: id.clone(),
            output: output
                .as_ref()
                .map(|s| serde_json::json!({ "output": s }))
                .unwrap_or_else(|| {
                    error
                        .clone()
                        .map(|e| serde_json::json!({ "error": e }))
                        .unwrap_or(serde_json::Value::Null)
                }),
            live_update,
            timestamp_ms: timestamp_now(),
        }),
        AgentEvent::Usage {
            input_tokens,
            output_tokens,
            cost_usd,
            ..
        } => {
            let mut val = serde_json::Map::new();
            if let Some(it) = input_tokens {
                val.insert("input_tokens".to_string(), serde_json::json!(it));
            }
            if let Some(ot) = output_tokens {
                val.insert("output_tokens".to_string(), serde_json::json!(ot));
            }
            if let Some(c) = cost_usd {
                val.insert("cost_usd".to_string(), serde_json::json!(c));
            }
            Some(StreamEvent::CostUpdate {
                input_tokens: input_tokens.unwrap_or(0),
                output_tokens: output_tokens.unwrap_or(0),
                timestamp_ms: timestamp_now(),
            })
        }
        AgentEvent::Log { level, message } => {
            // Log events at Error level become StreamEvent::Error
            if matches!(level, agent_adapter_core::types::LogLevel::Error) {
                Some(StreamEvent::Error {
                    message: message.clone(),
                    timestamp_ms: timestamp_now(),
                })
            } else {
                None // Info/Warn logs are emitted to stderr, not spool
            }
        }
        AgentEvent::RunFailed { message, .. } => Some(StreamEvent::Error {
            message: message.clone(),
            timestamp_ms: timestamp_now(),
        }),
        // Events that don't map to StreamEvent: RunStarted, PlanUpdate,
        // PermissionRequest, FileChange, RawProviderEvent, RunCompleted, RunCancelled
        _ => None,
    }
}
fn timestamp_now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
#[cfg(test)]
#[path = "adapter_worker_resume_tests.rs"]
mod resume_tests;
#[cfg(test)]
#[path = "adapter_worker_spool_tests.rs"]
mod spool_tests;
#[cfg(test)]
#[path = "adapter_worker_transport_retry_tests.rs"]
mod transport_retry_tests;
