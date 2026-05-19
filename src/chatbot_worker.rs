//! chatbot-worker — run a single CLI command as a detached process.
//!
//! Usage:  chatbot-worker <chat_id>
//!
//! Reads `<shared infra>/workers/<chat_id>/request.json`, runs the provider
//! CLI, and updates the durable worker spool on each chunk.

use crate::adapter_worker as claude_adapter;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{mpsc, Arc};

use crate::chatbot_worker_adapter::finalize_adapter_result;
use crate::chatbot_worker_runtime::{
    build_session, runsc_overrides_for_request, terminal_cli_error_message,
};
use crate::chatbot_worker_trace as trace;
use crate::config;
use crate::kimi_bridge;
use crate::model::ModelProvider;
use crate::provider_bridge::{call_provider_cli_stream, call_provider_cli_stream_with_overrides};
use crate::stream::StreamMessage;
use crate::worker::{self, WorkerJobSnapshot, WorkerRequest, WorkerResult, WorkerState};
use crate::{debug, ensure_path_symlink, hooks, worker_ext};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// ── SIGTERM handling ────────────────────────────────────────────────

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

fn execute_worker_extension_request<FB, FA>(
    request: WorkerExtensionRequest,
    stop_signal: Arc<AtomicBool>,
    background_runner: FB,
    ast_search_runner: FA,
) -> Result<String>
where
    FB: FnOnce(
        Option<usize>,
        Vec<BackgroundDispatchTaskRequest>,
        Arc<AtomicBool>,
    ) -> Result<worker_ext::background::MergedResults>,
    FA: FnOnce(
        String,
        worker_ext::ast_search::Language,
        Vec<String>,
    ) -> Result<worker_ext::ast_search::AstSearchResponse>,
{
    let response = match request {
        WorkerExtensionRequest::BackgroundDispatch {
            max_concurrent,
            tasks,
        } => WorkerExtensionResponse::BackgroundDispatch {
            max_concurrent: max_concurrent.unwrap_or_else(config::background_max_concurrent),
            merged: background_runner(max_concurrent, tasks, stop_signal)?,
        },
        WorkerExtensionRequest::AstSearch {
            pattern,
            language,
            paths,
        } => WorkerExtensionResponse::AstSearch {
            result: ast_search_runner(pattern, language, paths)?,
        },
    };

    serde_json::to_string_pretty(&response).context("failed to serialize worker extension response")
}

fn run_background_dispatch(
    max_concurrent: Option<usize>,
    tasks: Vec<BackgroundDispatchTaskRequest>,
    stop_signal: Arc<AtomicBool>,
) -> Result<worker_ext::background::MergedResults> {
    let mut dispatcher = worker_ext::background::BackgroundDispatcher::new(
        max_concurrent.unwrap_or_else(config::background_max_concurrent),
    );
    for task in tasks {
        dispatcher.add_task(worker_ext::background::BackgroundTask {
            id: uuid::Uuid::new_v4(),
            agent_profile: worker_ext::background::AgentProfile {
                role: task.role,
                provider: Some(task.provider),
            },
            prompt: task.prompt,
            context_budget: task.context_budget,
            system_prompt: task.system_prompt,
            working_dir: task.working_dir,
            resume_session_id: task.resume_session_id,
        });
    }

    let task_stop_signal = Arc::clone(&stop_signal);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(dispatcher.max_concurrent.max(1))
        .build()
        .context("failed to create background dispatch runtime")?;

    runtime.block_on(async move {
        dispatcher
            .run_all_with_executor(worker_ext::background::provider_task_executor(
                task_stop_signal,
            ))
            .await;
        Ok::<worker_ext::background::MergedResults, anyhow::Error>(dispatcher.take_merged().await)
    })
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

pub fn main() {
    let _log_guard = debug::init();
    let _debug_guard = rust_debug::init_debug_defaults("chatbot-worker");
    rust_debug::install_panic_hook();
    ensure_path_symlink();

    let args: Vec<String> = std::env::args().collect();
    if kimi_bridge::maybe_run_bridge_from_args(&args) {
        return;
    }
    if args.len() < 2 {
        eprintln!("Usage: chatbot-worker <chat_id>");
        std::process::exit(1);
    }
    let chat_id = &args[1];
    trace::worker_start(chat_id);

    // 1. Read request.json
    let req_path = worker::request_path(chat_id);
    let req_json = match fs::read_to_string(&req_path) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("Failed to read {}: {}", req_path.display(), e);
            std::process::exit(1);
        }
    };
    let request: WorkerRequest = match serde_json::from_str(&req_json) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to parse request.json: {}", e);
            std::process::exit(1);
        }
    };
    trace::request_loaded(chat_id, &request);

    let provider = match parse_provider(&request.provider) {
        Ok(provider) => provider,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    if let Err(err) = worker::clear_transient_spool_files(chat_id) {
        eprintln!("Failed to clear stale worker spool files: {}", err);
        std::process::exit(1);
    }

    let worker_pid = std::process::id();
    let parent_pid = request
        .parent_pid_override
        .unwrap_or_else(|| unsafe { libc::getppid() as u32 });
    let mut job = WorkerJobSnapshot::new(chat_id, &request.provider, worker_pid, parent_pid);
    trace::state_write(chat_id, &job, "initial");
    if let Err(e) = worker::write_json_atomic(&worker::state_path(chat_id), &job) {
        eprintln!("Failed to write initial worker state: {}", e);
    }

    // Set up stop signal: Arc<AtomicBool> shared with CLI thread + signal handler.
    // We store a raw pointer for the signal handler and reconstruct the Arc at the
    // end to avoid leaking.
    let (stop, stop_raw) = install_sigterm_stop_signal();

    if let Some(extension_request) = match parse_worker_extension_request(&request) {
        Ok(extension) => extension,
        Err(err) => {
            uninstall_sigterm_stop_signal(stop_raw);
            eprintln!("Failed to parse worker extension request: {err}");
            if let Err(write_err) = complete_worker_request(
                chat_id,
                &worker::output_path(chat_id),
                &worker::result_path(chat_id),
                &mut job,
                WorkerState::Error,
                &format!("Failed to parse worker extension request: {err}"),
                Some(err.to_string()),
            ) {
                eprintln!("warning: failed to persist extension parse failure: {write_err}");
            }
            std::process::exit(1);
        }
    } {
        trace::extension_start(chat_id);
        let extension_output = execute_worker_extension_request(
            extension_request,
            Arc::clone(&stop),
            run_background_dispatch,
            run_ast_search,
        );
        uninstall_sigterm_stop_signal(stop_raw);
        match extension_output {
            Ok(output) => {
                trace::extension_done(chat_id, output.len(), stop.load(Ordering::Acquire));
                let state = if stop.load(Ordering::Acquire) {
                    WorkerState::Stopped
                } else {
                    WorkerState::Done
                };
                let error = if state == WorkerState::Stopped {
                    Some("Stopped by user".to_string())
                } else {
                    None
                };
                if let Err(err) = complete_worker_request(
                    chat_id,
                    &worker::output_path(chat_id),
                    &worker::result_path(chat_id),
                    &mut job,
                    state,
                    &output,
                    error,
                ) {
                    eprintln!("warning: failed to persist worker extension result: {err}");
                    std::process::exit(1);
                }
                return;
            }
            Err(err) => {
                trace::extension_failed(chat_id, &err);
                let output = format!("Worker extension failed: {err}");
                if let Err(write_err) = complete_worker_request(
                    chat_id,
                    &worker::output_path(chat_id),
                    &worker::result_path(chat_id),
                    &mut job,
                    WorkerState::Error,
                    &output,
                    Some(err.to_string()),
                ) {
                    eprintln!("warning: failed to persist worker extension failure: {write_err}");
                }
                eprintln!("{output}");
                std::process::exit(1);
            }
        }
    }

    // 3. Build a minimal ChatSession for the CLI functions so provider launchers
    // can honor the session working directory even when there is no resume id.
    let session = Some(build_session(
        chat_id,
        provider,
        request.system_prompt.clone(),
        request.working_dir.clone(),
        request.resume_session_id.clone(),
        request.runsc_dirs.clone(),
        request.runsc_image.clone(),
    ));

    let overrides = runsc_overrides_for_request(provider, &request);

    // 4. Supported providers use the adapter-backed CLI path exclusively.
    if provider.is_supported() {
        trace::adapter_start(chat_id);
        let rt = tokio::runtime::Runtime::new().expect("failed to create runtime");
        let result = rt.block_on(claude_adapter::run_adapter(&request, Arc::clone(&stop)));
        trace::adapter_done(chat_id, result.is_ok());

        match result {
            Ok(adapter_result) => {
                if let Err(e) = finalize_adapter_result(chat_id, &mut job, adapter_result) {
                    eprintln!("warning: failed to persist result: {}", e);
                }
                uninstall_sigterm_stop_signal(stop_raw);
                return;
            }
            Err(e) => {
                eprintln!("Adapter error: {}", e);
                // Adapter failed — propagate error rather than falling through to legacy CLI
                // since all migrated providers should use the adapter path exclusively
                let _ = crate::stream::append_event(
                    &worker::events_path(chat_id),
                    &crate::stream::StreamEvent::error(format!("Adapter error: {e}")),
                );
                if let Err(write_err) = complete_worker_request(
                    chat_id,
                    &worker::output_path(chat_id),
                    &worker::result_path(chat_id),
                    &mut job,
                    WorkerState::Error,
                    "",
                    Some(e.to_string()),
                ) {
                    eprintln!("warning: failed to persist adapter error: {write_err}");
                }
                uninstall_sigterm_stop_signal(stop_raw);
                return;
            }
        }
    }

    // 5. Legacy CLI path for non-migrated providers.
    // Current interactive entrypoints only admit the supported adapter-backed
    // providers, so this remains as dead-end compatibility scaffolding until the
    // legacy bridge removal lands.
    let (tx, rx) = mpsc::channel::<StreamMessage>();
    let stop_signal = Arc::clone(&stop);
    let hook_registry = hooks::HookRegistry::load_default();
    let query = request.query.clone();
    let system_prompt = request.system_prompt.clone();
    let memory_context = request.memory_context.clone();
    let chat_id_for_thread = chat_id.to_string();

    let cli_thread = std::thread::spawn(move || {
        trace::cli_thread_start(
            &chat_id_for_thread,
            provider.as_str(),
            query.len(),
            system_prompt.is_some(),
            memory_context.is_some(),
        );
        if let Err(err) = hook_registry.execute(
            hooks::HookPoint::PreProviderCall,
            &serde_json::json!({"chat_id": chat_id_for_thread}),
        ) {
            eprintln!("PreProviderCall hook failed: {}", err);
            return Err(anyhow::anyhow!("PreProviderCall hook failed: {}", err));
        }

        let result = if let Some(ref ov) = overrides {
            call_provider_cli_stream_with_overrides(
                provider,
                &query,
                system_prompt.as_deref(),
                memory_context.as_ref(),
                session,
                Some(ov),
                tx,
                stop_signal,
            )
        } else {
            call_provider_cli_stream(
                provider,
                &query,
                system_prompt.as_deref(),
                memory_context.as_ref(),
                session,
                tx,
                stop_signal,
            )
        };
        trace::cli_thread_finish(&chat_id_for_thread, result.is_ok());

        let _ = hook_registry.execute(
            hooks::HookPoint::PostProviderCall,
            &serde_json::json!({
                "chat_id": chat_id_for_thread,
                "success": result.is_ok()
            }),
        );

        result
    });

    // 5. Drain the channel, write output file, update mmap on each message
    let output_path = worker::output_path(chat_id);
    let result_path = worker::result_path(chat_id);
    let mut final_session_id: Option<String> = None;
    let mut latest_session_id: Option<String> = None;
    let mut final_summary = None;
    let mut last_response = String::new();
    let mut had_error = false;
    let mut cli_error_message: Option<String> = None;

    for msg in rx.iter() {
        trace::stream_message(chat_id, &msg);
        last_response = msg.response.clone();

        // Write output file
        if let Err(e) = worker::write_text_atomic(&output_path, &last_response) {
            eprintln!("Failed to write output: {}", e);
        } else {
            trace::output_persisted(chat_id, &output_path, last_response.len(), "stream");
        }

        job.output_len = last_response.len() as u64;
        job.bump();
        trace::state_write(chat_id, &job, "stream");
        if let Err(e) = worker::write_json_atomic(&worker::state_path(chat_id), &job) {
            eprintln!("Failed to update worker state: {}", e);
        }

        if let Some(session_id) = msg.session_id.clone() {
            latest_session_id = Some(session_id);
        }

        let result_session_id = latest_session_id.clone().or(final_session_id.clone());

        // Always capture the latest summary so context data is available
        // when the result is read (even before the final message).
        if msg.summary.is_some() {
            final_summary = msg.summary;
            // Write intermediate result so the UI can read context data
            // while the worker is still running.
            let intermediate = WorkerResult {
                session_id: result_session_id.clone(),
                summary: final_summary.clone(),
                error: None,
            };
            trace::intermediate_result(chat_id, &intermediate);
            let _ = worker::write_json_atomic(&result_path, &intermediate);
        }
        if msg.is_final {
            final_session_id = result_session_id.or(msg.session_id);
            trace::final_session_id(chat_id, final_session_id.is_some());
        }
    }
    trace::stream_channel_closed(
        chat_id,
        last_response.len(),
        final_session_id.is_some(),
        final_summary.is_some(),
    );
    trace::cli_join_wait_start(chat_id);

    // 6. Wait for CLI thread
    match cli_thread.join() {
        Ok(Ok(())) => {
            trace::cli_join_ok(chat_id);
        }
        Ok(Err(e)) => {
            trace::cli_join_error(chat_id, &e);
            cli_error_message = Some(e.to_string());
            eprintln!("CLI error: {}", e);
            had_error = true;
        }
        Err(_) => {
            trace::cli_join_panic(chat_id);
            eprintln!("CLI thread panicked");
            had_error = true;
        }
    }

    // Reclaim the Arc we stored for the signal handler (prevents leak).
    // Must happen before any exit() path to avoid leaking.
    uninstall_sigterm_stop_signal(stop_raw);

    // 7. Check if we were stopped
    if stop.load(Ordering::Acquire) {
        trace::terminal_finish(
            chat_id,
            last_response.len(),
            final_session_id.is_some(),
            final_summary.is_some(),
            WorkerState::Stopped,
        );
        // Write final partial output
        let _ = worker::write_text_atomic(&output_path, &last_response);
        if let Err(err) = persist_terminal_result(
            &result_path,
            &mut job,
            WorkerState::Stopped,
            WorkerResult {
                session_id: final_session_id,
                summary: final_summary,
                error: Some("Stopped by user".to_string()),
            },
        ) {
            eprintln!("warning: failed to write result.json: {}", err);
            std::process::exit(1);
        }
        job.output_len = last_response.len() as u64;
        job.bump();
        let _ = worker::write_json_atomic(&worker::state_path(chat_id), &job);
        std::process::exit(0);
    }

    // 8. Write result.json and set final state
    if had_error {
        trace::terminal_finish(
            chat_id,
            last_response.len(),
            final_session_id.is_some(),
            final_summary.is_some(),
            WorkerState::Error,
        );
        if let Err(err) = persist_terminal_result(
            &result_path,
            &mut job,
            WorkerState::Error,
            WorkerResult {
                session_id: final_session_id,
                summary: final_summary,
                error: Some(terminal_cli_error_message(cli_error_message.as_deref())),
            },
        ) {
            eprintln!("warning: failed to write result.json: {}", err);
            std::process::exit(1);
        }
    } else {
        trace::terminal_finish(
            chat_id,
            last_response.len(),
            final_session_id.is_some(),
            final_summary.is_some(),
            WorkerState::Done,
        );
        if let Err(err) = persist_terminal_result(
            &result_path,
            &mut job,
            WorkerState::Done,
            WorkerResult {
                session_id: final_session_id,
                summary: final_summary,
                error: None,
            },
        ) {
            eprintln!("warning: failed to write result.json: {}", err);
            std::process::exit(1);
        }
    }
    job.output_len = last_response.len() as u64;
    job.bump();
    trace::state_write(chat_id, &job, "final");
    let _ = worker::write_json_atomic(&worker::state_path(chat_id), &job);
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
