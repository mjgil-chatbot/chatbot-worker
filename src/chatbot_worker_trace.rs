use std::path::Path;
use crate::worker::WorkerRequest;
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
pub(crate) fn output_persisted(chat_id: &str, path: &Path, output_len: usize, phase: &'static str) {
    rust_debug::tracing::debug!(
        chat_id=%chat_id,
        path=%path.display(),
        output_len,
        phase,
        "worker output persisted"
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
pub(crate) fn terminal_result_fallback(chat_id: &str, err: &anyhow::Error) {
    rust_debug::tracing::warn!(
        chat_id=%chat_id,
        error=%err,
        "terminal worker result persistence failed; writing fallback error payload"
    );
}
