use crate::types::*;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MemoryPacketConfig {
    pub max_items: usize,
    pub max_chars_per_item: usize,
    pub min_similarity: f64,
    pub include_verified: bool,
    pub include_exact: bool,
    pub include_episodes: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryPacketItem {
    pub kind: AgentMemoryKind,
    pub id: String,
    pub region: String,
    pub text: String,
    pub confidence: f64,
    pub source: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptMemoryPacket {
    pub state_id: String,
    pub exact_key: String,
    pub selected_regions: Vec<String>,
    pub items: Vec<MemoryPacketItem>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptMemoryPacketBuilder {
    pub config: MemoryPacketConfig,
}
