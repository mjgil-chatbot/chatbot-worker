use crate::types::clamp01;
use crate::types::ActionOutcome;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SearchConfig {
    pub cpuct: f64,
    pub puct_weight: f64,
    pub cost_penalty: f64,
    pub latency_penalty: f64,
    pub risk_penalty: f64,
}
impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            cpuct: 1.30,
            puct_weight: 0.24,
            cost_penalty: 0.20,
            latency_penalty: 0.08,
            risk_penalty: 0.45,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeStats {
    pub action_id: String,
    pub action_label: String,
    /// P(s, a): policy prior.
    pub prior: f64,
    /// N(s, a): visits/backups.
    pub visits: u64,
    /// Q(s, a): mean reward/value.
    pub q_value: f64,
    pub cost: f64,
    pub latency: f64,
    pub risk: f64,
    pub last_result: Option<String>,
}
impl EdgeStats {
    pub fn puct_score(&self, total_visits: u64, config: SearchConfig) -> f64 {
        let total = (total_visits.max(1) as f64).sqrt();
        let exploration = config.cpuct * self.prior * total / (1.0 + self.visits as f64);
        self.q_value + exploration
            - config.cost_penalty * self.cost
            - config.latency_penalty * self.latency
            - config.risk_penalty * self.risk
    }}
impl EdgeStats {
    pub fn backup(&mut self, outcome: &ActionOutcome) {
        self.visits += 1;
        let reward = clamp01(outcome.reward);
        let n = self.visits as f64;
        self.q_value = self.q_value + (reward - self.q_value) / n;
        self.cost = clamp01((self.cost + outcome.cost_paid) / 2.0);
        self.risk = clamp01((self.risk + outcome.risk_observed) / 2.0);
        self.last_result = Some(outcome.notes.clone());
    }}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchTree {
    pub state_id: String,
    pub edges: HashMap<String, EdgeStats>,
}
impl SearchTree {
    pub fn new(state_id: impl Into<String>) -> Self {
        Self {
            state_id: state_id.into(),
            edges: HashMap::new(),
        }
    }}
impl SearchTree {
    pub fn total_visits(&self) -> u64 {
        self.edges.values().map(|edge| edge.visits).sum()
    }}
impl SearchTree {
    pub fn backup(&mut self, outcome: &ActionOutcome) {
        let edge = self
            .edges
            .entry(outcome.candidate_id.clone())
            .or_insert_with(|| EdgeStats {
                action_id: outcome.candidate_id.clone(),
                action_label: outcome.action_label.clone(),
                prior: 0.25,
                visits: 0,
                q_value: 0.50,
                cost: outcome.cost_paid,
                latency: 0.0,
                risk: outcome.risk_observed,
                last_result: None,
            });
        edge.backup(outcome);
    }}
