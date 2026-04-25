//! Structured streaming events for worker execution.
//!
//! Provides a JSONL event log that workers append to during execution.
//! The existing flat `output` file is retained as a backward-compatible
//! computed view derived from `TextDelta` events.

use anyhow::{Context, Result};
use chrono::{Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use crate::adapter_tool_call_schema::ToolCallLiveUpdate;
/// A structured event emitted during worker execution.
///
/// Workers append these as newline-delimited JSON to `events.jsonl`.
/// The UI polls the file by tracking the last-read byte offset.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum StreamEvent {
    TextDelta {
        content: String,
        #[serde(default)]
        timestamp_ms: i64,
    },
    LiveSummary {
        content: String,
        #[serde(default)]
        timestamp_ms: i64,
    },
    ToolStart {
        tool: String,
        #[serde(default)]
        input: Value,
        #[serde(default)]
        live_update: Option<ToolCallLiveUpdate>,
        #[serde(default)]
        timestamp_ms: i64,
    },
    ToolEnd {
        tool: String,
        #[serde(default)]
        output: Value,
        #[serde(default)]
        live_update: Option<ToolCallLiveUpdate>,
        #[serde(default)]
        timestamp_ms: i64,
    },
    ThinkingStart {
        #[serde(default)]
        timestamp_ms: i64,
    },
    ThinkingEnd {
        #[serde(default)]
        timestamp_ms: i64,
    },
    CostUpdate {
        input_tokens: u64,
        output_tokens: u64,
        #[serde(default)]
        timestamp_ms: i64,
    },
    Error {
        message: String,
        #[serde(default)]
        timestamp_ms: i64,
    },
}
