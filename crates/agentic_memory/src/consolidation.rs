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
    pub fn consolidate(
        &self,
        store: &mut InMemoryAgentStore,
        episode: EpisodicTrace,
    ) -> Vec<Promotion> {
        let mut promotions = Vec::new();
        let mut deltas = Vec::new();

        let episode_id = episode.header.id.clone();
        deltas.push(MemoryDelta::Upsert {
            kind: AgentMemoryKind::EpisodicTrace,
            id: episode_id.clone(),
            summary: episode.header.summary.clone(),
        });
        store.episodes.push(episode.clone());
        promotions.push(Promotion::EpisodeStored);

        for request in &episode.memories_to_promote {
            match request.target_kind {
                AgentMemoryKind::SemanticFact
                | AgentMemoryKind::ProjectFact
                | AgentMemoryKind::UserPreference => {
                    self.promote_semantic_like(store, &episode, request, &mut deltas);
                    promotions.push(Promotion::SemanticFact);
                }
                AgentMemoryKind::ProceduralSkill => {
                    self.promote_procedure(store, &episode, request, &mut deltas);
                    promotions.push(Promotion::ProceduralSkill);
                }
                AgentMemoryKind::PolicyValuePrior => {
                    self.promote_policy(store, &episode, request, &mut deltas);
                    promotions.push(Promotion::PolicyValuePrior);
                }
                _ => {}
            }
        }

        if episode.successful() {
            for pattern in &episode.successful_patterns {
                let repeated = store
                    .episodes
                    .iter()
                    .filter(|item| item.successful_patterns.iter().any(|p| p == pattern))
                    .count();
                if repeated >= self.config.procedural_pattern_threshold {
                    let request = PromotionRequest {
                        claim_or_pattern: pattern.clone(),
                        target_kind: AgentMemoryKind::ProceduralSkill,
                        confidence: 0.72,
                        applies_when: vec![episode.initial_goal.clone()],
                    };
                    self.promote_procedure(store, &episode, &request, &mut deltas);
                    promotions.push(Promotion::ProceduralSkill);
                }
            }

            if episode.action_labels.len() >= self.config.policy_pattern_threshold {
                let request = PromotionRequest {
                    claim_or_pattern: episode.action_labels.join(" -> "),
                    target_kind: AgentMemoryKind::PolicyValuePrior,
                    confidence: 0.68,
                    applies_when: vec![episode.initial_goal.clone()],
                };
                self.promote_policy(store, &episode, &request, &mut deltas);
                promotions.push(Promotion::PolicyValuePrior);
            }
        }

        for id in &episode.memories_to_demote {
            if let Some(delta) = store.touch_memory(id, false) {
                deltas.push(delta);
                promotions.push(Promotion::Demoted);
            }
        }

        if !deltas.is_empty() {
            store.commit("agentic_memory", "consolidate agent episode", deltas);
        }

        promotions
    }

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
    }

    fn promote_procedure(
        &self,
        store: &mut InMemoryAgentStore,
        episode: &EpisodicTrace,
        request: &PromotionRequest,
        deltas: &mut Vec<MemoryDelta>,
    ) {
        if store
            .procedural
            .iter()
            .any(|skill| skill.name == request.claim_or_pattern)
        {
            return;
        }
        let steps = if episode.action_labels.is_empty() {
            vec![SkillStep {
                label: request.claim_or_pattern.clone(),
                action: AgentAction::Plan {
                    steps: vec![request.claim_or_pattern.clone()],
                },
                expected_observation: None,
                gate: None,
            }]
        } else {
            episode
                .action_labels
                .iter()
                .map(|label| SkillStep {
                    label: label.clone(),
                    action: AgentAction::Plan {
                        steps: vec![label.clone()],
                    },
                    expected_observation: None,
                    gate: None,
                })
                .collect()
        };

        let skill = ProceduralSkill {
            header: MemoryHeader::new(
                AgentMemoryKind::ProceduralSkill,
                episode.header.scope.clone(),
                request.claim_or_pattern.clone(),
            )
            .with_confidence(request.confidence)
            .with_evidence(Evidence::new(
                EvidenceSource::Episode,
                episode.header.id.clone(),
                episode.final_outcome.clone(),
            )),
            name: request.claim_or_pattern.clone(),
            applies_when: request.applies_when.clone(),
            steps,
            anti_patterns: episode.mistakes.clone(),
            expected_success: if episode.successful() { 0.75 } else { 0.45 },
            required_tools: Vec::new(),
            verification_gate: None,
        };
        deltas.push(MemoryDelta::Upsert {
            kind: AgentMemoryKind::ProceduralSkill,
            id: skill.header.id.clone(),
            summary: skill.name.clone(),
        });
        store.procedural.push(skill);
    }

    fn promote_policy(
        &self,
        store: &mut InMemoryAgentStore,
        episode: &EpisodicTrace,
        request: &PromotionRequest,
        deltas: &mut Vec<MemoryDelta>,
    ) {
        let actions = episode
            .action_labels
            .iter()
            .take(6)
            .map(|label| PolicyValueAction {
                action: AgentAction::Plan {
                    steps: vec![label.clone()],
                },
                prior: 0.55,
                expected_success: if episode.successful() { 0.72 } else { 0.38 },
                cost: 0.20,
                latency: 0.15,
                risk: 0.10,
                reversibility: 0.85,
                information_gain: 0.45,
                reason: format!("learned from episode: {}", episode.initial_goal),
            })
            .collect::<Vec<_>>();
        if actions.is_empty() {
            return;
        }
        let policy = PolicyValueMemory {
            header: MemoryHeader::new(
                AgentMemoryKind::PolicyValuePrior,
                episode.header.scope.clone(),
                request.claim_or_pattern.clone(),
            )
            .with_confidence(request.confidence)
            .with_evidence(Evidence::new(
                EvidenceSource::Episode,
                episode.header.id.clone(),
                episode.final_outcome.clone(),
            )),
            state_pattern: request.applies_when.join(" "),
            actions,
        };
        deltas.push(MemoryDelta::Upsert {
            kind: AgentMemoryKind::PolicyValuePrior,
            id: policy.header.id.clone(),
            summary: policy.state_pattern.clone(),
        });
        store.policy_value.push(policy);
    }
}
