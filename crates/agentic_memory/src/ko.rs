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

impl KoDetector {
    pub fn observe(
        &mut self,
        state: &AgentState,
        action: &ActionCandidate,
        result_signature: impl Into<String>,
    ) -> Option<KoRule> {
        let observation = KoObservation {
            state_id: state.id.clone(),
            exact_key: state.exact_key.clone(),
            action_label: action.label(),
            result_signature: result_signature.into(),
            timestamp_ms: now_millis(),
        };
        self.recent.push(observation.clone());
        if self.recent.len() > self.window {
            let overflow = self.recent.len() - self.window;
            self.recent.drain(0..overflow);
        }

        let repetitions = self
            .recent
            .iter()
            .filter(|item| {
                item.exact_key == observation.exact_key
                    && item.action_label == observation.action_label
                    && item.result_signature == observation.result_signature
            })
            .count();

        if repetitions >= self.max_repetitions {
            Some(KoRule {
                header: MemoryHeader::new(
                    AgentMemoryKind::KoRule,
                    state.scope.clone(),
                    format!("ko: repeated {}", observation.action_label),
                )
                .with_confidence(0.92),
                state_pattern: state.goal.clone(),
                forbidden_action_label: observation.action_label,
                reason: "same state/action/result repeated without new evidence".into(),
                allowed_if_new_evidence: vec![
                    "new observation".into(),
                    "changed artifact".into(),
                    "different arguments".into(),
                    "user provided new information".into(),
                ],
            })
        } else {
            None
        }
    }

    pub fn would_repeat(&self, state: &AgentState, action: &ActionCandidate) -> bool {
        let action_label = action.label();
        self.recent
            .iter()
            .filter(|item| item.exact_key == state.exact_key && item.action_label == action_label)
            .count()
            >= self.max_repetitions
    }
}
