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
