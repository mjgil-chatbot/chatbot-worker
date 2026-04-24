use crate::encoder::{normalize_text, AgentStateEncoder};
use crate::ko::KoDetector;
use crate::prompt::MemoryPacketConfig;
use crate::search::SearchConfig;
use crate::store::InMemoryAgentStore;
use crate::types::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AgentMemoryConfig {
    pub min_policy_similarity: f64,
    pub min_procedure_similarity: f64,
    pub min_ownership_similarity: f64,
    pub search: SearchConfig,
    pub value_weights: ValueWeights,
    pub quiescence_uncertainty_threshold: f64,
    pub allow_side_effects_without_confirmation: bool,
    pub memory_packet: MemoryPacketConfig,
}
impl Default for AgentMemoryConfig {
    fn default() -> Self {
        Self {
            min_policy_similarity: 0.06,
            min_procedure_similarity: 0.06,
            min_ownership_similarity: 0.06,
            search: SearchConfig::default(),
            value_weights: ValueWeights::default(),
            quiescence_uncertainty_threshold: 0.58,
            allow_side_effects_without_confirmation: false,
            memory_packet: MemoryPacketConfig::default(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentMemorySystem {
    pub store: InMemoryAgentStore,
    pub encoder: AgentStateEncoder,
    pub config: AgentMemoryConfig,
    pub ko_detector: KoDetector,
}
impl Default for AgentMemorySystem {
    fn default() -> Self {
        Self {
            store: InMemoryAgentStore::new(),
            encoder: AgentStateEncoder::default(),
            config: AgentMemoryConfig::default(),
            ko_detector: KoDetector::default(),
        }
    }
}
impl AgentMemorySystem {
    pub fn new(store: InMemoryAgentStore) -> Self {
        Self {
            store,
            ..Self::default()
        }
    }}
impl AgentMemorySystem {
    pub fn encode_state(&self, task: AgentTask) -> AgentState {
        self.encoder.encode(task)
    }}
impl AgentMemorySystem {
    fn override_decision(
        &self,
        state: AgentState,
        ownership: OwnershipMap,
        candidate: ActionCandidate,
        source: DecisionSource,
        source_memory_id: Option<String>,
        memory_packet: Option<crate::prompt::PromptMemoryPacket>,
        rationale: &str,
    ) -> AgentDecision {
        let ranked = vec![RankedAction {
            score: 1.0,
            utility_score: 1.0,
            puct_score: 0.0,
            ownership_gain: ownership.uncertainty_reduction_for(&candidate.label()),
            blocked_reason: None,
            candidate: candidate.clone(),
        }];
        AgentDecision {
            selected: candidate,
            source,
            source_memory_id,
            score: 1.0,
            rationale: format!("{} for state {}", rationale, state.id),
            ownership,
            ranked_candidates: ranked,
            memory_packet,
        }
    }}
fn fallback_candidate(state: &AgentState) -> ActionCandidate {
    let mut candidate = if state.unknowns.is_empty() {
        ActionCandidate::new(AgentAction::Respond {
            message: "answer using current evidence".into(),
        })
    } else {
        ActionCandidate::new(AgentAction::RetrieveMemory {
            query: state.goal.clone(),
            regions: state.unknowns.clone(),
        })
    };
    candidate.reason = "fallback candidate".into();
    candidate
}
fn dedupe_merge_candidates(candidates: Vec<ActionCandidate>) -> Vec<ActionCandidate> {
    let mut by_label: BTreeMap<String, ActionCandidate> = BTreeMap::new();
    for candidate in candidates {
        let key = normalize_text(&candidate.label());
        match by_label.get_mut(&key) {
            Some(existing) => {
                existing.prior = existing.prior.max(candidate.prior);
                existing.expected_success =
                    existing.expected_success.max(candidate.expected_success);
                existing.information_gain =
                    existing.information_gain.max(candidate.information_gain);
                existing.cost = existing.cost.min(candidate.cost);
                existing.latency = existing.latency.min(candidate.latency);
                existing.risk = existing.risk.max(candidate.risk);
                existing.reversibility = existing.reversibility.max(candidate.reversibility);
                if !candidate.reason.is_empty() && !existing.reason.contains(&candidate.reason) {
                    existing.reason = format!("{} | {}", existing.reason, candidate.reason);
                }
                for id in candidate.source_memory_ids {
                    if !existing.source_memory_ids.contains(&id) {
                        existing.source_memory_ids.push(id);
                    }
                }
                existing.requires_confirmation |= candidate.requires_confirmation;
            }
            None => {
                by_label.insert(key, candidate);
            }
        }
    }
    by_label.into_values().collect()
}
