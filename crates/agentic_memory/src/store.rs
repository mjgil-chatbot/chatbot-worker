use crate::encoder::token_similarity;
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
    }
}

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
    }

    pub fn save_json<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load_json<P: AsRef<Path>>(path: P) -> Result<Self> {
        let json = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }

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
    }

    pub fn upsert_exact(&mut self, exact: ExactReplay) {
        self.exact_replay.insert(exact.exact_key.clone(), exact);
    }

    pub fn touch_memory(&mut self, id: &str, success: bool) -> Option<MemoryDelta> {
        macro_rules! touch_vec {
            ($items:expr, $kind:expr) => {
                if let Some(item) = $items.iter_mut().find(|item| item.header.id == id) {
                    item.header.touch(success);
                    return Some(MemoryDelta::Touch {
                        kind: $kind,
                        id: id.to_string(),
                        success,
                    });
                }
            };
        }
        touch_vec!(self.semantic, AgentMemoryKind::SemanticFact);
        touch_vec!(self.user_preferences, AgentMemoryKind::UserPreference);
        touch_vec!(self.project_facts, AgentMemoryKind::ProjectFact);
        touch_vec!(self.episodes, AgentMemoryKind::EpisodicTrace);
        touch_vec!(self.procedural, AgentMemoryKind::ProceduralSkill);
        touch_vec!(self.tool_affordances, AgentMemoryKind::ToolAffordance);
        touch_vec!(self.policy_value, AgentMemoryKind::PolicyValuePrior);
        touch_vec!(self.ownership, AgentMemoryKind::Ownership);
        touch_vec!(self.verified_runbooks, AgentMemoryKind::VerifiedRunbook);
        touch_vec!(self.ko_rules, AgentMemoryKind::KoRule);
        touch_vec!(self.safety_rules, AgentMemoryKind::SafetyRule);
        if let Some(item) = self
            .exact_replay
            .values_mut()
            .find(|item| item.header.id == id)
        {
            item.header.touch(success);
            return Some(MemoryDelta::Touch {
                kind: AgentMemoryKind::ExactReplay,
                id: id.to_string(),
                success,
            });
        }
        None
    }

    pub fn high_level_context(&self, state: &AgentState, min_similarity: f64) -> Vec<String> {
        let text = state.all_text();
        let mut items = Vec::new();

        for fact in &self.semantic {
            if fact
                .header
                .is_valid_for(&state.scope, &state.changed_artifacts)
                && token_similarity(
                    &text,
                    &format!("{} {} {}", fact.subject, fact.predicate, fact.object),
                ) >= min_similarity
            {
                items.push(format!(
                    "fact: {} {} {}",
                    fact.subject, fact.predicate, fact.object
                ));
            }
        }
        for pref in &self.user_preferences {
            if pref
                .header
                .is_valid_for(&state.scope, &state.changed_artifacts)
                && token_similarity(&text, &pref.preference) >= min_similarity
            {
                items.push(format!("preference: {}", pref.preference));
            }
        }
        for fact in &self.project_facts {
            if fact
                .header
                .is_valid_for(&state.scope, &state.changed_artifacts)
                && token_similarity(&text, &fact.fact) >= min_similarity
            {
                items.push(format!("project fact: {}", fact.fact));
            }
        }
        items
    }
}

impl AgentMemoryStore for InMemoryAgentStore {
    fn verified_runbook_lookup(&self, state: &AgentState) -> Option<VerifiedRunbook> {
        self.verified_runbooks
            .iter()
            .filter(|runbook| runbook.preconditions_hold(state))
            .max_by(|a, b| {
                a.header
                    .confidence
                    .partial_cmp(&b.header.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
    }

    fn exact_lookup(&self, state: &AgentState) -> Option<ExactReplay> {
        self.exact_replay
            .get(&state.exact_key)
            .filter(|exact| exact.valid_for(state))
            .cloned()
    }

    fn policy_candidates(&self, state: &AgentState, min_similarity: f64) -> Vec<ActionCandidate> {
        let text = state.all_text();
        let mut candidates = Vec::new();
        for memory in &self.policy_value {
            if !memory
                .header
                .is_valid_for(&state.scope, &state.changed_artifacts)
            {
                continue;
            }
            let similarity = token_similarity(&text, &memory.state_pattern);
            if similarity < min_similarity {
                continue;
            }
            for action in memory.actions.clone() {
                let mut candidate = action.into_candidate(memory.header.id.clone());
                candidate.prior = clamp01(
                    candidate.prior * (0.40 + 0.60 * similarity) * memory.header.confidence,
                );
                candidate.expected_success =
                    clamp01(candidate.expected_success * (0.50 + 0.50 * memory.header.confidence));
                candidate.reason = format!(
                    "{}; matched policy pattern at {:.2}",
                    candidate.reason, similarity
                );
                candidates.push(candidate);
            }
        }
        candidates
    }

    fn procedural_candidates(
        &self,
        state: &AgentState,
        min_similarity: f64,
    ) -> Vec<ActionCandidate> {
        let text = state.all_text();
        let mut candidates = Vec::new();
        for skill in &self.procedural {
            if !skill
                .header
                .is_valid_for(&state.scope, &state.changed_artifacts)
            {
                continue;
            }
            let pattern_text = format!(
                "{} {} {}",
                skill.name,
                skill.applies_when.join(" "),
                skill.anti_patterns.join(" ")
            );
            let similarity = token_similarity(&text, &pattern_text);
            if similarity < min_similarity {
                continue;
            }
            if let Some(mut candidate) = skill.first_action_candidate() {
                candidate.prior = clamp01(candidate.prior * (0.55 + 0.45 * similarity));
                candidate.reason = format!(
                    "{}; procedural similarity {:.2}",
                    candidate.reason, similarity
                );
                candidates.push(candidate);
            }
        }
        candidates
    }

    fn tool_affordance_candidates(
        &self,
        state: &AgentState,
        min_similarity: f64,
    ) -> Vec<ActionCandidate> {
        let text = state.all_text();
        let mut candidates = Vec::new();
        for memory in &self.tool_affordances {
            if !memory
                .header
                .is_valid_for(&state.scope, &state.changed_artifacts)
            {
                continue;
            }
            let Some(tool) = state
                .available_tools
                .iter()
                .find(|tool| tool.name == memory.tool_name)
            else {
                continue;
            };
            let pattern_text = format!("{} {}", memory.tool_name, memory.works_well_for.join(" "));
            let similarity = token_similarity(&text, &pattern_text);
            if similarity < min_similarity {
                continue;
            }
            let mut candidate = ActionCandidate::new(AgentAction::UseTool {
                tool_name: tool.name.clone(),
                args: serde_json::Value::Object(Default::default()),
                expected_observation: Some(format!(
                    "tool affordance: {}",
                    memory.works_well_for.join(", ")
                )),
            });
            candidate.prior = clamp01(0.35 + 0.50 * similarity * memory.header.confidence);
            candidate.expected_success = clamp01(0.45 + 0.45 * memory.header.confidence);
            candidate.information_gain = clamp01(0.55 + 0.30 * similarity);
            candidate.cost = clamp01(memory.cost.max(tool.cost));
            candidate.latency = clamp01(memory.latency.max(tool.latency));
            candidate.risk = clamp01(memory.risk.max(tool.risk));
            candidate.reversibility = if tool.safety_class.is_side_effectful() {
                0.35
            } else {
                0.95
            };
            candidate.requires_confirmation = tool.safety_class.is_side_effectful();
            candidate.reason = format!("tool affordance memory matched at {:.2}", similarity);
            candidate.source_memory_ids.push(memory.header.id.clone());
            candidates.push(candidate);
        }
        candidates
    }

    fn ownership_for(&self, state: &AgentState, min_similarity: f64) -> OwnershipMap {
        let text = state.all_text();
        let mut map = OwnershipMap::default();
        for memory in &self.ownership {
            if !memory
                .header
                .is_valid_for(&state.scope, &state.changed_artifacts)
            {
                continue;
            }
            if token_similarity(&text, &memory.state_pattern) >= min_similarity {
                map.merge_prefer_confident(memory.ownership.clone());
            }
        }
        map
    }

    fn ko_blocks(&self, state: &AgentState, action: &ActionCandidate) -> Option<KoRule> {
        self.ko_rules
            .iter()
            .find(|rule| rule.blocks(state, action))
            .cloned()
    }

    fn safety_blocks(&self, state: &AgentState, action: &ActionCandidate) -> Option<SafetyRule> {
        let state_text = state.all_text().to_lowercase();
        let action_label = action.label().to_lowercase();
        self.safety_rules
            .iter()
            .find(|rule| {
                rule.header
                    .is_valid_for(&state.scope, &state.changed_artifacts)
                    && rule
                        .applies_when
                        .iter()
                        .all(|condition| condition_words_match(&state_text, condition))
                    && rule
                        .forbidden_action_patterns
                        .iter()
                        .any(|pattern| action_label.contains(&pattern.to_lowercase()))
            })
            .cloned()
    }

    fn search_tree_mut(&mut self, state_id: &str) -> &mut SearchTree {
        self.search_trees
            .entry(state_id.to_string())
            .or_insert_with(|| SearchTree::new(state_id))
    }

    fn backup_search(&mut self, outcome: &ActionOutcome) {
        self.search_tree_mut(&outcome.state_id).backup(outcome);
    }
}
