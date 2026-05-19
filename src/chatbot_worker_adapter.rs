use std::path::Path;

use anyhow::Result;

use crate::adapter_worker::AdapterResult;
use crate::chatbot_worker::persist_terminal_result;
use crate::chatbot_worker_trace as trace;
use crate::stream::{events_path, read_events_since};
use crate::stream_output::build_persisted_turn_output;
use crate::worker::{self, WorkerJobSnapshot, WorkerResult, WorkerState};

pub(crate) fn finalize_adapter_result(
    chat_id: &str,
    job: &mut WorkerJobSnapshot,
    adapter_result: AdapterResult,
) -> Result<()> {
    let output_path = worker::output_path(chat_id);
    let persisted_output = compose_persisted_output(chat_id, &adapter_result.output);
    write_adapter_output(chat_id, &output_path, &persisted_output);

    let final_state = match adapter_result.error.as_deref() {
        Some(error) if is_user_stop_error(error) => WorkerState::Stopped,
        Some(_) => WorkerState::Error,
        None => WorkerState::Done,
    };
    let final_result = WorkerResult {
        session_id: adapter_result.session_id,
        summary: adapter_result.summary,
        error: adapter_result.error,
    };

    job.output_len = persisted_output.len() as u64;
    job.bump();
    persist_terminal_result(
        &worker::result_path(chat_id),
        job,
        final_state,
        final_result,
    )?;
    trace::state_write(chat_id, job, "adapter_complete");
    worker::write_json_atomic(&worker::state_path(chat_id), job)?;
    Ok(())
}

fn compose_persisted_output(chat_id: &str, final_output: &str) -> String {
    let (events, _) = read_events_since(&events_path(&worker::worker_dir(chat_id)), 0);
    build_persisted_turn_output(&events, final_output).combined
}

fn write_adapter_output(chat_id: &str, output_path: &Path, output: &str) {
    if let Err(err) = worker::write_text_atomic(output_path, output) {
        eprintln!("Failed to write output: {}", err);
    } else {
        trace::output_persisted(chat_id, output_path, output.len(), "adapter_complete");
    }
}

fn is_user_stop_error(error: &str) -> bool {
    matches!(error, "Stopped by user" | "Cancelled by user")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalize_adapter_result_persists_error_state_to_spool() {
        let _lock = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let previous_home = std::env::var_os("HOME");
        let previous_config = std::env::var_os("RUST_CHATBOT_CONFIG_PATH");
        std::env::set_var("HOME", temp_dir.path());
        std::env::remove_var("RUST_CHATBOT_CONFIG_PATH");

        let chat_id = "adapter-finalize-error";
        std::fs::create_dir_all(worker::worker_dir(chat_id)).unwrap();
        let mut job = WorkerJobSnapshot::new(chat_id, "codex", 42, 24);
        let adapter_result = AdapterResult {
            output: String::new(),
            session_id: None,
            summary: None,
            error: Some("exit_code:1".to_string()),
        };

        finalize_adapter_result(chat_id, &mut job, adapter_result).unwrap();

        let persisted = worker::read_job_state(chat_id).unwrap().unwrap();
        let result_json = std::fs::read_to_string(worker::result_path(chat_id)).unwrap();

        match previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match previous_config {
            Some(value) => std::env::set_var("RUST_CHATBOT_CONFIG_PATH", value),
            None => std::env::remove_var("RUST_CHATBOT_CONFIG_PATH"),
        }

        assert_eq!(persisted.state, WorkerState::Error);
        assert!(result_json.contains("exit_code:1"));
    }

    #[test]
    fn finalize_adapter_result_persists_stopped_state_to_spool() {
        let _lock = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let previous_home = std::env::var_os("HOME");
        let previous_config = std::env::var_os("RUST_CHATBOT_CONFIG_PATH");
        std::env::set_var("HOME", temp_dir.path());
        std::env::remove_var("RUST_CHATBOT_CONFIG_PATH");

        let chat_id = "adapter-finalize-stopped";
        std::fs::create_dir_all(worker::worker_dir(chat_id)).unwrap();
        let mut job = WorkerJobSnapshot::new(chat_id, "codex", 42, 24);
        let adapter_result = AdapterResult {
            output: String::new(),
            session_id: None,
            summary: None,
            error: Some("Stopped by user".to_string()),
        };

        finalize_adapter_result(chat_id, &mut job, adapter_result).unwrap();

        let persisted = worker::read_job_state(chat_id).unwrap().unwrap();

        match previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match previous_config {
            Some(value) => std::env::set_var("RUST_CHATBOT_CONFIG_PATH", value),
            None => std::env::remove_var("RUST_CHATBOT_CONFIG_PATH"),
        }

        assert_eq!(persisted.state, WorkerState::Stopped);
    }
}
