use std::path::Path;

use crate::stream::StreamMessage;
use crate::worker::{WorkerJobSnapshot, WorkerRequest, WorkerResult, WorkerState};

pub(crate) fn worker_start(chat_id: &str) {
    rust_debug::tracing::info!(chat_id=%chat_id, "chatbot-worker starting");
}

pub(crate) fn request_loaded(chat_id: &str, request: &WorkerRequest) {
    rust_debug::tracing::info!(
        chat_id=%chat_id,
        provider=%request.provider,
        query_len=request.query.len(),
        has_system_prompt=request.system_prompt.is_some(),
        has_resume_session_id=request.resume_session_id.is_some(),
        has_memory_context=request.memory_context.is_some(),
        has_parent_pid_override=request.parent_pid_override.is_some(),
        command=%request.command,
        "worker request loaded"
    );
}

pub(crate) fn state_write(chat_id: &str, job: &WorkerJobSnapshot, phase: &'static str) {
    rust_debug::tracing::debug!(
        chat_id=%chat_id,
        state=%job.state.name(),
        sequence=job.sequence,
        output_len=job.output_len,
        worker_pid=job.worker_pid,
        parent_pid=job.parent_pid,
        phase,
        "writing worker state snapshot"
    );
}

pub(crate) fn extension_start(chat_id: &str) {
    rust_debug::tracing::info!(chat_id=%chat_id, "executing worker extension request");
}

pub(crate) fn extension_done(chat_id: &str, output_len: usize, stopped: bool) {
    rust_debug::tracing::info!(
        chat_id=%chat_id,
        output_len,
        stopped,
        "worker extension request completed"
    );
}

pub(crate) fn extension_failed(chat_id: &str, err: &anyhow::Error) {
    rust_debug::tracing::warn!(
        chat_id=%chat_id,
        error=%err,
        "worker extension request failed"
    );
}

pub(crate) fn adapter_start(chat_id: &str) {
    rust_debug::tracing::info!(chat_id=%chat_id, "starting agent-adapter-cli");
}

pub(crate) fn adapter_done(chat_id: &str, success: bool) {
    rust_debug::tracing::info!(
        chat_id=%chat_id,
        success,
        "agent-adapter-cli finished"
    );
}

pub(crate) fn cli_thread_start(
    chat_id: &str,
    provider: &str,
    query_len: usize,
    has_system_prompt: bool,
    has_memory_context: bool,
) {
    rust_debug::tracing::debug!(
        chat_id=%chat_id,
        provider,
        query_len,
        has_system_prompt,
        has_memory_context,
        "worker CLI thread starting provider call"
    );
}

pub(crate) fn cli_thread_finish(chat_id: &str, success: bool) {
    rust_debug::tracing::debug!(
        chat_id=%chat_id,
        success,
        "worker CLI thread finished provider call"
    );
}

pub(crate) fn stream_message(chat_id: &str, msg: &StreamMessage) {
    rust_debug::tracing::debug!(
        chat_id=%chat_id,
        response_len=msg.response.len(),
        is_final=msg.is_final,
        has_session_id=msg.session_id.is_some(),
        has_summary=msg.summary.is_some(),
        "worker stream message received"
    );
}

pub(crate) fn output_persisted(chat_id: &str, path: &Path, output_len: usize, phase: &'static str) {
    rust_debug::tracing::debug!(
        chat_id=%chat_id,
        path=%path.display(),
        output_len,
        phase,
        "worker output persisted"
    );
}

pub(crate) fn intermediate_result(chat_id: &str, result: &WorkerResult) {
    rust_debug::tracing::debug!(
        chat_id=%chat_id,
        has_session_id=result.session_id.is_some(),
        has_summary=result.summary.is_some(),
        has_error=result.error.is_some(),
        "writing intermediate worker result snapshot"
    );
}

pub(crate) fn final_session_id(chat_id: &str, has_final_session_id: bool) {
    rust_debug::tracing::debug!(
        chat_id=%chat_id,
        has_final_session_id,
        "worker stream marked final session id"
    );
}

pub(crate) fn stream_channel_closed(
    chat_id: &str,
    output_len: usize,
    has_final_session_id: bool,
    has_summary: bool,
) {
    rust_debug::tracing::debug!(
        chat_id=%chat_id,
        output_len,
        has_final_session_id,
        has_summary,
        "worker stream channel closed; all provider messages drained"
    );
}

pub(crate) fn cli_join_wait_start(chat_id: &str) {
    rust_debug::tracing::debug!(
        chat_id=%chat_id,
        "worker stream drained; waiting for provider thread join"
    );
}

pub(crate) fn cli_join_ok(chat_id: &str) {
    rust_debug::tracing::info!(chat_id=%chat_id, "worker CLI thread joined successfully");
}

pub(crate) fn cli_join_error(chat_id: &str, err: &anyhow::Error) {
    rust_debug::tracing::warn!(
        chat_id=%chat_id,
        error=%err,
        "worker CLI thread returned error"
    );
}

pub(crate) fn cli_join_panic(chat_id: &str) {
    rust_debug::tracing::warn!(chat_id=%chat_id, "worker CLI thread panicked");
}

pub(crate) fn terminal_finish(
    chat_id: &str,
    output_len: usize,
    has_final_session_id: bool,
    has_summary: bool,
    state: WorkerState,
) {
    let level = match state {
        WorkerState::Done => "info",
        WorkerState::Stopped => "info",
        WorkerState::Error => "warn",
        WorkerState::Running | WorkerState::Free => "debug",
    };
    match level {
        "warn" => rust_debug::tracing::warn!(
            chat_id=%chat_id,
            output_len,
            has_final_session_id,
            has_summary,
            state=%state.name(),
            "worker finishing in terminal state"
        ),
        _ => rust_debug::tracing::info!(
            chat_id=%chat_id,
            output_len,
            has_final_session_id,
            has_summary,
            state=%state.name(),
            "worker finishing in terminal state"
        ),
    }
}

pub(crate) fn complete_request(
    chat_id: &str,
    state: WorkerState,
    output_len: usize,
    has_error: bool,
) {
    rust_debug::tracing::debug!(
        chat_id=%chat_id,
        desired_state=%state.name(),
        output_len,
        has_error,
        "completing worker request"
    );
}

pub(crate) fn terminal_result_attempt(
    chat_id: &str,
    result_path: &Path,
    desired_state: WorkerState,
    result: &WorkerResult,
) {
    rust_debug::tracing::debug!(
        chat_id=%chat_id,
        desired_state=%desired_state.name(),
        path=%result_path.display(),
        has_session_id=result.session_id.is_some(),
        has_summary=result.summary.is_some(),
        has_error=result.error.is_some(),
        "persisting terminal worker result"
    );
}

pub(crate) fn terminal_result_ok(chat_id: &str, state: WorkerState) {
    rust_debug::tracing::debug!(
        chat_id=%chat_id,
        state=%state.name(),
        "terminal worker result persisted successfully"
    );
}

pub(crate) fn terminal_result_fallback(chat_id: &str, err: &anyhow::Error) {
    rust_debug::tracing::warn!(
        chat_id=%chat_id,
        error=%err,
        "terminal worker result persistence failed; writing fallback error payload"
    );
}

pub(crate) fn terminal_result_fallback_ok(chat_id: &str, state: WorkerState) {
    rust_debug::tracing::warn!(
        chat_id=%chat_id,
        state=%state.name(),
        "terminal worker fallback error payload persisted"
    );
}
