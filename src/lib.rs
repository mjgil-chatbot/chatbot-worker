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
pub mod config;
pub mod worker;
