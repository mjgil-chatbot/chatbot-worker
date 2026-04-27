//! Shared worker/runtime library for `rust-chatbot` and `auto-runs`.
//!
//! This crate owns the provider bridge, worker spool/types, hooks, shared
//! storage helpers, and the canonical `chatbot-worker` implementation.

use std::path::Path;
pub mod adapter_git_verifier;
pub mod adapter_live_summary;
pub mod adapter_tool_call_schema;
pub mod adapter_transport;
pub mod adapter_worker;
pub mod cgroup;
pub mod config;
pub mod model;
pub mod stream;
pub mod worker;
/// Shared test lock for tests that mutate the `HOME` environment variable.
pub static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
