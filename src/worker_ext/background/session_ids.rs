use ai_providers::session::ProviderSession;
use chrono::Utc;

use crate::data::ChatSession;
use crate::model::ModelProvider;

pub(super) fn resume_session_id(
    provider: ModelProvider,
    provider_session: Option<&ProviderSession>,
) -> Option<String> {
    let session = provider_session.map(chat_session_from_provider_session)?;
    provider.session_id_from_chat(&session).cloned()
}

pub(super) fn has_resume(
    provider: ModelProvider,
    provider_session: Option<&ProviderSession>,
) -> bool {
    resume_session_id(provider, provider_session).is_some()
}

fn chat_session_from_provider_session(provider_session: &ProviderSession) -> ChatSession {
    let timestamp = Utc::now();
    ChatSession {
        id: String::new(),
        name: String::new(),
        system_prompt: None,
        json_summary: None,
        claude_session_id: provider_session.claude_session_id.clone(),
        minimax_session_id: provider_session.minimax_session_id.clone(),
        qwen_session_id: provider_session.qwen_session_id.clone(),
        codex_session_id: provider_session.codex_session_id.clone(),
        nvidia_session_id: provider_session.nemotron_session_id.clone(),
        gemini_session_id: provider_session.gemini_session_id.clone(),
        openrouter_session_id: provider_session.openrouter_session_id.clone(),
        omniforge_session_id: provider_session.omniforge_session_id.clone(),
        omniforge_model_id: None,
        kimi_session_id: None,
        opencode_session_id: None,
        hermes_session_id: None,
        forge_session_id: None,
        claw_session_id: None,
        launched_from: provider_session.working_dir.clone(),
        runsc_dirs: Vec::new(),
        runsc_image: None,
        created_at: timestamp,
        updated_at: timestamp,
        messages: Vec::new(),
    }
}
