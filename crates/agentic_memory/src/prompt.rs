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
impl Default for MemoryPacketConfig {
    fn default() -> Self {
        Self {
            max_items: 12,
            max_chars_per_item: 360,
            min_similarity: 0.06,
            include_verified: true,
            include_exact: true,
            include_episodes: true,
        }
    }
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
impl PromptMemoryPacket {
    pub fn to_prompt_block(&self) -> String {
        let mut lines = Vec::new();
        lines.push("<agent_memory_packet>".to_string());
        lines.push(format!("state_id: {}", self.state_id));
        lines.push(format!("exact_key: {}", self.exact_key));
        if !self.selected_regions.is_empty() {
            lines.push(format!("regions: {}", self.selected_regions.join(", ")));
        }
        for item in &self.items {
            lines.push(format!(
                "- [{:?} | {:.2} | region={}] {}",
                item.kind, item.confidence, item.region, item.text
            ));
        }
        for warning in &self.warnings {
            lines.push(format!("warning: {warning}"));
        }
        lines.push("</agent_memory_packet>".to_string());
        lines.join("\n")
    }}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptMemoryPacketBuilder {
    pub config: MemoryPacketConfig,
}
impl Default for PromptMemoryPacketBuilder {
    fn default() -> Self {
        Self {
            config: MemoryPacketConfig::default(),
        }
    }
}
impl PromptMemoryPacketBuilder {
    fn push_item(&self, packet: &mut PromptMemoryPacket, mut item: MemoryPacketItem) {
        if packet.items.len() >= self.config.max_items {
            return;
        }
        if item.text.chars().count() > self.config.max_chars_per_item {
            item.text = truncate_chars(&item.text, self.config.max_chars_per_item);
        }
        packet.items.push(item);
    }}
fn item_from_header(
    header: &MemoryHeader,
    kind: AgentMemoryKind,
    region: impl Into<String>,
    text: String,
) -> MemoryPacketItem {
    MemoryPacketItem {
        kind,
        id: header.id.clone(),
        region: region.into(),
        text,
        confidence: header.confidence,
        source: header.summary.clone(),
    }
}
fn regional_query(state: &AgentState, regions: &[String]) -> String {
    let mut parts = vec![state.goal.clone(), regions.join(" ")];
    parts.extend(state.unknowns.clone());
    parts.extend(state.constraints.clone());
    parts.extend(state.artifacts.clone());
    parts.extend(state.observations.iter().map(|obs| obs.content.clone()));
    parts.join(" ")
}
fn truncate_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        input.to_string()
    } else {
        let mut out = input
            .chars()
            .take(max_chars.saturating_sub(3))
            .collect::<String>();
        out.push_str("...");
        out
    }
}
