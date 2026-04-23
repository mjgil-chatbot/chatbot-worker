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
    }}
impl Scope {
    pub fn user(user_id: impl Into<String>) -> Self {
        Self {
            user_id: Some(user_id.into()),
            ..Self::default()
        }
    }}
impl Scope {
    pub fn project(project_id: impl Into<String>) -> Self {
        Self {
            project_id: Some(project_id.into()),
            ..Self::default()
        }
    }}
impl Scope {
    pub fn domain(domain: impl Into<String>) -> Self {
        Self {
            domain: Some(domain.into()),
            ..Self::default()
        }
    }}
impl Scope {
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
    }}
impl Scope {
    pub fn text(&self) -> String {
        [
            self.user_id.as_deref().unwrap_or_default(),
            self.org_id.as_deref().unwrap_or_default(),
            self.project_id.as_deref().unwrap_or_default(),
            self.agent_id.as_deref().unwrap_or_default(),
            self.domain.as_deref().unwrap_or_default(),
        ]
        .join(" ")
    }}
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
    }}
impl Evidence {
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = clamp01(confidence);
        self
    }}
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
    }}
impl Invalidation {
    pub fn is_not_expired(&self, now: TimestampMillis) -> bool {
        self.expires_at_ms
            .map(|expires| expires > now)
            .unwrap_or(true)
    }}
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
    }}
impl MemoryHeader {
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = clamp01(confidence);
        self
    }}
impl MemoryHeader {
    pub fn with_tags(mut self, tags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }}
impl MemoryHeader {
    pub fn with_evidence(mut self, evidence: Evidence) -> Self {
        self.evidence.push(evidence);
        self
    }}
impl MemoryHeader {
    pub fn touch(&mut self, success: bool) {
        self.updated_at_ms = now_millis();
        self.use_count += 1;
        if !success {
            self.failure_count += 1;
            self.confidence = clamp01(self.confidence * 0.85);
        }
    }}
impl MemoryHeader {
    pub fn is_valid_for(&self, scope: &Scope, changed_artifacts: &[String]) -> bool {
        self.scope.matches(scope)
            && self.invalidation.is_not_expired(now_millis())
            && self
                .invalidation
                .valid_for_changed_artifacts(changed_artifacts)
    }}
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
    }}
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
    }}
impl ToolSafetyClass {
    pub fn is_side_effectful(self) -> bool {
        matches!(
            self,
            ToolSafetyClass::ExternalSideEffect
                | ToolSafetyClass::Write
                | ToolSafetyClass::Privileged
                | ToolSafetyClass::Dangerous
        )
    }}
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
    }}
impl ToolSpec {
    pub fn read_only(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self::new(name, description, ToolSafetyClass::ReadOnly)
    }}
impl ToolSpec {
    pub fn write(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self::new(name, description, ToolSafetyClass::Write)
    }}
impl ToolSpec {
    pub fn with_affordances(
        mut self,
        affordances: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.affordances = affordances.into_iter().map(Into::into).collect();
        self
    }}
impl ToolSpec {
    pub fn with_cost(mut self, cost: f64) -> Self {
        self.cost = clamp01(cost);
        self
    }}
impl ToolSpec {
    pub fn with_risk(mut self, risk: f64) -> Self {
        self.risk = clamp01(risk);
        self
    }}
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
    }}
impl AgentObservation {
    pub fn tool(tool_name: impl Into<String>, content: impl Into<String>) -> Self {
        let tool_name = tool_name.into();
        let mut observation = Self::new(EvidenceSource::ToolObservation, content);
        observation.tool_name = Some(tool_name);
        observation
    }}
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
    }}
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
    }}
impl OwnershipRegion {
    pub fn uncertainty(&self) -> f64 {
        clamp01((1.0 - self.confidence) * 0.45 + self.status.uncertainty() * 0.55)
    }}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OwnershipMap {
    pub regions: BTreeMap<String, OwnershipRegion>,
}
impl OwnershipMap {
    pub fn insert(&mut self, region: OwnershipRegion) {
        self.regions.insert(region.name.clone(), region);
    }}
impl OwnershipMap {
    pub fn max_uncertainty(&self) -> f64 {
        self.regions
            .values()
            .map(OwnershipRegion::uncertainty)
            .fold(0.0, f64::max)
    }}
impl OwnershipMap {
    pub fn uncertain_regions(&self, threshold: f64) -> Vec<String> {
        self.regions
            .values()
            .filter(|region| region.uncertainty() >= threshold)
            .map(|region| region.name.clone())
            .collect()
    }}
impl OwnershipMap {
    pub fn merge_prefer_confident(&mut self, other: OwnershipMap) {
        for (name, incoming) in other.regions {
            match self.regions.get(&name) {
                Some(existing) if existing.confidence >= incoming.confidence => {}
                _ => {
                    self.regions.insert(name, incoming);
                }
            }
        }
    }}
impl OwnershipMap {
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
    }}
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
    }}
impl AgentAction {
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
    }}
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OutputContract {
    pub format: Option<String>,
    pub must_include: Vec<String>,
    pub must_not_include: Vec<String>,
    pub citation_required: bool,
    pub verifier_required: bool,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KoRule {
    pub header: MemoryHeader,
    pub state_pattern: String,
    pub forbidden_action_label: String,
    pub reason: String,
    pub allowed_if_new_evidence: Vec<String>,
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
