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
pub mod chatbot_worker;
pub mod chatbot_worker_adapter;
pub mod chatbot_worker_runtime;
pub mod chatbot_worker_trace;
pub mod companion_binary;
pub mod config;
pub mod data;
pub mod debug;
pub mod delta_resolution;
pub mod hooks;
pub mod memory;
pub mod model;
pub mod provider_catalog;
pub mod storage;
pub mod stream;
pub mod worker;
pub mod worker_ext;
/// Shared test lock for tests that mutate the `HOME` environment variable.
pub static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
