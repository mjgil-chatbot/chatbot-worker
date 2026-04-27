//! Worker types and helpers shared between the UI and chatbot-worker binary.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;
use crate::config;
use crate::memory::MemoryContext;
/// Request written by the UI before spawning a worker.
#[derive(Debug, Clone)]
pub struct WorkerRequest {
    pub provider: String,
    pub query: String,
    pub working_dir: Option<String>,
    pub system_prompt: Option<String>,
    pub resume_session_id: Option<String>,
    pub memory_context: Option<MemoryContext>,
    pub parent_pid_override: Option<u32>,
    pub runsc_dirs: Vec<String>,
    pub runsc_image: Option<String>,
    pub model_override: Option<String>,
    pub command: String,
    pub chat_id: String,
}
/// Root directory for all worker data.
pub fn workers_dir() -> PathBuf {
    config::shared_infra_dir().join("workers")
}
/// Validate chat_id to prevent path traversal attacks.
/// Returns the chat_id if valid, otherwise None.
fn validate_chat_id(chat_id: &str) -> Option<&str> {
    // Reject empty strings
    if chat_id.is_empty() {
        return None;
    }
    // Reject path traversal characters
    if chat_id.contains("..") || chat_id.contains('/') || chat_id.contains('\\') {
        return None;
    }
    // Reject characters that could be used for path manipulation
    if chat_id.contains('\0') || chat_id.contains(':') {
        return None;
    }
    Some(chat_id)
}
fn safe_chat_id_component(chat_id: &str) -> Cow<'_, str> {
    if let Some(valid) = validate_chat_id(chat_id) {
        return Cow::Borrowed(valid);
    }

    let mut encoded = String::with_capacity(chat_id.len() * 2 + 8);
    encoded.push_str("chat-");
    for byte in chat_id.as_bytes() {
        encoded.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
        encoded.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap());
    }
    Cow::Owned(encoded)
}
/// Directory for a specific worker (by chat_id).
pub fn worker_dir(chat_id: &str) -> PathBuf {
    workers_dir().join(safe_chat_id_component(chat_id).as_ref())
}
/// Path to per-run transport retry diagnostics for a worker.
pub fn transport_errors_path(chat_id: &str) -> PathBuf {
    worker_dir(chat_id).join("transport-errors.log")
}
/// Atomically write text content inside the worker spool directory.
pub fn write_text_atomic(path: &Path, content: &str) -> io::Result<()> {
    write_bytes_atomic(path, content.as_bytes())
}
fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    fs::create_dir_all(parent)?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let tmp_path = parent.join(format!(".{}.tmp-{}", file_name, std::process::id()));

    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}
