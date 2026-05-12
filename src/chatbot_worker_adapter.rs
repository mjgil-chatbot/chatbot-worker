use std::path::Path;
use anyhow::Result;
use crate::chatbot_worker_trace as trace;
use crate::stream::{events_path, read_events_since};
use crate::stream_output::build_persisted_turn_output;
use crate::worker;
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
