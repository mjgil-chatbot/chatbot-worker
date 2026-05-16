use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use uuid::Uuid;

pub type BoxedBackgroundFuture = Pin<Box<dyn Future<Output = BackgroundResult> + Send>>;
pub type BackgroundTaskExecutor = dyn Fn(BackgroundTask) -> BoxedBackgroundFuture + Send + Sync;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentProfile {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackgroundTask {
    pub id: Uuid,
    pub agent_profile: AgentProfile,
    pub prompt: String,
    pub context_budget: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_session_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackgroundResult {
    pub id: Uuid,
    pub output: Result<String, String>,
}

/// Merged output from all completed background tasks.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MergedResults {
    pub succeeded: Vec<(Uuid, String)>,
    pub failed: Vec<(Uuid, String)>,
}

impl MergedResults {
    /// Build a MergedResults from a vec of BackgroundResult.
    pub fn from_results(results: Vec<BackgroundResult>) -> Self {
        let mut merged = MergedResults::default();
        for r in results {
            match r.output {
                Ok(text) => merged.succeeded.push((r.id, text)),
                Err(err) => merged.failed.push((r.id, err)),
            }
        }
        merged
    }

    pub fn total(&self) -> usize {
        self.succeeded.len() + self.failed.len()
    }
}

/// Default concurrency limit when none is specified.
pub const DEFAULT_MAX_CONCURRENT: usize = 4;

pub const CLAUDE_FAILURE_PREFIX: &str = "Claude command failed";
pub const CODEX_FAILURE_PREFIX: &str = "Codex command failed";
pub const NVIDIA_FAILURE_PREFIX: &str = "Nvidia command failed";
pub const GEMINI_FAILURE_PREFIX: &str = "Gemini command failed";
pub const QWEN_CODE_FAILURE_PREFIX: &str = "Qwen OpenRouter background run failed";
#[cfg(test)]
pub const MINIMAX_MISSING_KEY_PREFIX: &str = "MINIMAX_API_KEY";
#[cfg(test)]
pub const QWEN_MISSING_KEY_PREFIX: &str = "OPENROUTER_API_KEY";
#[cfg(test)]
pub const CLAUDE_LOGIN_PREFIX: &str = "API billing detected.";
#[cfg(test)]
pub const CLAUDE_SUBSCRIPTION_PREFIX: &str = "Unable to detect subscription type.";
pub const SPAWN_FAILURE_PREFIX: &str = "Failed to spawn ";
