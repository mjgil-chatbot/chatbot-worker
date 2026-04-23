use crate::encoder::AgentStateEncoder;
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentMemorySystem {
    pub store: InMemoryAgentStore,
    pub encoder: AgentStateEncoder,
    pub config: AgentMemoryConfig,
    pub ko_detector: KoDetector,
}
