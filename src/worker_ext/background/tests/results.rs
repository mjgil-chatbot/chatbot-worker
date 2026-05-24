use super::*;
use crate::model::ModelProvider;
use std::sync::Arc;
use uuid::Uuid;

use super::super::executor;

#[tokio::test]
async fn test_result_merging() {
    let results = vec![
        BackgroundResult {
            id: Uuid::new_v4(),
            output: Ok("good1".into()),
        },
        BackgroundResult {
            id: Uuid::new_v4(),
            output: Err("bad1".into()),
        },
        BackgroundResult {
            id: Uuid::new_v4(),
            output: Ok("good2".into()),
        },
    ];

    let merged = MergedResults::from_results(results);
    assert_eq!(merged.succeeded.len(), 2);
    assert_eq!(merged.failed.len(), 1);
    assert_eq!(merged.total(), 3);
    assert_eq!(merged.succeeded[0].1, "good1");
    assert_eq!(merged.failed[0].1, "bad1");
}

#[tokio::test]
async fn test_take_merged_drains() {
    let mut dispatcher = BackgroundDispatcher::new(4);
    for i in 0..3 {
        dispatcher.add_task(make_task(&format!("m_{}", i)));
    }
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

    let merged = dispatcher.take_merged().await;
    assert_eq!(merged.total(), 3);

    // After take_merged, internal results vec is empty
    let remaining = dispatcher.results.lock().await;
    assert!(remaining.is_empty());
}

// --- MergedResults edge cases ---

#[test]
fn test_merged_results_from_empty_input() {
    let merged = MergedResults::from_results(vec![]);
    assert_eq!(merged.succeeded.len(), 0);
    assert_eq!(merged.failed.len(), 0);
    assert_eq!(merged.total(), 0);
}

#[test]
fn test_merged_results_all_failures() {
    let results = vec![
        BackgroundResult {
            id: Uuid::new_v4(),
            output: Err("err1".into()),
        },
        BackgroundResult {
            id: Uuid::new_v4(),
            output: Err("err2".into()),
        },
    ];
    let merged = MergedResults::from_results(results);
    assert_eq!(merged.succeeded.len(), 0);
    assert_eq!(merged.failed.len(), 2);
    assert_eq!(merged.total(), 2);
}

#[test]
fn test_merged_results_all_successes() {
    let results = vec![
        BackgroundResult {
            id: Uuid::new_v4(),
            output: Ok("ok1".into()),
        },
        BackgroundResult {
            id: Uuid::new_v4(),
            output: Ok("ok2".into()),
        },
    ];
    let merged = MergedResults::from_results(results);
    assert_eq!(merged.succeeded.len(), 2);
    assert_eq!(merged.failed.len(), 0);
}

// --- take_merged drain semantics ---

#[tokio::test]
async fn test_take_merged_twice_returns_empty_second_time() {
    let mut dispatcher = BackgroundDispatcher::new(4);
    dispatcher.add_task(make_task("drain_test"));
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

    let second = dispatcher.take_merged().await;
    assert_eq!(
        second.total(),
        0,
        "second take_merged should be empty after drain"
    );
}

#[tokio::test]
async fn test_run_all_with_no_tasks() {
    let mut dispatcher = BackgroundDispatcher::new(2);
    dispatcher
        .run_all_with_executor(Arc::new(|task| {
            Box::pin(async move {
                BackgroundResult {
                    id: task.id,
                    output: Ok("x".into()),
                }
            })
        }))
        .await;
    let merged = dispatcher.take_merged().await;
    assert_eq!(merged.total(), 0, "no tasks means no results");
}

// --- provider_failure_message branches ---

#[test]
fn test_provider_failure_message_with_stderr() {
    let msg = executor::provider_failure_message(ModelProvider::Claude, Some(1), "error text", "");
    assert!(msg.starts_with(CLAUDE_FAILURE_PREFIX));
    assert!(msg.contains("Exit status: 1"));
    assert!(msg.contains("error text"));
}

#[test]
fn test_provider_failure_message_with_stdout_only() {
    let msg =
        executor::provider_failure_message(ModelProvider::Codex, Some(1), "", "some stdout output");
    assert!(msg.starts_with(CODEX_FAILURE_PREFIX));
    assert!(msg.contains("some stdout output"));
}

#[test]
fn test_provider_failure_message_with_exit_code_only() {
    let msg = executor::provider_failure_message(ModelProvider::Gemini, Some(42), "", "");
    assert!(msg.starts_with(GEMINI_FAILURE_PREFIX));
    assert!(msg.contains("Exit status: 42"));
}

#[test]
fn test_provider_failure_message_no_exit_code() {
    let msg = executor::provider_failure_message(ModelProvider::Qwen, None, "", "");
    assert!(msg.starts_with(QWEN_CODE_FAILURE_PREFIX));
    assert!(msg.contains("Exit status unavailable"));
}

#[test]
fn test_provider_failure_message_minimax_uses_claude_prefix() {
    let msg = executor::provider_failure_message(ModelProvider::MiniMax, Some(1), "err", "");
    assert!(msg.starts_with(CLAUDE_FAILURE_PREFIX));
}

// --- is_provider_failure branches ---

#[test]
fn test_is_provider_failure_claude_login() {
    assert!(executor::is_provider_failure(
        ModelProvider::Claude,
        "API billing detected. Please login"
    ));
}

#[test]
fn test_is_provider_failure_claude_subscription() {
    assert!(executor::is_provider_failure(
        ModelProvider::Claude,
        "Unable to detect subscription type. Please ensure"
    ));
}

#[test]
fn test_is_provider_failure_minimax_missing_key() {
    assert!(executor::is_provider_failure(
        ModelProvider::MiniMax,
        "MINIMAX_API_KEY is not set"
    ));
}

#[test]
fn test_is_provider_failure_qwen_missing_key() {
    assert!(executor::is_provider_failure(
        ModelProvider::Qwen,
        "OPENROUTER_API_KEY is not set"
    ));
}

#[test]
fn test_is_provider_failure_spawn_failure_universal() {
    // SPAWN_FAILURE_PREFIX is detected for all providers
    assert!(executor::is_provider_failure(
        ModelProvider::Claude,
        "Failed to spawn provider CLI: err"
    ));
    assert!(executor::is_provider_failure(
        ModelProvider::Codex,
        "Failed to spawn foo"
    ));
    assert!(executor::is_provider_failure(
        ModelProvider::Gemini,
        "Failed to spawn bar"
    ));
    assert!(executor::is_provider_failure(
        ModelProvider::Qwen,
        "Failed to spawn baz"
    ));
}

#[test]
fn test_is_provider_failure_normal_output_not_failure() {
    assert!(!executor::is_provider_failure(
        ModelProvider::Claude,
        "Here is your answer"
    ));
    assert!(!executor::is_provider_failure(
        ModelProvider::Codex,
        "All done"
    ));
    assert!(!executor::is_provider_failure(
        ModelProvider::Gemini,
        "Result: OK"
    ));
    assert!(!executor::is_provider_failure(
        ModelProvider::Qwen,
        "Success"
    ));
}

// --- classify_provider_output ---

#[test]
fn test_provider_failures_become_error_results() {
    assert_eq!(
        executor::classify_provider_output(
            ModelProvider::Codex,
            "Codex command failed. Recent stderr output:\nboom".to_string(),
        ),
        Err("Codex command failed. Recent stderr output:\nboom".to_string())
    );
    assert_eq!(
        executor::classify_provider_output(ModelProvider::Codex, "final answer".to_string()),
        Ok("final answer".to_string())
    );
}

#[test]
fn test_classify_provider_output_success_for_normal_text() {
    assert_eq!(
        executor::classify_provider_output(ModelProvider::Claude, "good output".into()),
        Ok("good output".to_string())
    );
}

#[test]
fn test_classify_provider_output_error_for_failure_prefix() {
    let fail_msg = format!("{GEMINI_FAILURE_PREFIX}. some error");
    assert!(executor::classify_provider_output(ModelProvider::Gemini, fail_msg).is_err());
}

// --- error merging preserves insertion order ---

#[test]
fn test_merged_results_preserves_insertion_order() {
    let ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
    let results = vec![
        BackgroundResult {
            id: ids[0],
            output: Ok("a".into()),
        },
        BackgroundResult {
            id: ids[1],
            output: Err("e1".into()),
        },
        BackgroundResult {
            id: ids[2],
            output: Ok("b".into()),
        },
        BackgroundResult {
            id: ids[3],
            output: Err("e2".into()),
        },
        BackgroundResult {
            id: ids[4],
            output: Ok("c".into()),
        },
    ];
    let merged = MergedResults::from_results(results);
    assert_eq!(merged.succeeded.len(), 3);
    assert_eq!(merged.failed.len(), 2);
    // Succeeded preserves insertion order
    assert_eq!(merged.succeeded[0], (ids[0], "a".to_string()));
    assert_eq!(merged.succeeded[1], (ids[2], "b".to_string()));
    assert_eq!(merged.succeeded[2], (ids[4], "c".to_string()));
    // Failed preserves insertion order
    assert_eq!(merged.failed[0], (ids[1], "e1".to_string()));
    assert_eq!(merged.failed[1], (ids[3], "e2".to_string()));
}
