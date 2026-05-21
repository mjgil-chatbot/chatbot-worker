use anyhow::Result;
use chrono::{DateTime, Utc};
use fs2::FileExt;
use rust_debug::tracing::{debug, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::model::ModelProvider;
use crate::summary::{JsonSummary, TurnUsage};

mod manager;
mod provider_normalization;
#[cfg(test)]
mod tests;

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0001_0000_01b3;
pub const SESSION_HEIGHT_CACHE_VERSION: u32 = 2;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SessionHeightCache {
    pub session_id: String,
    pub version: u32,
    pub widths: HashMap<u32, HeightCacheEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct HeightCacheEntry {
    pub row_heights: Vec<f32>,
    pub total_height: f32,
    pub message_count: usize,
    pub updated_at: DateTime<Utc>,
}

impl SessionHeightCache {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            version: SESSION_HEIGHT_CACHE_VERSION,
            widths: HashMap::new(),
        }
    }
}

pub fn height_cache_width_key(width: f32) -> u32 {
    width.round().max(0.0) as u32
}

pub fn resolve_height_cache_entry(
    cache: &SessionHeightCache,
    width_key: u32,
) -> Option<&HeightCacheEntry> {
    const WIDTH_TOLERANCE: u32 = 24;
    if cache.version < SESSION_HEIGHT_CACHE_VERSION {
        return None;
    }

    if let Some(entry) = cache.widths.get(&width_key) {
        return Some(entry);
    }

    cache
        .widths
        .iter()
        .filter(|(cached_width, _)| cached_width.abs_diff(width_key) <= WIDTH_TOLERANCE)
        .min_by_key(|(cached_width, _)| cached_width.abs_diff(width_key))
        .map(|(_, entry)| entry)
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub content: String,
    pub is_user: bool,
    pub timestamp: DateTime<Utc>,
    pub command: Option<String>, // The CLI command that was executed
    #[serde(default)]
    pub retrieved_memory: Option<String>, // Retrieved memory preview shown alongside the turn
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatSession {
    pub id: String,
    pub name: String,
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub json_summary: Option<JsonSummary>,
    pub claude_session_id: Option<String>, // Claude CLI session ID for interactive mode
    pub minimax_session_id: Option<String>, // MiniMax Claude-compatible session ID
    pub qwen_session_id: Option<String>,   // Qwen OpenRouter session ID
    pub codex_session_id: Option<String>,  // Codex CLI session ID for resume mode
    #[serde(default, alias = "nemotron_session_id")]
    pub nvidia_session_id: Option<String>, // Nvidia (Nemotron) Codex-compatible session ID
    pub gemini_session_id: Option<String>, // Gemini CLI session ID for resume mode
    #[serde(default)]
    pub openrouter_session_id: Option<String>, // OpenRouter session ID for resume mode
    #[serde(default)]
    pub omniforge_session_id: Option<String>, // OmniForge compatibility session ID
    #[serde(default)]
    pub omniforge_model_id: Option<String>, // Selected OmniForge model catalog id
    #[serde(default)]
    pub kimi_session_id: Option<String>, // Kimi CLI session ID for resume mode
    #[serde(default)]
    pub opencode_session_id: Option<String>, // OpenCode CLI session ID (not used, but present for completeness)
    #[serde(default)]
    pub hermes_session_id: Option<String>, // Hermes Agent session ID for resume mode
    #[serde(default)]
    pub forge_session_id: Option<String>, // Forge session ID (not used, but present for completeness)
    #[serde(default)]
    pub claw_session_id: Option<String>, // Claw session ID (not used, but present for completeness)
    #[serde(default)]
    pub launched_from: Option<String>, // Directory from which the desktop was launched
    #[serde(default)]
    pub runsc_dirs: Vec<String>, // Directories to mount into the runsc container
    #[serde(default)]
    pub runsc_image: Option<String>, // Optional per-session runsc image override
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub messages: Vec<ChatMessage>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct SessionMetadata {
    pub sessions: HashMap<String, SessionInfo>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: usize,
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Clone, Debug)]
pub struct SessionLogSnapshot {
    pub total_messages: usize,
    pub user_messages: usize,
    pub assistant_messages: usize,
    pub first_message_at: Option<DateTime<Utc>>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub history_fingerprint: u64,
}

pub fn session_log_snapshot(session: &ChatSession) -> SessionLogSnapshot {
    let user_messages = session.messages.iter().filter(|m| m.is_user).count();
    SessionLogSnapshot {
        total_messages: session.messages.len(),
        user_messages,
        assistant_messages: session.messages.len().saturating_sub(user_messages),
        first_message_at: session.messages.first().map(|message| message.timestamp),
        last_message_at: session.messages.last().map(|message| message.timestamp),
        history_fingerprint: session.history_fingerprint(),
    }
}

#[derive(Clone)]
pub struct DataManager {
    data_dir: PathBuf,
    provider: ModelProvider,
}

struct MetadataLock(std::fs::File);

impl Drop for MetadataLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

impl ChatSession {
    pub fn add_message(&mut self, content: String, is_user: bool) {
        self.messages.push(ChatMessage {
            content,
            is_user,
            timestamp: Utc::now(),
            command: None,
            retrieved_memory: None,
        });
        self.updated_at = Utc::now();
    }

    pub fn add_message_with_command(
        &mut self,
        content: String,
        is_user: bool,
        command: Option<String>,
    ) {
        self.messages.push(ChatMessage {
            content,
            is_user,
            timestamp: Utc::now(),
            command,
            retrieved_memory: None,
        });
        self.updated_at = Utc::now();
    }

    /// Update the last assistant message content, or add a new assistant
    /// message if the last message is from the user (or there are no messages).
    pub fn update_or_add_assistant_message(&mut self, content: String) {
        if let Some(last) = self.messages.last_mut() {
            if !last.is_user {
                last.content = content;
                self.updated_at = Utc::now();
                return;
            }
        }
        self.add_message(content, false);
    }

    pub fn update_user_message_request_metadata(
        &mut self,
        message_timestamp_ms: i64,
        command: Option<String>,
        retrieved_memory: Option<String>,
    ) -> Result<()> {
        let Some(message) = self.messages.iter_mut().find(|message| {
            message.is_user && message.timestamp.timestamp_millis() == message_timestamp_ms
        }) else {
            anyhow::bail!("pending user message disappeared before request metadata update");
        };
        message.command = command;
        message.retrieved_memory = retrieved_memory;
        self.updated_at = Utc::now();
        Ok(())
    }

    pub fn set_system_prompt(&mut self, prompt: Option<String>) {
        self.system_prompt = prompt;
        self.updated_at = Utc::now();
    }

    pub fn history_fingerprint(&self) -> u64 {
        let mut hash = FNV_OFFSET_BASIS;
        hash = fnv_hash_bytes(hash, self.id.as_bytes());
        hash = fnv_hash_bytes(hash, self.name.as_bytes());
        hash = fnv_hash_bytes(hash, self.created_at.timestamp().to_le_bytes().as_ref());
        hash = fnv_hash_bytes(
            hash,
            self.created_at
                .timestamp_subsec_nanos()
                .to_le_bytes()
                .as_ref(),
        );
        hash = fnv_hash_bytes(hash, self.updated_at.timestamp().to_le_bytes().as_ref());
        hash = fnv_hash_bytes(
            hash,
            self.updated_at
                .timestamp_subsec_nanos()
                .to_le_bytes()
                .as_ref(),
        );

        if let Some(prompt) = self.system_prompt.as_ref() {
            hash = fnv_hash_bytes(hash, prompt.as_bytes());
        }

        for message in &self.messages {
            hash = fnv_hash_bytes(hash, &[u8::from(message.is_user)]);
            hash = fnv_hash_bytes(hash, message.timestamp.timestamp().to_le_bytes().as_ref());
            hash = fnv_hash_bytes(
                hash,
                message
                    .timestamp
                    .timestamp_subsec_nanos()
                    .to_le_bytes()
                    .as_ref(),
            );
            hash = fnv_hash_bytes(hash, message.content.as_bytes());
            if let Some(command) = message.command.as_ref() {
                hash = fnv_hash_bytes(hash, command.as_bytes());
            }
            if let Some(retrieved_memory) = message.retrieved_memory.as_ref() {
                hash = fnv_hash_bytes(hash, retrieved_memory.as_bytes());
            }
            hash = fnv_hash_bytes(hash, &[0xff]);
        }

        hash
    }
}
fn fnv_hash_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod inline_tests;
