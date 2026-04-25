use agent_adapter_core::types::LogLevel;
use agent_adapter_core::AgentEvent;
use serde_json::Value;
use crate::model::ModelProvider;
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
    }}
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
