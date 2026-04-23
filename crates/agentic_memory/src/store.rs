use crate::error::Result;
use crate::search::SearchTree;
use crate::types::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MemoryDelta {
    Upsert {
        kind: AgentMemoryKind,
        id: String,
        summary: String,
    },
    Delete {
        kind: AgentMemoryKind,
        id: String,
    },
    Touch {
        kind: AgentMemoryKind,
        id: String,
        success: bool,
    },
    SearchBackup {
        state_id: String,
        action_id: String,
        reward: f64,
    },
    Snapshot {
        note: String,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryCommit {
    pub id: String,
    pub parent: Option<String>,
    pub timestamp_ms: TimestampMillis,
    pub author: String,
    pub message: String,
    pub deltas: Vec<MemoryDelta>,
    pub checksum: String,
}
impl MemoryCommit {
    pub fn new(
        parent: Option<String>,
        author: impl Into<String>,
        message: impl Into<String>,
        deltas: Vec<MemoryDelta>,
    ) -> Self {
        let timestamp_ms = now_millis();
        let author = author.into();
        let message = message.into();
        let material =
            serde_json::to_string(&(parent.clone(), timestamp_ms, &author, &message, &deltas))
                .unwrap_or_else(|_| format!("{timestamp_ms}:{author}:{message}"));
        let checksum = stable_hash_text(&material);
        Self {
            id: format!("commit_{checksum}"),
            parent,
            timestamp_ms,
            author,
            message,
            deltas,
            checksum,
        }
    }}
pub trait AgentMemoryStore {
    fn verified_runbook_lookup(&self, state: &AgentState) -> Option<VerifiedRunbook>;
    fn exact_lookup(&self, state: &AgentState) -> Option<ExactReplay>;
    fn policy_candidates(&self, state: &AgentState, min_similarity: f64) -> Vec<ActionCandidate>;
    fn procedural_candidates(
        &self,
        state: &AgentState,
        min_similarity: f64,
    ) -> Vec<ActionCandidate>;
    fn tool_affordance_candidates(
        &self,
        state: &AgentState,
        min_similarity: f64,
    ) -> Vec<ActionCandidate>;
    fn ownership_for(&self, state: &AgentState, min_similarity: f64) -> OwnershipMap;
    fn ko_blocks(&self, state: &AgentState, action: &ActionCandidate) -> Option<KoRule>;
    fn safety_blocks(&self, state: &AgentState, action: &ActionCandidate) -> Option<SafetyRule>;
    fn search_tree_mut(&mut self, state_id: &str) -> &mut SearchTree;
    fn backup_search(&mut self, outcome: &ActionOutcome);
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct InMemoryAgentStore {
    pub semantic: Vec<SemanticFact>,
    pub user_preferences: Vec<UserPreference>,
    pub project_facts: Vec<ProjectFact>,
    pub episodes: Vec<EpisodicTrace>,
    pub procedural: Vec<ProceduralSkill>,
    pub tool_affordances: Vec<ToolAffordanceMemory>,
    pub policy_value: Vec<PolicyValueMemory>,
    pub ownership: Vec<OwnershipMemory>,
    pub exact_replay: HashMap<String, ExactReplay>,
    pub verified_runbooks: Vec<VerifiedRunbook>,
    pub ko_rules: Vec<KoRule>,
    pub safety_rules: Vec<SafetyRule>,
    pub search_trees: HashMap<String, SearchTree>,
    pub commits: Vec<MemoryCommit>,
}
impl InMemoryAgentStore {
    pub fn new() -> Self {
        Self::default()
    }}
impl InMemoryAgentStore {
    pub fn save_json<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }}
impl InMemoryAgentStore {
    pub fn load_json<P: AsRef<Path>>(path: P) -> Result<Self> {
        let json = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }}
impl InMemoryAgentStore {
    pub fn commit(
        &mut self,
        author: impl Into<String>,
        message: impl Into<String>,
        deltas: Vec<MemoryDelta>,
    ) -> MemoryCommit {
        let parent = self.commits.last().map(|commit| commit.id.clone());
        let commit = MemoryCommit::new(parent, author, message, deltas);
        self.commits.push(commit.clone());
        commit
    }}
impl InMemoryAgentStore {
    pub fn upsert_exact(&mut self, exact: ExactReplay) {
        self.exact_replay.insert(exact.exact_key.clone(), exact);
    }}
