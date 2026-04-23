use crate::search::SearchTree;
use crate::types::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
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
