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
use crate::summary::JsonSummary;
pub const WORKER_SPOOL_SCHEMA_VERSION: u32 = 1;
fn default_worker_spool_schema_version() -> u32 {
    WORKER_SPOOL_SCHEMA_VERSION
}
fn validate_worker_spool_schema_version(version: u32, artifact: &str) -> Result<()> {
    if version == WORKER_SPOOL_SCHEMA_VERSION {
        return Ok(());
    }

    anyhow::bail!(
        "unsupported {} schema_version {} (expected {})",
        artifact,
        version,
        WORKER_SPOOL_SCHEMA_VERSION,
    );
}
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
/// Result written by the worker on completion.
#[derive(Debug, Clone)]
pub struct WorkerResult {
    pub session_id: Option<String>,
    pub summary: Option<JsonSummary>,
    pub error: Option<String>,
}
/// Worker state in the mmap registry.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum WorkerState {
    Free = 0,
    Running = 1,
    Done = 2,
    Stopped = 3,
    Error = 4,
}
impl WorkerState {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Free => "FREE",
            Self::Running => "RUNNING",
            Self::Done => "DONE",
            Self::Stopped => "STOPPED",
            Self::Error => "ERROR",
        }
    }}
impl WorkerState {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::Running,
            2 => Self::Done,
            3 => Self::Stopped,
            4 => Self::Error,
            _ => Self::Free,
        }
    }}
/// Snapshot of a worker slot read from the mmap.
#[derive(Debug, Clone)]
pub struct WorkerSlotSnapshot {
    pub slot_index: usize,
    pub worker_pid: u32,
    pub parent_pid: u32,
    pub provider: u32,
    pub state: WorkerState,
    pub chat_id: String,
    pub started_at: i64,
    pub updated_at: i64,
    pub output_len: u64,
    pub sequence: u64,
}
/// Durable job state written into the worker spool directory.
#[derive(Debug, Clone)]
pub struct WorkerJobSnapshot {
    pub chat_id: String,
    pub provider: String,
    pub worker_pid: u32,
    pub parent_pid: u32,
    pub state: WorkerState,
    pub started_at: i64,
    pub updated_at: i64,
    pub output_len: u64,
    pub sequence: u64,
}
#[derive(Serialize, Deserialize)]
struct WorkerRequestSerde {
    #[serde(default = "default_worker_spool_schema_version")]
    schema_version: u32,
    provider: String,
    query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    working_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resume_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    memory_context: Option<MemoryContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_pid_override: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    runsc_dirs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runsc_image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model_override: Option<String>,
    command: String,
    chat_id: String,
}
#[derive(Serialize)]
struct WorkerRequestSerialize<'a> {
    schema_version: u32,
    provider: &'a str,
    query: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    working_dir: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_prompt: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resume_session_id: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    memory_context: Option<&'a MemoryContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_pid_override: Option<u32>,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    runsc_dirs: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    runsc_image: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_override: Option<&'a String>,
    command: &'a str,
    chat_id: &'a str,
}
#[derive(Serialize, Deserialize)]
struct WorkerResultSerde {
    #[serde(default = "default_worker_spool_schema_version")]
    schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<JsonSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}
#[derive(Serialize)]
struct WorkerResultSerialize<'a> {
    schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<&'a JsonSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a String>,
}
#[derive(Serialize, Deserialize)]
struct WorkerJobSnapshotSerde {
    #[serde(default = "default_worker_spool_schema_version")]
    schema_version: u32,
    chat_id: String,
    provider: String,
    worker_pid: u32,
    parent_pid: u32,
    state: WorkerState,
    started_at: i64,
    updated_at: i64,
    output_len: u64,
    sequence: u64,
}
#[derive(Serialize)]
struct WorkerJobSnapshotSerialize<'a> {
    schema_version: u32,
    chat_id: &'a str,
    provider: &'a str,
    worker_pid: u32,
    parent_pid: u32,
    state: WorkerState,
    started_at: i64,
    updated_at: i64,
    output_len: u64,
    sequence: u64,
}
impl Serialize for WorkerRequest {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WorkerRequestSerialize {
            schema_version: WORKER_SPOOL_SCHEMA_VERSION,
            provider: &self.provider,
            query: &self.query,
            working_dir: self.working_dir.as_ref(),
            system_prompt: self.system_prompt.as_ref(),
            resume_session_id: self.resume_session_id.as_ref(),
            memory_context: self.memory_context.as_ref(),
            parent_pid_override: self.parent_pid_override,
            runsc_dirs: &self.runsc_dirs,
            runsc_image: self.runsc_image.as_ref(),
            model_override: self.model_override.as_ref(),
            command: &self.command,
            chat_id: &self.chat_id,
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for WorkerRequest {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = WorkerRequestSerde::deserialize(deserializer)?;
        validate_worker_spool_schema_version(raw.schema_version, "request.json")
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            provider: raw.provider,
            query: raw.query,
            working_dir: raw.working_dir,
            system_prompt: raw.system_prompt,
            resume_session_id: raw.resume_session_id,
            memory_context: raw.memory_context,
            parent_pid_override: raw.parent_pid_override,
            runsc_dirs: raw.runsc_dirs,
            runsc_image: raw.runsc_image,
            model_override: raw.model_override,
            command: raw.command,
            chat_id: raw.chat_id,
        })
    }
}
impl Serialize for WorkerResult {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WorkerResultSerialize {
            schema_version: WORKER_SPOOL_SCHEMA_VERSION,
            session_id: self.session_id.as_ref(),
            summary: self.summary.as_ref(),
            error: self.error.as_ref(),
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for WorkerResult {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = WorkerResultSerde::deserialize(deserializer)?;
        validate_worker_spool_schema_version(raw.schema_version, "result.json")
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            session_id: raw.session_id,
            summary: raw.summary,
            error: raw.error,
        })
    }
}
impl Serialize for WorkerJobSnapshot {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WorkerJobSnapshotSerialize {
            schema_version: WORKER_SPOOL_SCHEMA_VERSION,
            chat_id: &self.chat_id,
            provider: &self.provider,
            worker_pid: self.worker_pid,
            parent_pid: self.parent_pid,
            state: self.state,
            started_at: self.started_at,
            updated_at: self.updated_at,
            output_len: self.output_len,
            sequence: self.sequence,
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for WorkerJobSnapshot {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = WorkerJobSnapshotSerde::deserialize(deserializer)?;
        validate_worker_spool_schema_version(raw.schema_version, "state.json")
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            chat_id: raw.chat_id,
            provider: raw.provider,
            worker_pid: raw.worker_pid,
            parent_pid: raw.parent_pid,
            state: raw.state,
            started_at: raw.started_at,
            updated_at: raw.updated_at,
            output_len: raw.output_len,
            sequence: raw.sequence,
        })
    }
}
impl WorkerJobSnapshot {
    pub fn new(chat_id: &str, provider: &str, worker_pid: u32, parent_pid: u32) -> Self {
        let now = Utc::now().timestamp();
        Self {
            chat_id: chat_id.to_string(),
            provider: provider.to_string(),
            worker_pid,
            parent_pid,
            state: WorkerState::Running,
            started_at: now,
            updated_at: now,
            output_len: 0,
            sequence: 1,
        }
    }}
impl WorkerJobSnapshot {
    pub fn bump(&mut self) {
        self.sequence = self.sequence.saturating_add(1);
        self.updated_at = Utc::now().timestamp();
    }}
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
/// Path to the request.json file for a worker.
pub fn request_path(chat_id: &str) -> PathBuf {
    worker_dir(chat_id).join("request.json")
}
/// Path to the output file for a worker.
pub fn output_path(chat_id: &str) -> PathBuf {
    worker_dir(chat_id).join("output")
}
/// Path to the result.json file for a worker.
pub fn result_path(chat_id: &str) -> PathBuf {
    worker_dir(chat_id).join("result.json")
}
/// Path to the state.json file for a worker/job spool entry.
pub fn state_path(chat_id: &str) -> PathBuf {
    worker_dir(chat_id).join("state.json")
}
/// Path to the JSONL event log for a worker.
pub fn events_path(chat_id: &str) -> PathBuf {
    worker_dir(chat_id).join("events.jsonl")
}
/// Path to captured raw adapter event JSONL for a worker.
pub fn raw_adapter_events_path(chat_id: &str) -> PathBuf {
    worker_dir(chat_id).join("adapter-events.raw.jsonl")
}
/// Path to captured raw provider event JSONL for a worker.
pub fn raw_provider_events_path(chat_id: &str) -> PathBuf {
    worker_dir(chat_id).join("provider-events.raw.jsonl")
}
/// Path to captured git verifier context for a worker.
pub fn git_verifier_context_path(chat_id: &str) -> PathBuf {
    worker_dir(chat_id).join("git-verifier-context.json")
}
/// Path to per-run transport retry diagnostics for a worker.
pub fn transport_errors_path(chat_id: &str) -> PathBuf {
    worker_dir(chat_id).join("transport-errors.log")
}
const MISSING_STATE_STARTUP_GRACE: Duration = Duration::from_secs(5);
/// Read the current spool state for one worker.
pub fn read_job_state(chat_id: &str) -> Result<Option<WorkerJobSnapshot>> {
    let path = state_path(chat_id);
    let json = match fs::read_to_string(&path) {
        Ok(json) => json,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", path.display())),
    };
    let job = serde_json::from_str(&json)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(job))
}
/// List all spool states currently present on disk.
pub fn list_job_states() -> Result<Vec<WorkerJobSnapshot>> {
    let mut jobs = Vec::new();
    let entries = match fs::read_dir(workers_dir()) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(jobs),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", workers_dir().display()))
        }
    };

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let Some(chat_id) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        match read_job_state(&chat_id) {
            Ok(Some(job)) => jobs.push(job),
            Ok(None) => {
                if missing_state_spool_is_transient(&chat_id) {
                    continue;
                }
                cleanup_missing_state_spool_entry(&chat_id);
            }
            Err(err) => {
                rust_debug::tracing::warn!(
                    chat_id=%chat_id,
                    error=%err,
                    "skipping unreadable worker spool entry"
                );
            }
        }
    }

    Ok(jobs)
}
fn missing_state_spool_is_transient(chat_id: &str) -> bool {
    [request_path(chat_id), worker_dir(chat_id)]
        .into_iter()
        .filter_map(|path| path.metadata().ok())
        .filter_map(|metadata| metadata.modified().ok())
        .filter_map(|modified| modified.elapsed().ok())
        .any(|elapsed| elapsed <= MISSING_STATE_STARTUP_GRACE)
}
fn cleanup_missing_state_spool_entry(chat_id: &str) {
    let dir = worker_dir(chat_id);
    match fs::remove_dir_all(&dir) {
        Ok(()) => {
            rust_debug::tracing::info!(
                chat_id=%chat_id,
                path=%dir.display(),
                "removed stale worker spool entry without state.json"
            );
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => {
            rust_debug::tracing::warn!(
                chat_id=%chat_id,
                error=%err,
                path=%dir.display(),
                "failed to remove stale worker spool entry without state.json"
            );
        }
    }
}
/// Read the worker's latest accumulated output from the spool.
///
/// Prefers the flat `output` file for backward compatibility.  When it is
/// empty or missing, falls back to reconstructing the output from the
/// structured `events.jsonl` log.
pub fn read_output(chat_id: &str) -> String {
    let flat = fs::read_to_string(output_path(chat_id)).unwrap_or_default();
    if !flat.is_empty() {
        return flat;
    }
    // Fall back to the event log.
    let events = crate::stream::read_events(&events_path(chat_id));
    if events.is_empty() {
        return String::new();
    }
    crate::stream::compute_output_from_events(&events)
}
/// Read new streaming events since a byte offset.
///
/// Returns `(new_events, new_offset)`.  The caller should persist the
/// returned offset and pass it in on the next poll cycle.
pub fn read_events_since(chat_id: &str, offset: u64) -> (Vec<crate::stream::StreamEvent>, u64) {
    crate::stream::read_events_since(&events_path(chat_id), offset)
}
/// Remove transient spool artifacts from a previous run while keeping the
/// current request intact.
pub fn clear_transient_spool_files(chat_id: &str) -> Result<()> {
    for path in [
        output_path(chat_id),
        result_path(chat_id),
        events_path(chat_id),
        git_verifier_context_path(chat_id),
        raw_adapter_events_path(chat_id),
        raw_provider_events_path(chat_id),
        transport_errors_path(chat_id),
    ] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed to remove stale {}", path.display()));
            }
        }
    }

    Ok(())
}
/// Atomically write text content inside the worker spool directory.
pub fn write_text_atomic(path: &Path, content: &str) -> io::Result<()> {
    write_bytes_atomic(path, content.as_bytes())
}
/// Atomically write JSON content inside the worker spool directory.
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    write_bytes_atomic(path, json.as_bytes())?;
    Ok(())
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
