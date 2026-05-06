use super::*;
use anyhow::{anyhow, Context, Result};
use reqwest::blocking::{Client, Response};
use reqwest::StatusCode;
use rust_debug::tracing::warn;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::time::Duration;
const MEMMACHINE_READ_TIMEOUT: Duration = Duration::from_secs(10);
const MEMMACHINE_WRITE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
#[derive(Debug, Clone)]
pub struct MemMachineStatus {
    pub base_url: String,
    pub org_id: String,
    pub project_id: String,
    pub project_exists: bool,
}
#[derive(Debug)]
pub struct MemMachineClient {
    client: Client,
    base_url: String,
    org_id: String,
    project_id: String,
}
#[derive(Debug, Serialize)]
struct ProjectPayload<'a> {
    org_id: &'a str,
    project_id: &'a str,
}
#[derive(Debug, Serialize)]
struct CreateProjectPayload<'a> {
    org_id: &'a str,
    project_id: &'a str,
    description: &'a str,
}
#[derive(Debug, Serialize)]
struct SearchPayload<'a> {
    org_id: &'a str,
    project_id: &'a str,
    query: &'a str,
    top_k: usize,
    types: [&'a str; 1],
    agent_mode: bool,
}
#[derive(Debug, Serialize)]
struct AddMemoriesPayload {
    org_id: String,
    project_id: String,
    types: Vec<&'static str>,
    messages: Vec<MemMachineMessage>,
}
#[derive(Debug, Serialize)]
struct MemMachineMessage {
    content: String,
    producer: String,
    produced_for: String,
    role: String,
    timestamp: String,
    metadata: BTreeMap<String, String>,
}
#[derive(Debug, Deserialize)]
struct AddMemoriesResponse {
    results: Vec<AddMemoryResult>,
}
#[derive(Debug, Deserialize)]
struct AddMemoryResult {
    uid: String,
}
#[derive(Debug, Deserialize)]
struct SearchResponse {
    content: SearchResponseContent,
}
#[derive(Debug, Deserialize)]
struct SearchResponseContent {
    #[serde(default)]
    episodic_memory: Option<EpisodicSearchResponse>,
}
#[derive(Debug, Deserialize)]
struct EpisodicSearchResponse {
    long_term_memory: EpisodicMemoryBucket,
    short_term_memory: EpisodicShortTermBucket,
}
#[derive(Debug, Deserialize)]
struct EpisodicMemoryBucket {
    episodes: Vec<EpisodicEpisode>,
}
#[derive(Debug, Deserialize)]
struct EpisodicShortTermBucket {
    episodes: Vec<EpisodicEpisode>,
}
#[derive(Debug, Deserialize)]
struct EpisodicEpisode {
    uid: String,
    content: String,
    #[serde(default)]
    score: Option<f32>,
    #[serde(default)]
    created_at: Option<String>,
    #[allow(dead_code)]
    producer_id: String,
    producer_role: String,
    #[serde(default)]
    metadata: Option<BTreeMap<String, serde_json::Value>>,
}
impl MemMachineClient {
    fn project_exists(&self) -> Result<bool> {
        let response = self
            .client
            .post(self.endpoint("projects/get"))
            .timeout(MEMMACHINE_READ_TIMEOUT)
            .json(&ProjectPayload {
                org_id: &self.org_id,
                project_id: &self.project_id,
            })
            .send()
            .with_context(|| format!("query MemMachine project {}", self.project_id))?;

        match response.status() {
            StatusCode::OK => Ok(true),
            StatusCode::NOT_FOUND => Ok(false),
            status => Err(response_error(
                response,
                format!("check MemMachine project {}", self.project_id),
                status,
            )),
        }
    }}
impl MemMachineClient {
    fn ensure_project(&self, description: &str) -> Result<()> {
        if self.project_exists()? {
            return Ok(());
        }

        let response = self
            .client
            .post(self.endpoint("projects"))
            .timeout(MEMMACHINE_READ_TIMEOUT)
            .json(&CreateProjectPayload {
                org_id: &self.org_id,
                project_id: &self.project_id,
                description,
            })
            .send()
            .with_context(|| format!("create MemMachine project {}", self.project_id))?;

        match response.status() {
            StatusCode::OK | StatusCode::CREATED | StatusCode::CONFLICT => Ok(()),
            status => Err(response_error(
                response,
                format!("create MemMachine project {}", self.project_id),
                status,
            )),
        }
    }}
impl MemMachineClient {
    fn delete_project(&self) -> Result<()> {
        let response = self
            .client
            .post(self.endpoint("projects/delete"))
            .timeout(MEMMACHINE_READ_TIMEOUT)
            .json(&ProjectPayload {
                org_id: &self.org_id,
                project_id: &self.project_id,
            })
            .send()
            .with_context(|| format!("delete MemMachine project {}", self.project_id))?;

        match response.status() {
            StatusCode::NO_CONTENT | StatusCode::OK | StatusCode::NOT_FOUND => Ok(()),
            status => Err(response_error(
                response,
                format!("delete MemMachine project {}", self.project_id),
                status,
            )),
        }
    }}
impl MemMachineClient {
    fn endpoint(&self, path: &str) -> String {
        format!("{}/api/v2/{path}", self.base_url)
    }}
fn workspace_description(paths: &WorkspaceMemoryPaths) -> String {
    format!(
        "rust-chatbot workspace memory for {}",
        paths.workspace_root.display()
    )
}
fn map_search_hits(response: SearchResponse) -> Vec<MemoryHit> {
    let Some(episodic_memory) = response.content.episodic_memory else {
        return Vec::new();
    };

    let mut seen_doc_keys = HashSet::new();
    let mut out = Vec::new();
    for episode in episodic_memory
        .long_term_memory
        .episodes
        .into_iter()
        .chain(episodic_memory.short_term_memory.episodes)
    {
        let metadata = episode.metadata.unwrap_or_default();
        let doc_key = metadata_string(&metadata, "doc_key").unwrap_or_else(|| episode.uid.clone());
        if !seen_doc_keys.insert(doc_key.clone()) {
            continue;
        }

        let snippet = sanitize_retrieved_memory_text(&episode.content);
        if snippet.is_empty() {
            continue;
        }

        let session_name = metadata_string(&metadata, "session_name").unwrap_or_default();
        out.push(MemoryHit {
            doc_key,
            chat_id: metadata_string(&metadata, "chat_id").unwrap_or_else(|| episode.uid.clone()),
            role: metadata_string(&metadata, "role").unwrap_or_else(|| episode.producer_role),
            session_name,
            timestamp: episode
                .created_at
                .as_deref()
                .and_then(parse_timestamp)
                .or_else(|| {
                    metadata_string(&metadata, "timestamp")
                        .and_then(|value| parse_timestamp(&value))
                })
                .unwrap_or_else(default_memory_timestamp),
            snippet,
            title: None,
            message_index: metadata_string(&metadata, "message_index")
                .and_then(|value| value.parse::<usize>().ok()),
            turn_index: metadata_string(&metadata, "turn_index")
                .and_then(|value| value.parse::<usize>().ok()),
            score: episode.score,
        });
    }
    out
}
fn metadata_string(metadata: &BTreeMap<String, serde_json::Value>, key: &str) -> Option<String> {
    metadata.get(key).and_then(|value| match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        other => Some(other.to_string()),
    })
}
fn response_error(response: Response, action: String, status: StatusCode) -> anyhow::Error {
    let body = response
        .text()
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "<empty response body>".to_string());
    anyhow!("{action} failed with HTTP {status}: {body}")
}
