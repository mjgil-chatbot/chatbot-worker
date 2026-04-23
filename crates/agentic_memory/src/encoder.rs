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
