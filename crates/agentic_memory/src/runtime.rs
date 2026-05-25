use crate::encoder::{normalize_text, AgentStateEncoder};
use crate::ko::KoDetector;
use crate::prompt::{MemoryPacketConfig, PromptMemoryPacketBuilder};
use crate::search::SearchConfig;
use crate::store::{AgentMemoryStore, InMemoryAgentStore, MemoryDelta};
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
    }

    pub fn choose_next_action(&mut self, task: AgentTask) -> AgentDecision {
        let mut state = self.encoder.encode(task);
        let stored_ownership = self
            .store
            .ownership_for(&state, self.config.min_ownership_similarity);
        state.ownership.merge_prefer_confident(stored_ownership);
        let ownership = state.ownership.clone();
        let packet_builder = PromptMemoryPacketBuilder {
            config: self.config.memory_packet,
        };
        let memory_packet = Some(packet_builder.build(&self.store, &state, &ownership));

        if let Some(runbook) = self.store.verified_runbook_lookup(&state) {
            let candidate = runbook.candidate();
            if self.block_reason(&state, &candidate).is_none() {
                return self.override_decision(
                    state,
                    ownership,
                    candidate,
                    DecisionSource::VerifiedRunbook,
                    Some(runbook.header.id),
                    memory_packet,
                    "verified runbook preconditions hold; exact/verified memory outranks fuzzy priors",
                );
            }
        }

        if let Some(exact) = self.store.exact_lookup(&state) {
            let candidate = exact.candidate();
            if self.block_reason(&state, &candidate).is_none() {
                return self.override_decision(
                    state,
                    ownership,
                    candidate,
                    DecisionSource::ExactReplay,
                    Some(exact.header.id),
                    memory_packet,
                    "exact replay matched current exact key; exact memory outranks fuzzy priors",
                );
            }
        }

        let mut candidates = state.candidates.clone();
        candidates.extend(
            self.store
                .policy_candidates(&state, self.config.min_policy_similarity),
        );
        candidates.extend(
            self.store
                .procedural_candidates(&state, self.config.min_procedure_similarity),
        );
        candidates.extend(
            self.store
                .tool_affordance_candidates(&state, self.config.min_policy_similarity),
        );
        candidates = dedupe_merge_candidates(candidates);

        if candidates.is_empty() {
            candidates.push(fallback_candidate(&state));
        }

        {
            let tree = self.store.search_tree_mut(&state.id);
            for candidate in &candidates {
                tree.ensure_candidate(candidate);
            }
        }
        let tree_snapshot = self
            .store
            .search_trees
            .get(&state.id)
            .cloned()
            .unwrap_or_else(|| crate::search::SearchTree::new(&state.id));

        let mut ranked = candidates
            .iter()
            .map(|candidate| {
                let ownership_gain = ownership
                    .uncertainty_reduction_for(&candidate.label())
                    .max(candidate.information_gain * 0.65);
                let utility_score = self.config.value_weights.utility(candidate, ownership_gain);
                let puct_score = tree_snapshot.puct_for(candidate, self.config.search);
                let blocked_reason = self.block_reason(&state, candidate);
                let score = if blocked_reason.is_some() {
                    -1_000_000.0
                } else {
                    utility_score + self.config.search.puct_weight * puct_score
                };
                RankedAction {
                    candidate: candidate.clone(),
                    score,
                    utility_score,
                    puct_score,
                    ownership_gain,
                    blocked_reason,
                }
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let Some(mut selected) = ranked
            .iter()
            .find(|ranked| ranked.blocked_reason.is_none())
            .cloned()
        else {
            let blocked = ActionCandidate::new(AgentAction::Reflect {
                focus: "all candidate actions were blocked; inspect constraints and ask for safe alternative".into(),
            });
            return AgentDecision {
                selected: blocked,
                source: DecisionSource::Blocked,
                source_memory_id: None,
                score: 0.0,
                rationale: "all proposed actions were blocked by current constraints, safety rules, or ko rules".into(),
                ownership,
                ranked_candidates: ranked,
                memory_packet,
            };
        };

        let mut source = if selected.candidate.source_memory_ids.is_empty() {
            DecisionSource::Fallback
        } else {
            DecisionSource::PolicyValueSearch
        };
        let mut rationale = format!(
            "selected by policy/value/search scoring: utility={:.3}, puct={:.3}, ownership_gain={:.3}",
            selected.utility_score, selected.puct_score, selected.ownership_gain
        );

        if should_quiesce(
            &state,
            &ownership,
            &selected.candidate,
            self.config.quiescence_uncertainty_threshold,
        ) {
            if let Some(info_action) = ranked.iter().find(|ranked| {
                ranked.blocked_reason.is_none()
                    && (ranked.candidate.action.is_information_gathering()
                        || read_only_tool(&ranked.candidate, &state.available_tools))
                    && !ranked.candidate.action.is_terminal()
            }) {
                selected = info_action.clone();
                source = DecisionSource::QuiescenceOverride;
                rationale = format!(
                    "quiescence override: uncertainty {:.2} is high, so choose a low-risk information action before a terminal or risky action",
                    ownership.max_uncertainty()
                );
            }
        }

        let selected_candidate = selected.candidate.clone();
        let source_memory_id = selected_candidate.source_memory_ids.first().cloned();
        AgentDecision {
            selected: selected_candidate,
            source,
            source_memory_id,
            score: selected.score,
            rationale,
            ownership,
            ranked_candidates: ranked,
            memory_packet,
        }
    }

    pub fn encode_state(&self, task: AgentTask) -> AgentState {
        self.encoder.encode(task)
    }

    pub fn record_outcome(&mut self, outcome: ActionOutcome) {
        self.store.backup_search(&outcome);
    }

    pub fn record_action_result(
        &mut self,
        state: &AgentState,
        candidate: &ActionCandidate,
        result_signature: impl Into<String>,
        outcome: ActionOutcome,
    ) -> Option<KoRule> {
        self.store.backup_search(&outcome);
        let rule = self.ko_detector.observe(state, candidate, result_signature);
        if let Some(rule) = &rule {
            self.store.ko_rules.push(rule.clone());
            self.store.commit(
                "agentic_memory",
                "add ko rule from repeated state/action/result",
                vec![MemoryDelta::Upsert {
                    kind: AgentMemoryKind::KoRule,
                    id: rule.header.id.clone(),
                    summary: rule.reason.clone(),
                }],
            );
        }
        rule
    }

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
    }

    fn block_reason(&self, state: &AgentState, candidate: &ActionCandidate) -> Option<String> {
        if state.no_side_effects_requested()
            && candidate.is_side_effectful_tool(&state.available_tools)
        {
            return Some(
                "current task constraints request no side effects/read-only behavior".into(),
            );
        }

        if candidate.requires_confirmation
            && !self.config.allow_side_effects_without_confirmation
            && candidate.is_side_effectful_tool(&state.available_tools)
        {
            return Some("side-effectful tool requires confirmation before execution".into());
        }

        if state.output_contract.verifier_required
            && candidate.action.is_terminal()
            && !state
                .observations
                .iter()
                .any(|obs| obs.source == EvidenceSource::Verifier)
        {
            return Some("output contract requires verification before terminal response".into());
        }

        let constraints = state.constraints.join(" ").to_lowercase();
        if constraints.contains("do not ask")
            && matches!(&candidate.action, AgentAction::AskUser { .. })
        {
            return Some("current constraints forbid asking the user".into());
        }

        if let Some(rule) = self.store.safety_blocks(state, candidate) {
            return Some(format!("safety rule `{}`: {}", rule.name, rule.reason));
        }

        if let Some(rule) = self.store.ko_blocks(state, candidate) {
            return Some(format!("ko rule: {}", rule.reason));
        }

        if self.ko_detector.would_repeat(state, candidate) {
            return Some("recent history indicates this action would repeat a loop".into());
        }

        None
    }
}

fn should_quiesce(
    state: &AgentState,
    ownership: &OwnershipMap,
    candidate: &ActionCandidate,
    threshold: f64,
) -> bool {
    ownership.max_uncertainty() >= threshold
        && (candidate.action.is_terminal()
            || candidate.risk >= 0.40
            || candidate.is_side_effectful_tool(&state.available_tools))
}

fn read_only_tool(candidate: &ActionCandidate, tools: &[ToolSpec]) -> bool {
    let Some(tool_name) = candidate.action.tool_name() else {
        return false;
    };
    tools
        .iter()
        .find(|tool| tool.name.as_str() == tool_name)
        .map(|tool| {
            matches!(
                tool.safety_class,
                ToolSafetyClass::Safe | ToolSafetyClass::ReadOnly
            )
        })
        .unwrap_or(false)
}

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
