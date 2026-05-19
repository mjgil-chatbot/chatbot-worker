use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::{Context, Result};
use ai_providers::config::{
    ClaudeCliConfig, CodexCliConfig, GeminiCliConfig, OpenRouterCliConfig, OpenRouterConfig,
    QwenCodeCliConfig,
};
use ai_providers::memory::{
    MemoryContext as ProviderMemoryContext, MemoryHit as ProviderMemoryHit,
};
use ai_providers::sandbox::RunscMount;
use ai_providers::session::{ProviderMessage, ProviderSession};
use ai_providers::ProviderConfig;
use crate::config;
use crate::data::ChatSession;
use crate::memory::MemoryContext;
use crate::model::ModelProvider;
use crate::stream::StreamMessage;
use crate::workspace;
use crate::AutonomousExecutionEnvironment;
include!("overrides.rs");
include!("nvidia.rs");
include!("config_builders.rs");
include!("prompt_instructions.rs");
include!("openrouter.rs");
include!("command.rs");
include!("mcp.rs");
include!("cli.rs");
include!("autonomous.rs");
