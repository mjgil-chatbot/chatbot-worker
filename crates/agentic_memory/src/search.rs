use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchTree {
    pub state_id: String,
    pub edges: HashMap<String, EdgeStats>,
}
