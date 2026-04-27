use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Rust,
    Python,
    TypeScript,
    JavaScript,
    Go,
}
