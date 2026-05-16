use super::types::*;
use crate::model::ModelProvider;

pub(super) fn provider_failure_message(
    provider: ModelProvider,
    exit_code: Option<i32>,
    stderr: &str,
    stdout: &str,
) -> String {
    let prefix = match provider {
        ModelProvider::Claude | ModelProvider::MiniMax => CLAUDE_FAILURE_PREFIX,
        ModelProvider::Qwen => QWEN_CODE_FAILURE_PREFIX,
        ModelProvider::Codex => CODEX_FAILURE_PREFIX,
        ModelProvider::Nvidia => NVIDIA_FAILURE_PREFIX,
        ModelProvider::Gemini => GEMINI_FAILURE_PREFIX,
        ModelProvider::OpenRouter => "OpenRouter background run failed",
        ModelProvider::OmniForge => "OmniForge background run failed",
        ModelProvider::GeminiForge => "GeminiForge background run failed",
        ModelProvider::GeminiCliForge => "GeminiCliForge background run failed",
        ModelProvider::MiniMaxForge => "MiniMaxForge background run failed",
        ModelProvider::Kimi => "Kimi background run failed",
        ModelProvider::OpenCode => "OpenCode background run failed",
        ModelProvider::Hermes => "Hermes background run failed",
        ModelProvider::Forge => "Forge background run failed",
        ModelProvider::Claw => "Claw background run failed",
    };

    if !stderr.is_empty() {
        match exit_code {
            Some(code) => {
                format!("{prefix}. Exit status: {code}. Recent stderr output:\n{stderr}")
            }
            None => format!("{prefix}. Recent stderr output:\n{stderr}"),
        }
    } else if !stdout.is_empty() {
        format!(
            "{prefix}. Recent stdout output:\n{}",
            ai_providers::stream::tail_text_lines(stdout, 120)
        )
    } else if let Some(code) = exit_code {
        format!("{prefix}. Exit status: {code}")
    } else {
        format!("{prefix}. Exit status unavailable")
    }
}

#[cfg(test)]
pub(super) fn classify_provider_output(
    provider: ModelProvider,
    output: String,
) -> Result<String, String> {
    if is_provider_failure(provider, output.trim()) {
        Err(output)
    } else {
        Ok(output)
    }
}

#[cfg(test)]
pub(super) fn is_provider_failure(provider: ModelProvider, output: &str) -> bool {
    let provider_specific_failure = match provider {
        ModelProvider::Claude => {
            output.starts_with(CLAUDE_FAILURE_PREFIX)
                || output.starts_with(CLAUDE_LOGIN_PREFIX)
                || output.starts_with(CLAUDE_SUBSCRIPTION_PREFIX)
        }
        ModelProvider::MiniMax => {
            output.starts_with(CLAUDE_FAILURE_PREFIX)
                || output.starts_with(MINIMAX_MISSING_KEY_PREFIX)
        }
        ModelProvider::Qwen => {
            output.starts_with(QWEN_CODE_FAILURE_PREFIX)
                || output.starts_with(QWEN_MISSING_KEY_PREFIX)
        }
        ModelProvider::Codex => output.starts_with(CODEX_FAILURE_PREFIX),
        ModelProvider::Nvidia => output.starts_with(NVIDIA_FAILURE_PREFIX),
        ModelProvider::Gemini => output.starts_with(GEMINI_FAILURE_PREFIX),
        ModelProvider::OpenRouter => output.starts_with("OpenRouter background run failed"),
        ModelProvider::OmniForge => output.starts_with("OmniForge background run failed"),
        ModelProvider::GeminiForge => output.starts_with("GeminiForge background run failed"),
        ModelProvider::GeminiCliForge => output.starts_with("GeminiCliForge background run failed"),
        ModelProvider::MiniMaxForge => output.starts_with("MiniMaxForge background run failed"),
        ModelProvider::Kimi => output.starts_with("Kimi background run failed"),
        ModelProvider::OpenCode => output.starts_with("OpenCode background run failed"),
        ModelProvider::Hermes => output.starts_with("Hermes background run failed"),
        ModelProvider::Forge => output.starts_with("Forge background run failed"),
        ModelProvider::Claw => output.starts_with("Claw background run failed"),
    };

    provider_specific_failure || output.starts_with(SPAWN_FAILURE_PREFIX)
}
