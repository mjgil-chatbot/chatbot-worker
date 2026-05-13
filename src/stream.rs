//! Structured streaming events for worker execution.
//!
//! Provides a JSONL event log that workers append to during execution.
//! The existing flat `output` file is retained as a backward-compatible
//! computed view derived from `TextDelta` events.

pub use ai_providers::stream::*;

use anyhow::{Context, Result};
use chrono::{Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use crate::adapter_tool_call_schema::ToolCallLiveUpdate;

const MAX_SERIALIZED_EVENT_BYTES: usize = 96 * 1024;
const MAX_TEXT_EVENT_CHARS: usize = 24 * 1024;
const MAX_SUMMARY_EVENT_CHARS: usize = 4 * 1024;
const MAX_ERROR_EVENT_CHARS: usize = 8 * 1024;
const MAX_TOOL_NAME_CHARS: usize = 256;
const MAX_TOOL_VALUE_BYTES: usize = 32 * 1024;
const MAX_TOOL_VALUE_PREVIEW_CHARS: usize = 2 * 1024;

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

impl StreamEvent {
    /// Create a `TextDelta` event with the current timestamp.
    pub fn text_delta(content: impl Into<String>) -> Self {
        Self::TextDelta {
            content: content.into(),
            timestamp_ms: Utc::now().timestamp_millis(),
        }
    }

    /// Create a `LiveSummary` event with the current timestamp.
    pub fn live_summary(content: impl Into<String>) -> Self {
        Self::LiveSummary {
            content: content.into(),
            timestamp_ms: Utc::now().timestamp_millis(),
        }
    }

    /// Create a `ToolStart` event with the current timestamp.
    pub fn tool_start(tool: impl Into<String>, input: Value) -> Self {
        Self::ToolStart {
            tool: tool.into(),
            input,
            live_update: None,
            timestamp_ms: Utc::now().timestamp_millis(),
        }
    }

    /// Create a `ToolEnd` event with the current timestamp.
    pub fn tool_end(tool: impl Into<String>, output: Value) -> Self {
        Self::ToolEnd {
            tool: tool.into(),
            output,
            live_update: None,
            timestamp_ms: Utc::now().timestamp_millis(),
        }
    }

    /// Create an `Error` event with the current timestamp.
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
            timestamp_ms: Utc::now().timestamp_millis(),
        }
    }

    /// Create a `CostUpdate` event with the current timestamp.
    pub fn cost_update(input_tokens: u64, output_tokens: u64) -> Self {
        Self::CostUpdate {
            input_tokens,
            output_tokens,
            timestamp_ms: Utc::now().timestamp_millis(),
        }
    }
}

// ── JSONL persistence ──────────────────────────────────────────────

/// Append a single event as a JSONL line to the given file path.
///
/// Creates the parent directory and file if they do not exist.
pub fn append_event(path: &Path, event: &StreamEvent) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open event log {}", path.display()))?;
    let sanitized = sanitize_event_for_spool(event);
    let bounded = bound_event_size(&sanitized);
    let mut line = serde_json::to_string(&bounded)?;
    line.push('\n');
    file.write_all(line.as_bytes())
        .with_context(|| format!("failed to append to event log {}", path.display()))?;
    Ok(())
}

fn sanitize_event_for_spool(event: &StreamEvent) -> StreamEvent {
    match event {
        StreamEvent::TextDelta {
            content,
            timestamp_ms,
        } => StreamEvent::TextDelta {
            content: content.clone(),
            timestamp_ms: *timestamp_ms,
        },
        StreamEvent::LiveSummary {
            content,
            timestamp_ms,
        } => StreamEvent::LiveSummary {
            content: truncate_string(content, MAX_SUMMARY_EVENT_CHARS),
            timestamp_ms: *timestamp_ms,
        },
        StreamEvent::ToolStart {
            tool,
            input,
            live_update,
            timestamp_ms,
        } => StreamEvent::ToolStart {
            tool: truncate_string(tool, MAX_TOOL_NAME_CHARS),
            input: truncate_value(input),
            live_update: live_update.clone(),
            timestamp_ms: *timestamp_ms,
        },
        StreamEvent::ToolEnd {
            tool,
            output,
            live_update,
            timestamp_ms,
        } => StreamEvent::ToolEnd {
            tool: truncate_string(tool, MAX_TOOL_NAME_CHARS),
            output: truncate_value(output),
            live_update: live_update.clone(),
            timestamp_ms: *timestamp_ms,
        },
        StreamEvent::ThinkingStart { timestamp_ms } => StreamEvent::ThinkingStart {
            timestamp_ms: *timestamp_ms,
        },
        StreamEvent::ThinkingEnd { timestamp_ms } => StreamEvent::ThinkingEnd {
            timestamp_ms: *timestamp_ms,
        },
        StreamEvent::CostUpdate {
            input_tokens,
            output_tokens,
            timestamp_ms,
        } => StreamEvent::CostUpdate {
            input_tokens: *input_tokens,
            output_tokens: *output_tokens,
            timestamp_ms: *timestamp_ms,
        },
        StreamEvent::Error {
            message,
            timestamp_ms,
        } => StreamEvent::Error {
            message: truncate_string(message, MAX_ERROR_EVENT_CHARS),
            timestamp_ms: *timestamp_ms,
        },
    }
}

fn bound_event_size(event: &StreamEvent) -> StreamEvent {
    match serde_json::to_vec(event) {
        Ok(bytes) if bytes.len() <= MAX_SERIALIZED_EVENT_BYTES => event.clone(),
        Ok(bytes) => minimal_event(event, bytes.len()),
        Err(_) => minimal_event(event, 0),
    }
}

fn minimal_event(event: &StreamEvent, serialized_bytes: usize) -> StreamEvent {
    match event {
        StreamEvent::TextDelta {
            content,
            timestamp_ms,
        } => StreamEvent::TextDelta {
            content: truncate_string(content, MAX_TEXT_EVENT_CHARS),
            timestamp_ms: *timestamp_ms,
        },
        StreamEvent::LiveSummary {
            content,
            timestamp_ms,
        } => StreamEvent::LiveSummary {
            content: truncate_string(content, MAX_SUMMARY_EVENT_CHARS),
            timestamp_ms: *timestamp_ms,
        },
        StreamEvent::ToolStart {
            tool,
            live_update,
            timestamp_ms,
            ..
        } => StreamEvent::ToolStart {
            tool: truncate_string(tool, MAX_TOOL_NAME_CHARS),
            input: truncated_value_stub(serialized_bytes),
            live_update: live_update.clone(),
            timestamp_ms: *timestamp_ms,
        },
        StreamEvent::ToolEnd {
            tool,
            live_update,
            timestamp_ms,
            ..
        } => StreamEvent::ToolEnd {
            tool: truncate_string(tool, MAX_TOOL_NAME_CHARS),
            output: truncated_value_stub(serialized_bytes),
            live_update: live_update.clone(),
            timestamp_ms: *timestamp_ms,
        },
        StreamEvent::ThinkingStart { timestamp_ms } => StreamEvent::ThinkingStart {
            timestamp_ms: *timestamp_ms,
        },
        StreamEvent::ThinkingEnd { timestamp_ms } => StreamEvent::ThinkingEnd {
            timestamp_ms: *timestamp_ms,
        },
        StreamEvent::CostUpdate {
            input_tokens,
            output_tokens,
            timestamp_ms,
        } => StreamEvent::CostUpdate {
            input_tokens: *input_tokens,
            output_tokens: *output_tokens,
            timestamp_ms: *timestamp_ms,
        },
        StreamEvent::Error {
            message,
            timestamp_ms,
        } => StreamEvent::Error {
            message: truncate_string(message, MAX_ERROR_EVENT_CHARS),
            timestamp_ms: *timestamp_ms,
        },
    }
}

fn truncate_value(value: &Value) -> Value {
    let serialized = match serde_json::to_string(value) {
        Ok(serialized) => serialized,
        Err(err) => {
            return serde_json::json!({
                "truncated": true,
                "preview": format!("failed to serialize tool payload: {err}"),
                "original_bytes": 0,
            });
        }
    };

    if serialized.len() <= MAX_TOOL_VALUE_BYTES {
        return value.clone();
    }

    serde_json::json!({
        "truncated": true,
        "preview": truncate_string(&serialized, MAX_TOOL_VALUE_PREVIEW_CHARS),
        "original_bytes": serialized.len(),
    })
}

fn truncated_value_stub(serialized_bytes: usize) -> Value {
    serde_json::json!({
        "truncated": true,
        "preview": "tool payload omitted from spool",
        "original_bytes": serialized_bytes,
    })
}

fn truncate_string(value: &str, max_chars: usize) -> String {
    let total_chars = value.chars().count();
    if total_chars <= max_chars {
        return value.to_string();
    }

    let omitted = total_chars.saturating_sub(max_chars);
    let suffix = format!("... [truncated {omitted} chars]");
    let suffix_chars = suffix.chars().count();
    let keep_chars = max_chars.saturating_sub(suffix_chars).max(1);
    let cutoff = value
        .char_indices()
        .nth(keep_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(value.len());
    let mut truncated = value[..cutoff].to_string();
    truncated.push_str(&suffix);
    truncated
}

/// Parse all events from a JSONL file.  Returns an empty vec if the file
/// does not exist.  Malformed lines are skipped with a warning.
pub fn read_events(path: &Path) -> Vec<StreamEvent> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            rust_debug::tracing::warn!(
                path=%path.display(),
                error=%e,
                "failed to open event log"
            );
            return Vec::new();
        }
    };
    let reader = io::BufReader::new(file);
    let mut events = Vec::new();
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<StreamEvent>(&line) {
            Ok(event) => events.push(event),
            Err(e) => {
                rust_debug::tracing::warn!(
                    line=%line,
                    error=%e,
                    "skipping malformed event log line"
                );
            }
        }
    }
    events
}

/// Read events starting from a byte offset.  Returns the new events and
/// the updated byte offset for the next call.
pub fn read_events_since(path: &Path, offset: u64) -> (Vec<StreamEvent>, u64) {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return (Vec::new(), offset),
    };
    let metadata = match file.metadata() {
        Ok(m) => m,
        Err(_) => return (Vec::new(), offset),
    };
    let file_len = metadata.len();
    if file_len <= offset {
        return (Vec::new(), offset);
    }

    use std::io::{Seek, SeekFrom};
    let mut reader = io::BufReader::new(file);
    if offset > 0 {
        if reader.seek(SeekFrom::Start(offset)).is_err() {
            return (Vec::new(), offset);
        }
    }

    let mut events = Vec::new();
    let mut bytes_read = offset;
    let mut line_buf = String::new();
    loop {
        line_buf.clear();
        match reader.read_line(&mut line_buf) {
            Ok(0) => break,
            Ok(n) => {
                bytes_read += n as u64;
                let trimmed = line_buf.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(event) = serde_json::from_str::<StreamEvent>(trimmed) {
                    events.push(event);
                }
            }
            Err(_) => break,
        }
    }
    (events, bytes_read)
}

/// Append flat text output for a sequence of events onto an existing transcript.
///
/// This reconstructs the backward-compatible `output` file content by
/// formatting live transcript rows from `TextDelta` and `LiveSummary` events.
pub fn append_output_from_events(transcript: &mut String, events: &[StreamEvent]) {
    for event in events {
        match event {
            StreamEvent::TextDelta {
                content,
                timestamp_ms,
            } => append_timestamped_transcript_lines(transcript, *timestamp_ms, content),
            StreamEvent::LiveSummary {
                content,
                timestamp_ms,
            } => {
                if content.is_empty() {
                    continue;
                }
                let formatted = format_live_summary_line(*timestamp_ms, content);
                if transcript
                    .lines()
                    .last()
                    .is_some_and(|existing| existing == formatted)
                {
                    continue;
                }
                push_transcript_line(transcript, &formatted);
            }
            _ => {}
        }
    }
}

/// Compute the flat text output from a sequence of events.
pub fn compute_output_from_events(events: &[StreamEvent]) -> String {
    let mut transcript = String::new();
    append_output_from_events(&mut transcript, events);
    transcript
}

fn format_live_summary_line(timestamp_ms: i64, content: &str) -> String {
    format_timestamped_transcript_line(timestamp_ms, content)
}

fn format_timestamped_transcript_line(timestamp_ms: i64, content: &str) -> String {
    if timestamp_ms <= 0 {
        return content.to_string();
    }
    match Local.timestamp_millis_opt(timestamp_ms).single() {
        Some(timestamp) => format!("[{}] {}", timestamp.format("%b %-d %I:%M%P"), content),
        None => content.to_string(),
    }
}

fn append_timestamped_transcript_lines(output: &mut String, timestamp_ms: i64, content: &str) {
    for line in content.split('\n').filter(|line| !line.is_empty()) {
        push_transcript_line(
            output,
            &format_timestamped_transcript_line(timestamp_ms, line),
        );
    }
}

fn push_transcript_line(output: &mut String, line: &str) {
    if !output.is_empty() {
        output.push('\n');
    }
    output.push_str(line);
}

/// Path to the event log file for a worker spool directory.
pub fn events_path(worker_dir: &Path) -> PathBuf {
    worker_dir.join("events.jsonl")
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
