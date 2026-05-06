use crate::cgroup::run_cgroup;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

pub use crate::cgroup::CGROUP_MEMORY_HIGH as AGENT_CGROUP_MEMORY_HIGH;
/// Cgroup v2 memory limits applied to agent/hook commands.
/// MemoryMax is the hard limit; MemoryHigh is the pressure throttle point.
/// These re-export the constants from `cgroup.rs` for public use.
pub use crate::cgroup::CGROUP_MEMORY_MAX as AGENT_CGROUP_MEMORY_MAX;
pub const AGENT_CGROUP_TASKS_MAX: u64 = 256;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum HookPoint {
    PreProviderCall,
    PostProviderCall,
    TaskClaimed,
    TaskCompleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FailAction {
    Continue,
    Abort,
    Retry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEntry {
    pub name: String,
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "default_fail_action")]
    pub fail_action: FailAction,
}

fn default_timeout() -> u64 {
    5000
}

fn default_fail_action() -> FailAction {
    FailAction::Continue
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct HookConfig {
    pub hooks: HashMap<HookPoint, Vec<HookEntry>>,
}

#[derive(Debug)]
pub struct HookRegistry {
    pub config: HookConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookResult {
    Success,
    Failed(FailAction, String),
}

#[cfg(test)]
use crate::TEST_ENV_LOCK;

impl HookRegistry {
    pub fn new() -> Self {
        Self {
            config: HookConfig::default(),
        }
    }

    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let config: HookConfig =
            serde_json::from_str(&content).map_err(|e| format!("JSON Parse Error: {}", e))?;
        Ok(Self { config })
    }

    pub fn load_default() -> Self {
        let paths = vec![
            crate::config::canonical_shared_root_dir()
                .join("hooks.json")
                .to_string_lossy()
                .into_owned(),
            dirs::home_dir()
                .unwrap_or_default()
                .join(".claude-desktop")
                .join("hooks.json")
                .to_string_lossy()
                .into_owned(),
            ".rust-chatbot/hooks.json".to_string(),
            ".claude-desktop/hooks.json".to_string(),
        ];

        for path in paths {
            if Path::new(&path).exists() {
                if let Ok(registry) = Self::load_from_file(&path) {
                    return registry;
                }
            }
        }

        Self::new()
    }

    pub fn execute<T: Serialize>(&self, point: HookPoint, context: &T) -> Result<(), String> {
        let entries = match self.config.hooks.get(&point) {
            Some(e) => e,
            None => return Ok(()),
        };

        let context_json = serde_json::to_string(context).map_err(|e| e.to_string())?;

        for entry in entries {
            let mut current_action = entry.fail_action.clone();
            let mut retries_left = if current_action == FailAction::Retry {
                1
            } else {
                0
            };

            loop {
                let result = Self::run_command(entry, &context_json);
                match result {
                    HookResult::Success => break,
                    HookResult::Failed(_, err) => {
                        match current_action {
                            FailAction::Continue => {
                                eprintln!("Hook {} failed but continuing: {}", entry.name, err);
                                break;
                            }
                            FailAction::Abort => {
                                return Err(format!(
                                    "Hook {} failed and aborted: {}",
                                    entry.name, err
                                ));
                            }
                            FailAction::Retry => {
                                if retries_left > 0 {
                                    eprintln!("Hook {} failed, retrying: {}", entry.name, err);
                                    retries_left -= 1;
                                    current_action = FailAction::Abort; // Abort if it fails the second time
                                    continue;
                                } else {
                                    return Err(format!(
                                        "Hook {} failed after retry: {}",
                                        entry.name, err
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn run_command(entry: &HookEntry, context_json: &str) -> HookResult {
        let mut child = match run_cgroup(shell_program(), &["-c", &entry.command])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => return HookResult::Failed(entry.fail_action.clone(), e.to_string()),
        };

        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(context_json.as_bytes()) {
                if e.kind() != std::io::ErrorKind::BrokenPipe {
                    return HookResult::Failed(
                        entry.fail_action.clone(),
                        format!("Failed to write stdin: {}", e),
                    );
                }
            }
        }

        let timeout = Duration::from_millis(entry.timeout_ms);
        let poll_interval = Duration::from_millis(entry.timeout_ms.clamp(1, 50));
        let started_at = Instant::now();

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return if status.success() {
                        HookResult::Success
                    } else {
                        HookResult::Failed(
                            entry.fail_action.clone(),
                            format!("Exited with {}", status),
                        )
                    };
                }
                Ok(None) => {
                    if started_at.elapsed() >= timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        return HookResult::Failed(
                            entry.fail_action.clone(),
                            format!("Timed out after {} ms", entry.timeout_ms),
                        );
                    }
                    std::thread::sleep(poll_interval);
                }
                Err(e) => return HookResult::Failed(entry.fail_action.clone(), e.to_string()),
            }
        }
    }
}

#[cfg(unix)]
fn shell_program() -> &'static str {
    "/bin/sh"
}

#[cfg(not(unix))]
fn shell_program() -> &'static str {
    "sh"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::path::PathBuf;

    fn temp_file() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "test_hook_config_{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_micros()
        ));
        path
    }

    #[test]
    fn test_config_loading() {
        let path = temp_file();
        let mut file = File::create(&path).unwrap();
        let json = r#"{
            "hooks": {
                "PreProviderCall": [
                    {
                        "name": "test_hook",
                        "command": "echo 'hello'",
                        "timeout_ms": 1000,
                        "fail_action": "Abort"
                    }
                ]
            }
        }"#;
        file.write_all(json.as_bytes()).unwrap();

        let registry = HookRegistry::load_from_file(&path).unwrap();
        assert!(registry
            .config
            .hooks
            .contains_key(&HookPoint::PreProviderCall));
        let hooks = registry
            .config
            .hooks
            .get(&HookPoint::PreProviderCall)
            .unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].name, "test_hook");
        assert_eq!(hooks[0].timeout_ms, 1000);
        assert_eq!(hooks[0].fail_action, FailAction::Abort);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_hook_execution_success() {
        let mut registry = HookRegistry::new();
        registry.config.hooks.insert(
            HookPoint::TaskCompleted,
            vec![HookEntry {
                name: "success_hook".to_string(),
                command: "exit 0".to_string(),
                timeout_ms: 1000,
                fail_action: FailAction::Abort,
            }],
        );

        let context = serde_json::json!({"task_id": 123});
        let result = registry.execute(HookPoint::TaskCompleted, &context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_hook_execution_continue() {
        let mut registry = HookRegistry::new();
        registry.config.hooks.insert(
            HookPoint::TaskClaimed,
            vec![HookEntry {
                name: "fail_continue".to_string(),
                command: "exit 1".to_string(),
                timeout_ms: 1000,
                fail_action: FailAction::Continue,
            }],
        );

        let context = serde_json::json!({"task_id": 123});
        let result = registry.execute(HookPoint::TaskClaimed, &context);
        assert!(
            result.is_ok(),
            "Should succeed because failure action is Continue"
        );
    }

    #[test]
    fn test_hook_execution_abort() {
        let mut registry = HookRegistry::new();
        registry.config.hooks.insert(
            HookPoint::PreProviderCall,
            vec![HookEntry {
                name: "fail_abort".to_string(),
                command: "exit 1".to_string(),
                timeout_ms: 1000,
                fail_action: FailAction::Abort,
            }],
        );

        let context = serde_json::json!({"task_id": 123});
        let result = registry.execute(HookPoint::PreProviderCall, &context);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("fail_abort"));
    }

    #[test]
    fn test_stdin_context() {
        let mut registry = HookRegistry::new();
        registry.config.hooks.insert(
            HookPoint::PostProviderCall,
            vec![HookEntry {
                name: "check_stdin".to_string(),
                command: "grep -q 'task_id'".to_string(),
                timeout_ms: 1000,
                fail_action: FailAction::Abort,
            }],
        );

        let context = serde_json::json!({"task_id": 999});
        let result = registry.execute(HookPoint::PostProviderCall, &context);
        assert!(result.is_ok());

        let bad_context = serde_json::json!({"other_field": 999});
        let bad_result = registry.execute(HookPoint::PostProviderCall, &bad_context);
        assert!(bad_result.is_err());
    }

    // ── load_default fallback tests ─────────────────────────────────

    #[test]
    fn test_load_default_returns_empty_when_no_config_files() {
        let _lock = match TEST_ENV_LOCK.lock() {
            Ok(lock) => lock,
            Err(poisoned) => poisoned.into_inner(),
        };
        let tmp = tempfile::tempdir().unwrap();
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", tmp.path());

        let registry = HookRegistry::load_default();
        assert!(registry.config.hooks.is_empty());

        match original_home {
            Some(home) => std::env::set_var("HOME", home),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn test_load_default_from_tempdir() {
        // Write a config to a temp path, override HOME so load_default finds it
        let _lock = match TEST_ENV_LOCK.lock() {
            Ok(lock) => lock,
            Err(poisoned) => poisoned.into_inner(),
        };
        let tmp = std::env::temp_dir().join(format!("hook_home_{}", std::process::id()));
        let config_dir = tmp.join(".rust-chatbot");
        std::fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("hooks.json");
        let json = r#"{
            "hooks": {
                "TaskClaimed": [{
                    "name": "load_default_test",
                    "command": "exit 0",
                    "timeout_ms": 500,
                    "fail_action": "Continue"
                }]
            }
        }"#;
        std::fs::write(&config_path, json).unwrap();

        // Temporarily set HOME to our temp dir
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &tmp);

        let registry = HookRegistry::load_default();
        assert!(registry.config.hooks.contains_key(&HookPoint::TaskClaimed));
        let hooks = registry.config.hooks.get(&HookPoint::TaskClaimed).unwrap();
        assert_eq!(hooks[0].name, "load_default_test");

        // Restore HOME
        if let Some(h) = original_home {
            std::env::set_var("HOME", h);
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_from_file_invalid_json() {
        let path = temp_file();
        std::fs::write(&path, "not valid json!!!").unwrap();

        let result = HookRegistry::load_from_file(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("JSON Parse Error"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_load_from_file_missing_file() {
        let result = HookRegistry::load_from_file("/tmp/nonexistent_hook_config_99999.json");
        assert!(result.is_err());
    }

    // ── retry logic tests ───────────────────────────────────────────

    #[test]
    fn test_hook_retry_then_succeed() {
        // Use a temp file as a counter: first run creates it (and fails),
        // second run sees it exists (and succeeds).
        let marker = std::env::temp_dir().join(format!("hook_retry_marker_{}", std::process::id()));
        let _ = std::fs::remove_file(&marker); // ensure clean slate

        let cmd = format!(
            "if [ -f '{}' ]; then exit 0; else touch '{}'; exit 1; fi",
            marker.display(),
            marker.display()
        );

        let mut registry = HookRegistry::new();
        registry.config.hooks.insert(
            HookPoint::TaskClaimed,
            vec![HookEntry {
                name: "retry_hook".to_string(),
                command: cmd,
                timeout_ms: 2000,
                fail_action: FailAction::Retry,
            }],
        );

        let context = serde_json::json!({"run_id": "r1", "task_id": "t1"});
        let result = registry.execute(HookPoint::TaskClaimed, &context);
        assert!(result.is_ok(), "Should succeed on retry: {:?}", result);

        let _ = std::fs::remove_file(&marker);
    }

    #[test]
    fn test_hook_retry_then_fail() {
        // Command always fails — retry should exhaust then abort
        let mut registry = HookRegistry::new();
        registry.config.hooks.insert(
            HookPoint::TaskClaimed,
            vec![HookEntry {
                name: "always_fail_retry".to_string(),
                command: "exit 1".to_string(),
                timeout_ms: 1000,
                fail_action: FailAction::Retry,
            }],
        );

        let context = serde_json::json!({"run_id": "r1", "task_id": "t1"});
        let result = registry.execute(HookPoint::TaskClaimed, &context);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("aborted"));
    }

    #[test]
    fn test_hook_execution_timeout_is_enforced() {
        let mut registry = HookRegistry::new();
        registry.config.hooks.insert(
            HookPoint::PreProviderCall,
            vec![HookEntry {
                name: "sleepy_hook".to_string(),
                command: "while :; do :; done".to_string(),
                timeout_ms: 50,
                fail_action: FailAction::Abort,
            }],
        );

        let started_at = Instant::now();
        let result = registry.execute(HookPoint::PreProviderCall, &serde_json::json!({}));

        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("Timed out after 50 ms"),
            "timeout failure should surface clearly"
        );
        assert!(
            started_at.elapsed() < Duration::from_millis(900),
            "timeout should fail before the full hook command duration elapses"
        );
    }

    // ── worker context shape tests ──────────────────────────────────
    // These verify the exact JSON context shapes used in chatbot_worker.rs

    #[test]
    fn test_worker_task_claimed_context_shape() {
        let mut registry = HookRegistry::new();
        // Validate that the hook receives run_id and task_id via stdin
        registry.config.hooks.insert(HookPoint::TaskClaimed, vec![HookEntry {
            name: "validate_claimed_ctx".to_string(),
            command: "python3 -c \"import sys,json; d=json.load(sys.stdin); assert 'run_id' in d; assert 'task_id' in d\"".to_string(),
            timeout_ms: 3000,
            fail_action: FailAction::Abort,
        }]);

        let context = serde_json::json!({"run_id": "run-abc", "task_id": "task-123"});
        let result = registry.execute(HookPoint::TaskClaimed, &context);
        assert!(
            result.is_ok(),
            "TaskClaimed context should contain run_id and task_id: {:?}",
            result
        );
    }

    #[test]
    fn test_worker_task_completed_context_shape() {
        let mut registry = HookRegistry::new();
        registry.config.hooks.insert(HookPoint::TaskCompleted, vec![HookEntry {
            name: "validate_completed_ctx".to_string(),
            command: "python3 -c \"import sys,json; d=json.load(sys.stdin); assert 'run_id' in d; assert 'task_id' in d; assert 'success' in d\"".to_string(),
            timeout_ms: 3000,
            fail_action: FailAction::Abort,
        }]);

        let context =
            serde_json::json!({"run_id": "run-abc", "task_id": "task-123", "success": true});
        let result = registry.execute(HookPoint::TaskCompleted, &context);
        assert!(
            result.is_ok(),
            "TaskCompleted context should contain run_id, task_id, success: {:?}",
            result
        );
    }

    #[test]
    fn test_worker_pre_provider_call_context_shape() {
        let mut registry = HookRegistry::new();
        registry.config.hooks.insert(
            HookPoint::PreProviderCall,
            vec![HookEntry {
                name: "validate_pre_ctx".to_string(),
                command:
                    "python3 -c \"import sys,json; d=json.load(sys.stdin); assert 'chat_id' in d\""
                        .to_string(),
                timeout_ms: 3000,
                fail_action: FailAction::Abort,
            }],
        );

        let context = serde_json::json!({"chat_id": "chat-456"});
        let result = registry.execute(HookPoint::PreProviderCall, &context);
        assert!(
            result.is_ok(),
            "PreProviderCall context should contain chat_id: {:?}",
            result
        );
    }

    #[test]
    fn test_worker_post_provider_call_context_shape() {
        let mut registry = HookRegistry::new();
        registry.config.hooks.insert(HookPoint::PostProviderCall, vec![HookEntry {
            name: "validate_post_ctx".to_string(),
            command: "python3 -c \"import sys,json; d=json.load(sys.stdin); assert 'chat_id' in d; assert 'success' in d\"".to_string(),
            timeout_ms: 3000,
            fail_action: FailAction::Abort,
        }]);

        let context = serde_json::json!({"chat_id": "chat-456", "success": true});
        let result = registry.execute(HookPoint::PostProviderCall, &context);
        assert!(
            result.is_ok(),
            "PostProviderCall context should contain chat_id and success: {:?}",
            result
        );
    }

    // ── multi-hook chaining tests ───────────────────────────────────

    #[test]
    fn test_multiple_hooks_all_succeed() {
        let mut registry = HookRegistry::new();
        registry.config.hooks.insert(
            HookPoint::PreProviderCall,
            vec![
                HookEntry {
                    name: "hook_a".to_string(),
                    command: "exit 0".to_string(),
                    timeout_ms: 1000,
                    fail_action: FailAction::Abort,
                },
                HookEntry {
                    name: "hook_b".to_string(),
                    command: "exit 0".to_string(),
                    timeout_ms: 1000,
                    fail_action: FailAction::Abort,
                },
            ],
        );

        let context = serde_json::json!({"chat_id": "c1"});
        let result = registry.execute(HookPoint::PreProviderCall, &context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_hooks_second_aborts() {
        let mut registry = HookRegistry::new();
        registry.config.hooks.insert(
            HookPoint::PreProviderCall,
            vec![
                HookEntry {
                    name: "hook_pass".to_string(),
                    command: "exit 0".to_string(),
                    timeout_ms: 1000,
                    fail_action: FailAction::Abort,
                },
                HookEntry {
                    name: "hook_fail".to_string(),
                    command: "exit 1".to_string(),
                    timeout_ms: 1000,
                    fail_action: FailAction::Abort,
                },
            ],
        );

        let context = serde_json::json!({"chat_id": "c1"});
        let result = registry.execute(HookPoint::PreProviderCall, &context);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("hook_fail"));
    }

    #[test]
    fn test_multiple_hooks_first_continues_second_runs() {
        // First hook fails with Continue, second should still run and succeed
        let marker = std::env::temp_dir().join(format!("hook_chain_marker_{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);

        let mut registry = HookRegistry::new();
        registry.config.hooks.insert(
            HookPoint::PostProviderCall,
            vec![
                HookEntry {
                    name: "hook_continue".to_string(),
                    command: "exit 1".to_string(),
                    timeout_ms: 1000,
                    fail_action: FailAction::Continue,
                },
                HookEntry {
                    name: "hook_marker".to_string(),
                    command: format!("touch '{}'", marker.display()),
                    timeout_ms: 1000,
                    fail_action: FailAction::Abort,
                },
            ],
        );

        let context = serde_json::json!({"chat_id": "c1", "success": true});
        let result = registry.execute(HookPoint::PostProviderCall, &context);
        assert!(result.is_ok());
        assert!(
            marker.exists(),
            "Second hook should have run after first continued past failure"
        );

        let _ = std::fs::remove_file(&marker);
    }

    // ── edge cases ──────────────────────────────────────────────────

    #[test]
    fn test_execute_with_no_hooks_registered_for_point() {
        let registry = HookRegistry::new();
        let context = serde_json::json!({"chat_id": "c1"});
        // Should be a no-op, return Ok
        let result = registry.execute(HookPoint::PreProviderCall, &context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_new_registry_is_empty() {
        let registry = HookRegistry::new();
        assert!(registry.config.hooks.is_empty());
    }

    #[test]
    fn test_config_defaults() {
        // Verify default_timeout and default_fail_action work in deserialization
        let json = r#"{
            "hooks": {
                "TaskClaimed": [{
                    "name": "minimal_hook",
                    "command": "exit 0"
                }]
            }
        }"#;
        let config: HookConfig = serde_json::from_str(json).unwrap();
        let hooks = config.hooks.get(&HookPoint::TaskClaimed).unwrap();
        assert_eq!(
            hooks[0].timeout_ms, 5000,
            "default timeout should be 5000ms"
        );
        assert_eq!(
            hooks[0].fail_action,
            FailAction::Continue,
            "default fail_action should be Continue"
        );
    }

    #[test]
    fn test_run_command_receives_full_json_context() {
        // Verify the hook command receives the complete serialized context on stdin
        let mut registry = HookRegistry::new();
        let output_path =
            std::env::temp_dir().join(format!("hook_stdin_dump_{}", std::process::id()));
        let _ = std::fs::remove_file(&output_path);

        registry.config.hooks.insert(
            HookPoint::TaskCompleted,
            vec![HookEntry {
                name: "dump_stdin".to_string(),
                command: format!("cat > '{}'", output_path.display()),
                timeout_ms: 2000,
                fail_action: FailAction::Abort,
            }],
        );

        let context = serde_json::json!({"run_id": "r99", "task_id": "t42", "success": false});
        let result = registry.execute(HookPoint::TaskCompleted, &context);
        assert!(result.is_ok());

        let dumped = std::fs::read_to_string(&output_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&dumped).unwrap();
        assert_eq!(parsed["run_id"], "r99");
        assert_eq!(parsed["task_id"], "t42");
        assert_eq!(parsed["success"], false);

        let _ = std::fs::remove_file(&output_path);
    }

    #[test]
    fn test_hook_point_serialization_roundtrip() {
        // All four HookPoint variants should survive JSON roundtrip
        let points = vec![
            HookPoint::PreProviderCall,
            HookPoint::PostProviderCall,
            HookPoint::TaskClaimed,
            HookPoint::TaskCompleted,
        ];
        for point in &points {
            let serialized = serde_json::to_string(point).unwrap();
            let deserialized: HookPoint = serde_json::from_str(&serialized).unwrap();
            assert_eq!(*point, deserialized);
        }
    }

    #[test]
    fn test_config_with_all_hook_points() {
        let json = r#"{
            "hooks": {
                "PreProviderCall":  [{"name": "h1", "command": "exit 0"}],
                "PostProviderCall": [{"name": "h2", "command": "exit 0"}],
                "TaskClaimed":      [{"name": "h3", "command": "exit 0"}],
                "TaskCompleted":    [{"name": "h4", "command": "exit 0"}]
            }
        }"#;
        let config: HookConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.hooks.len(), 4);
    }
}
