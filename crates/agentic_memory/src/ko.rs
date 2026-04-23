use crate::types::*;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KoObservation {
    pub state_id: String,
    pub exact_key: String,
    pub action_label: String,
    pub result_signature: String,
    pub timestamp_ms: TimestampMillis,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KoDetector {
    pub max_repetitions: usize,
    pub window: usize,
    pub recent: Vec<KoObservation>,
}
impl Default for KoDetector {
    fn default() -> Self {
        Self {
            max_repetitions: 2,
            window: 32,
            recent: Vec::new(),
        }
    }
}
