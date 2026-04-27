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
