use crate::config;
use crate::data::ChatSession;
use crate::model::ModelProvider;
use crate::provider_bridge::ProviderRequestOverrides;
use crate::summary::JsonSummary;
use crate::worker::WorkerRequest;

pub(crate) fn build_session(
    chat_id: &str,
    provider: ModelProvider,
    system_prompt: Option<String>,
    working_dir: Option<String>,
    resume_session_id: Option<String>,
    runsc_dirs: Vec<String>,
    runsc_image: Option<String>,
) -> ChatSession {
    let mut session = ChatSession {
        id: chat_id.to_string(),
        name: String::new(),
        system_prompt,
        json_summary: None::<JsonSummary>,
        claude_session_id: None,
        minimax_session_id: None,
        qwen_session_id: None,
        codex_session_id: None,
        nvidia_session_id: None,
        gemini_session_id: None,
        openrouter_session_id: None,
        omniforge_session_id: None,
        omniforge_model_id: None,
        kimi_session_id: None,
        opencode_session_id: None,
        hermes_session_id: None,
        forge_session_id: None,
        claw_session_id: None,
        launched_from: working_dir,
        runsc_dirs,
        runsc_image,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        messages: vec![],
    };
    if let Some(sid) = resume_session_id.as_ref() {
        provider.set_session_id_on_chat(&mut session, sid.clone());
    }
    session
}

pub(crate) fn runsc_workspace_includes_for_request(request: &WorkerRequest) -> Vec<String> {
    let mut includes = Vec::new();

    if let Some(working_dir) = request
        .working_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        includes.push(working_dir.to_string());
    }

    for dir in &request.runsc_dirs {
        let dir = dir.trim();
        if dir.is_empty() || includes.iter().any(|existing| existing == dir) {
            continue;
        }
        includes.push(dir.to_string());
    }

    includes
}

pub(crate) fn runsc_overrides_for_request(
    provider: ModelProvider,
    request: &WorkerRequest,
) -> Option<ProviderRequestOverrides> {
    let runsc_image = request
        .runsc_image
        .clone()
        .or_else(|| default_runsc_image_for_provider(provider));
    if runsc_image.is_none() && request.runsc_dirs.is_empty() && request.model_override.is_none() {
        return None;
    }

    let mut overrides = ProviderRequestOverrides::default();
    overrides.runsc_image = runsc_image;
    overrides.runsc_dirs = request.runsc_dirs.clone();
    overrides.model = request.model_override.clone();
    overrides.runsc_workspace_includes = runsc_workspace_includes_for_request(request);
    Some(overrides)
}

pub(crate) fn terminal_cli_error_message(error: Option<&str>) -> String {
    error
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "CLI command failed".to_string())
}

fn default_runsc_image_for_provider(provider: ModelProvider) -> Option<String> {
    match provider {
        ModelProvider::Gemini | ModelProvider::GeminiCliForge => {
            let image = config::background_runsc_image();
            (!image.trim().is_empty()).then_some(image)
        }
        _ => None,
    }
}
