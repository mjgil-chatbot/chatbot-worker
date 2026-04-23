use crate::store::{InMemoryAgentStore, MemoryDelta};
use crate::types::*;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Promotion {
    EpisodeStored,
    SemanticFact,
    ProceduralSkill,
    PolicyValuePrior,
    Demoted,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ConsolidationConfig {
    pub procedural_pattern_threshold: usize,
    pub policy_pattern_threshold: usize,
    pub demote_after_failures: u64,
}
impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            procedural_pattern_threshold: 2,
            policy_pattern_threshold: 2,
            demote_after_failures: 2,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Consolidator {
    pub config: ConsolidationConfig,
}
impl Default for Consolidator {
    fn default() -> Self {
        Self {
            config: ConsolidationConfig::default(),
        }
    }
}
impl Consolidator {
    fn promote_semantic_like(
        &self,
        store: &mut InMemoryAgentStore,
        episode: &EpisodicTrace,
        request: &PromotionRequest,
        deltas: &mut Vec<MemoryDelta>,
    ) {
        match request.target_kind {
            AgentMemoryKind::UserPreference => {
                let pref = UserPreference {
                    header: MemoryHeader::new(
                        AgentMemoryKind::UserPreference,
                        episode.header.scope.clone(),
                        request.claim_or_pattern.clone(),
                    )
                    .with_confidence(request.confidence)
                    .with_evidence(Evidence::new(
                        EvidenceSource::Episode,
                        episode.header.id.clone(),
                        episode.final_outcome.clone(),
                    )),
                    preference: request.claim_or_pattern.clone(),
                    strength: request.confidence,
                    not_a_constraint: true,
                    examples: vec![episode.initial_goal.clone()],
                };
                deltas.push(MemoryDelta::Upsert {
                    kind: AgentMemoryKind::UserPreference,
                    id: pref.header.id.clone(),
                    summary: pref.preference.clone(),
                });
                store.user_preferences.push(pref);
            }
            AgentMemoryKind::ProjectFact => {
                let fact = ProjectFact {
                    header: MemoryHeader::new(
                        AgentMemoryKind::ProjectFact,
                        episode.header.scope.clone(),
                        request.claim_or_pattern.clone(),
                    )
                    .with_confidence(request.confidence)
                    .with_evidence(Evidence::new(
                        EvidenceSource::Episode,
                        episode.header.id.clone(),
                        episode.final_outcome.clone(),
                    )),
                    project: episode
                        .header
                        .scope
                        .project_id
                        .clone()
                        .unwrap_or_else(|| "unknown_project".into()),
                    fact: request.claim_or_pattern.clone(),
                    applies_when: request.applies_when.clone(),
                    invalidates_on: Vec::new(),
                };
                deltas.push(MemoryDelta::Upsert {
                    kind: AgentMemoryKind::ProjectFact,
                    id: fact.header.id.clone(),
                    summary: fact.fact.clone(),
                });
                store.project_facts.push(fact);
            }
            _ => {
                let fact = SemanticFact {
                    header: MemoryHeader::new(
                        AgentMemoryKind::SemanticFact,
                        episode.header.scope.clone(),
                        request.claim_or_pattern.clone(),
                    )
                    .with_confidence(request.confidence)
                    .with_evidence(Evidence::new(
                        EvidenceSource::Episode,
                        episode.header.id.clone(),
                        episode.final_outcome.clone(),
                    )),
                    subject: episode.initial_goal.clone(),
                    predicate: "implies".into(),
                    object: request.claim_or_pattern.clone(),
                    applies_when: request.applies_when.clone(),
                    does_not_apply_when: Vec::new(),
                };
                deltas.push(MemoryDelta::Upsert {
                    kind: AgentMemoryKind::SemanticFact,
                    id: fact.header.id.clone(),
                    summary: fact.object.clone(),
                });
                store.semantic.push(fact);
            }
        }
    }}
