use std::collections::HashMap;

use agent_adapter_core::AgentEvent;
use ai_providers::summary::format_tool_input_preview;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const TOOL_COMMAND_PREVIEW_MAX_CHARS: usize = 57;
const TOOL_GREP_PREVIEW_MAX_CHARS: usize = 37;
const TOOL_RESULT_PREVIEW_MAX_CHARS: usize = 80;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallLiveUpdateKind {
    Bash,
    Semble,
    Fff,
    Read,
    Write,
    Edit,
    Glob,
    Grep,
    Mcp,
    Tool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallLiveUpdatePhase {
    Started,
    Finished,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCallLiveUpdate {
    #[serde(default = "tool_call_live_update_schema_version")]
    pub schema_version: u8,
    pub call_id: String,
    pub phase: ToolCallLiveUpdatePhase,
    pub kind: ToolCallLiveUpdateKind,
    pub raw_name: String,
    pub label: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub result_preview: Option<String>,
}

impl ToolCallLiveUpdate {
    pub fn transcript_detail(&self) -> String {
        match self
            .detail
            .as_deref()
            .filter(|detail| !detail.trim().is_empty())
        {
            Some(detail) => format!("{} {}", self.label, detail),
            None => self.label.clone(),
        }
    }
}

#[derive(Default)]
pub struct ToolCallLiveUpdateTracker {
    started: HashMap<String, ToolCallLiveUpdate>,
}

impl ToolCallLiveUpdateTracker {
    pub fn track(&mut self, event: &AgentEvent) -> Option<ToolCallLiveUpdate> {
        match event {
            AgentEvent::ToolCallStarted { id, name, input } => {
                let update = build_started_update(id, name, input);
                self.started.insert(id.clone(), update.clone());
                Some(update)
            }
            AgentEvent::ToolCallFinished { id, output, error } => {
                let mut update = self
                    .started
                    .remove(id)
                    .unwrap_or_else(|| build_unknown_update(id, id));
                update.phase = ToolCallLiveUpdatePhase::Finished;
                update.result_preview = output
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| error.as_deref().filter(|value| !value.trim().is_empty()))
                    .map(summarize_tool_result)
                    .filter(|value| !value.is_empty())
                    .map(|value| truncate_with_ellipsis(&value, TOOL_RESULT_PREVIEW_MAX_CHARS));
                Some(update)
            }
            _ => None,
        }
    }
}

fn tool_call_live_update_schema_version() -> u8 {
    1
}

fn build_started_update(call_id: &str, raw_name: &str, input: &Value) -> ToolCallLiveUpdate {
    let (kind, label, detail) = match raw_name {
        "Bash" | "bash" | "Shell" | "shell" | "mcp__bash__run" => input
            .get("command")
            .and_then(|value| value.as_str())
            .map(shell_tool_parts)
            .unwrap_or((ToolCallLiveUpdateKind::Bash, "Bash".to_string(), None)),
        "Read" | "read" | "Read File" | "View" | "read_file" => (
            ToolCallLiveUpdateKind::Read,
            "Read".to_string(),
            input
                .get("file_path")
                .or_else(|| input.get("path"))
                .and_then(|value| value.as_str())
                .map(file_name_detail),
        ),
        "Write" | "write" | "Write File" | "write_file" => (
            ToolCallLiveUpdateKind::Write,
            "Write".to_string(),
            input
                .get("file_path")
                .or_else(|| input.get("path"))
                .and_then(|value| value.as_str())
                .map(file_name_detail),
        ),
        "Edit" | "edit" | "str_replace" => (
            ToolCallLiveUpdateKind::Edit,
            "Edit".to_string(),
            input
                .get("file_path")
                .or_else(|| input.get("path"))
                .and_then(|value| value.as_str())
                .map(file_name_detail),
        ),
        "Glob" | "glob" => (
            ToolCallLiveUpdateKind::Glob,
            "Glob".to_string(),
            input
                .get("pattern")
                .and_then(|value| value.as_str())
                .map(|pattern| markdown_inline_code(pattern)),
        ),
        "Grep" | "grep" => (
            ToolCallLiveUpdateKind::Grep,
            "Grep".to_string(),
            input
                .get("pattern")
                .and_then(|value| value.as_str())
                .map(|pattern| {
                    markdown_inline_code(&truncate_with_ellipsis(
                        pattern,
                        TOOL_GREP_PREVIEW_MAX_CHARS,
                    ))
                }),
        ),
        _ if raw_name.starts_with("mcp__") => {
            let preview = markdownify_tool_preview(&format_tool_input_preview(raw_name, input));
            (
                ToolCallLiveUpdateKind::Mcp,
                mcp_tool_label(raw_name),
                (!preview.is_empty()).then_some(preview),
            )
        }
        _ => generic_tool_parts(raw_name, input),
    };

    ToolCallLiveUpdate {
        schema_version: tool_call_live_update_schema_version(),
        call_id: call_id.to_string(),
        phase: ToolCallLiveUpdatePhase::Started,
        kind,
        raw_name: raw_name.to_string(),
        label,
        detail,
        result_preview: None,
    }
}

fn build_unknown_update(call_id: &str, raw_name: &str) -> ToolCallLiveUpdate {
    ToolCallLiveUpdate {
        schema_version: tool_call_live_update_schema_version(),
        call_id: call_id.to_string(),
        phase: ToolCallLiveUpdatePhase::Started,
        kind: ToolCallLiveUpdateKind::Tool,
        raw_name: raw_name.to_string(),
        label: format_generic_tool_name(raw_name),
        detail: None,
        result_preview: None,
    }
}

fn generic_tool_parts(
    raw_name: &str,
    input: &Value,
) -> (ToolCallLiveUpdateKind, String, Option<String>) {
    if is_generic_tool_name(raw_name) {
        if let Some(command) = tool_command_input(input) {
            return shell_tool_parts(command);
        }

        if let Some(pattern) = input.get("pattern").and_then(|value| value.as_str()) {
            let has_path = input.get("path").is_some() || input.get("file_path").is_some();
            if has_path {
                return (
                    ToolCallLiveUpdateKind::Grep,
                    "Grep".to_string(),
                    Some(markdown_inline_code(&truncate_with_ellipsis(
                        pattern,
                        TOOL_GREP_PREVIEW_MAX_CHARS,
                    ))),
                );
            }
        }

        if let Some(file_parts) = generic_file_tool_parts(input) {
            return file_parts;
        }

        if let Some(pattern) = input.get("pattern").and_then(|value| value.as_str()) {
            return (
                ToolCallLiveUpdateKind::Glob,
                "Glob".to_string(),
                Some(markdown_inline_code(&truncate_with_ellipsis(
                    pattern,
                    TOOL_GREP_PREVIEW_MAX_CHARS,
                ))),
            );
        }
    }

    let preview = markdownify_tool_preview(&format_tool_input_preview(raw_name, input));
    (
        ToolCallLiveUpdateKind::Tool,
        if is_generic_tool_name(raw_name) {
            "Tool".to_string()
        } else {
            format_generic_tool_name(raw_name)
        },
        (!preview.is_empty()).then_some(preview),
    )
}

fn is_generic_tool_name(raw_name: &str) -> bool {
    raw_name.eq_ignore_ascii_case("tool")
}

fn tool_command_input(input: &Value) -> Option<&str> {
    input
        .get("command")
        .and_then(|value| value.as_str())
        .or_else(|| input.get("command_line").and_then(|value| value.as_str()))
        .or_else(|| input.get("cmd").and_then(|value| value.as_str()))
}

fn generic_file_tool_parts(
    input: &Value,
) -> Option<(ToolCallLiveUpdateKind, String, Option<String>)> {
    let path = input
        .get("file_path")
        .or_else(|| input.get("path"))
        .and_then(|value| value.as_str())?;

    let (kind, label) = if input.get("content").is_some()
        || input.get("contents").is_some()
        || input.get("text").is_some()
    {
        (ToolCallLiveUpdateKind::Write, "Write")
    } else if input.get("old").is_some()
        || input.get("new").is_some()
        || input.get("old_str").is_some()
        || input.get("new_str").is_some()
        || input.get("replacement").is_some()
        || input.get("diff").is_some()
    {
        (ToolCallLiveUpdateKind::Edit, "Edit")
    } else {
        (ToolCallLiveUpdateKind::Read, "Read")
    };

    Some((kind, label.to_string(), Some(file_name_detail(path))))
}

fn shell_tool_parts(command: &str) -> (ToolCallLiveUpdateKind, String, Option<String>) {
    let normalized = unwrap_shell_wrapper(command).unwrap_or(command);
    let first_line = normalized.lines().next().unwrap_or(normalized).trim();
    let (program, rest) = split_shell_command(first_line);

    if is_shell_program(program, "semble") {
        return (
            ToolCallLiveUpdateKind::Semble,
            "Semble".to_string(),
            (!rest.is_empty()).then(|| {
                markdown_inline_code(&truncate_with_ellipsis(
                    rest,
                    TOOL_COMMAND_PREVIEW_MAX_CHARS,
                ))
            }),
        );
    }

    if is_shell_program(program, "fff") {
        return (
            ToolCallLiveUpdateKind::Fff,
            "FFF".to_string(),
            (!rest.is_empty()).then(|| {
                markdown_inline_code(&truncate_with_ellipsis(
                    rest,
                    TOOL_COMMAND_PREVIEW_MAX_CHARS,
                ))
            }),
        );
    }

    (
        ToolCallLiveUpdateKind::Bash,
        "Bash".to_string(),
        Some(markdown_inline_code(&truncate_with_ellipsis(
            first_line,
            TOOL_COMMAND_PREVIEW_MAX_CHARS,
        ))),
    )
}

fn file_name_detail(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn mcp_tool_label(tool_name: &str) -> String {
    match tool_name
        .strip_prefix("mcp__")
        .and_then(|rest| rest.split_once("__"))
    {
        Some((server, tool)) => format!("MCP {}.{}", server, tool),
        None => tool_name.to_string(),
    }
}

fn markdownify_tool_preview(preview: &str) -> String {
    if preview.trim().is_empty() {
        return String::new();
    }
    preview
        .split(" | ")
        .map(markdownify_preview_segment)
        .collect::<Vec<_>>()
        .join(" | ")
}

fn markdownify_preview_segment(segment: &str) -> String {
    match segment.split_once('=') {
        Some((key, value)) if !value.trim().is_empty() => {
            format!("{key}={}", markdown_inline_code(value))
        }
        _ => segment.to_string(),
    }
}

fn markdown_inline_code(text: &str) -> String {
    format!("`{}`", text.replace('`', "'"))
}

fn unwrap_shell_wrapper(command: &str) -> Option<&str> {
    let trimmed = command.trim();
    for prefix in ["/bin/bash -lc ", "bash -lc ", "/bin/sh -lc ", "sh -lc "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return Some(rest.trim_matches('\'').trim_matches('"'));
        }
    }
    None
}

fn split_shell_command(command: &str) -> (&str, &str) {
    let trimmed = command.trim();
    match trimmed.find(char::is_whitespace) {
        Some(index) => (&trimmed[..index], trimmed[index..].trim_start()),
        None => (trimmed, ""),
    }
}

fn is_shell_program(program: &str, binary_name: &str) -> bool {
    program == binary_name || program.rsplit('/').next() == Some(binary_name)
}

fn summarize_tool_result(text: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        if let Some(summary) = summarize_tool_result_value(&value) {
            return summary;
        }
    }
    first_non_empty_line(text)
        .unwrap_or(text)
        .trim()
        .to_string()
}

fn summarize_tool_result_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(
            first_non_empty_line(text)
                .unwrap_or(text)
                .trim()
                .to_string(),
        ),
        Value::Array(items) => items.iter().find_map(summarize_tool_result_value),
        Value::Object(map) => {
            for key in [
                "stdout", "text", "content", "output", "result", "message", "stderr", "error",
            ] {
                if let Some(summary) = map
                    .get(key)
                    .and_then(summarize_tool_result_value)
                    .filter(|value| !value.is_empty())
                {
                    return Some(summary);
                }
            }
            serde_json::to_string(value).ok()
        }
        _ => Some(value.to_string()),
    }
}

fn first_non_empty_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

fn format_generic_tool_name(name: &str) -> String {
    let formatted = name
        .chars()
        .enumerate()
        .map(|(index, character)| {
            if character.is_uppercase() && index > 0 {
                format!(" {}", character)
            } else if character == '_' {
                " ".to_string()
            } else {
                character.to_string()
            }
        })
        .collect::<String>();
    formatted.trim().to_string()
}

fn truncate_with_ellipsis(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>()
        + "..."
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn shell_aliases_share_same_started_schema() {
        let mut tracker = ToolCallLiveUpdateTracker::default();
        let shell = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-1".to_string(),
                name: "Shell".to_string(),
                input: json!({"command": "pwd"}),
            })
            .expect("tool update");
        let bash = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-2".to_string(),
                name: "mcp__bash__run".to_string(),
                input: json!({"command": "pwd"}),
            })
            .expect("tool update");

        assert_eq!(shell.kind, ToolCallLiveUpdateKind::Bash);
        assert_eq!(shell.label, "Bash");
        assert_eq!(shell.detail.as_deref(), Some("`pwd`"));
        assert_eq!(bash.kind, ToolCallLiveUpdateKind::Bash);
        assert_eq!(bash.label, "Bash");
        assert_eq!(bash.detail.as_deref(), Some("`pwd`"));
    }

    #[test]
    fn absolute_semble_path_keeps_semble_schema() {
        let mut tracker = ToolCallLiveUpdateTracker::default();
        let shell = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-1".to_string(),
                name: "Shell".to_string(),
                input: json!({
                    "command": "/workspace/semble-rs/semble-rs-work/target/release/semble search \"query\" /tmp/project"
                }),
            })
            .expect("tool update");

        assert_eq!(shell.kind, ToolCallLiveUpdateKind::Semble);
        assert_eq!(shell.label, "Semble");
        assert_eq!(
            shell.detail.as_deref(),
            Some("`search \"query\" /tmp/project`")
        );
    }

    #[test]
    fn read_aliases_share_same_started_schema() {
        let mut tracker = ToolCallLiveUpdateTracker::default();
        let read = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-1".to_string(),
                name: "Read".to_string(),
                input: json!({"file_path": "/tmp/main.rs"}),
            })
            .expect("tool update");
        let view = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-2".to_string(),
                name: "read_file".to_string(),
                input: json!({"path": "/tmp/main.rs"}),
            })
            .expect("tool update");
        let read_file = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-3".to_string(),
                name: "Read File".to_string(),
                input: json!({"path": "/tmp/main.rs"}),
            })
            .expect("tool update");

        assert_eq!(read.kind, ToolCallLiveUpdateKind::Read);
        assert_eq!(read.label, "Read");
        assert_eq!(read.detail.as_deref(), Some("main.rs"));
        assert_eq!(view.kind, ToolCallLiveUpdateKind::Read);
        assert_eq!(view.label, "Read");
        assert_eq!(view.detail.as_deref(), Some("main.rs"));
        assert_eq!(read_file.kind, ToolCallLiveUpdateKind::Read);
        assert_eq!(read_file.label, "Read");
        assert_eq!(read_file.detail.as_deref(), Some("main.rs"));
    }

    #[test]
    fn write_aliases_share_same_started_schema() {
        let mut tracker = ToolCallLiveUpdateTracker::default();
        let write = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-1".to_string(),
                name: "Write".to_string(),
                input: json!({"file_path": "/tmp/main.rs"}),
            })
            .expect("tool update");
        let write_file = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-2".to_string(),
                name: "Write File".to_string(),
                input: json!({"path": "/tmp/main.rs"}),
            })
            .expect("tool update");

        assert_eq!(write.kind, ToolCallLiveUpdateKind::Write);
        assert_eq!(write.label, "Write");
        assert_eq!(write.detail.as_deref(), Some("main.rs"));
        assert_eq!(write_file.kind, ToolCallLiveUpdateKind::Write);
        assert_eq!(write_file.label, "Write");
        assert_eq!(write_file.detail.as_deref(), Some("main.rs"));
    }

    #[test]
    fn shell_without_command_uses_bash_fallback_label() {
        let mut tracker = ToolCallLiveUpdateTracker::default();
        let shell = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-1".to_string(),
                name: "Shell".to_string(),
                input: Value::Null,
            })
            .expect("tool update");

        assert_eq!(shell.kind, ToolCallLiveUpdateKind::Bash);
        assert_eq!(shell.label, "Bash");
        assert_eq!(shell.detail, None);
    }

    #[test]
    fn finished_update_reuses_started_detail_and_summarizes_stdout() {
        let mut tracker = ToolCallLiveUpdateTracker::default();
        tracker.track(&AgentEvent::ToolCallStarted {
            id: "tool-1".to_string(),
            name: "mcp__bash__run".to_string(),
            input: json!({"command": "pwd"}),
        });
        let finished = tracker
            .track(&AgentEvent::ToolCallFinished {
                id: "tool-1".to_string(),
                output: Some(
                    "{\"success\":true,\"stdout\":\"/tmp/project\\n\",\"stderr\":\"\"}".to_string(),
                ),
                error: None,
            })
            .expect("tool update");

        assert_eq!(finished.phase, ToolCallLiveUpdatePhase::Finished);
        assert_eq!(finished.label, "Bash");
        assert_eq!(finished.detail.as_deref(), Some("`pwd`"));
        assert_eq!(finished.result_preview.as_deref(), Some("/tmp/project"));
    }

    #[test]
    fn mcp_tool_uses_stable_schema() {
        let mut tracker = ToolCallLiveUpdateTracker::default();
        let started = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-1".to_string(),
                name: "mcp__lightweight_builds__run_build".to_string(),
                input: json!({
                    "project_path": "/tmp/project",
                    "command_override": ["cargo", "test", "--lib"]
                }),
            })
            .expect("tool update");

        assert_eq!(started.kind, ToolCallLiveUpdateKind::Mcp);
        assert_eq!(started.label, "MCP lightweight_builds.run_build");
        assert!(started
            .detail
            .as_deref()
            .expect("detail")
            .contains("project_path=`/tmp/project`"));
    }

    #[test]
    fn generic_tool_with_command_input_infers_shell_schema() {
        let mut tracker = ToolCallLiveUpdateTracker::default();
        let started = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-1".to_string(),
                name: "tool".to_string(),
                input: json!({"command": "/workspace/.local/bin/semble search \"bootstrap\" ."}),
            })
            .expect("tool update");

        assert_eq!(started.kind, ToolCallLiveUpdateKind::Semble);
        assert_eq!(started.label, "Semble");
        assert_eq!(started.detail.as_deref(), Some("`search \"bootstrap\" .`"));
    }

    #[test]
    fn generic_tool_with_file_path_infers_read_schema() {
        let mut tracker = ToolCallLiveUpdateTracker::default();
        let started = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-1".to_string(),
                name: "tool".to_string(),
                input: json!({"path": "/tmp/page_runtime.rs"}),
            })
            .expect("tool update");

        assert_eq!(started.kind, ToolCallLiveUpdateKind::Read);
        assert_eq!(started.label, "Read");
        assert_eq!(started.detail.as_deref(), Some("page_runtime.rs"));
    }

    #[test]
    fn generic_tool_with_content_and_path_infers_write_schema() {
        let mut tracker = ToolCallLiveUpdateTracker::default();
        let started = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-1".to_string(),
                name: "tool".to_string(),
                input: json!({"file_path": "/tmp/page_runtime.rs", "content": "patched"}),
            })
            .expect("tool update");

        assert_eq!(started.kind, ToolCallLiveUpdateKind::Write);
        assert_eq!(started.label, "Write");
        assert_eq!(started.detail.as_deref(), Some("page_runtime.rs"));
    }

    #[test]
    fn generic_tool_with_replacement_fields_infers_edit_schema() {
        let mut tracker = ToolCallLiveUpdateTracker::default();
        let started = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-1".to_string(),
                name: "tool".to_string(),
                input: json!({"path": "/tmp/page_runtime.rs", "old_str": "a", "new_str": "b"}),
            })
            .expect("tool update");

        assert_eq!(started.kind, ToolCallLiveUpdateKind::Edit);
        assert_eq!(started.label, "Edit");
        assert_eq!(started.detail.as_deref(), Some("page_runtime.rs"));
    }

    #[test]
    fn generic_tool_with_pattern_and_path_infers_grep_schema() {
        let mut tracker = ToolCallLiveUpdateTracker::default();
        let started = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-1".to_string(),
                name: "tool".to_string(),
                input: json!({"path": "/tmp/project", "pattern": "bootstrap_globals"}),
            })
            .expect("tool update");

        assert_eq!(started.kind, ToolCallLiveUpdateKind::Grep);
        assert_eq!(started.label, "Grep");
        assert_eq!(started.detail.as_deref(), Some("`bootstrap_globals`"));
    }

    #[test]
    fn generic_tool_with_pattern_only_infers_glob_schema() {
        let mut tracker = ToolCallLiveUpdateTracker::default();
        let started = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-1".to_string(),
                name: "tool".to_string(),
                input: json!({"pattern": "**/*.rs"}),
            })
            .expect("tool update");

        assert_eq!(started.kind, ToolCallLiveUpdateKind::Glob);
        assert_eq!(started.label, "Glob");
        assert_eq!(started.detail.as_deref(), Some("`**/*.rs`"));
    }

    #[test]
    fn generic_tool_with_command_line_field_infers_shell_schema() {
        let mut tracker = ToolCallLiveUpdateTracker::default();
        let started = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-1".to_string(),
                name: "tool".to_string(),
                input: json!({"command_line": "fff command dispatch src"}),
            })
            .expect("tool update");

        assert_eq!(started.kind, ToolCallLiveUpdateKind::Fff);
        assert_eq!(started.label, "FFF");
        assert_eq!(started.detail.as_deref(), Some("`command dispatch src`"));
    }

    #[test]
    fn generic_tool_without_known_shape_keeps_preview_detail() {
        let mut tracker = ToolCallLiveUpdateTracker::default();
        let started = tracker
            .track(&AgentEvent::ToolCallStarted {
                id: "tool-1".to_string(),
                name: "tool".to_string(),
                input: json!({"query": "imports", "workspace_root": "/tmp/project"}),
            })
            .expect("tool update");

        assert_eq!(started.kind, ToolCallLiveUpdateKind::Tool);
        assert_eq!(started.label, "Tool");
        assert!(started
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("query=`imports`")));
    }
}
