use agent_adapter_core::types::LogLevel;
use agent_adapter_core::AgentEvent;
use serde_json::Value;

use crate::adapter_tool_call_schema::ToolCallLiveUpdate;
use crate::config;
use crate::model::ModelProvider;

#[cfg(test)]
use crate::adapter_tool_call_schema::ToolCallLiveUpdateTracker;

#[derive(Clone, Debug)]
pub struct LiveSummaryContext {
    provider: Option<ModelProvider>,
    configured_model: Option<String>,
}

impl LiveSummaryContext {
    pub fn new(provider: Option<ModelProvider>, configured_model: Option<String>) -> Self {
        Self {
            provider,
            configured_model,
        }
    }

    pub fn line_for_agent_event(
        &self,
        event: &AgentEvent,
        live_update: Option<&ToolCallLiveUpdate>,
    ) -> Option<String> {
        match event {
            AgentEvent::ToolCallStarted { .. } => live_update.map(|update| {
                self.detail_line(self.provider_model_label(), &update.transcript_detail())
            }),
            AgentEvent::ToolCallFinished { .. } => live_update.and_then(|update| {
                update.result_preview.as_deref().map(|preview| {
                    self.detail_line(
                        self.provider_model_label(),
                        &format!("{} | {}", update.transcript_detail(), preview),
                    )
                })
            }),
            AgentEvent::RawProviderEvent { provider, payload } => {
                self.line_for_raw_provider_event(provider, payload)
            }
            AgentEvent::Log { level, message } => self.line_for_log_event(level, message),
            _ => None,
        }
    }

    fn line_for_log_event(&self, level: &LogLevel, message: &str) -> Option<String> {
        if !matches!(level, LogLevel::Info | LogLevel::Warn) {
            return None;
        }
        if !message.starts_with("Resume diagnostics:") && !message.starts_with("Resume fallback:") {
            return None;
        }
        Some(self.detail_line(self.provider_model_label(), message))
    }

    fn line_for_raw_provider_event(&self, provider: &str, payload: &Value) -> Option<String> {
        match provider {
            "claude" | "minimax" => self.line_for_claude_compat(payload),
            "codex" => self.line_for_codex(payload),
            "gemini" => self.line_for_gemini(payload),
            _ => None,
        }
    }

    fn line_for_claude_compat(&self, payload: &Value) -> Option<String> {
        match payload.get("type").and_then(|value| value.as_str()) {
            Some("system") => Some(
                self.started_line(
                    payload
                        .get("model")
                        .and_then(|value| value.as_str())
                        .or(self.provider_model_label()),
                ),
            ),
            _ => None,
        }
    }

    fn line_for_codex(&self, payload: &Value) -> Option<String> {
        match payload.get("type").and_then(|value| value.as_str()) {
            Some("thread.started") => Some(
                self.started_line(
                    payload
                        .get("model")
                        .and_then(|value| value.as_str())
                        .or(self.provider_model_label()),
                ),
            ),
            Some("item.completed") => {
                let item = payload.get("item")?;
                match item.get("type").and_then(|value| value.as_str()) {
                    Some("context_compaction") => {
                        Some(self.detail_line(self.provider_model_label(), "context compaction"))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn line_for_gemini(&self, payload: &Value) -> Option<String> {
        match payload.get("type").and_then(|value| value.as_str()) {
            Some("init") => Some(
                self.started_line(
                    payload
                        .get("model")
                        .and_then(|value| value.as_str())
                        .or(self.provider_model_label()),
                ),
            ),
            _ => None,
        }
    }

    fn provider_model_label(&self) -> Option<&str> {
        self.configured_model.as_deref().or_else(|| {
            self.provider
                .and_then(config::interactive_model_for)
                .map(|value| value as &str)
        })
    }

    fn started_line(&self, model: Option<&str>) -> String {
        format!("{} agent started", self.model_label(model))
    }

    fn detail_line(&self, model: Option<&str>, detail: &str) -> String {
        format!("{} | {}", self.model_label(model), detail)
    }

    pub fn diagnostic_line(&self, detail: &str) -> String {
        self.detail_line(self.provider_model_label(), detail)
    }

    fn model_label(&self, model: Option<&str>) -> String {
        let fallback = self
            .provider
            .map(|provider| provider.display_name())
            .unwrap_or("Agent");
        friendly_model_label(model.unwrap_or(fallback), fallback)
    }
}

fn friendly_model_label(model: &str, fallback: &str) -> String {
    let lower = model.to_ascii_lowercase();
    if model == "unknown" {
        return fallback.to_string();
    }
    if lower.contains("gpt-5.4-high") {
        return "GPT-5.4-High".to_string();
    }
    if lower.contains("gpt-5.4") {
        return "GPT-5.4".to_string();
    }
    if lower.contains("gpt-5") {
        return "GPT-5".to_string();
    }
    if lower.contains("opus") {
        return "Opus".to_string();
    }
    if lower.contains("haiku") {
        return "Haiku".to_string();
    }
    if lower.contains("sonnet") {
        return "Sonnet".to_string();
    }
    if lower.contains("minimax") {
        return minimax_friendly_label(&lower);
    }
    if lower.contains("gemini") {
        return "Gemini".to_string();
    }
    if lower.contains("codex") {
        return "Codex".to_string();
    }
    if lower.contains("kimi") {
        return "Kimi".to_string();
    }
    if lower.contains("opencode") {
        return "OpenCode".to_string();
    }
    if lower.contains("hermes") {
        return "Hermes".to_string();
    }
    if lower.contains("claw") {
        return "Claw".to_string();
    }
    if lower.contains("forge") {
        return "Forge".to_string();
    }
    model.to_string()
}

fn minimax_friendly_label(model_lower: &str) -> String {
    let version = if model_lower.contains("m2.7") {
        "2.7"
    } else if model_lower.contains("m2.5") {
        "2.5"
    } else if model_lower.contains("m2.1") {
        "2.1"
    } else if model_lower.contains("m2") {
        "2"
    } else {
        return "MiniMax".to_string();
    };

    let high_speed = if model_lower.contains("highspeed") {
        "-HS"
    } else {
        ""
    };

    format!("MM-{}{}", version, high_speed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_adapter_core::types::LogLevel;
    use serde_json::json;

    fn summary_line(
        context: &LiveSummaryContext,
        tracker: &mut ToolCallLiveUpdateTracker,
        event: AgentEvent,
    ) -> Option<String> {
        let update = tracker.track(&event);
        context.line_for_agent_event(&event, update.as_ref())
    }

    #[test]
    fn generic_bash_tool_start_uses_short_model_and_truncated_preview() {
        let context =
            LiveSummaryContext::new(Some(ModelProvider::Codex), Some("gpt-5.4".to_string()));
        let line = summary_line(
            &context,
            &mut ToolCallLiveUpdateTracker::default(),
            AgentEvent::ToolCallStarted {
                id: "cmd_1".to_string(),
                name: "bash".to_string(),
                input: json!({
                    "command": "/bin/bash -lc 'git status --short --branch'"
                }),
            },
        )
        .expect("summary line");

        assert_eq!(line, "GPT-5.4 | Bash `git status --short --branch`");
    }

    #[test]
    fn shell_wrapped_semble_search_is_labeled_as_semble() {
        let context = LiveSummaryContext::new(Some(ModelProvider::MiniMax), None);
        let line = summary_line(
            &context,
            &mut ToolCallLiveUpdateTracker::default(),
            AgentEvent::ToolCallStarted {
                id: "cmd_1".to_string(),
                name: "Shell".to_string(),
                input: json!({
                    "command": "semble search \"cli argument parsing\" /tmp/project"
                }),
            },
        )
        .expect("summary line");

        assert_eq!(
            line,
            "MM-2.7 | Semble `search \"cli argument parsing\" /tmp/project`"
        );
    }

    #[test]
    fn absolute_semble_path_is_labeled_as_semble() {
        let context = LiveSummaryContext::new(Some(ModelProvider::MiniMax), None);
        let line = summary_line(
            &context,
            &mut ToolCallLiveUpdateTracker::default(),
            AgentEvent::ToolCallStarted {
                id: "cmd_1".to_string(),
                name: "Shell".to_string(),
                input: json!({
                    "command": "/workspace/semble-rs/semble-rs-work/target/release/semble search \"cli argument parsing\" /tmp/project"
                }),
            },
        )
        .expect("summary line");

        assert_eq!(
            line,
            "MM-2.7 | Semble `search \"cli argument parsing\" /tmp/project`"
        );
    }

    #[test]
    fn shell_wrapped_fff_match_is_labeled_as_fff() {
        let context = LiveSummaryContext::new(Some(ModelProvider::MiniMax), None);
        let line = summary_line(
            &context,
            &mut ToolCallLiveUpdateTracker::default(),
            AgentEvent::ToolCallStarted {
                id: "cmd_1".to_string(),
                name: "Shell".to_string(),
                input: json!({
                    "command": "fff command dispatch src"
                }),
            },
        )
        .expect("summary line");

        assert_eq!(line, "MM-2.7 | FFF `command dispatch src`");
    }

    #[test]
    fn shell_without_command_is_labeled_as_bash_without_preview() {
        let context = LiveSummaryContext::new(Some(ModelProvider::MiniMax), None);
        let line = summary_line(
            &context,
            &mut ToolCallLiveUpdateTracker::default(),
            AgentEvent::ToolCallStarted {
                id: "cmd_1".to_string(),
                name: "Shell".to_string(),
                input: Value::Null,
            },
        )
        .expect("summary line");

        assert_eq!(line, "MM-2.7 | Bash");
    }

    #[test]
    fn generic_tool_start_formats_mcp_preview() {
        let context = LiveSummaryContext::new(Some(ModelProvider::MiniMax), None);
        let line = summary_line(
            &context,
            &mut ToolCallLiveUpdateTracker::default(),
            AgentEvent::ToolCallStarted {
                id: "tool-1".to_string(),
                name: "mcp__lightweight_builds__run_build".to_string(),
                input: json!({
                    "project_path": "/tmp/project",
                    "command_override": ["cargo", "test", "--lib"]
                }),
            },
        )
        .expect("summary line");

        assert!(line.starts_with("MM-2.7 | MCP lightweight_builds.run_build"));
        assert!(line.contains("project_path=`/tmp/project`"));
        assert!(line.contains("command=`cargo test --lib`"));
    }

    #[test]
    fn tool_result_uses_started_tool_name() {
        let context = LiveSummaryContext::new(Some(ModelProvider::Kimi), None);
        let mut tracker = ToolCallLiveUpdateTracker::default();
        let started = summary_line(
            &context,
            &mut tracker,
            AgentEvent::ToolCallStarted {
                id: "tool-1".to_string(),
                name: "read_file".to_string(),
                input: json!({
                    "path": "/tmp/main.rs"
                }),
            },
        )
        .expect("summary line");
        let line = summary_line(
            &context,
            &mut tracker,
            AgentEvent::ToolCallFinished {
                id: "tool-1".to_string(),
                output: Some("very useful output".to_string()),
                error: None,
            },
        )
        .expect("summary line");

        assert_eq!(started, "Kimi | Read main.rs");
        assert_eq!(line, "Kimi | Read main.rs | very useful output");
    }

    #[test]
    fn tool_result_prefers_stdout_from_json_payload() {
        let context =
            LiveSummaryContext::new(Some(ModelProvider::Codex), Some("gpt-5.4".to_string()));
        let mut tracker = ToolCallLiveUpdateTracker::default();
        let _ = summary_line(
            &context,
            &mut tracker,
            AgentEvent::ToolCallStarted {
                id: "tool_1".to_string(),
                name: "mcp__bash__run".to_string(),
                input: json!({
                    "command": "pwd"
                }),
            },
        );
        let line = summary_line(
            &context,
            &mut tracker,
            AgentEvent::ToolCallFinished {
                id: "tool_1".to_string(),
                output: Some(
                    "{\"success\":true,\"stdout\":\"/workspace/rust-chatbot\\n\",\"stderr\":\"\"}"
                        .to_string(),
                ),
                error: None,
            },
        )
        .expect("summary line");

        assert_eq!(line, "GPT-5.4 | Bash `pwd` | /workspace/rust-chatbot");
    }

    #[test]
    fn tool_result_uses_first_non_empty_line_only() {
        let context =
            LiveSummaryContext::new(Some(ModelProvider::Codex), Some("gpt-5.4".to_string()));
        let mut tracker = ToolCallLiveUpdateTracker::default();
        let _ = summary_line(
            &context,
            &mut tracker,
            AgentEvent::ToolCallStarted {
                id: "cmd_1".to_string(),
                name: "bash".to_string(),
                input: json!({
                    "command": "git status --short"
                }),
            },
        );
        let line = summary_line(
            &context,
            &mut tracker,
            AgentEvent::ToolCallFinished {
                id: "cmd_1".to_string(),
                output: Some(" M src/main.rs\n M src/lib.rs\n".to_string()),
                error: None,
            },
        )
        .expect("summary line");

        assert_eq!(line, "GPT-5.4 | Bash `git status --short` | M src/main.rs");
    }

    #[test]
    fn resume_diagnostics_warn_log_is_persisted() {
        let context = LiveSummaryContext::new(Some(ModelProvider::MiniMax), None);
        let line = context
            .line_for_agent_event(
                &AgentEvent::Log {
                    level: LogLevel::Warn,
                    message:
                        "Resume diagnostics: missing bridge transcript for session sess-123; attempting provider resume anyway"
                            .to_string(),
                },
                None,
            )
            .expect("summary line");

        assert_eq!(
            line,
            "MM-2.7 | Resume diagnostics: missing bridge transcript for session sess-123; attempting provider resume anyway"
        );
    }

    #[test]
    fn non_diagnostic_warn_log_is_not_persisted() {
        let context = LiveSummaryContext::new(Some(ModelProvider::MiniMax), None);
        assert!(context
            .line_for_agent_event(
                &AgentEvent::Log {
                    level: LogLevel::Warn,
                    message: "regular warning".to_string(),
                },
                None,
            )
            .is_none());
    }
}
