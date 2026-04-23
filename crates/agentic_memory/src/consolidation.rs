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
