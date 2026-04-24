use crate::types::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub fn stable_hash(input: &str) -> String {
    stable_hash_text(input)
}

pub fn normalize_text(input: &str) -> String {
    input
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn token_similarity(a: &str, b: &str) -> f64 {
    let a_tokens = token_set(a);
    let b_tokens = token_set(b);
    if a_tokens.is_empty() || b_tokens.is_empty() {
        return 0.0;
    }
    let intersection = a_tokens.intersection(&b_tokens).count() as f64;
    let union = a_tokens.union(&b_tokens).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn token_set(input: &str) -> BTreeSet<String> {
    input
        .to_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 2)
        .map(ToOwned::to_owned)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AgentStateEncoder {
    /// Include message text in the exact key. Use only for deterministic tasks;
    /// most agent tasks should keep exactness restricted to stable fingerprints.
    pub include_messages_in_exact_key: bool,
    /// Include observation text in the exact key. Useful for repeated API/tool
    /// response shapes, exact error messages, or document-question pairs.
    pub include_observations_in_exact_key: bool,
}

impl Default for AgentStateEncoder {
    fn default() -> Self {
        Self {
            include_messages_in_exact_key: false,
            include_observations_in_exact_key: false,
        }
    }
}

impl AgentStateEncoder {
    pub fn encode(&self, task: AgentTask) -> AgentState {
        let goal = task.goal;
        let scope = task.scope;
        let messages = task.messages;
        let observations = task.observations;
        let constraints = dedupe_keep_original(task.constraints);
        let unknowns = dedupe_keep_original(task.unknowns);
        let artifacts = dedupe_keep_original(task.artifacts);
        let available_tools = task.available_tools;
        let output_contract = task.output_contract;

        let mut exact_parts = Vec::new();
        exact_parts.push(format!("scope:{}", scope.text()));
        exact_parts.extend(
            task.exact_fingerprint_fields
                .into_iter()
                .map(|field| format!("field:{field}")),
        );
        exact_parts.extend(
            observations
                .iter()
                .filter_map(|obs| obs.error_signature.as_ref())
                .map(|sig| format!("error:{sig}")),
        );

        if self.include_messages_in_exact_key {
            exact_parts.extend(
                messages
                    .iter()
                    .map(|msg| format!("msg:{:?}:{}", msg.role, msg.content)),
            );
        }
        if self.include_observations_in_exact_key {
            exact_parts.extend(
                observations
                    .iter()
                    .map(|obs| format!("obs:{}", obs.content)),
            );
        }
        if exact_parts.len() == 1 {
            exact_parts.push(format!("goal:{}", normalize_text(&goal)));
        }
        exact_parts.sort();
        exact_parts.dedup();
        let exact_key = format!("exact_{}", stable_hash(&exact_parts.join("|")));

        let mut fuzzy_terms = Vec::new();
        fuzzy_terms.push(goal.clone());
        fuzzy_terms.extend(messages.iter().map(|msg| msg.content.clone()));
        fuzzy_terms.extend(observations.iter().map(|obs| obs.content.clone()));
        fuzzy_terms.extend(constraints.clone());
        fuzzy_terms.extend(unknowns.clone());
        fuzzy_terms.extend(artifacts.clone());
        fuzzy_terms.extend(available_tools.iter().flat_map(|tool| {
            let mut terms = vec![tool.name.clone(), tool.description.clone()];
            terms.extend(tool.affordances.clone());
            terms
        }));
        fuzzy_terms = dedupe_keep_original(fuzzy_terms);

        let mut candidates = task.candidate_actions;
        candidates.extend(default_candidates(
            &goal,
            &unknowns,
            &constraints,
            &available_tools,
            &output_contract,
        ));
        candidates = dedupe_candidates(candidates);

        let mut ownership = build_initial_ownership(
            &goal,
            &constraints,
            &unknowns,
            &available_tools,
            &output_contract,
        );
        for obs in &observations {
            if obs.error_signature.is_some() {
                ownership.insert(OwnershipRegion::new(
                    "failure_signature",
                    RegionStatus::Owned,
                    0.88,
                ));
            }
        }

        let state_material = format!(
            "{}|{}|{}",
            exact_key,
            normalize_text(&goal),
            fuzzy_terms.join("|")
        );
        AgentState {
            id: format!("state_{}", stable_hash(&state_material)),
            exact_key,
            scope,
            goal,
            messages,
            observations,
            constraints,
            unknowns,
            changed_artifacts: artifacts.clone(),
            artifacts,
            fuzzy_terms,
            available_tools,
            candidates,
            ownership,
            output_contract,
        }
    }
}

fn default_candidates(
    goal: &str,
    unknowns: &[String],
    constraints: &[String],
    tools: &[ToolSpec],
    output_contract: &OutputContract,
) -> Vec<ActionCandidate> {
    let mut candidates = Vec::new();

    if !goal.trim().is_empty() {
        let mut plan = ActionCandidate::new(AgentAction::Plan {
            steps: vec![
                "identify knowns".into(),
                "resolve highest-uncertainty region".into(),
            ],
        });
        plan.prior = 0.28;
        plan.expected_success = 0.55;
        plan.information_gain = 0.45;
        plan.cost = 0.10;
        plan.risk = 0.03;
        plan.reason = "planning is a safe fallback for open-ended agent tasks".into();
        candidates.push(plan);
    }

    if !unknowns.is_empty() {
        let mut retrieve = ActionCandidate::new(AgentAction::RetrieveMemory {
            query: format!("{} {}", goal, unknowns.join(" ")),
            regions: unknowns.to_vec(),
        });
        retrieve.prior = 0.42;
        retrieve.expected_success = 0.58;
        retrieve.information_gain = 0.72;
        retrieve.cost = 0.15;
        retrieve.risk = 0.02;
        retrieve.reason = "unknown task regions should retrieve regional memory first".into();
        candidates.push(retrieve);

        let mut ask = ActionCandidate::new(AgentAction::AskUser {
            question: format!("Can you clarify: {}?", unknowns.join(", ")),
        });
        ask.prior = 0.22;
        ask.expected_success = 0.48;
        ask.information_gain = 0.80;
        ask.cost = 0.35;
        ask.risk = 0.02;
        ask.reason = "open unknowns may require user input".into();
        candidates.push(ask);
    }

    for tool in tools {
        let mut tool_candidate = ActionCandidate::new(AgentAction::UseTool {
            tool_name: tool.name.clone(),
            args: Value::Object(Default::default()),
            expected_observation: Some(tool.description.clone()),
        });
        tool_candidate.prior = if tool.safety_class == ToolSafetyClass::ReadOnly {
            0.40
        } else {
            0.18
        };
        tool_candidate.expected_success = 0.55;
        tool_candidate.information_gain = if tool.safety_class.is_side_effectful() {
            0.35
        } else {
            0.62
        };
        tool_candidate.cost = clamp01(tool.cost);
        tool_candidate.latency = clamp01(tool.latency);
        tool_candidate.risk = clamp01(tool.risk);
        tool_candidate.reversibility = if tool.safety_class.is_side_effectful() {
            0.35
        } else {
            0.95
        };
        tool_candidate.requires_confirmation = tool.safety_class.is_side_effectful();
        tool_candidate.reason = format!("available tool: {}", tool.description);
        candidates.push(tool_candidate);
    }

    if output_contract.verifier_required
        || constraints
            .iter()
            .any(|c| c.to_lowercase().contains("verify"))
    {
        let mut verify = ActionCandidate::new(AgentAction::Verify {
            claim: goal.to_string(),
            verifier: "default_verifier".into(),
        });
        verify.prior = 0.50;
        verify.expected_success = 0.62;
        verify.information_gain = 0.70;
        verify.cost = 0.20;
        verify.risk = 0.03;
        verify.reason = "task or output contract requires verification".into();
        candidates.push(verify);
    }

    if unknowns.is_empty() && !goal.trim().is_empty() {
        let mut respond = ActionCandidate::new(AgentAction::Respond {
            message: "answer using current evidence".into(),
        });
        respond.prior = 0.30;
        respond.expected_success = 0.50;
        respond.information_gain = 0.05;
        respond.cost = 0.08;
        respond.risk = 0.10;
        respond.reason = "no explicit unknowns remain".into();
        candidates.push(respond);
    }

    candidates
}

fn build_initial_ownership(
    goal: &str,
    constraints: &[String],
    unknowns: &[String],
    tools: &[ToolSpec],
    output_contract: &OutputContract,
) -> OwnershipMap {
    let mut map = OwnershipMap::default();
    map.insert(OwnershipRegion::new(
        "user_goal",
        if goal.trim().is_empty() {
            RegionStatus::Unknown
        } else {
            RegionStatus::Owned
        },
        if goal.trim().is_empty() { 0.10 } else { 0.86 },
    ));

    map.insert(OwnershipRegion::new(
        "constraints",
        if constraints.is_empty() {
            RegionStatus::Unknown
        } else {
            RegionStatus::Owned
        },
        if constraints.is_empty() { 0.35 } else { 0.82 },
    ));

    if unknowns.is_empty() {
        map.insert(OwnershipRegion::new(
            "open_unknowns",
            RegionStatus::Owned,
            0.70,
        ));
    } else {
        let mut region = OwnershipRegion::new("open_unknowns", RegionStatus::Unknown, 0.18);
        region.blocking_unknowns = unknowns.to_vec();
        region.information_gain_actions =
            vec!["retrieve memory".into(), "ask user".into(), "verify".into()];
        map.insert(region);
    }

    let mut tools_region = OwnershipRegion::new(
        "tooling",
        if tools.is_empty() {
            RegionStatus::Unknown
        } else {
            RegionStatus::Owned
        },
        if tools.is_empty() { 0.35 } else { 0.76 },
    );
    tools_region.candidates = tools.iter().map(|tool| tool.name.clone()).collect();
    tools_region.information_gain_actions = tools
        .iter()
        .map(|tool| format!("use tool {}", tool.name))
        .collect();
    map.insert(tools_region);

    if output_contract.citation_required
        || output_contract.verifier_required
        || output_contract.format.is_some()
    {
        let mut region = OwnershipRegion::new("output_contract", RegionStatus::Risky, 0.64);
        region.information_gain_actions = vec!["verify".into(), "retrieve memory".into()];
        map.insert(region);
    }

    let no_side_effects = constraints.iter().any(|constraint| {
        let lower = constraint.to_lowercase();
        lower.contains("do not modify")
            || lower.contains("read only")
            || lower.contains("no side effect")
            || lower.contains("do not call external")
    });
    if no_side_effects
        && tools
            .iter()
            .any(|tool| tool.safety_class.is_side_effectful())
    {
        let mut region = OwnershipRegion::new("side_effect_safety", RegionStatus::Risky, 0.70);
        region.blocking_unknowns =
            vec!["side-effectful tools are available but constrained".into()];
        map.insert(region);
    }

    map
}

fn dedupe_keep_original(items: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for item in items {
        let key = normalize_text(&item);
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        out.push(item);
    }
    out
}

fn dedupe_candidates(candidates: Vec<ActionCandidate>) -> Vec<ActionCandidate> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for candidate in candidates {
        let key = normalize_text(&candidate.label());
        if seen.insert(key) {
            out.push(candidate);
        }
    }
    out
}
