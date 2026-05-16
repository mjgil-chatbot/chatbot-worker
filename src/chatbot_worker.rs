//! chatbot-worker — run a single CLI command as a detached process.
//!
//! Usage:  chatbot-worker <chat_id>
//!
//! Reads `<shared infra>/workers/<chat_id>/request.json`, runs the provider
//! CLI, and updates the durable worker spool on each chunk.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{mpsc, Arc};
use crate::chatbot_worker_trace as trace;
use crate::model::ModelProvider;
use crate::worker::{self, WorkerJobSnapshot, WorkerRequest, WorkerResult, WorkerState};
use crate::worker_ext;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
/// Raw pointer to the Arc's inner AtomicBool, set once before any signal
/// can arrive. The signal handler uses this to set the flag.
static STOP_PTR: AtomicPtr<AtomicBool> = AtomicPtr::new(std::ptr::null_mut());
extern "C" fn sigterm_handler(_sig: libc::c_int) {
    let ptr = STOP_PTR.load(Ordering::Acquire);
    if !ptr.is_null() {
        unsafe { &*ptr }.store(true, Ordering::Release);
    }
}
fn install_sigterm_stop_signal() -> (Arc<AtomicBool>, *mut AtomicBool) {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_raw = Arc::into_raw(Arc::clone(&stop)) as *mut AtomicBool;
    STOP_PTR.store(stop_raw, Ordering::Release);

    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = sigterm_handler as *const () as libc::sighandler_t;
        libc::sigemptyset(&mut sa.sa_mask);
        sa.sa_flags = 0;
        libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
    }

    (stop, stop_raw)
}
fn uninstall_sigterm_stop_signal(stop_raw: *mut AtomicBool) {
    STOP_PTR.store(std::ptr::null_mut(), Ordering::Release);
    unsafe {
        Arc::from_raw(stop_raw as *const AtomicBool);
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
enum WorkerExtensionRequest {
    BackgroundDispatch {
        #[serde(default)]
        max_concurrent: Option<usize>,
        tasks: Vec<BackgroundDispatchTaskRequest>,
    },
    AstSearch {
        pattern: String,
        language: worker_ext::ast_search::Language,
        paths: Vec<String>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackgroundDispatchTaskRequest {
    provider: String,
    prompt: String,
    #[serde(default = "default_background_role")]
    role: String,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default)]
    working_dir: Option<String>,
    #[serde(default)]
    resume_session_id: Option<String>,
    #[serde(default = "default_background_context_budget")]
    context_budget: usize,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
enum WorkerExtensionResponse {
    BackgroundDispatch {
        max_concurrent: usize,
        merged: worker_ext::background::MergedResults,
    },
    AstSearch {
        result: worker_ext::ast_search::AstSearchResponse,
    },
}
fn default_background_role() -> String {
    "worker".to_string()
}
fn default_background_context_budget() -> usize {
    0
}
fn parse_provider(provider: &str) -> Result<ModelProvider> {
    ModelProvider::from_supported_alias(provider)
        .ok_or_else(|| anyhow::anyhow!("Unknown provider: {provider}"))
}
fn parse_worker_extension_request(
    request: &WorkerRequest,
) -> Result<Option<WorkerExtensionRequest>> {
    if request.command.trim() != "worker-ext" {
        return Ok(None);
    }
    let parsed =
        serde_json::from_str(&request.query).context("failed to parse worker extension request")?;
    Ok(Some(parsed))
}
fn run_ast_search(
    pattern: String,
    language: worker_ext::ast_search::Language,
    paths: Vec<String>,
) -> Result<worker_ext::ast_search::AstSearchResponse> {
    let tool = worker_ext::ast_search::AstSearchTool::new(
        pattern,
        language,
        paths.into_iter().map(PathBuf::from).collect(),
    );
    tool.execute().map_err(anyhow::Error::msg)
}
fn complete_worker_request(
    chat_id: &str,
    output_path: &Path,
    result_path: &Path,
    job: &mut WorkerJobSnapshot,
    state: WorkerState,
    output: &str,
    error: Option<String>,
) -> Result<()> {
    trace::complete_request(chat_id, state, output.len(), error.is_some());
    worker::write_text_atomic(output_path, output)
        .with_context(|| format!("failed to write {}", output_path.display()))?;
    trace::output_persisted(
        chat_id,
        output_path,
        output.len(),
        "complete_worker_request",
    );
    persist_terminal_result(
        result_path,
        job,
        state,
        WorkerResult {
            session_id: None,
            summary: None,
            error,
        },
    )?;
    job.output_len = output.len() as u64;
    job.bump();
    trace::state_write(chat_id, job, "complete_worker_request");
    worker::write_json_atomic(&worker::state_path(chat_id), job)
}
fn write_result(path: &std::path::Path, result: &WorkerResult) -> anyhow::Result<()> {
    worker::write_json_atomic(path, result)
}
pub(crate) fn persist_terminal_result(
    result_path: &std::path::Path,
    job: &mut WorkerJobSnapshot,
    desired_state: WorkerState,
    result: WorkerResult,
) -> anyhow::Result<()> {
    trace::terminal_result_attempt(&job.chat_id, result_path, desired_state, &result);
    match write_result(result_path, &result) {
        Ok(()) => {
            job.state = desired_state;
            trace::terminal_result_ok(&job.chat_id, job.state);
        }
        Err(err) => {
            trace::terminal_result_fallback(&job.chat_id, &err);
            let fallback = WorkerResult {
                session_id: None,
                summary: None,
                error: Some(format!("Failed to persist terminal result payload: {err}")),
            };
            write_result(result_path, &fallback)?;
            job.state = WorkerState::Error;
            trace::terminal_result_fallback_ok(&job.chat_id, job.state);
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests;
