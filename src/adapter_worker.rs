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
use crate::adapter_git_verifier::{
    persist_failed_verification_artifacts, GitSideEffectGuard, RawJsonlCapture,
};
use crate::adapter_live_summary::LiveSummaryContext;
use crate::adapter_tool_call_schema::ToolCallLiveUpdateTracker;
use crate::adapter_transport::{
    append_transport_error_log, configure_subprocess_group, drain_stderr, finish_stderr_task,
    preferred_terminal_adapter_error_detail, should_retry_transport_error, signal_process_group,
    terminal_adapter_error_message, transport_retry_notice, wait_for_child_exit,
    wait_for_retry_backoff,
};
use crate::chatbot_worker_runtime::runsc_workspace_includes_for_request;
use crate::companion_binary::{resolve_agent_adapter_cli, resolve_minimax_chatbot_adapter};
use crate::config;
use crate::memory::build_query_with_memory;
use crate::model::ModelProvider;
use crate::provider_bridge::{
    claude_compat_config, effective_provider_system_prompt, minimax_api_key,
};
use crate::stream::{append_event, events_path, StreamEvent};
use crate::worker::WorkerRequest;
/// Result from running an adapter-backed provider CLI.
pub struct AdapterResult {
    /// Final accumulated text output.
    pub output: String,
    /// Session ID for resumption (provider-specific).
    pub session_id: Option<String>,
    /// Provider-specific summary data.
    pub summary: Option<crate::summary::JsonSummary>,
    /// Error message if the run failed.
    pub error: Option<String>,
}
/// Run agent-adapter-cli with the given request, streaming events to the spool.
///
/// Returns when the process completes (success or failure). On success, returns
/// the final text output and any session ID. On failure, returns the error.
///
/// The caller is responsible for writing output.txt and result.json after this
/// returns.
pub async fn run_adapter(
    request: &WorkerRequest,
    stop_signal: Arc<AtomicBool>,
) -> Result<AdapterResult> {
    let model_provider = ModelProvider::from_alias(&request.provider);
    let chat_request = match worker_request_to_chat_request(request) {
        Ok(chat_request) => chat_request,
        Err(err) => return Ok(adapter_failure(String::new(), None, err)),
    };
    let summary_context = LiveSummaryContext::new(model_provider, chat_request.model.clone());
    let events = events_path(&request.worker_spool_dir());
    let mut attempt = 0usize;

    loop {
        attempt += 1;
        let result = run_adapter_attempt(
            request,
            Arc::clone(&stop_signal),
            model_provider,
            &chat_request,
            &summary_context,
            &events,
        )
        .await?;

        let Some(error) = result.error.as_deref() else {
            return Ok(result);
        };
        if !should_retry_transport_error(&result.output, error) {
            return Ok(result);
        }

        log_transport_retry(
            request,
            &summary_context,
            &events,
            &transport_retry_notice(attempt, error),
        );
        if wait_for_retry_backoff(stop_signal.as_ref()).await {
            return Ok(AdapterResult {
                output: result.output,
                session_id: result.session_id,
                summary: None,
                error: Some("Stopped by user".to_string()),
            });
        }
    }
}
async fn run_adapter_attempt(
    request: &WorkerRequest,
    stop_signal: Arc<AtomicBool>,
    model_provider: Option<ModelProvider>,
    chat_request: &ChatRequest,
    summary_context: &LiveSummaryContext,
    events: &Path,
) -> Result<AdapterResult> {
    let uses_shared_tool_call_schema =
        provider_uses_shared_tool_call_live_update_schema(model_provider);
    let system_prompt = adapter_system_prompt(request);
    let adapter_program = resolve_agent_adapter_cli();
    let git_side_effect_guard = GitSideEffectGuard::prepare(&request.query, &chat_request.cwd)?;

    let mut cmd = TokioCommand::new(&adapter_program);
    cmd.arg("run");
    cmd.arg("--provider")
        .arg(provider_string(&chat_request.provider));
    cmd.arg("--prompt").arg(&chat_request.prompt);
    cmd.arg("--cwd")
        .arg(chat_request.cwd.to_string_lossy().as_ref());

    if let Some(timeout_ms) = chat_request.timeout_ms {
        cmd.arg("--timeout").arg(timeout_ms.to_string());
    }
    if let Some(model) = &chat_request.model {
        cmd.arg("--model").arg(model);
    }
    if let Some(system_prompt) = system_prompt.as_deref() {
        cmd.arg("--system-prompt").arg(system_prompt);
    }
    if let Some(session_id) = external_session_id(&chat_request.resume) {
        cmd.arg("--session-id").arg(session_id);
        cmd.arg("--resume");
    }

    // Safe mode vs dangerous default
    if chat_request.approval_policy.is_safe() {
        cmd.arg("--safe");
    }

    // Pass allowed env vars (whitelist approach for API keys)
    for (key, value) in &chat_request.env {
        cmd.env(key, value);
    }

    // Pass sandbox directories (runsc workspace includes)
    let sandbox_dirs = runsc_workspace_includes_for_request(request);
    if provider_supports_sandbox_dirs(&chat_request.provider) && !sandbox_dirs.is_empty() {
        cmd.arg("--sandbox-dirs").arg(sandbox_dirs.join(","));
    }

    // JSON output mode
    cmd.arg("--json");

    // Read stdout as JSONL and drain stderr concurrently so adapter/provider
    // diagnostics cannot fill the pipe and leave the worker stuck in Running.
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    configure_subprocess_group(&mut cmd);

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            return Ok(adapter_failure(
                String::new(),
                None,
                anyhow::Error::new(err)
                    .context(format!("failed to spawn {}", adapter_program.display())),
            ));
        }
    };

    let stdout = child.stdout.take().expect("stdout captured");
    let stderr = child.stderr.take().expect("stderr captured");
    let mut reader = BufReader::new(stdout).lines();
    let stderr_task = tokio::spawn(async move { drain_stderr(stderr).await });

    let mut output = String::new();
    let mut session_id = None;
    let mut error_message = None;
    let mut stop_requested = false;
    let mut completed_result = None;
    let mut live_update_tracker = ToolCallLiveUpdateTracker::default();
    let mut raw_adapter_events = RawJsonlCapture::default();
    let mut raw_provider_events = RawJsonlCapture::default();

    // Process output line by line
    loop {
        let line = tokio::select! {
            line = reader.next_line() => line,
            _ = wait_for_stop_signal(stop_signal.as_ref()) => {
                stop_requested = true;
                let _ = signal_process_group(child.id(), libc::SIGTERM);
                break;
            }
        };

        let Some(line) = (match line {
            Ok(line) => line,
            Err(err) => {
                return Ok(adapter_failure(
                    output,
                    session_id,
                    anyhow::Error::new(err).context("read stdout line"),
                ));
            }
        }) else {
            break;
        };
        raw_adapter_events.push_line(line.clone());

        // Parse as AgentEvent
        let event: AgentEvent = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => {
                // Not JSON — might be a progress indicator or warning; emit as log
                eprintln!("[agent-adapter] {}", line);
                continue;
            }
        };

        // Map to StreamEvent and persist
        let live_update = uses_shared_tool_call_schema
            .then(|| live_update_tracker.track(&event))
            .flatten();

        if let Some(summary_line) =
            summary_context.line_for_agent_event(&event, live_update.as_ref())
        {
            if let Err(err) = append_event(&events, &StreamEvent::live_summary(summary_line)) {
                eprintln!("Failed to append live summary event: {}", err);
            }
        }

        if let Some(stream_event) = agent_event_to_stream_event(&event, live_update) {
            if let Err(err) = append_event(&events, &stream_event) {
                eprintln!("Failed to append event: {}", err);
            }
        }

        if let AgentEvent::RawProviderEvent { payload, .. } = &event {
            if let Ok(serialized) = serde_json::to_string(payload) {
                raw_provider_events.push_line(serialized);
            }
            error_message = preferred_terminal_adapter_error_detail(
                error_message.as_deref(),
                raw_provider_event_error_message(payload).as_deref(),
            );
        }

        // Track accumulated output for TextDelta
        if let AgentEvent::TextDelta { text, .. } = &event {
            output.push_str(text);
        }

        // Extract session ID for resumption
        if let AgentEvent::RunStarted {
            external_session, ..
        } = &event
        {
            if let Some(ExternalSessionRef { id, .. }) = external_session {
                session_id = Some(id.clone());
            }
        }

        // Handle terminal events
        match &event {
            AgentEvent::RunCompleted {
                final_text,
                external_session,
            } => {
                if let Some(text) = final_text {
                    output = text.clone();
                }
                if let Some(ext) = external_session {
                    session_id = Some(ext.id.clone());
                }
                completed_result = Some(AdapterResult {
                    output: output.clone(),
                    session_id: session_id.clone(),
                    summary: None,
                    error: None,
                });
                break;
            }
            AgentEvent::RunFailed { code: _, message } => {
                error_message = preferred_terminal_adapter_error_detail(
                    error_message.as_deref(),
                    Some(message),
                );
            }
            AgentEvent::RunCancelled => {
                error_message = Some("Cancelled by user".to_string());
            }
            _ => {}
        }
    }

    // Wait for child to exit
    let status = match wait_for_child_exit(&mut child, stop_requested).await {
        Ok(status) => status,
        Err(err) => {
            return Ok(adapter_failure(output, session_id, err));
        }
    };
    let stderr_tail = finish_stderr_task(stderr_task).await;

    // Check for signal stop first
    if stop_signal.load(Ordering::Relaxed) {
        return Ok(AdapterResult {
            output,
            session_id,
            summary: None,
            error: Some("Stopped by user".to_string()),
        });
    }

    // Check exit status
    if status.success() {
        let mut result = completed_result.unwrap_or(AdapterResult {
            output,
            session_id,
            summary: None,
            error: None,
        });
        if let Some(guard) = git_side_effect_guard.as_ref() {
            if let Err(err) = guard.verify_success(&result.output) {
                let message = err.to_string();
                let _ = append_event(
                    &events,
                    &StreamEvent::live_summary(
                        summary_context
                            .diagnostic_line(&format!("git verification failed | {message}")),
                    ),
                );
                let _ = append_event(&events, &StreamEvent::error(message.clone()));
                let _ = persist_failed_verification_artifacts(
                    &request.worker_spool_dir(),
                    &request.query,
                    &chat_request.prompt,
                    guard,
                    &raw_adapter_events,
                    &raw_provider_events,
                );
                result.error = Some(err.to_string());
            }
        }
        Ok(result)
    } else {
        let stderr_detail = (!stderr_tail.trim().is_empty()).then_some(stderr_tail.as_str());
        let preferred_error_detail =
            preferred_terminal_adapter_error_detail(error_message.as_deref(), stderr_detail);
        let error_text = terminal_adapter_error_message(
            status.code(),
            &output,
            preferred_error_detail.as_deref(),
        );
        Ok(adapter_failure(
            output,
            session_id,
            anyhow::anyhow!("{error_text}"),
        ))
    }
}
fn adapter_failure(
    output: String,
    session_id: Option<String>,
    error: impl std::fmt::Display,
) -> AdapterResult {
    AdapterResult {
        output,
        session_id,
        summary: None,
        error: Some(error.to_string()),
    }
}
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
fn log_transport_retry(
    request: &WorkerRequest,
    summary_context: &LiveSummaryContext,
    events: &Path,
    message: &str,
) {
    match append_transport_error_log(&request.chat_id, message) {
        Ok(path) => rust_debug::tracing::warn!(
            chat_id=%request.chat_id,
            path=%path.display(),
            message,
            "logged adapter transport error"
        ),
        Err(err) => rust_debug::tracing::warn!(
            chat_id=%request.chat_id,
            error=%err,
            message,
            "failed to persist adapter transport error log"
        ),
    }
    let _ = append_event(
        events,
        &StreamEvent::live_summary(summary_context.diagnostic_line(message)),
    );
}
/// Convert a WorkerRequest into a ChatRequest for agent-adapter-cli.
fn worker_request_to_chat_request(req: &WorkerRequest) -> Result<ChatRequest> {
    let provider = model_provider_to_provider_id(&req.provider)?;
    let provider_for_chat = provider.clone();
    let model_provider = ModelProvider::from_alias(&req.provider);

    let cwd = req
        .working_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let prompt = build_query_with_memory(&req.query, req.memory_context.as_ref());
    let mut chat = ChatRequest::new(prompt, cwd, provider_for_chat);

    chat.model = req.model_override.clone();

    // Handle resume session
    if let Some(ref session_id) = req.resume_session_id {
        chat.resume = ResumeMode::External(ExternalSessionRef {
            id: session_id.clone(),
            provider,
        });
    }

    // Timeout
    chat.timeout_ms = None; // Worker-level timeout handled by stop_signal

    // Approval policy — use dangerous default (matches existing CLI behavior)
    chat.approval_policy = ApprovalPolicy::DangerousDefault;

    // Provider-specific adapter env is injected here when the native contract
    // requires it (for example MiniMax's Claude-compatible auth/base URL).
    chat.env = HashMap::new();

    if model_provider == Some(ModelProvider::MiniMax) {
        apply_minimax_native_contract(&mut chat)?;
    }

    Ok(chat)
}
fn apply_minimax_native_contract(chat: &mut ChatRequest) -> Result<()> {
    let minimax_env = config::api_key_env_for(ModelProvider::MiniMax)
        .expect("MiniMax catalog entry must define an api_key_env");
    let auth_token = minimax_api_key().ok_or_else(|| {
        anyhow::anyhow!(
            "{} is not set. Export it or write the key to {} before using MiniMax.",
            minimax_env,
            config::minimax_api_key_path()
        )
    })?;

    if chat.model.is_none() {
        chat.model = config::interactive_model_for(ModelProvider::MiniMax).map(str::to_string);
    }

    if let Some(config_dir) = config::get_config_dir(ModelProvider::MiniMax).map(PathBuf::from) {
        ensure_minimax_adapter_layout(&config_dir);
        chat.env.insert(
            "CLAUDE_CONFIG_DIR".to_string(),
            config_dir.to_string_lossy().into_owned(),
        );
    }

    chat.env.insert(
        "AGENT_ADAPTER_MINIMAX_EXE".to_string(),
        resolve_minimax_chatbot_adapter()
            .to_string_lossy()
            .into_owned(),
    );
    if let Some(base_url) = config::base_url_for(ModelProvider::MiniMax) {
        chat.env
            .insert("ANTHROPIC_BASE_URL".to_string(), base_url.to_string());
    }
    chat.env
        .insert("ANTHROPIC_AUTH_TOKEN".to_string(), auth_token);
    chat.env.insert(
        "MINIMAX_API_KEY".to_string(),
        chat.env["ANTHROPIC_AUTH_TOKEN"].clone(),
    );
    if let Some(reasoning_effort) =
        config::effective_reasoning_effort_for_provider(ModelProvider::MiniMax)
    {
        chat.env
            .insert("ANTHROPIC_REASONING_EFFORT".to_string(), reasoning_effort);
    }

    Ok(())
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
/// Map ModelProvider string to agent-adapter ProviderId.
fn model_provider_to_provider_id(provider: &str) -> Result<ProviderId> {
    let lower = provider.to_lowercase();
    match lower.as_str() {
        "claude" => Ok(ProviderId::Claude),
        "minimax" => Ok(ProviderId::MiniMax),
        "codex" => Ok(ProviderId::Codex),
        "gemini" => Ok(ProviderId::Gemini),
        "kimi" => Ok(ProviderId::Kimi),
        "opencode" => Ok(ProviderId::OpenCode),
        "hermes" => Ok(ProviderId::Hermes),
        "forge" => Ok(ProviderId::Forge),
        "claw" => Ok(ProviderId::Claw),
        _ => {
            // Try as a ModelProvider enum value
            ModelProvider::from_alias(provider)
                .map(|mp| {
                    model_provider_to_provider_id(mp.as_str())
                        .unwrap_or(ProviderId::Custom(provider.to_string()))
                })
                .ok_or_else(|| anyhow::anyhow!("Unknown provider: {}", provider))
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
fn adapter_system_prompt(request: &WorkerRequest) -> Option<String> {
    let provider = ModelProvider::from_alias(&request.provider)?;
    let base_prompt = effective_provider_system_prompt(
        provider,
        request.system_prompt.as_deref(),
        &request.query,
        None,
    );

    match provider {
        ModelProvider::Claude | ModelProvider::MiniMax => {
            let config = claude_compat_config(provider, None);
            let session = ProviderSession {
                working_dir: request.working_dir.clone(),
                claude_session_id: None,
                glm_session_id: None,
                minimax_session_id: request.resume_session_id.clone(),
                qwen_session_id: None,
                codex_session_id: None,
                nemotron_session_id: None,
                gemini_session_id: None,
                openrouter_session_id: None,
                omniforge_session_id: None,
                kimi_session_id: None,
                messages: Vec::new(),
            };
            effective_claude_system_prompt(
                &config,
                Some(&session),
                base_prompt.as_deref(),
                request.resume_session_id.is_some(),
            )
        }
        _ => base_prompt,
    }
}
impl WorkerRequest {
    /// Path to the worker spool directory for this chat_id.
    fn worker_spool_dir(&self) -> PathBuf {
        crate::worker::worker_dir(&self.chat_id)
    }}
#[cfg(test)]
#[path = "adapter_worker_resume_tests.rs"]
mod resume_tests;
#[cfg(test)]
#[path = "adapter_worker_spool_tests.rs"]
mod spool_tests;
#[cfg(test)]
#[path = "adapter_worker_transport_retry_tests.rs"]
mod transport_retry_tests;
