//! Schema-backed runtime configuration for rust-chatbot.
//!
//! Search order (first hit wins):
//!   1. `$RUST_CHATBOT_CONFIG_PATH` if set — exclusive, no fallback. Lets test
//!      harnesses pin the config file regardless of the repo root or `$HOME`.
//!   2. the shared runtime root `config.toml`.
//!   3. Legacy repo-local `chatbot-config.toml` files from the split repos.
//!
//! All fields are optional; missing fields fall back to the hardcoded defaults
//! in this module so existing callers are never forced to change.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::RwLock;
mod constants;
mod paths;
pub use constants::*;
pub use paths::*;
