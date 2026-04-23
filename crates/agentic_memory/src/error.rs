use std::fmt::{Display, Formatter};

pub type Result<T> = std::result::Result<T, AgentMemoryError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentMemoryError {
    Io(String),
    Json(String),
    NotFound(String),
    InvalidInput(String),
    SafetyBlocked(String),
}

impl Display for AgentMemoryError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentMemoryError::Io(msg) => write!(f, "I/O error: {msg}"),
            AgentMemoryError::Json(msg) => write!(f, "JSON error: {msg}"),
            AgentMemoryError::NotFound(msg) => write!(f, "not found: {msg}"),
            AgentMemoryError::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
            AgentMemoryError::SafetyBlocked(msg) => write!(f, "safety blocked: {msg}"),
        }
    }
}

impl std::error::Error for AgentMemoryError {}

impl From<std::io::Error> for AgentMemoryError {
    fn from(value: std::io::Error) -> Self {
        AgentMemoryError::Io(value.to_string())
    }
}

impl From<serde_json::Error> for AgentMemoryError {
    fn from(value: serde_json::Error) -> Self {
        AgentMemoryError::Json(value.to_string())
    }
}
