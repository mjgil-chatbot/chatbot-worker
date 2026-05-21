use super::*;
use crate::chatbot_worker_runtime::runsc_workspace_includes_for_request;

fn worker_request(command: &str, query: &str) -> WorkerRequest {
    WorkerRequest {
        provider: "codex".to_string(),
        query: query.to_string(),
        working_dir: None,
        system_prompt: None,
        resume_session_id: None,
        memory_context: None,
        parent_pid_override: None,
        runsc_dirs: Vec::new(),
        runsc_image: None,
        model_override: None,
        command: command.to_string(),
        chat_id: "chat-test".to_string(),
    }
}

#[test]
fn parse_provider_accepts_supported_catalog_names_and_aliases() {
    for provider in ModelProvider::SUPPORTED {
        assert_eq!(parse_provider(provider.as_str()).unwrap(), provider);
        assert_eq!(
            parse_provider(&provider.as_str().to_uppercase()).unwrap(),
            provider
        );
    }
}

#[test]
fn parse_provider_rejects_disabled_catalog_names() {
    for provider in ModelProvider::ALL {
        if provider.is_supported() {
            continue;
        }

        assert!(parse_provider(provider.as_str()).is_err());
        assert!(parse_provider(&provider.as_str().to_uppercase()).is_err());
    }
}

#[test]
fn build_session_sets_resume_session_id_on_selected_provider() {
    let session = build_session(
        "chat-test",
        ModelProvider::Nvidia,
        None,
        None,
        Some("resume-123".to_string()),
        Vec::new(),
        None,
    );

    assert_eq!(
        ModelProvider::Nvidia.session_id_from_chat(&session),
        Some(&"resume-123".to_string())
    );
    assert_eq!(ModelProvider::Claude.session_id_from_chat(&session), None);

    let forge_session = build_session(
        "chat-test",
        ModelProvider::GeminiForge,
        None,
        None,
        Some("forge-resume-456".to_string()),
        Vec::new(),
        None,
    );

    assert_eq!(
        ModelProvider::GeminiForge.session_id_from_chat(&forge_session),
        Some(&"forge-resume-456".to_string())
    );

    let omniforge_session = build_session(
        "chat-test",
        ModelProvider::OmniForge,
        None,
        None,
        Some("omniforge-resume-789".to_string()),
        Vec::new(),
        None,
    );

    assert_eq!(
        ModelProvider::OmniForge.session_id_from_chat(&omniforge_session),
        Some(&"omniforge-resume-789".to_string())
    );
}

#[test]
fn terminal_cli_error_message_prefers_provider_error_text() {
    assert_eq!(
        terminal_cli_error_message(Some(" Gemini command failed \n")),
        "Gemini command failed"
    );
    assert_eq!(
        terminal_cli_error_message(Some("   ")),
        "CLI command failed"
    );
    assert_eq!(terminal_cli_error_message(None), "CLI command failed");
}

#[test]
fn runsc_workspace_includes_adds_working_dir_before_extra_dirs() {
    let mut request = worker_request("codex exec", "plain text prompt");
    request.working_dir = Some("/workspace/rust-chatbot".to_string());
    request.runsc_dirs = vec![
        "/workspace/rust-codegen-cranelift-wasm".to_string(),
        "/workspace/rust-chatbot".to_string(),
    ];

    assert_eq!(
        runsc_workspace_includes_for_request(&request),
        vec![
            "/workspace/rust-chatbot".to_string(),
            "/workspace/rust-codegen-cranelift-wasm".to_string(),
        ]
    );
}

#[test]
fn runsc_overrides_for_request_infers_gemini_default_image_and_workspace() {
    let mut request = worker_request("gemini -p hi", "hi");
    request.provider = "gemini".to_string();
    request.working_dir = Some("/workspace/rust-chatbot".to_string());

    let overrides = runsc_overrides_for_request(ModelProvider::Gemini, &request)
        .expect("gemini should stage workspace when runsc is enabled by default");

    assert_eq!(
        overrides.runsc_image.as_deref(),
        Some(config::background_runsc_image().as_str())
    );
    assert_eq!(
        overrides.runsc_workspace_includes,
        vec!["/workspace/rust-chatbot".to_string()]
    );
}

#[test]
fn runsc_overrides_for_request_skips_non_runsc_provider_without_request_config() {
    let mut request = worker_request("codex exec", "plain text prompt");
    request.working_dir = Some("/workspace/rust-chatbot".to_string());

    assert!(runsc_overrides_for_request(ModelProvider::Codex, &request).is_none());
}

#[test]
fn runsc_overrides_for_request_preserves_model_override_without_runsc_settings() {
    let mut request = worker_request("omniforge exec", "plain text prompt");
    request.provider = "omniforge".to_string();
    request.model_override = Some("gemini-3-flash-preview".to_string());

    let overrides = runsc_overrides_for_request(ModelProvider::OmniForge, &request)
        .expect("model override should produce provider overrides");

    assert_eq!(overrides.model.as_deref(), Some("gemini-3-flash-preview"));
    assert!(overrides.runsc_image.is_none());
    assert!(overrides.runsc_dirs.is_empty());
}

#[test]
fn parse_worker_extension_request_skips_standard_provider_requests() {
    let request = worker_request("codex exec", "plain text prompt");
    assert!(parse_worker_extension_request(&request).unwrap().is_none());
}

#[test]
fn background_extension_smoke_test_uses_in_process_dispatch_path() {
    let request = WorkerExtensionRequest::BackgroundDispatch {
        max_concurrent: Some(2),
        tasks: vec![
            BackgroundDispatchTaskRequest {
                provider: "codex".to_string(),
                prompt: "task one".to_string(),
                role: "worker".to_string(),
                system_prompt: None,
                working_dir: None,
                resume_session_id: None,
                context_budget: 0,
            },
            BackgroundDispatchTaskRequest {
                provider: "claude".to_string(),
                prompt: "task two".to_string(),
                role: "worker".to_string(),
                system_prompt: None,
                working_dir: None,
                resume_session_id: None,
                context_budget: 0,
            },
        ],
    };

    let output = execute_worker_extension_request(
        request,
        Arc::new(AtomicBool::new(false)),
        |max_concurrent, tasks, _| {
            assert_eq!(max_concurrent, Some(2));
            assert_eq!(tasks.len(), 2);
            Ok(worker_ext::background::MergedResults {
                succeeded: vec![
                    (uuid::Uuid::new_v4(), "first result".to_string()),
                    (uuid::Uuid::new_v4(), "second result".to_string()),
                ],
                failed: vec![],
            })
        },
        |_, _, _| unreachable!("ast search handler should not run"),
    )
    .unwrap();

    assert!(output.contains("\"tool\": \"background_dispatch\""));
    assert!(output.contains("first result"));
    assert!(output.contains("\"max_concurrent\": 2"));
}

#[test]
fn ast_search_extension_smoke_test_serializes_results() {
    let request = WorkerExtensionRequest::AstSearch {
        pattern: "fn main".to_string(),
        language: worker_ext::ast_search::Language::Rust,
        paths: vec!["src".to_string()],
    };

    let output = execute_worker_extension_request(
        request,
        Arc::new(AtomicBool::new(false)),
        |_, _, _| unreachable!("background handler should not run"),
        |pattern, language, paths| {
            assert_eq!(pattern, "fn main");
            assert_eq!(language, worker_ext::ast_search::Language::Rust);
            assert_eq!(paths, vec!["src".to_string()]);
            Ok(worker_ext::ast_search::AstSearchResponse {
                backend: worker_ext::ast_search::AstSearchBackend::Ripgrep,
                matches: vec![worker_ext::ast_search::AstSearchResult {
                    file: "src/main.rs".to_string(),
                    line: 12,
                    matched_node: "fn main() {}".to_string(),
                    context: "fn main() {}".to_string(),
                }],
                degraded_reason: Some("ast-grep binary `sg` is unavailable".to_string()),
            })
        },
    )
    .unwrap();

    assert!(output.contains("\"tool\": \"ast_search\""));
    assert!(output.contains("\"backend\": \"ripgrep\""));
    assert!(output.contains("src/main.rs"));
}

#[test]
fn complete_worker_request_persists_error_state_for_adapter_failures() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let temp_dir = tempfile::tempdir().unwrap();
    let previous_home = std::env::var_os("HOME");
    let previous_config = std::env::var_os("RUST_CHATBOT_CONFIG_PATH");
    std::env::set_var("HOME", temp_dir.path());
    std::env::remove_var("RUST_CHATBOT_CONFIG_PATH");

    let chat_id = "adapter-main-error";
    std::fs::create_dir_all(worker::worker_dir(chat_id)).unwrap();
    let mut job = WorkerJobSnapshot::new(chat_id, "codex", 123, 456);

    complete_worker_request(
        chat_id,
        &worker::output_path(chat_id),
        &worker::result_path(chat_id),
        &mut job,
        WorkerState::Error,
        "",
        Some("adapter runtime failure".to_string()),
    )
    .unwrap();

    let persisted = worker::read_job_state(chat_id).unwrap().unwrap();
    let result_json = std::fs::read_to_string(worker::result_path(chat_id)).unwrap();

    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    match previous_config {
        Some(value) => std::env::set_var("RUST_CHATBOT_CONFIG_PATH", value),
        None => std::env::remove_var("RUST_CHATBOT_CONFIG_PATH"),
    }

    assert_eq!(persisted.state, WorkerState::Error);
    assert!(result_json.contains("adapter runtime failure"));
}
