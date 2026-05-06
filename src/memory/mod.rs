//! MemMachine-backed memory foundations.
//!
//! This module intentionally contains only Phase 1 scaffolding from
//! `docs/architecture/memmachine-integration.md`: workspace-scoped paths,
//! request payload types, ingest checkpoints, and small helpers for the
//! workspace-scoped memory setup.

use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use fs2::FileExt;
use rust_debug::tracing::warn;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use crate::data::ChatMessage;
mod agentic;
mod backend;
mod retrieval_filters;
use self::retrieval_filters::sanitize_memory_text;
use self::retrieval_filters::sanitize_retrieved_memory_text;
pub use self::backend::MemMachineStatus;
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0001_0000_01b3;
pub const MEMORY_DIR_NAME: &str = "memory";
pub const INGEST_STATE_FILE_NAME: &str = "ingest-state.json";
pub const MANUAL_MEMORIES_FILE_NAME: &str = "manual-memories.json";
pub const AGENT_MEMORY_STORE_FILE_NAME: &str = "agent-memory-store.json";
pub const WRITER_LOCK_FILE_NAME: &str = "writer.lock";
pub const DEFAULT_RETRIEVAL_LIMIT: usize = 6;
const WRITER_LOCK_RETRY_ATTEMPTS: usize = 6;
const WRITER_LOCK_RETRY_BASE_MS: u64 = 50;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMemoryPaths {
    pub workspace_id: String,
    pub workspace_root: PathBuf,
    pub root_dir: PathBuf,
    pub ingest_state_path: PathBuf,
    pub manual_memories_path: PathBuf,
    pub agent_memory_store_path: PathBuf,
    pub writer_lock_path: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryContext {
    pub workspace_id: String,
    pub query: String,
    pub hits: Vec<MemoryHit>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryHit {
    pub doc_key: String,
    pub chat_id: String,
    pub role: String,
    pub session_name: String,
    pub timestamp: DateTime<Utc>,
    pub snippet: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
#[serde(rename_all = "snake_case")]
pub enum ManualMemoryPriority {
    Normal,
    #[default]
    High,
}
impl ManualMemoryPriority {
    fn boost(self) -> f32 {
        match self {
            Self::High => 1_000.0,
            Self::Normal => 0.0,
        }
    }}
impl ManualMemoryPriority {
    pub fn label(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Normal => "normal",
        }
    }}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManualMemoryFact {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub priority: ManualMemoryPriority,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManualMemoriesState {
    pub version: u32,
    #[serde(default)]
    pub memories: BTreeMap<String, ManualMemoryFact>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionIngestState {
    pub indexed_message_count: usize,
    pub history_fingerprint: String,
    pub updated_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryInvalidation {
    pub invalidated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IngestState {
    pub version: u32,
    #[serde(default)]
    pub sessions: BTreeMap<String, SessionIngestState>,
    #[serde(default)]
    pub invalidated_doc_keys: BTreeMap<String, MemoryInvalidation>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryIndexSummary {
    pub indexed_sessions: usize,
    pub indexed_messages: usize,
    pub rebuilt: bool,
}
impl Default for IngestState {
    fn default() -> Self {
        Self {
            version: 1,
            sessions: BTreeMap::new(),
            invalidated_doc_keys: BTreeMap::new(),
        }
    }
}
impl Default for ManualMemoriesState {
    fn default() -> Self {
        Self {
            version: 1,
            memories: BTreeMap::new(),
        }
    }
}
fn canonical_workspace_path(workspace_path: impl AsRef<Path>) -> Result<PathBuf> {
    workspace_path.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to canonicalize {}",
            workspace_path.as_ref().display()
        )
    })
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    crate::storage::atomic::atomic_write(path, bytes).map(|_| ())
}
fn fnv_hash_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}
fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(timestamp) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(timestamp.with_timezone(&Utc));
    }

    trimmed
        .parse::<i64>()
        .ok()
        .and_then(|seconds| Utc.timestamp_opt(seconds, 0).single())
}
fn default_memory_timestamp() -> DateTime<Utc> {
    Utc.timestamp_opt(0, 0)
        .single()
        .expect("unix epoch timestamp should be valid")
}
fn dedupe_memory_hits(hits: &mut Vec<MemoryHit>) {
    let mut seen = HashSet::new();
    hits.retain(|hit| seen.insert(hit.doc_key.clone()));
}
fn score_manual_memory(
    memory: &ManualMemoryFact,
    normalized_query: &str,
    query_terms: &[String],
) -> Option<MemoryHit> {
    let normalized_text = memory.text.to_lowercase();
    let mut score = memory.priority.boost();
    let full_query_match = normalized_text.contains(normalized_query);
    if full_query_match {
        score += 100.0;
    }

    let term_matches = query_terms
        .iter()
        .filter(|term| normalized_text.contains(term.as_str()))
        .count();
    if !full_query_match && term_matches == 0 {
        return None;
    }
    score += term_matches as f32;

    Some(MemoryHit {
        doc_key: format!("manual:{}", memory.id),
        chat_id: "manual".to_string(),
        role: "system".to_string(),
        session_name: "Pinned Memory".to_string(),
        timestamp: memory.updated_at,
        snippet: memory.text.clone(),
        title: Some(format!("Pinned Memory ({})", memory.priority.label())),
        message_index: None,
        turn_index: None,
        score: Some(score),
    })
}
fn normalize_invalidation_reason(reason: Option<String>) -> Option<String> {
    reason.and_then(|reason| {
        let sanitized = sanitize_memory_text(&reason);
        if sanitized.is_empty() {
            None
        } else {
            Some(sanitized)
        }
    })
}
fn acquire_writer_lock(paths: &WorkspaceMemoryPaths) -> Result<WriterLock> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&paths.writer_lock_path)
        .with_context(|| format!("failed to open {}", paths.writer_lock_path.display()))?;

    for attempt in 0..WRITER_LOCK_RETRY_ATTEMPTS {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(WriterLock(file)),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if attempt + 1 == WRITER_LOCK_RETRY_ATTEMPTS {
                    return Err(err).with_context(|| {
                        format!("timed out acquiring {}", paths.writer_lock_path.display())
                    });
                }
                let backoff_ms = WRITER_LOCK_RETRY_BASE_MS * (1_u64 << attempt);
                std::thread::sleep(Duration::from_millis(backoff_ms));
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("failed to lock {}", paths.writer_lock_path.display())
                });
            }
        }
    }

    unreachable!("writer lock loop should return before exhaustion");
}
fn indexed_message_content(message: &ChatMessage) -> String {
    if message.is_user {
        return message.content.clone();
    }

    assistant_memory_body(&message.content).unwrap_or_else(|| message.content.clone())
}
fn assistant_memory_body(content: &str) -> Option<String> {
    let lines: Vec<&str> = content.split('\n').collect();
    let transcript_line_count = lines
        .iter()
        .take_while(|line| {
            let trimmed = line.trim_start();
            !trimmed.is_empty() && is_standard_transcript_line_for_memory(trimmed)
        })
        .count();

    if transcript_line_count == 0 || transcript_line_count >= lines.len() {
        return None;
    }

    let body = lines[transcript_line_count..]
        .join("\n")
        .trim_start_matches(['\r', '\n'])
        .to_string();
    (!body.trim().is_empty()).then_some(body)
}
fn is_standard_transcript_line_for_memory(line: &str) -> bool {
    let stripped = strip_standard_transcript_timestamp_prefix(line);
    if stripped.len() != line.len() {
        return !stripped.is_empty();
    }
    if let Some(model) = stripped.strip_suffix(" agent started") {
        return looks_like_standard_model_label(model.trim_end());
    }
    if let Some((model, _detail)) = stripped.split_once(" | ") {
        return looks_like_standard_model_label(model.trim());
    }
    false
}
fn strip_standard_transcript_timestamp_prefix(line: &str) -> &str {
    let bytes = line.as_bytes();
    if bytes.len() < 16 || bytes[0] != b'[' {
        return line;
    }
    if !bytes[1].is_ascii_alphabetic()
        || !bytes[2].is_ascii_alphabetic()
        || !bytes[3].is_ascii_alphabetic()
    {
        return line;
    }
    if bytes[4] != b' ' {
        return line;
    }
    let mut index = 5usize;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == 5 || index + 9 > bytes.len() || bytes[index] != b' ' {
        return line;
    }
    if !bytes[index + 1].is_ascii_digit()
        || !bytes[index + 2].is_ascii_digit()
        || bytes[index + 3] != b':'
        || !bytes[index + 4].is_ascii_digit()
        || !bytes[index + 5].is_ascii_digit()
        || !bytes[index + 6].is_ascii_lowercase()
        || !bytes[index + 7].is_ascii_lowercase()
    {
        return line;
    }
    if bytes[index + 8] != b']' || index + 9 >= bytes.len() || bytes[index + 9] != b' ' {
        return line;
    }
    &line[index + 10..]
}
fn looks_like_standard_model_label(label: &str) -> bool {
    if label.is_empty()
        || label.len() > 24
        || !label
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '.' | '-' | '+'))
    {
        return false;
    }

    let lower = label.to_ascii_lowercase();
    label.chars().any(|ch| ch.is_ascii_digit())
        || [
            "gpt", "gemini", "sonnet", "haiku", "opus", "claude", "minimax", "kimi", "codex",
            "qwen", "deepseek", "mistral", "llama",
        ]
        .iter()
        .any(|token| lower.contains(token))
}
struct WriterLock(std::fs::File);
impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
