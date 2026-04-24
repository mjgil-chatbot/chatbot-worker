//! Shared worker/runtime library for `rust-chatbot` and `auto-runs`.
//!
//! This crate owns the provider bridge, worker spool/types, hooks, shared
//! storage helpers, and the canonical `chatbot-worker` implementation.

pub mod adapter_git_verifier;
pub mod worker;
