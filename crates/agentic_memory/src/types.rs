use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};
pub type TimestampMillis = u128;
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Scope {
    pub user_id: Option<String>,
    pub org_id: Option<String>,
    pub project_id: Option<String>,
    pub agent_id: Option<String>,
    pub domain: Option<String>,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Invalidation {
    pub triggers: Vec<String>,
    pub changed_artifacts: Vec<String>,
    pub expires_at_ms: Option<TimestampMillis>,
    pub notes: Option<String>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolSafetyClass {
    Safe,
    ReadOnly,
    ExternalSideEffect,
    Write,
    Privileged,
    Dangerous,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegionStatus {
    Owned,
    Unknown,
    Contested,
    Risky,
    Stale,
    Blocked,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OwnershipMap {
    pub regions: BTreeMap<String, OwnershipRegion>,
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
