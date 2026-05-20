use super::*;
use agentic_memory::AgentMemoryKind;
use agentic_memory::Consolidator;
use agentic_memory::EpisodicTrace;
use agentic_memory::EvidenceSource as AgentEvidenceSource;
use agentic_memory::InMemoryAgentStore;
use agentic_memory::MemoryHeader;
use agentic_memory::MemoryPacketItem;
use agentic_memory::Scope;
use anyhow::{Context, Result};
use std::collections::{BTreeSet, HashSet};
const AGENT_MEMORY_MIN_SIMILARITY: f64 = 0.12;
const AGENT_MEMORY_PACKET_ITEMS: usize = 4;
const AGENT_MEMORY_ITEM_CHARS: usize = 220;
pub(super) fn rebuild_agent_memory_store(
    paths: &WorkspaceMemoryPaths,
    sessions: &[&ChatSession],
    final_state_overrides: &BTreeMap<String, String>,
) -> Result<()> {
    let mut store = InMemoryAgentStore::new();
    let consolidator = Consolidator::default();

    for session in sessions {
        let final_state = final_state_overrides.get(&session.id).map(String::as_str);
        for episode in session_episodes(paths, session, final_state) {
            consolidator.consolidate(&mut store, episode);
        }
    }

    store
        .save_json(&paths.agent_memory_store_path)
        .with_context(|| {
            format!(
                "save agent-memory store {}",
                paths.agent_memory_store_path.display()
            )
        })?;
    Ok(())
}
fn load_agent_memory_store(paths: &WorkspaceMemoryPaths) -> Result<Option<InMemoryAgentStore>> {
    if !paths.agent_memory_store_path.is_file() {
        return Ok(None);
    }

    let store =
        InMemoryAgentStore::load_json(&paths.agent_memory_store_path).with_context(|| {
            format!(
                "load agent-memory store {}",
                paths.agent_memory_store_path.display()
            )
        })?;
    Ok(Some(store))
}
fn session_episodes(
    paths: &WorkspaceMemoryPaths,
    session: &ChatSession,
    final_state: Option<&str>,
) -> Vec<EpisodicTrace> {
    let mut episodes = Vec::new();

    for (message_index, message) in session.messages.iter().enumerate() {
        if message.is_user {
            continue;
        }
        let Some((goal, turn_index)) = preceding_user_goal(session, message_index) else {
            continue;
        };
        let body = sanitize_memory_text(&indexed_message_content(message));
        if body.is_empty() {
            continue;
        }

        let doc_key = format!("{}:{}:assistant", session.id, message_index);
        let mut episode = EpisodicTrace::new(
            Scope::project(paths.workspace_id.clone()),
            goal.clone(),
            format!("state:{doc_key}"),
        );
        episode.header.tags = vec![
            format!("doc_key:{doc_key}"),
            format!("chat_id:{}", session.id),
            format!("session_name:{}", sanitize_memory_text(&session.name)),
            format!("role:assistant"),
            format!("message_index:{message_index}"),
            format!("turn_index:{turn_index}"),
            format!("timestamp:{}", message.timestamp.to_rfc3339()),
        ];
        episode.action_labels = extract_action_labels(&message.content);
        episode.observations = extract_observations(&body);
        episode.successful_patterns = extract_successful_patterns(&body);
        episode.final_outcome = episode_outcome_text(
            &body,
            final_state.filter(|_| is_last_message(session, message_index)),
        );
        episodes.push(episode);
    }

    episodes
}
fn packet_item_to_hit(store: &InMemoryAgentStore, item: MemoryPacketItem) -> Option<MemoryHit> {
    let header = find_memory_header(store, &item.id)?;
    let meta = lookup_source_meta(store, header);
    let timestamp = meta
        .as_ref()
        .and_then(|meta| meta.timestamp.as_deref())
        .and_then(parse_timestamp)
        .unwrap_or_else(default_memory_timestamp);

    Some(MemoryHit {
        doc_key: meta
            .as_ref()
            .and_then(|meta| meta.doc_key.clone())
            .unwrap_or_else(|| format!("agentic:{}", item.id)),
        chat_id: meta
            .as_ref()
            .and_then(|meta| meta.chat_id.clone())
            .unwrap_or_else(|| "agentic".to_string()),
        role: meta
            .as_ref()
            .and_then(|meta| meta.role.clone())
            .unwrap_or_else(|| "system".to_string()),
        session_name: meta
            .as_ref()
            .and_then(|meta| meta.session_name.clone())
            .unwrap_or_else(|| "Agent Memory".to_string()),
        timestamp,
        snippet: sanitize_retrieved_memory_text(&item.text),
        title: Some(format!("Agent Memory ({})", kind_label(item.kind))),
        message_index: meta.as_ref().and_then(|meta| meta.message_index),
        turn_index: meta.as_ref().and_then(|meta| meta.turn_index),
        score: Some(item.confidence as f32),
    })
}
fn find_memory_header<'a>(
    store: &'a InMemoryAgentStore,
    item_id: &str,
) -> Option<&'a MemoryHeader> {
    macro_rules! find_vec_header {
        ($items:expr) => {
            if let Some(item) = $items.iter().find(|item| item.header.id == item_id) {
                return Some(&item.header);
            }
        };
    }

    find_vec_header!(store.semantic);
    find_vec_header!(store.user_preferences);
    find_vec_header!(store.project_facts);
    find_vec_header!(store.episodes);
    find_vec_header!(store.procedural);
    find_vec_header!(store.tool_affordances);
    find_vec_header!(store.policy_value);
    find_vec_header!(store.ownership);
    find_vec_header!(store.verified_runbooks);
    find_vec_header!(store.ko_rules);
    find_vec_header!(store.safety_rules);
    store
        .exact_replay
        .values()
        .find(|item| item.header.id == item_id)
        .map(|item| &item.header)
}
fn lookup_source_meta(store: &InMemoryAgentStore, header: &MemoryHeader) -> Option<SourceMeta> {
    parse_source_meta(&header.tags).or_else(|| {
        header
            .evidence
            .iter()
            .find(|evidence| evidence.source == AgentEvidenceSource::Episode)
            .and_then(|evidence| {
                store
                    .episodes
                    .iter()
                    .find(|episode| episode.header.id == evidence.id)
                    .and_then(|episode| parse_source_meta(&episode.header.tags))
            })
    })
}
fn parse_source_meta(tags: &[String]) -> Option<SourceMeta> {
    let mut meta = SourceMeta::default();
    for tag in tags {
        let Some((key, value)) = tag.split_once(':') else {
            continue;
        };
        match key {
            "doc_key" => meta.doc_key = Some(value.to_string()),
            "chat_id" => meta.chat_id = Some(value.to_string()),
            "session_name" => meta.session_name = Some(value.to_string()),
            "role" => meta.role = Some(value.to_string()),
            "message_index" => meta.message_index = value.parse::<usize>().ok(),
            "turn_index" => meta.turn_index = value.parse::<usize>().ok(),
            "timestamp" => meta.timestamp = Some(value.to_string()),
            _ => {}
        }
    }
    meta.doc_key.as_ref()?;
    Some(meta)
}
fn preceding_user_goal(session: &ChatSession, message_index: usize) -> Option<(String, usize)> {
    session.messages[..message_index]
        .iter()
        .enumerate()
        .rev()
        .find(|(_, message)| message.is_user)
        .map(|(index, message)| {
            (
                sanitize_memory_text(&message.content),
                message_turn_index(session, index),
            )
        })
        .filter(|(goal, _)| !goal.is_empty())
}
fn extract_action_labels(content: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut labels = Vec::new();

    for line in content.lines() {
        let stripped = strip_standard_transcript_timestamp_prefix(line.trim());
        let Some((model, detail)) = stripped.split_once(" | ") else {
            continue;
        };
        if !looks_like_standard_model_label(model.trim()) {
            continue;
        }
        let detail = sanitize_memory_text(detail);
        if detail.is_empty() || !seen.insert(detail.clone()) {
            continue;
        }
        labels.push(detail);
        if labels.len() >= 4 {
            break;
        }
    }

    labels
}
fn extract_observations(body: &str) -> Vec<String> {
    body.lines()
        .map(sanitize_memory_text)
        .filter(|line| !line.is_empty())
        .take(3)
        .collect()
}
fn extract_successful_patterns(body: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut patterns = Vec::new();

    for line in body.lines() {
        let line = truncate_text(&sanitize_memory_text(line), 120);
        if line.len() < 8 || !seen.insert(line.clone()) {
            continue;
        }
        patterns.push(line);
        if patterns.len() >= 2 {
            break;
        }
    }

    patterns
}
fn episode_outcome_text(body: &str, final_state: Option<&str>) -> String {
    let summary = truncate_text(&sanitize_memory_text(body), 140);
    match final_state.map(str::trim).filter(|state| !state.is_empty()) {
        Some("done") => {
            if summary.is_empty() {
                "resolved".to_string()
            } else {
                format!("resolved: {summary}")
            }
        }
        Some("stopped") => "stopped".to_string(),
        Some("error") => {
            if summary.is_empty() {
                "error".to_string()
            } else {
                format!("error: {summary}")
            }
        }
        Some(other) => format!("{}: {}", sanitize_memory_text(other), summary),
        None => {
            if summary.is_empty() {
                "recorded".to_string()
            } else {
                format!("resolved: {summary}")
            }
        }
    }
}
fn is_last_message(session: &ChatSession, message_index: usize) -> bool {
    message_index + 1 == session.messages.len()
}
fn truncate_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let mut out = value
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    out.push_str("...");
    out
}
fn kind_label(kind: AgentMemoryKind) -> &'static str {
    match kind {
        AgentMemoryKind::SemanticFact => "fact",
        AgentMemoryKind::UserPreference => "preference",
        AgentMemoryKind::ProjectFact => "project",
        AgentMemoryKind::EpisodicTrace => "episode",
        AgentMemoryKind::ProceduralSkill => "procedure",
        AgentMemoryKind::ToolAffordance => "tool",
        AgentMemoryKind::PolicyValuePrior => "policy",
        AgentMemoryKind::Ownership => "ownership",
        AgentMemoryKind::ExactReplay => "exact",
        AgentMemoryKind::VerifiedRunbook => "runbook",
        AgentMemoryKind::KoRule => "ko",
        AgentMemoryKind::SafetyRule => "safety",
    }
}
#[derive(Default)]
struct SourceMeta {
    doc_key: Option<String>,
    chat_id: Option<String>,
    session_name: Option<String>,
    role: Option<String>,
    message_index: Option<usize>,
    turn_index: Option<usize>,
    timestamp: Option<String>,
}
