use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub type TimestampMillis = u128;

pub fn now_millis() -> TimestampMillis {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

pub fn clamp01(value: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else if value < 0.0 {
        0.0
    } else if value > 1.0 {
        1.0
    } else {
        value
    }
}

/// Deterministic FNV-1a hash used for IDs and exact keys. This is not intended
/// as a cryptographic hash; swap it for BLAKE3/SHA-256 in a production store.
pub fn stable_hash_text(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn slug(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars().take(36) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Scope {
    pub user_id: Option<String>,
    pub org_id: Option<String>,
    pub project_id: Option<String>,
    pub agent_id: Option<String>,
    pub domain: Option<String>,
}

impl Scope {
    pub fn global() -> Self {
        Self::default()
    }

    pub fn user(user_id: impl Into<String>) -> Self {
        Self {
            user_id: Some(user_id.into()),
            ..Self::default()
        }
    }

    pub fn project(project_id: impl Into<String>) -> Self {
        Self {
            project_id: Some(project_id.into()),
            ..Self::default()
        }
    }

    pub fn domain(domain: impl Into<String>) -> Self {
        Self {
            domain: Some(domain.into()),
            ..Self::default()
        }
    }

    /// A memory with a missing field is treated as broadly applicable. If both
    /// sides provide a field, they must agree.
    pub fn matches(&self, other: &Scope) -> bool {
        fn field_matches(a: &Option<String>, b: &Option<String>) -> bool {
            a.is_none() || b.is_none() || a == b
        }

        field_matches(&self.user_id, &other.user_id)
            && field_matches(&self.org_id, &other.org_id)
            && field_matches(&self.project_id, &other.project_id)
            && field_matches(&self.agent_id, &other.agent_id)
            && field_matches(&self.domain, &other.domain)
    }

    pub fn text(&self) -> String {
        [
            self.user_id.as_deref().unwrap_or_default(),
            self.org_id.as_deref().unwrap_or_default(),
            self.project_id.as_deref().unwrap_or_default(),
            self.agent_id.as_deref().unwrap_or_default(),
            self.domain.as_deref().unwrap_or_default(),
        ]
        .join(" ")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceSource {
    UserMessage,
    SystemInstruction,
    ToolObservation,
    Verifier,
    HumanFeedback,
    MemoryCommit,
    Episode,
    Synthetic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub source: EvidenceSource,
    pub id: String,
    pub summary: String,
    pub confidence: f64,
    pub timestamp_ms: TimestampMillis,
    pub uri: Option<String>,
}

impl Evidence {
    pub fn new(source: EvidenceSource, id: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            source,
            id: id.into(),
            summary: summary.into(),
            confidence: 1.0,
            timestamp_ms: now_millis(),
            uri: None,
        }
    }

    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = clamp01(confidence);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Invalidation {
    pub triggers: Vec<String>,
    pub changed_artifacts: Vec<String>,
    pub expires_at_ms: Option<TimestampMillis>,
    pub notes: Option<String>,
}

impl Invalidation {
    pub fn valid_for_changed_artifacts(&self, changed_artifacts: &[String]) -> bool {
        if self.changed_artifacts.is_empty() || changed_artifacts.is_empty() {
            return true;
        }
        let changed = changed_artifacts
            .iter()
            .map(|item| item.to_lowercase())
            .collect::<Vec<_>>();
        !self
            .changed_artifacts
            .iter()
            .map(|item| item.to_lowercase())
            .any(|artifact| {
                changed
                    .iter()
                    .any(|changed| changed.contains(&artifact) || artifact.contains(changed))
            })
    }

    pub fn is_not_expired(&self, now: TimestampMillis) -> bool {
        self.expires_at_ms
            .map(|expires| expires > now)
            .unwrap_or(true)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentMemoryKind {
    SemanticFact,
    UserPreference,
    ProjectFact,
    EpisodicTrace,
    ProceduralSkill,
    ToolAffordance,
    PolicyValuePrior,
    Ownership,
    ExactReplay,
    VerifiedRunbook,
    KoRule,
    SafetyRule,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryHeader {
    pub id: String,
    pub kind: AgentMemoryKind,
    pub scope: Scope,
    pub summary: String,
    pub tags: Vec<String>,
    pub created_at_ms: TimestampMillis,
    pub updated_at_ms: TimestampMillis,
    pub confidence: f64,
    pub evidence: Vec<Evidence>,
    pub invalidation: Invalidation,
    pub use_count: u64,
    pub failure_count: u64,
}

impl MemoryHeader {
    pub fn new(kind: AgentMemoryKind, scope: Scope, summary: impl Into<String>) -> Self {
        let summary = summary.into();
        let timestamp = now_millis();
        let id = format!(
            "mem_{}_{}",
            stable_hash_text(&format!("{kind:?}:{summary}:{}", scope.text())),
            slug(&summary)
        );
        Self {
            id,
            kind,
            scope,
            summary,
            tags: Vec::new(),
            created_at_ms: timestamp,
            updated_at_ms: timestamp,
            confidence: 0.75,
            evidence: Vec::new(),
            invalidation: Invalidation::default(),
            use_count: 0,
            failure_count: 0,
        }
    }

    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = clamp01(confidence);
        self
    }

    pub fn with_tags(mut self, tags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_evidence(mut self, evidence: Evidence) -> Self {
        self.evidence.push(evidence);
        self
    }

    pub fn touch(&mut self, success: bool) {
        self.updated_at_ms = now_millis();
        self.use_count += 1;
        if !success {
            self.failure_count += 1;
            self.confidence = clamp01(self.confidence * 0.85);
        }
    }

    pub fn is_valid_for(&self, scope: &Scope, changed_artifacts: &[String]) -> bool {
        self.scope.matches(scope)
            && self.invalidation.is_not_expired(now_millis())
            && self
                .invalidation
                .valid_for_changed_artifacts(changed_artifacts)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
    Developer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentMessage {
    pub role: MessageRole,
    pub content: String,
    pub timestamp_ms: TimestampMillis,
}

impl AgentMessage {
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            timestamp_ms: now_millis(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolSafetyClass {
    Safe,
    ReadOnly,
    ExternalSideEffect,
    Write,
    Privileged,
    Dangerous,
}

impl ToolSafetyClass {
    pub fn default_risk(self) -> f64 {
        match self {
            ToolSafetyClass::Safe => 0.02,
            ToolSafetyClass::ReadOnly => 0.05,
            ToolSafetyClass::ExternalSideEffect => 0.35,
            ToolSafetyClass::Write => 0.45,
            ToolSafetyClass::Privileged => 0.65,
            ToolSafetyClass::Dangerous => 0.95,
        }
    }

    pub fn is_side_effectful(self) -> bool {
        matches!(
            self,
            ToolSafetyClass::ExternalSideEffect
                | ToolSafetyClass::Write
                | ToolSafetyClass::Privileged
                | ToolSafetyClass::Dangerous
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub affordances: Vec<String>,
    pub constraints: Vec<String>,
    pub cost: f64,
    pub latency: f64,
    pub risk: f64,
    pub safety_class: ToolSafetyClass,
}

impl ToolSpec {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        safety_class: ToolSafetyClass,
    ) -> Self {
        let safety_class = safety_class;
        Self {
            name: name.into(),
            description: description.into(),
            input_schema: Value::Object(Default::default()),
            affordances: Vec::new(),
            constraints: Vec::new(),
            cost: 0.15,
            latency: 0.15,
            risk: safety_class.default_risk(),
            safety_class,
        }
    }

    pub fn read_only(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self::new(name, description, ToolSafetyClass::ReadOnly)
    }

    pub fn write(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self::new(name, description, ToolSafetyClass::Write)
    }

    pub fn with_affordances(
        mut self,
        affordances: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.affordances = affordances.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_cost(mut self, cost: f64) -> Self {
        self.cost = clamp01(cost);
        self
    }

    pub fn with_risk(mut self, risk: f64) -> Self {
        self.risk = clamp01(risk);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentObservation {
    pub id: String,
    pub source: EvidenceSource,
    pub content: String,
    pub artifact_refs: Vec<String>,
    pub error_signature: Option<String>,
    pub tool_name: Option<String>,
    pub timestamp_ms: TimestampMillis,
}

impl AgentObservation {
    pub fn new(source: EvidenceSource, content: impl Into<String>) -> Self {
        let content = content.into();
        Self {
            id: format!("obs_{}", stable_hash_text(&content)),
            source,
            content,
            artifact_refs: Vec::new(),
            error_signature: None,
            tool_name: None,
            timestamp_ms: now_millis(),
        }
    }

    pub fn tool(tool_name: impl Into<String>, content: impl Into<String>) -> Self {
        let tool_name = tool_name.into();
        let mut observation = Self::new(EvidenceSource::ToolObservation, content);
        observation.tool_name = Some(tool_name);
        observation
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegionStatus {
    Owned,
    Unknown,
    Contested,
    Risky,
    Stale,
    Blocked,
}

impl RegionStatus {
    pub fn uncertainty(self) -> f64 {
        match self {
            RegionStatus::Owned => 0.05,
            RegionStatus::Unknown => 0.90,
            RegionStatus::Contested => 0.72,
            RegionStatus::Risky => 0.62,
            RegionStatus::Stale => 0.55,
            RegionStatus::Blocked => 0.80,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnershipRegion {
    pub name: String,
    pub status: RegionStatus,
    pub confidence: f64,
    pub candidates: Vec<String>,
    pub blocking_unknowns: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub information_gain_actions: Vec<String>,
}

impl OwnershipRegion {
    pub fn new(name: impl Into<String>, status: RegionStatus, confidence: f64) -> Self {
        Self {
            name: name.into(),
            status,
            confidence: clamp01(confidence),
            candidates: Vec::new(),
            blocking_unknowns: Vec::new(),
            evidence_ids: Vec::new(),
            information_gain_actions: Vec::new(),
        }
    }

    pub fn uncertainty(&self) -> f64 {
        clamp01((1.0 - self.confidence) * 0.45 + self.status.uncertainty() * 0.55)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OwnershipMap {
    pub regions: BTreeMap<String, OwnershipRegion>,
}

impl OwnershipMap {
    pub fn insert(&mut self, region: OwnershipRegion) {
        self.regions.insert(region.name.clone(), region);
    }

    pub fn max_uncertainty(&self) -> f64 {
        self.regions
            .values()
            .map(OwnershipRegion::uncertainty)
            .fold(0.0, f64::max)
    }

    pub fn uncertain_regions(&self, threshold: f64) -> Vec<String> {
        self.regions
            .values()
            .filter(|region| region.uncertainty() >= threshold)
            .map(|region| region.name.clone())
            .collect()
    }

    pub fn merge_prefer_confident(&mut self, other: OwnershipMap) {
        for (name, incoming) in other.regions {
            match self.regions.get(&name) {
                Some(existing) if existing.confidence >= incoming.confidence => {}
                _ => {
                    self.regions.insert(name, incoming);
                }
            }
        }
    }

    pub fn uncertainty_reduction_for(&self, action_label: &str) -> f64 {
        let action = action_label.to_lowercase();
        let mut gain: f64 = 0.0;
        for region in self.regions.values() {
            let region_matches = region.information_gain_actions.iter().any(|candidate| {
                action.contains(&candidate.to_lowercase())
                    || candidate.to_lowercase().contains(&action)
            });
            let implicit_matches = (action.contains("ask")
                && region.status == RegionStatus::Unknown)
                || (action.contains("verify")
                    && matches!(region.status, RegionStatus::Contested | RegionStatus::Risky))
                || (action.contains("read")
                    && matches!(region.status, RegionStatus::Unknown | RegionStatus::Stale))
                || (action.contains("inspect") && region.status != RegionStatus::Owned)
                || (action.contains("retrieve") && region.status != RegionStatus::Owned);
            if region_matches || implicit_matches {
                gain = gain.max(region.uncertainty());
            }
        }
        clamp01(gain)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentAction {
    Respond {
        message: String,
    },
    AskUser {
        question: String,
    },
    UseTool {
        tool_name: String,
        args: Value,
        expected_observation: Option<String>,
    },
    RetrieveMemory {
        query: String,
        regions: Vec<String>,
    },
    Verify {
        claim: String,
        verifier: String,
    },
    Plan {
        steps: Vec<String>,
    },
    Reflect {
        focus: String,
    },
    Delegate {
        agent: String,
        task: String,
    },
    Finish {
        summary: String,
    },
}

impl AgentAction {
    pub fn label(&self) -> String {
        match self {
            AgentAction::Respond { message } => format!("respond: {}", short(message)),
            AgentAction::AskUser { question } => format!("ask user: {}", short(question)),
            AgentAction::UseTool {
                tool_name,
                expected_observation,
                ..
            } => match expected_observation {
                Some(expected) => format!("use tool {tool_name}: {}", short(expected)),
                None => format!("use tool {tool_name}"),
            },
            AgentAction::RetrieveMemory { query, regions } => {
                if regions.is_empty() {
                    format!("retrieve memory: {}", short(query))
                } else {
                    format!("retrieve memory [{}]: {}", regions.join(","), short(query))
                }
            }
            AgentAction::Verify { claim, verifier } => {
                format!("verify with {verifier}: {}", short(claim))
            }
            AgentAction::Plan { steps } => format!("plan: {} steps", steps.len()),
            AgentAction::Reflect { focus } => format!("reflect: {}", short(focus)),
            AgentAction::Delegate { agent, task } => {
                format!("delegate to {agent}: {}", short(task))
            }
            AgentAction::Finish { summary } => format!("finish: {}", short(summary)),
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match self {
            AgentAction::Respond { .. } => "respond",
            AgentAction::AskUser { .. } => "ask_user",
            AgentAction::UseTool { .. } => "tool_call",
            AgentAction::RetrieveMemory { .. } => "retrieve_memory",
            AgentAction::Verify { .. } => "verify",
            AgentAction::Plan { .. } => "plan",
            AgentAction::Reflect { .. } => "reflect",
            AgentAction::Delegate { .. } => "delegate",
            AgentAction::Finish { .. } => "finish",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            AgentAction::Respond { .. } | AgentAction::Finish { .. }
        )
    }

    pub fn is_information_gathering(&self) -> bool {
        matches!(
            self,
            AgentAction::AskUser { .. }
                | AgentAction::RetrieveMemory { .. }
                | AgentAction::Verify { .. }
                | AgentAction::Reflect { .. }
        )
    }

    pub fn tool_name(&self) -> Option<&str> {
        match self {
            AgentAction::UseTool { tool_name, .. } => Some(tool_name.as_str()),
            _ => None,
        }
    }
}

fn short(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.chars().count() <= 72 {
        trimmed.to_string()
    } else {
        let mut out = trimmed.chars().take(69).collect::<String>();
        out.push_str("...");
        out
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionCandidate {
    pub id: String,
    pub action: AgentAction,
    pub prior: f64,
    pub expected_success: f64,
    pub information_gain: f64,
    pub cost: f64,
    pub latency: f64,
    pub risk: f64,
    pub reversibility: f64,
    pub reason: String,
    pub source_memory_ids: Vec<String>,
    pub requires_confirmation: bool,
}

impl ActionCandidate {
    pub fn new(action: AgentAction) -> Self {
        let label = action.label();
        Self {
            id: format!("act_{}", stable_hash_text(&label)),
            action,
            prior: 0.25,
            expected_success: 0.50,
            information_gain: 0.25,
            cost: 0.20,
            latency: 0.20,
            risk: 0.10,
            reversibility: 0.80,
            reason: "generated candidate".into(),
            source_memory_ids: Vec::new(),
            requires_confirmation: false,
        }
    }

    pub fn with_scores(
        mut self,
        prior: f64,
        expected_success: f64,
        information_gain: f64,
        cost: f64,
        risk: f64,
    ) -> Self {
        self.prior = clamp01(prior);
        self.expected_success = clamp01(expected_success);
        self.information_gain = clamp01(information_gain);
        self.cost = clamp01(cost);
        self.risk = clamp01(risk);
        self
    }

    pub fn label(&self) -> String {
        self.action.label()
    }

    pub fn is_side_effectful_tool(&self, tools: &[ToolSpec]) -> bool {
        let Some(tool_name) = self.action.tool_name() else {
            return false;
        };
        tools
            .iter()
            .find(|tool| tool.name.as_str() == tool_name)
            .map(|tool| tool.safety_class.is_side_effectful())
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OutputContract {
    pub format: Option<String>,
    pub must_include: Vec<String>,
    pub must_not_include: Vec<String>,
    pub citation_required: bool,
    pub verifier_required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentTask {
    pub goal: String,
    pub scope: Scope,
    pub messages: Vec<AgentMessage>,
    pub observations: Vec<AgentObservation>,
    pub constraints: Vec<String>,
    pub unknowns: Vec<String>,
    pub artifacts: Vec<String>,
    pub available_tools: Vec<ToolSpec>,
    pub candidate_actions: Vec<ActionCandidate>,
    pub exact_fingerprint_fields: Vec<String>,
    pub output_contract: OutputContract,
}

impl Default for AgentTask {
    fn default() -> Self {
        Self {
            goal: String::new(),
            scope: Scope::default(),
            messages: Vec::new(),
            observations: Vec::new(),
            constraints: Vec::new(),
            unknowns: Vec::new(),
            artifacts: Vec::new(),
            available_tools: Vec::new(),
            candidate_actions: Vec::new(),
            exact_fingerprint_fields: Vec::new(),
            output_contract: OutputContract::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentState {
    pub id: String,
    pub exact_key: String,
    pub scope: Scope,
    pub goal: String,
    pub messages: Vec<AgentMessage>,
    pub observations: Vec<AgentObservation>,
    pub constraints: Vec<String>,
    pub unknowns: Vec<String>,
    pub artifacts: Vec<String>,
    pub changed_artifacts: Vec<String>,
    pub fuzzy_terms: Vec<String>,
    pub available_tools: Vec<ToolSpec>,
    pub candidates: Vec<ActionCandidate>,
    pub ownership: OwnershipMap,
    pub output_contract: OutputContract,
}

impl AgentState {
    pub fn all_text(&self) -> String {
        let mut parts = vec![self.goal.clone(), self.scope.text()];
        parts.extend(self.constraints.clone());
        parts.extend(self.unknowns.clone());
        parts.extend(self.artifacts.clone());
        parts.extend(self.fuzzy_terms.clone());
        parts.extend(self.observations.iter().map(|obs| obs.content.clone()));
        parts.extend(self.messages.iter().map(|msg| msg.content.clone()));
        parts.join(" ")
    }

    pub fn no_side_effects_requested(&self) -> bool {
        let text = self.constraints.join(" ").to_lowercase();
        text.contains("do not modify")
            || text.contains("don't modify")
            || text.contains("no side effect")
            || text.contains("read only")
            || text.contains("readonly")
            || text.contains("do not call external")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionSource {
    CurrentConstraint,
    VerifiedRunbook,
    ExactReplay,
    ProceduralSkill,
    PolicyValueSearch,
    EpisodicExample,
    QuiescenceOverride,
    Fallback,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankedAction {
    pub candidate: ActionCandidate,
    pub score: f64,
    pub utility_score: f64,
    pub puct_score: f64,
    pub ownership_gain: f64,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentDecision {
    pub selected: ActionCandidate,
    pub source: DecisionSource,
    pub source_memory_id: Option<String>,
    pub score: f64,
    pub rationale: String,
    pub ownership: OwnershipMap,
    pub ranked_candidates: Vec<RankedAction>,
    pub memory_packet: Option<crate::prompt::PromptMemoryPacket>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifierResult {
    pub name: String,
    pub claim: String,
    pub passed: bool,
    pub evidence: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionOutcome {
    pub state_id: String,
    pub candidate_id: String,
    pub action_label: String,
    pub success: bool,
    pub reward: f64,
    pub cost_paid: f64,
    pub risk_observed: f64,
    pub observations: Vec<AgentObservation>,
    pub verifier_result: Option<VerifierResult>,
    pub user_feedback: Option<String>,
    pub notes: String,
}

impl ActionOutcome {
    pub fn success(
        state_id: impl Into<String>,
        candidate: &ActionCandidate,
        notes: impl Into<String>,
    ) -> Self {
        Self {
            state_id: state_id.into(),
            candidate_id: candidate.id.clone(),
            action_label: candidate.label(),
            success: true,
            reward: 1.0,
            cost_paid: candidate.cost,
            risk_observed: candidate.risk,
            observations: Vec::new(),
            verifier_result: None,
            user_feedback: None,
            notes: notes.into(),
        }
    }

    pub fn failure(
        state_id: impl Into<String>,
        candidate: &ActionCandidate,
        notes: impl Into<String>,
    ) -> Self {
        Self {
            state_id: state_id.into(),
            candidate_id: candidate.id.clone(),
            action_label: candidate.label(),
            success: false,
            reward: 0.0,
            cost_paid: candidate.cost,
            risk_observed: candidate.risk,
            observations: Vec::new(),
            verifier_result: None,
            user_feedback: None,
            notes: notes.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticFact {
    pub header: MemoryHeader,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub applies_when: Vec<String>,
    pub does_not_apply_when: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserPreference {
    pub header: MemoryHeader,
    pub preference: String,
    pub strength: f64,
    pub not_a_constraint: bool,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectFact {
    pub header: MemoryHeader,
    pub project: String,
    pub fact: String,
    pub applies_when: Vec<String>,
    pub invalidates_on: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillStep {
    pub label: String,
    pub action: AgentAction,
    pub expected_observation: Option<String>,
    pub gate: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProceduralSkill {
    pub header: MemoryHeader,
    pub name: String,
    pub applies_when: Vec<String>,
    pub steps: Vec<SkillStep>,
    pub anti_patterns: Vec<String>,
    pub expected_success: f64,
    pub required_tools: Vec<String>,
    pub verification_gate: Option<String>,
}

impl ProceduralSkill {
    pub fn first_action_candidate(&self) -> Option<ActionCandidate> {
        self.steps.first().map(|step| {
            let mut candidate = ActionCandidate::new(step.action.clone()).with_scores(
                self.header.confidence,
                self.expected_success,
                0.45,
                0.20,
                0.12,
            );
            candidate.reason = format!("first step of procedural skill: {}", self.name);
            candidate.source_memory_ids.push(self.header.id.clone());
            candidate
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolAffordanceMemory {
    pub header: MemoryHeader,
    pub tool_name: String,
    pub works_well_for: Vec<String>,
    pub failure_modes: Vec<String>,
    pub cost: f64,
    pub latency: f64,
    pub risk: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyValueAction {
    pub action: AgentAction,
    pub prior: f64,
    pub expected_success: f64,
    pub cost: f64,
    pub latency: f64,
    pub risk: f64,
    pub reversibility: f64,
    pub information_gain: f64,
    pub reason: String,
}

impl PolicyValueAction {
    pub fn into_candidate(self, source_memory_id: String) -> ActionCandidate {
        let mut candidate = ActionCandidate::new(self.action);
        candidate.prior = clamp01(self.prior);
        candidate.expected_success = clamp01(self.expected_success);
        candidate.cost = clamp01(self.cost);
        candidate.latency = clamp01(self.latency);
        candidate.risk = clamp01(self.risk);
        candidate.reversibility = clamp01(self.reversibility);
        candidate.information_gain = clamp01(self.information_gain);
        candidate.reason = self.reason;
        candidate.source_memory_ids.push(source_memory_id);
        candidate
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyValueMemory {
    pub header: MemoryHeader,
    pub state_pattern: String,
    pub actions: Vec<PolicyValueAction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnershipMemory {
    pub header: MemoryHeader,
    pub state_pattern: String,
    pub ownership: OwnershipMap,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExactReplay {
    pub header: MemoryHeader,
    pub exact_key: String,
    pub best_action: AgentAction,
    pub outcome_summary: String,
    pub validity: Vec<String>,
    pub expires_on_change: Vec<String>,
}

impl ExactReplay {
    pub fn valid_for(&self, state: &AgentState) -> bool {
        if self.exact_key != state.exact_key {
            return false;
        }
        let mut invalidation = self.header.invalidation.clone();
        invalidation
            .changed_artifacts
            .extend(self.expires_on_change.clone());
        self.header
            .is_valid_for(&state.scope, &state.changed_artifacts)
            && invalidation.valid_for_changed_artifacts(&state.changed_artifacts)
    }

    pub fn candidate(&self) -> ActionCandidate {
        let mut candidate = ActionCandidate::new(self.best_action.clone()).with_scores(
            self.header.confidence,
            self.header.confidence,
            0.20,
            0.08,
            0.05,
        );
        candidate.reason = format!("exact replay: {}", self.outcome_summary);
        candidate.source_memory_ids.push(self.header.id.clone());
        candidate
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifiedRunbook {
    pub header: MemoryHeader,
    pub name: String,
    pub preconditions: Vec<String>,
    pub procedure: Vec<AgentAction>,
    pub recommended_next_action: AgentAction,
    pub verification: Vec<String>,
    pub invalidation_notes: Vec<String>,
}

impl VerifiedRunbook {
    pub fn preconditions_hold(&self, state: &AgentState) -> bool {
        if !self
            .header
            .is_valid_for(&state.scope, &state.changed_artifacts)
        {
            return false;
        }
        if self.preconditions.is_empty() {
            return true;
        }
        let text = state.all_text().to_lowercase();
        self.preconditions
            .iter()
            .all(|condition| condition_words_match(&text, condition))
    }

    pub fn candidate(&self) -> ActionCandidate {
        let mut candidate = ActionCandidate::new(self.recommended_next_action.clone()).with_scores(
            1.0,
            self.header.confidence,
            0.25,
            0.10,
            0.04,
        );
        candidate.reason = format!("verified runbook: {}", self.name);
        candidate.source_memory_ids.push(self.header.id.clone());
        candidate
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KoRule {
    pub header: MemoryHeader,
    pub state_pattern: String,
    pub forbidden_action_label: String,
    pub reason: String,
    pub allowed_if_new_evidence: Vec<String>,
}

impl KoRule {
    pub fn blocks(&self, state: &AgentState, action: &ActionCandidate) -> bool {
        if !self
            .header
            .is_valid_for(&state.scope, &state.changed_artifacts)
        {
            return false;
        }
        let state_text = state.all_text().to_lowercase();
        if !condition_words_match(&state_text, &self.state_pattern) {
            return false;
        }
        let action_label = action.label().to_lowercase();
        if !action_label.contains(&self.forbidden_action_label.to_lowercase())
            && !self
                .forbidden_action_label
                .to_lowercase()
                .contains(&action_label)
        {
            return false;
        }
        let evidence_text = state
            .observations
            .iter()
            .map(|obs| obs.content.to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        !self
            .allowed_if_new_evidence
            .iter()
            .any(|allowed| condition_words_match(&evidence_text, allowed))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SafetyRule {
    pub header: MemoryHeader,
    pub name: String,
    pub applies_when: Vec<String>,
    pub forbidden_action_patterns: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpisodicTrace {
    pub header: MemoryHeader,
    pub initial_goal: String,
    pub state_id: String,
    pub action_labels: Vec<String>,
    pub observations: Vec<String>,
    pub mistakes: Vec<String>,
    pub successful_patterns: Vec<String>,
    pub final_outcome: String,
    pub user_feedback: Option<String>,
    pub memories_to_promote: Vec<PromotionRequest>,
    pub memories_to_demote: Vec<String>,
}

impl EpisodicTrace {
    pub fn new(scope: Scope, initial_goal: impl Into<String>, state_id: impl Into<String>) -> Self {
        let initial_goal = initial_goal.into();
        Self {
            header: MemoryHeader::new(
                AgentMemoryKind::EpisodicTrace,
                scope,
                format!("episode: {initial_goal}"),
            ),
            initial_goal,
            state_id: state_id.into(),
            action_labels: Vec::new(),
            observations: Vec::new(),
            mistakes: Vec::new(),
            successful_patterns: Vec::new(),
            final_outcome: String::new(),
            user_feedback: None,
            memories_to_promote: Vec::new(),
            memories_to_demote: Vec::new(),
        }
    }

    pub fn successful(&self) -> bool {
        let outcome = self.final_outcome.to_lowercase();
        outcome.contains("success") || outcome.contains("resolved") || outcome.contains("passed")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromotionRequest {
    pub claim_or_pattern: String,
    pub target_kind: AgentMemoryKind,
    pub confidence: f64,
    pub applies_when: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ValueWeights {
    pub success: f64,
    pub prior: f64,
    pub information_gain: f64,
    pub cost: f64,
    pub latency: f64,
    pub risk: f64,
    pub irreversibility: f64,
    pub ownership_gain: f64,
}

impl Default for ValueWeights {
    fn default() -> Self {
        Self {
            success: 1.00,
            prior: 0.35,
            information_gain: 0.32,
            cost: 0.22,
            latency: 0.10,
            risk: 0.55,
            irreversibility: 0.25,
            ownership_gain: 0.38,
        }
    }
}

impl ValueWeights {
    pub fn utility(&self, candidate: &ActionCandidate, ownership_gain: f64) -> f64 {
        self.success * candidate.expected_success
            + self.prior * candidate.prior
            + self.information_gain * candidate.information_gain
            + self.ownership_gain * ownership_gain
            - self.cost * candidate.cost
            - self.latency * candidate.latency
            - self.risk * candidate.risk
            - self.irreversibility * (1.0 - candidate.reversibility)
    }
}

pub fn condition_words_match(haystack_lower: &str, condition: &str) -> bool {
    let words = condition
        .to_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|word| word.len() >= 3)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if words.is_empty() {
        return true;
    }
    let needed = words.len().min(3);
    words
        .iter()
        .filter(|word| haystack_lower.contains(word.as_str()))
        .count()
        >= needed
}
