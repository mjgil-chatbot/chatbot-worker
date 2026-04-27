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
