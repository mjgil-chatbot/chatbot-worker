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
use crate::worker::WorkerRequest;
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
fn default_background_role() -> String {
    "worker".to_string()
}
fn default_background_context_budget() -> usize {
    0
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
