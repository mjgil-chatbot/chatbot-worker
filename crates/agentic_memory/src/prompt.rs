use crate::encoder::token_similarity;
use crate::store::{AgentMemoryStore, InMemoryAgentStore};
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
    }
}

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
    pub fn build(
        &self,
        store: &InMemoryAgentStore,
        state: &AgentState,
        ownership: &OwnershipMap,
    ) -> PromptMemoryPacket {
        let mut packet = PromptMemoryPacket {
            state_id: state.id.clone(),
            exact_key: state.exact_key.clone(),
            selected_regions: ownership.uncertain_regions(0.45),
            items: Vec::new(),
            warnings: Vec::new(),
        };

        if packet.selected_regions.is_empty() {
            packet.selected_regions = vec!["user_goal".into(), "constraints".into()];
        }
        let query = regional_query(state, &packet.selected_regions);

        if self.config.include_verified {
            if let Some(runbook) = store.verified_runbook_lookup(state) {
                self.push_item(
                    &mut packet,
                    MemoryPacketItem {
                        kind: AgentMemoryKind::VerifiedRunbook,
                        id: runbook.header.id.clone(),
                        region: "verified".into(),
                        text: format!(
                            "Verified runbook `{}` applies. Next action: {}. Preconditions: {}.",
                            runbook.name,
                            runbook.recommended_next_action.label(),
                            runbook.preconditions.join("; ")
                        ),
                        confidence: runbook.header.confidence,
                        source: "verified_runbook".into(),
                    },
                );
            }
        }

        if self.config.include_exact {
            if let Some(exact) = store.exact_lookup(state) {
                self.push_item(
                    &mut packet,
                    MemoryPacketItem {
                        kind: AgentMemoryKind::ExactReplay,
                        id: exact.header.id.clone(),
                        region: "exact_state".into(),
                        text: format!(
                            "Exact replay matched. Best action: {}. Outcome: {}.",
                            exact.best_action.label(),
                            exact.outcome_summary
                        ),
                        confidence: exact.header.confidence,
                        source: "exact_replay".into(),
                    },
                );
            }
        }

        for fact in &store.semantic {
            let text = format!("{} {} {}", fact.subject, fact.predicate, fact.object);
            if fact
                .header
                .is_valid_for(&state.scope, &state.changed_artifacts)
                && token_similarity(&query, &text) >= self.config.min_similarity
            {
                self.push_item(
                    &mut packet,
                    item_from_header(&fact.header, AgentMemoryKind::SemanticFact, "fact", text),
                );
            }
        }

        for pref in &store.user_preferences {
            if pref
                .header
                .is_valid_for(&state.scope, &state.changed_artifacts)
                && token_similarity(&query, &pref.preference) >= self.config.min_similarity
            {
                self.push_item(
                    &mut packet,
                    item_from_header(
                        &pref.header,
                        AgentMemoryKind::UserPreference,
                        "user_preference",
                        format!(
                            "{}{}",
                            pref.preference,
                            if pref.not_a_constraint {
                                " (influence, not a hard constraint)"
                            } else {
                                ""
                            }
                        ),
                    ),
                );
            }
        }

        for fact in &store.project_facts {
            if fact
                .header
                .is_valid_for(&state.scope, &state.changed_artifacts)
                && token_similarity(&query, &fact.fact) >= self.config.min_similarity
            {
                self.push_item(
                    &mut packet,
                    item_from_header(
                        &fact.header,
                        AgentMemoryKind::ProjectFact,
                        "project",
                        fact.fact.clone(),
                    ),
                );
            }
        }

        for skill in &store.procedural {
            let text = format!("{} {}", skill.name, skill.applies_when.join(" "));
            if skill
                .header
                .is_valid_for(&state.scope, &state.changed_artifacts)
                && token_similarity(&query, &text) >= self.config.min_similarity
            {
                let steps = skill
                    .steps
                    .iter()
                    .map(|step| step.label.clone())
                    .collect::<Vec<_>>()
                    .join(" -> ");
                self.push_item(
                    &mut packet,
                    item_from_header(
                        &skill.header,
                        AgentMemoryKind::ProceduralSkill,
                        "procedure",
                        format!("Skill `{}`: {}", skill.name, steps),
                    ),
                );
            }
        }

        for tool in &store.tool_affordances {
            let text = format!("{} {}", tool.tool_name, tool.works_well_for.join(" "));
            if tool
                .header
                .is_valid_for(&state.scope, &state.changed_artifacts)
                && token_similarity(&query, &text) >= self.config.min_similarity
            {
                self.push_item(
                    &mut packet,
                    item_from_header(
                        &tool.header,
                        AgentMemoryKind::ToolAffordance,
                        "tooling",
                        format!(
                            "Tool `{}` works well for: {}",
                            tool.tool_name,
                            tool.works_well_for.join(", ")
                        ),
                    ),
                );
            }
        }

        for policy in &store.policy_value {
            if policy
                .header
                .is_valid_for(&state.scope, &state.changed_artifacts)
                && token_similarity(&query, &policy.state_pattern) >= self.config.min_similarity
            {
                let actions = policy
                    .actions
                    .iter()
                    .take(3)
                    .map(|action| {
                        format!(
                            "{} p={:.2} q={:.2}",
                            action.action.label(),
                            action.prior,
                            action.expected_success
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                self.push_item(
                    &mut packet,
                    item_from_header(
                        &policy.header,
                        AgentMemoryKind::PolicyValuePrior,
                        "policy_value",
                        format!("Pattern `{}` suggests: {}", policy.state_pattern, actions),
                    ),
                );
            }
        }

        if self.config.include_episodes {
            for episode in &store.episodes {
                let text = format!(
                    "{} {} {}",
                    episode.initial_goal,
                    episode.successful_patterns.join(" "),
                    episode.final_outcome
                );
                if episode
                    .header
                    .is_valid_for(&state.scope, &state.changed_artifacts)
                    && token_similarity(&query, &text) >= self.config.min_similarity
                {
                    self.push_item(
                        &mut packet,
                        item_from_header(
                            &episode.header,
                            AgentMemoryKind::EpisodicTrace,
                            "episode",
                            format!(
                                "Past episode `{}` ended `{}`; useful patterns: {}",
                                episode.initial_goal,
                                episode.final_outcome,
                                episode.successful_patterns.join(", ")
                            ),
                        ),
                    );
                }
            }
        }

        if packet.items.is_empty() {
            packet.warnings.push("No matching durable memories found; rely on current evidence and low-risk information gathering.".into());
        }

        packet
    }

    fn push_item(&self, packet: &mut PromptMemoryPacket, mut item: MemoryPacketItem) {
        if packet.items.len() >= self.config.max_items {
            return;
        }
        if item.text.chars().count() > self.config.max_chars_per_item {
            item.text = truncate_chars(&item.text, self.config.max_chars_per_item);
        }
        packet.items.push(item);
    }
}

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
