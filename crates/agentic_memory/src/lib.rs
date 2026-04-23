//! Agent-native hybrid memory for AI agents.
//!
//! The crate exposes a concrete reference implementation, `AgentMemorySystem`,
//! that takes an `AgentTask` and returns an `AgentDecision`. The design keeps
//! hard precedence for current constraints and verified memories while using
//! policy/value/ownership/search statistics to allocate attention in fuzzy tasks.

pub mod error;
pub mod ko;
pub mod prompt;
pub mod search;
pub mod store;
pub mod types;
pub use error::{AgentMemoryError, Result};
pub use ko::KoDetector;
pub use prompt::{
    MemoryPacketConfig, MemoryPacketItem, PromptMemoryPacket, PromptMemoryPacketBuilder,
};
