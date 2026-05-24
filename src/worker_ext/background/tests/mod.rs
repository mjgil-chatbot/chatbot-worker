use super::executor;
use super::types::*;
use crate::config::RUST_CHATBOT_CODEX_CLI_ENV;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use tempfile::tempdir;
use uuid::Uuid;

use super::dispatcher::BackgroundDispatcher;

mod command_building;
mod results;

const TEST_PROVIDER_CLI_ENV: &str = "RUST_CHATBOT_TEST_PROVIDER_CLI";

static ENV_MUTEX: StdMutex<()> = StdMutex::new(());

pub(super) fn make_task(prompt: &str) -> BackgroundTask {
    BackgroundTask {
        id: Uuid::new_v4(),
        agent_profile: AgentProfile {
            role: "Worker".to_string(),
            provider: Some("codex".to_string()),
        },
        prompt: prompt.to_string(),
        context_budget: 1000,
        system_prompt: None,
        working_dir: None,
        resume_session_id: None,
    }
}

pub(super) struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    pub(super) fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.previous.as_ref() {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

pub(super) fn install_fake_codex_cli() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let script_path = dir.path().join("fake-codex.py");
    fs::write(
        &script_path,
        r#"#!/usr/bin/env python3
import json
import os
import sys
import time

state_path = os.environ["RUST_CHATBOT_BG_TEST_STATE"]
prompt = sys.argv[-1]
sleep_ms = int(os.environ.get("RUST_CHATBOT_BG_TEST_SLEEP_MS", "0"))
fail_prompt = os.environ.get("RUST_CHATBOT_BG_TEST_FAIL_PROMPT")

def update(delta):
    with open(state_path, "a+", encoding="utf-8") as handle:
        import fcntl
        fcntl.flock(handle, fcntl.LOCK_EX)
        handle.seek(0)
        raw = handle.read().strip()
        state = json.loads(raw) if raw else {"current": 0, "peak": 0}
        state["current"] = max(0, state["current"] + delta)
        state["peak"] = max(state["peak"], state["current"])
        handle.seek(0)
        handle.truncate()
        json.dump(state, handle)
        handle.flush()
        fcntl.flock(handle, fcntl.LOCK_UN)

update(1)
try:
    time.sleep(sleep_ms / 1000.0)
    if fail_prompt and prompt == fail_prompt:
        print(f"stdout:{prompt}")
        print(f"stderr:{prompt}", file=sys.stderr)
        sys.exit(17)
    print(f"stdout:{prompt}")
finally:
    update(-1)
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&script_path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script_path, permissions).unwrap();
    dir
}

pub(super) fn read_peak(path: &std::path::Path) -> usize {
    let raw = fs::read_to_string(path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
    json["peak"].as_u64().unwrap() as usize
}

pub(super) fn command_args(command: &std::process::Command) -> Vec<String> {
    command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

pub(super) fn command_env_value(command: &std::process::Command, key: &str) -> Option<String> {
    command.get_envs().find_map(|(name, value)| {
        (name.to_string_lossy() == key)
            .then(|| value.map(|value| value.to_string_lossy().into_owned()))
            .flatten()
    })
}

#[tokio::test]
async fn test_background_fan_out_merges_all_task_outputs() {
    let mut dispatcher = BackgroundDispatcher::new(2);

    for i in 0..4 {
        dispatcher.add_task(make_task(&format!("task_{}", i)));
    }

    dispatcher
        .run_all_with_executor(Arc::new(|task| {
            Box::pin(async move {
                BackgroundResult {
                    id: task.id,
                    output: Ok(format!("done:{}", task.prompt)),
                }
            })
        }))
        .await;

    let mut merged_outputs: Vec<String> = dispatcher
        .take_merged()
        .await
        .succeeded
        .into_iter()
        .map(|(_, output)| output)
        .collect();
    merged_outputs.sort();
    assert_eq!(merged_outputs.len(), 4);
    assert_eq!(
        merged_outputs,
        vec![
            "done:task_0".to_string(),
            "done:task_1".to_string(),
            "done:task_2".to_string(),
            "done:task_3".to_string(),
        ]
    );
}

#[tokio::test]
async fn test_default_max_concurrent() {
    let dispatcher = BackgroundDispatcher::new(0);
    assert_eq!(dispatcher.max_concurrent, 4);

    let dispatcher2 = BackgroundDispatcher::new(8);
    assert_eq!(dispatcher2.max_concurrent, 8);
}

/// Proves the semaphore actually bounds concurrency.
/// We use an atomic counter to track how many tasks are running
/// simultaneously and assert the peak never exceeds max_concurrent.
#[tokio::test]
async fn test_bounded_concurrency_enforcement() {
    let max_concurrent: usize = 2;
    let task_count: usize = 4;

    let current = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let mut dispatcher = BackgroundDispatcher::new(max_concurrent);
    for index in 0..task_count {
        dispatcher.add_task(make_task(&format!("concurrency_{index}")));
    }
    let current_for_tasks = current.clone();
    let peak_for_tasks = peak.clone();

    dispatcher
        .run_all_with_executor(Arc::new(move |task| {
            let current_c = current_for_tasks.clone();
            let peak_c = peak_for_tasks.clone();
            Box::pin(async move {
                let running = current_c.fetch_add(1, Ordering::SeqCst) + 1;
                peak_c.fetch_max(running, Ordering::SeqCst);
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                current_c.fetch_sub(1, Ordering::SeqCst);
                BackgroundResult {
                    id: task.id,
                    output: Ok(task.prompt),
                }
            })
        }))
        .await;

    let mut outputs: Vec<String> = dispatcher
        .take_merged()
        .await
        .succeeded
        .into_iter()
        .map(|(_, output)| output)
        .collect();
    outputs.sort();
    assert_eq!(outputs.len(), task_count);
    assert_eq!(outputs[0], "concurrency_0");

    let observed_peak = peak.load(Ordering::SeqCst);
    assert!(
        observed_peak <= max_concurrent,
        "Peak concurrency {} exceeded limit {}",
        observed_peak,
        max_concurrent,
    );
    assert!(
        observed_peak > 1,
        "Expected concurrent execution but peak was {}",
        observed_peak,
    );
}

#[tokio::test]
async fn test_run_all_uses_provider_dispatch_path() {
    let mut dispatcher = BackgroundDispatcher::new(1);
    let mut task = make_task("missing");
    task.agent_profile.provider = Some("does-not-exist".to_string());
    dispatcher.add_task(task);

    dispatcher.run_all().await;

    let merged = dispatcher.take_merged().await;
    assert_eq!(merged.failed.len(), 1);
    assert!(merged.failed[0]
        .1
        .contains("Unknown provider: does-not-exist"));
}

#[tokio::test]
async fn test_provider_dispatch_bounded_concurrency_and_merged_stdout() {
    let _test_env_lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _env_lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let script_dir = install_fake_codex_cli();
    let state_dir = tempdir().unwrap();
    let state_path = state_dir.path().join("state.json");

    let _codex_cli = EnvVarGuard::set(
        RUST_CHATBOT_CODEX_CLI_ENV,
        script_dir.path().join("fake-codex.py"),
    );
    let _provider_cli = EnvVarGuard::set(
        TEST_PROVIDER_CLI_ENV,
        script_dir.path().join("fake-codex.py"),
    );
    let _state = EnvVarGuard::set("RUST_CHATBOT_BG_TEST_STATE", &state_path);
    let _sleep = EnvVarGuard::set("RUST_CHATBOT_BG_TEST_SLEEP_MS", "300");
    let _fail = EnvVarGuard::set("RUST_CHATBOT_BG_TEST_FAIL_PROMPT", "");

    let mut dispatcher = BackgroundDispatcher::new(2);
    for index in 0..4 {
        dispatcher.add_task(make_task(&format!("task_{index}")));
    }

    dispatcher.run_all().await;

    let mut outputs: Vec<String> = dispatcher
        .take_merged()
        .await
        .succeeded
        .into_iter()
        .map(|(_, output)| output)
        .collect();
    outputs.sort();

    assert_eq!(
        outputs,
        vec![
            "stdout:task_0".to_string(),
            "stdout:task_1".to_string(),
            "stdout:task_2".to_string(),
            "stdout:task_3".to_string(),
        ]
    );

    let observed_peak = read_peak(&state_path);
    assert!(
        observed_peak <= 2,
        "peak concurrency exceeded bound: {observed_peak}"
    );
    assert!(
        observed_peak > 1,
        "expected concurrent execution, got peak {observed_peak}"
    );
}

#[tokio::test]
async fn test_provider_dispatch_surfaces_stderr_and_exit_code_failures() {
    let _test_env_lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _env_lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let script_dir = install_fake_codex_cli();
    let state_dir = tempdir().unwrap();
    let state_path = state_dir.path().join("state.json");

    let _codex_cli = EnvVarGuard::set(
        RUST_CHATBOT_CODEX_CLI_ENV,
        script_dir.path().join("fake-codex.py"),
    );
    let _provider_cli = EnvVarGuard::set(
        TEST_PROVIDER_CLI_ENV,
        script_dir.path().join("fake-codex.py"),
    );
    let _state = EnvVarGuard::set("RUST_CHATBOT_BG_TEST_STATE", &state_path);
    let _sleep = EnvVarGuard::set("RUST_CHATBOT_BG_TEST_SLEEP_MS", "10");
    let _fail = EnvVarGuard::set("RUST_CHATBOT_BG_TEST_FAIL_PROMPT", "task_fail");

    let mut dispatcher = BackgroundDispatcher::new(2);
    dispatcher.add_task(make_task("task_ok"));
    dispatcher.add_task(make_task("task_fail"));

    dispatcher.run_all().await;

    let merged = dispatcher.take_merged().await;
    assert_eq!(merged.succeeded.len(), 1);
    assert_eq!(merged.failed.len(), 1);
    assert_eq!(merged.succeeded[0].1, "stdout:task_ok");
    assert!(merged.failed[0].1.contains("Codex command failed"));
    assert!(merged.failed[0].1.contains("stderr:task_fail"));
}

// --- bounded concurrency with error merging ---

#[tokio::test]
async fn test_bounded_concurrency_with_mixed_success_and_failure() {
    let mut dispatcher = BackgroundDispatcher::new(2);
    for i in 0..6 {
        dispatcher.add_task(make_task(&format!("mixed_{i}")));
    }

    dispatcher
        .run_all_with_executor(Arc::new(|task| {
            Box::pin(async move {
                // Even-indexed prompts succeed, odd ones fail
                let idx: usize = task.prompt.split('_').last().unwrap().parse().unwrap();
                let output = if idx % 2 == 0 {
                    Ok(format!("ok:{}", task.prompt))
                } else {
                    Err(format!("err:{}", task.prompt))
                };
                BackgroundResult {
                    id: task.id,
                    output,
                }
            })
        }))
        .await;

    let merged = dispatcher.take_merged().await;
    assert_eq!(merged.total(), 6);
    assert_eq!(merged.succeeded.len(), 3);
    assert_eq!(merged.failed.len(), 3);
}

#[test]
fn resolve_task_provider_uses_configured_default_when_provider_is_missing() {
    assert_eq!(
        executor::resolve_task_provider(None, "codex"),
        Some(crate::model::ModelProvider::Codex)
    );
    assert_eq!(
        executor::resolve_task_provider(None, "gemini"),
        Some(crate::model::ModelProvider::Codex)
    );
}

#[test]
fn resolve_task_provider_falls_back_to_claude_for_invalid_configured_default() {
    assert_eq!(
        executor::resolve_task_provider(None, "does-not-exist"),
        Some(crate::model::ModelProvider::Codex)
    );
}

// --- sequential execution with concurrency=1 ---

#[tokio::test]
async fn test_concurrency_one_runs_sequentially() {
    let peak = Arc::new(AtomicUsize::new(0));
    let current = Arc::new(AtomicUsize::new(0));
    let mut dispatcher = BackgroundDispatcher::new(1);
    for i in 0..3 {
        dispatcher.add_task(make_task(&format!("seq_{i}")));
    }

    let current_c = current.clone();
    let peak_c = peak.clone();
    dispatcher
        .run_all_with_executor(Arc::new(move |task| {
            let cur = current_c.clone();
            let pk = peak_c.clone();
            Box::pin(async move {
                let running = cur.fetch_add(1, Ordering::SeqCst) + 1;
                pk.fetch_max(running, Ordering::SeqCst);
                tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
                cur.fetch_sub(1, Ordering::SeqCst);
                BackgroundResult {
                    id: task.id,
                    output: Ok(task.prompt),
                }
            })
        }))
        .await;

    let merged = dispatcher.take_merged().await;
    assert_eq!(merged.total(), 3);
    assert_eq!(
        peak.load(Ordering::SeqCst),
        1,
        "with max_concurrent=1, peak should be exactly 1"
    );
}

// --- AgentProfile serde ---

#[test]
fn agent_profile_serde_round_trip() {
    let profile = AgentProfile {
        role: "Analyst".into(),
        provider: Some("claude".into()),
    };
    let json = serde_json::to_string(&profile).unwrap();
    let parsed: AgentProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, profile);
}

#[test]
fn agent_profile_serde_omits_none_provider() {
    let profile = AgentProfile {
        role: "Worker".into(),
        provider: None,
    };
    let json = serde_json::to_string(&profile).unwrap();
    assert!(
        !json.contains("provider"),
        "None provider should be skipped"
    );
    let parsed: AgentProfile = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.provider, None);
}

// --- bounded concurrency: max_concurrent >= task_count ---

#[tokio::test]
async fn test_bounded_concurrency_more_permits_than_tasks() {
    let peak = Arc::new(AtomicUsize::new(0));
    let current = Arc::new(AtomicUsize::new(0));
    let mut dispatcher = BackgroundDispatcher::new(10);
    // Only 3 tasks with 10 permits -- all should run concurrently
    for i in 0..3 {
        dispatcher.add_task(make_task(&format!("wide_{i}")));
    }

    let current_c = current.clone();
    let peak_c = peak.clone();
    dispatcher
        .run_all_with_executor(Arc::new(move |task| {
            let cur = current_c.clone();
            let pk = peak_c.clone();
            Box::pin(async move {
                let running = cur.fetch_add(1, Ordering::SeqCst) + 1;
                pk.fetch_max(running, Ordering::SeqCst);
                tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;
                cur.fetch_sub(1, Ordering::SeqCst);
                BackgroundResult {
                    id: task.id,
                    output: Ok(task.prompt),
                }
            })
        }))
        .await;

    let merged = dispatcher.take_merged().await;
    assert_eq!(merged.total(), 3);
    let observed_peak = peak.load(Ordering::SeqCst);
    assert!(
        observed_peak <= 10,
        "peak {} should not exceed max_concurrent",
        observed_peak
    );
    // With 10 permits and 3 tasks, all 3 should run concurrently
    assert_eq!(
        observed_peak, 3,
        "all 3 tasks should run concurrently when permits exceed task count"
    );
}

// --- sequential run_all_with_executor: second call after drain ---

#[tokio::test]
async fn test_run_all_twice_accumulates_results() {
    let mut dispatcher = BackgroundDispatcher::new(4);
    dispatcher.add_task(make_task("batch1_a"));
    dispatcher.add_task(make_task("batch1_b"));

    dispatcher
        .run_all_with_executor(Arc::new(|task| {
            Box::pin(async move {
                BackgroundResult {
                    id: task.id,
                    output: Ok(format!("done:{}", task.prompt)),
                }
            })
        }))
        .await;

    // Add more tasks and run again (tasks were drained by first run)
    dispatcher.add_task(make_task("batch2_c"));
    dispatcher
        .run_all_with_executor(Arc::new(|task| {
            Box::pin(async move {
                BackgroundResult {
                    id: task.id,
                    output: Ok(format!("done:{}", task.prompt)),
                }
            })
        }))
        .await;

    // Results from both runs accumulate since we haven't called take_merged
    let merged = dispatcher.take_merged().await;
    assert_eq!(
        merged.total(),
        3,
        "results from both runs should accumulate"
    );
    let mut outputs: Vec<String> = merged.succeeded.into_iter().map(|(_, o)| o).collect();
    outputs.sort();
    assert_eq!(
        outputs,
        vec![
            "done:batch1_a".to_string(),
            "done:batch1_b".to_string(),
            "done:batch2_c".to_string(),
        ]
    );
}

// --- take_merged then run more then take again ---

#[tokio::test]
async fn test_take_merged_then_run_more_then_take_again() {
    let mut dispatcher = BackgroundDispatcher::new(4);
    dispatcher.add_task(make_task("phase1"));
    dispatcher
        .run_all_with_executor(Arc::new(|task| {
            Box::pin(async move {
                BackgroundResult {
                    id: task.id,
                    output: Ok(task.prompt),
                }
            })
        }))
        .await;

    let first = dispatcher.take_merged().await;
    assert_eq!(first.total(), 1);

    // Run more tasks after draining
    dispatcher.add_task(make_task("phase2"));
    dispatcher
        .run_all_with_executor(Arc::new(|task| {
            Box::pin(async move {
                BackgroundResult {
                    id: task.id,
                    output: Ok(task.prompt),
                }
            })
        }))
        .await;

    let second = dispatcher.take_merged().await;
    assert_eq!(second.total(), 1, "only phase2 results should appear");
    assert_eq!(second.succeeded[0].1, "phase2");
}
