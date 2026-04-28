use super::*;
fn worker_request(command: &str, query: &str) -> WorkerRequest {
    WorkerRequest {
        provider: "codex".to_string(),
        query: query.to_string(),
        working_dir: None,
        system_prompt: None,
        resume_session_id: None,
        memory_context: None,
        parent_pid_override: None,
        runsc_dirs: Vec::new(),
        runsc_image: None,
        model_override: None,
        command: command.to_string(),
        chat_id: "chat-test".to_string(),
    }
}
