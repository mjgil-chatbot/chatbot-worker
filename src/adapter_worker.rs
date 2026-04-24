//! Adapter-based worker that routes CLI requests through agent-adapter-cli.
//!
//! This module replaces direct CLI spawning (`call_provider_cli_stream`) with
//! the `agent-adapter-cli` unified adapter layer, providing:
//! - Protocol-first design: ACP > JSONL > PTY fallback hierarchy
//! - Secret redaction: Automatic redaction of API keys/tokens from logs
//! - Process group cleanup: Proper child process termination on cancel/timeout
//! - Session resumption: For providers that support it

use std::collections::HashMap;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use agent_adapter_core::{
    AgentEvent, ApprovalPolicy, ChatRequest, ExternalSessionRef, ProviderId, ResumeMode,
};
use ai_providers::claude::cli::effective_claude_system_prompt;
use ai_providers::session::ProviderSession;
use anyhow::Result;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::time::{sleep, Duration};
pub(crate) async fn wait_for_stop_signal(stop_signal: &AtomicBool) {
    while !stop_signal.load(Ordering::Relaxed) {
        sleep(Duration::from_millis(50)).await;
    }
}
