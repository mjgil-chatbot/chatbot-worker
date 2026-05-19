fn apply_anthropic_transport_overrides(
    extra_env: &mut Vec<(String, String)>,
    overrides: Option<&ProviderRequestOverrides>,
    default_base_url: Option<String>,
    default_auth_token: Option<String>,
) {
    let base_url = optional_string_override(
        overrides.and_then(|overrides| overrides.anthropic_base_url.as_ref()),
        TEST_ANTHROPIC_BASE_URL_ENV,
    )
    .or(default_base_url);
    if let Some(base_url) = base_url {
        upsert_env_var(extra_env, "ANTHROPIC_BASE_URL", base_url);
    }

    let auth_token = optional_string_override(
        overrides.and_then(|overrides| overrides.anthropic_auth_token.as_ref()),
        TEST_ANTHROPIC_AUTH_TOKEN_ENV,
    )
    .or(default_auth_token);
    if let Some(auth_token) = auth_token {
        upsert_env_var(extra_env, "ANTHROPIC_AUTH_TOKEN", auth_token);
    }
}
fn push_runsc_mount(mounts: &mut Vec<RunscMount>, mount: RunscMount) {
    if !mount.host.exists() {
        return;
    }
    if mounts
        .iter()
        .any(|existing| existing.host == mount.host && existing.target == mount.target)
    {
        return;
    }
    mounts.push(mount);
}
fn push_shared_runsc_mounts(mounts: &mut Vec<RunscMount>) {
    push_runsc_mount(
        mounts,
        RunscMount::same_path(PathBuf::from(TASK_REPORT_VALIDATOR_RUNSC_PATH)),
    );
    push_runsc_mount(mounts, RunscMount::same_path(mcp_coordinator_binary_path()));
}
fn push_runsc_dirs_mounts(mounts: &mut Vec<RunscMount>, runsc_dirs: &[String]) {
    for dir in runsc_dirs {
        let path = PathBuf::from(dir);
        if path.exists() {
            push_runsc_mount(mounts, RunscMount::same_path(path));
        }
    }
}
fn resolved_gemini_runsc_home_dir(
    session: Option<&ChatSession>,
    overrides: Option<&ProviderRequestOverrides>,
) -> Option<PathBuf> {
    let settings_path = resolved_gemini_settings_path(session, overrides);
    let settings_dir = settings_path.parent().map(Path::to_path_buf);
    let shared_dir = config::get_config_dir(ModelProvider::Gemini)
        .map(PathBuf::from)
        .filter(|path| path.exists());
    let legacy_dir = dirs::home_dir()
        .map(|home| home.join(".gemini"))
        .filter(|path| path.exists());

    for candidate in [&settings_dir, &shared_dir, &legacy_dir] {
        if candidate
            .as_ref()
            .is_some_and(|path| path.join("projects.json").exists())
        {
            return candidate.clone();
        }
    }

    shared_dir.or(settings_dir).or(legacy_dir)
}
const GEMINI_RUNSC_HOME_SIDECAR_FILES: &[&str] = &[
    "google_accounts.json",
    "installation_id",
    "mcp-oauth-tokens-v2.json",
    "oauth_creds.json",
    "projects.json",
    "state.json",
    "trustedFolders.json",
];
fn is_writable_dir(path: &Path) -> bool {
    if std::fs::create_dir_all(path).is_err() {
        return false;
    }

    let probe = path.join(".rust-chatbot-gemini-runsc-probe");
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
}
fn gemini_runsc_home_sidecar_path(source: &Path) -> Option<PathBuf> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&source.to_string_lossy(), &mut hasher);
    let source_hash = std::hash::Hasher::finish(&hasher);

    let shared_base = config::shared_infra_dir().join("provider-bridge-gemini-home");
    let temp_base = std::env::temp_dir().join("rust-chatbot-provider-bridge-gemini-home");
    let fallback_base = PathBuf::from("/tmp").join("rust-chatbot-provider-bridge-gemini-home");

    for base in [shared_base, temp_base, fallback_base] {
        let candidate = base.join(format!("{source_hash:016x}"));
        if is_writable_dir(&candidate) {
            return Some(candidate);
        }
    }

    None
}
fn prepare_gemini_runsc_home_dir(
    session: Option<&ChatSession>,
    overrides: Option<&ProviderRequestOverrides>,
) -> Option<PathBuf> {
    let source_dir = resolved_gemini_runsc_home_dir(session, overrides)?;
    let sidecar_dir = gemini_runsc_home_sidecar_path(&source_dir)?;
    std::fs::create_dir_all(&sidecar_dir).ok()?;

    for file_name in GEMINI_RUNSC_HOME_SIDECAR_FILES {
        let source = source_dir.join(file_name);
        let destination = sidecar_dir.join(file_name);
        if source.is_file() {
            std::fs::copy(&source, &destination).ok()?;
        } else {
            let _ = std::fs::remove_file(&destination);
        }
    }

    Some(sidecar_dir)
}
fn output_schema_sidecar_path(schema: &str) -> PathBuf {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&schema, &mut hasher);
    let schema_hash = std::hash::Hasher::finish(&hasher);
    config::shared_infra_dir()
        .join("provider-bridge-output-schemas")
        .join(format!("{schema_hash:016x}.json"))
}
fn persist_output_schema_sidecar(schema: &str) -> Option<PathBuf> {
    let trimmed = schema.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = output_schema_sidecar_path(trimmed);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    if std::fs::read_to_string(&path).ok().as_deref() != Some(trimmed) {
        std::fs::write(&path, trimmed).ok()?;
    }
    Some(path)
}
fn resolved_output_schema_path(overrides: Option<&ProviderRequestOverrides>) -> Option<PathBuf> {
    overrides
        .and_then(|overrides| overrides.output_json_schema.as_deref())
        .and_then(persist_output_schema_sidecar)
}
fn resolve_node_asdf_binary(binary: &str) -> Option<PathBuf> {
    let root = std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(dirs::home_dir)?
        .join(".asdf")
        .join("installs")
        .join("nodejs");
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(root).ok()? {
        let Ok(entry) = entry else {
            continue;
        };
        let candidate = entry.path().join("bin").join(binary);
        if !candidate.is_file() {
            continue;
        }
        let modified = candidate
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        match &newest {
            Some((best, _)) if modified <= *best => {}
            _ => newest = Some((modified, candidate)),
        }
    }
    newest.map(|(_, path)| path)
}
fn resolve_cli_launch_binary(command: &str) -> Option<PathBuf> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }

    let candidate = PathBuf::from(trimmed);
    if candidate.is_absolute() || candidate.components().count() > 1 {
        return candidate.is_file().then_some(candidate);
    }

    std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join(trimmed))
                .find(|path| path.is_file())
        })
        .or_else(|| resolve_node_asdf_binary(trimmed))
}
pub(crate) fn resolve_cli_launch_path(command: &str) -> String {
    resolve_cli_launch_binary(command)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| command.to_string())
}
fn resolved_cli_runtime_path(cli: &str) -> Option<std::ffi::OsString> {
    let Some(cli_dir) =
        resolve_cli_launch_binary(cli).and_then(|path| path.parent().map(Path::to_path_buf))
    else {
        return None;
    };

    let mut paths = vec![cli_dir.clone()];
    if let Some(existing) = std::env::var_os("PATH") {
        for entry in std::env::split_paths(&existing) {
            if entry != cli_dir {
                paths.push(entry);
            }
        }
    }

    std::env::join_paths(paths).ok()
}
pub fn apply_cli_runtime_env(command: &mut std::process::Command, cli: &str) {
    if let Some(joined) = resolved_cli_runtime_path(cli) {
        command.env("PATH", joined);
    }
}
fn push_cli_runtime_path_env(extra_env: &mut Vec<(String, String)>, cli: &str) {
    if let Some(joined) = resolved_cli_runtime_path(cli) {
        upsert_env_var(extra_env, "PATH", joined.to_string_lossy().into_owned());
    }
}
fn read_api_key_file(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
fn native_gemini_api_key() -> Option<String> {
    let legacy_dir = dirs::home_dir().map(|home| home.join(".gemini").join("api.key"));
    let config_dir = config::get_config_dir(ModelProvider::Gemini)
        .map(PathBuf::from)
        .map(|dir| dir.join("api.key"));

    for candidate in [config_dir.as_ref(), legacy_dir.as_ref()]
        .into_iter()
        .flatten()
        .map(PathBuf::from)
    {
        if let Some(api_key) = read_api_key_file(candidate) {
            return Some(api_key);
        }
    }

    None
}
fn gemini_cli_fallback_api_key() -> Option<String> {
    std::env::var("GEMINI_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(native_gemini_api_key)
}
fn native_gemini_oauth_creds_path() -> PathBuf {
    config::get_config_dir(ModelProvider::Gemini)
        .map(PathBuf::from)
        .unwrap_or_else(|| config::canonical_shared_root_dir().join(".gemini"))
        .join("oauth_creds.json")
}
pub(crate) fn gemini_subscription_auth_available() -> bool {
    native_gemini_oauth_creds_path().is_file()
}
fn provider_api_key(provider: ModelProvider) -> Option<String> {
    let entry = crate::provider_catalog::by_id(provider);
    if entry.api_key_env.is_empty() {
        return None;
    }
    std::env::var(entry.api_key_env)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            config::provider_api_key_path(provider)
                .and_then(read_api_key_file)
                .or_else(|| match provider {
                    // MiniMaxForge uses the same upstream credential as native MiniMax.
                    // Reuse the existing MiniMax key file when the Forge-specific slot
                    // has not been created yet so users do not need to duplicate secrets.
                    ModelProvider::MiniMaxForge => provider_api_key(ModelProvider::MiniMax),
                    // GeminiForge uses the same upstream credential as native Gemini.
                    // Reuse the native Gemini key file when the Forge-specific slot is
                    // absent so users only have to maintain one secret.
                    ModelProvider::GeminiForge => native_gemini_api_key(),
                    _ => None,
                })
                .or_else(|| {
                    (provider == ModelProvider::OpenRouter)
                        .then(|| read_api_key_file(config::legacy_openrouter_api_key_path()))
                        .flatten()
                })
        })
}
fn claude_compat_default_config_dir(provider: ModelProvider) -> Option<PathBuf> {
    match provider {
        ModelProvider::Claude => config::get_claude_config_dir().map(PathBuf::from),
        ModelProvider::MiniMax => config::get_minimax_config_dir().map(PathBuf::from),
        _ => None,
    }
}
fn claude_compat_runsc_mounts(
    _provider: ModelProvider,
    overrides: Option<&ProviderRequestOverrides>,
) -> Vec<RunscMount> {
    let mut mounts = Vec::new();
    // Auth sidecar mounts are disabled for all providers except gemini,
    // but user-specified runsc dirs are still honored.
    if let Some(runsc_dirs) = overrides.and_then(|o| Some(&o.runsc_dirs)).filter(|d| !d.is_empty()) {
        push_runsc_dirs_mounts(&mut mounts, runsc_dirs);
    }
    mounts
}
pub fn claude_compat_config(
    provider: ModelProvider,
    overrides: Option<&ProviderRequestOverrides>,
) -> ClaudeCliConfig {
    let entry = crate::provider_catalog::by_id(provider);
    let config_dir = path_override(
        overrides.and_then(|o| o.config_dir.as_ref()),
        TEST_PROVIDER_CONFIG_DIR_ENV,
        || claude_compat_default_config_dir(provider),
    );

    // Provider-specific directory setup hooks
    if let Some(path) = config_dir.as_ref() {
        if provider == ModelProvider::MiniMax {
            ensure_minimax_layout(path);
        }
    }

    let mut extra_env = Vec::new();
    let api_key = provider_api_key(provider);
    apply_anthropic_transport_overrides(
        &mut extra_env,
        overrides,
        entry.base_url.map(str::to_string),
        api_key.clone(),
    );
    if let Some(reasoning_effort) = config::effective_reasoning_effort_for_provider(provider) {
        upsert_env_var(
            &mut extra_env,
            "ANTHROPIC_REASONING_EFFORT",
            reasoning_effort,
        );
    }
    let cli = resolve_cli_launch_path(&string_override(
        overrides.and_then(|o| o.cli.as_ref()),
        TEST_PROVIDER_CLI_ENV,
        || entry.cli_binary.to_string(),
    ));
    push_cli_runtime_path_env(&mut extra_env, &cli);

    ClaudeCliConfig {
        cli,
        model: entry.interactive_model.to_string(),
        json_schema: overrides.and_then(|o| o.output_json_schema.clone()),
        output_format: config::OUTPUT_FORMAT.to_string(),
        skip_permissions: config::SKIP_PERMISSIONS,
        verbose_output: config::VERBOSE_OUTPUT,
        allow_api_billing: entry.base_url.is_some(),
        config_dir,
        require_mcp_servers: provider == ModelProvider::Claude,
        extra_env,
        runsc_image: optional_string_override(
            overrides.and_then(|o| o.runsc_image.as_ref()),
            "RUST_CHATBOT_BACKGROUND_RUNSC_IMAGE",
        ), // Disabled fallback
        runsc_workspace_includes: string_list_override(
            overrides.and_then(|o| Some(&o.runsc_workspace_includes)),
            BACKGROUND_RUNSC_WORKSPACE_INCLUDES_ENV,
        ),
        runsc_mounts: claude_compat_runsc_mounts(provider, overrides),
    }
}
fn codex_runsc_mounts(overrides: Option<&ProviderRequestOverrides>) -> Vec<RunscMount> {
    let mut mounts = Vec::new();
    let codex_toml = resolved_codex_toml_path(overrides);
    push_runsc_mount(&mut mounts, RunscMount::same_path(codex_toml));
    if let Some(output_schema) = resolved_output_schema_path(overrides) {
        push_runsc_mount(&mut mounts, RunscMount::same_path(output_schema));
    }
    if let Some(runsc_dirs) = overrides.and_then(|o| Some(&o.runsc_dirs)).filter(|d| !d.is_empty()) {
        push_runsc_dirs_mounts(&mut mounts, runsc_dirs);
    }
    mounts
}
fn ensure_codex_home_dir(overrides: Option<&ProviderRequestOverrides>) -> PathBuf {
    let codex_toml_path = resolved_codex_toml_path(overrides);
    let codex_home = codex_toml_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| config::canonical_shared_root_dir().join(".codex"));
    let _ = std::fs::create_dir_all(&codex_home);
    codex_home
}
fn gemini_runsc_mounts(
    session: Option<&ChatSession>,
    overrides: Option<&ProviderRequestOverrides>,
) -> Vec<RunscMount> {
    let mut mounts = Vec::new();
    push_shared_runsc_mounts(&mut mounts);
    let settings_path = resolved_gemini_settings_path(session, overrides);
    push_runsc_mount(&mut mounts, RunscMount::same_path(settings_path.clone()));
    push_runsc_mount(
        &mut mounts,
        RunscMount::same_path(gemini_coordinator_sidecar_path(&settings_path)),
    );
    if let Some(global_dir) = prepare_gemini_runsc_home_dir(session, overrides) {
        push_runsc_mount(
            &mut mounts,
            RunscMount::container_home(global_dir, Path::new(".gemini")),
        );
    }
    push_runsc_mount(
        &mut mounts,
        RunscMount::container_home(&settings_path, Path::new(".gemini/settings.json")),
    );
    // Push user-specified runsc dirs
    if let Some(runsc_dirs) = overrides
        .and_then(|o| Some(&o.runsc_dirs))
        .filter(|d| !d.is_empty())
    {
        push_runsc_dirs_mounts(&mut mounts, runsc_dirs);
    }
    mounts
}
fn qwen_runsc_mounts(overrides: Option<&ProviderRequestOverrides>) -> Vec<RunscMount> {
    let mut mounts = Vec::new();
    if let Some(runsc_dirs) = overrides.and_then(|o| Some(&o.runsc_dirs)).filter(|d| !d.is_empty()) {
        push_runsc_dirs_mounts(&mut mounts, runsc_dirs);
    }
    mounts
}
fn kimi_runsc_mounts(overrides: Option<&ProviderRequestOverrides>) -> Vec<RunscMount> {
    let mut mounts = Vec::new();
    if let Some(runsc_dirs) = overrides.and_then(|o| Some(&o.runsc_dirs)).filter(|d| !d.is_empty()) {
        push_runsc_dirs_mounts(&mut mounts, runsc_dirs);
    }
    mounts
}
pub fn kimi_cli_config(
    overrides: Option<&ProviderRequestOverrides>,
) -> ai_providers::config::KimiCliConfig {
    let entry = crate::provider_catalog::by_id(ModelProvider::Kimi);
    let cli = resolve_cli_launch_path(entry.cli_binary);
    let mut extra_env = Vec::new();
    push_cli_runtime_path_env(&mut extra_env, &cli);

    let model = if entry.interactive_model.is_empty() {
        None
    } else {
        Some(entry.interactive_model.to_string())
    };

    ai_providers::config::KimiCliConfig {
        cli,
        model,
        output_format: "stream-json".to_string(),
        yolo: true,
        resume_session_id: None,
        extra_env,
        runsc_image: optional_string_override(
            overrides.and_then(|o| o.runsc_image.as_ref()),
            "RUST_CHATBOT_BACKGROUND_RUNSC_IMAGE",
        ), // Disabled fallback
        runsc_workspace_includes: string_list_override(
            overrides.and_then(|o| Some(&o.runsc_workspace_includes)),
            BACKGROUND_RUNSC_WORKSPACE_INCLUDES_ENV,
        ),
        runsc_mounts: kimi_runsc_mounts(overrides),
    }
}
pub fn mcp_server_names(overrides: Option<&ProviderRequestOverrides>) -> Vec<String> {
    let mcp_json = resolved_mcp_json_path(overrides);
    let Some(json) = load_mcp_json_value(&mcp_json) else {
        return Vec::new();
    };
    filtered_profile_mcp_servers(&json, None, overrides)
        .keys()
        .cloned()
        .collect()
}
fn provider_facing_claude_mcp_server_names(
    provider: ModelProvider,
    overrides: Option<&ProviderRequestOverrides>,
) -> Vec<String> {
    let Some(config_dir) = claude_compat_config(provider, overrides).config_dir else {
        return Vec::new();
    };
    let Some(json) = load_mcp_json_value(&config_dir.join(".claude.json")) else {
        return Vec::new();
    };

    json.get("mcpServers")
        .and_then(|value| value.as_object())
        .map(|servers| servers.keys().cloned().collect())
        .unwrap_or_default()
}
fn configured_claude_family_has_mcp_servers(
    provider: ModelProvider,
    overrides: Option<&ProviderRequestOverrides>,
) -> bool {
    let mcp_json = resolved_mcp_json_path(overrides);
    let Some(json) = load_mcp_json_value(&mcp_json) else {
        return false;
    };

    configured_mcp_servers(&json, provider_mcp_profile(provider))
        .map(|servers| !filtered_mcp_servers(servers, overrides).is_empty())
        .unwrap_or(false)
}
fn visible_claude_family_mcp_server_names(
    provider: ModelProvider,
    overrides: Option<&ProviderRequestOverrides>,
) -> Vec<String> {
    let mut names = provider_facing_claude_mcp_server_names(provider, overrides);
    if names.is_empty() && configured_claude_family_has_mcp_servers(provider, overrides) {
        names.push(MCP_COORDINATOR_SERVER_NAME.to_string());
    }
    names.sort_unstable();
    names.dedup();
    names
}
pub fn qwen_cli_config(overrides: Option<&ProviderRequestOverrides>) -> QwenCodeCliConfig {
    let mcp_server_names = mcp_server_names(overrides);
    let cli = resolve_cli_launch_path(&config::qwen_code_cli_path());
    let mut extra_env = Vec::new();
    push_cli_runtime_path_env(&mut extra_env, &cli);

    QwenCodeCliConfig {
        cli,
        model: config::interactive_model_for(ModelProvider::Qwen).map(str::to_string),
        output_format: "stream-json".to_string(),
        yolo: true,
        append_system_prompt: None,
        resume_session_id: None,
        include_directories: Vec::new(),
        allowed_mcp_server_names: mcp_server_names,
        auth_type: Some(config::QWEN_AUTH_TYPE.to_string()),
        extra_env,
        runsc_image: optional_string_override(
            overrides.and_then(|o| o.runsc_image.as_ref()),
            "RUST_CHATBOT_BACKGROUND_RUNSC_IMAGE",
        ), // Disabled fallback
        runsc_workspace_includes: string_list_override(
            overrides.and_then(|o| Some(&o.runsc_workspace_includes)),
            BACKGROUND_RUNSC_WORKSPACE_INCLUDES_ENV,
        ),
        runsc_mounts: qwen_runsc_mounts(overrides),
    }
}
fn ensure_repo_config_dir(path: &PathBuf) {
    let _ = std::fs::create_dir_all(path);
}
fn ensure_minimax_layout(path: &PathBuf) {
    ensure_repo_config_dir(path);

    let _minimax_json = path.join(".minimax.json");
    let claude_json = path.join(".claude.json");

    if claude_json.exists() {
        return;
    }

    #[cfg(unix)]
    {
        let _ = std::os::unix::fs::symlink(".minimax.json", &claude_json);
    }

    #[cfg(not(unix))]
    {
        if _minimax_json.exists() {
            let _ = std::fs::copy(&_minimax_json, &claude_json);
        }
    }
}
pub fn to_provider_session(session: Option<&ChatSession>) -> Option<ProviderSession> {
    let session = session?;
    Some(ProviderSession {
        working_dir: session.launched_from.clone(),
        claude_session_id: session.claude_session_id.clone(),
        glm_session_id: None,
        minimax_session_id: session.minimax_session_id.clone(),
        qwen_session_id: session.qwen_session_id.clone(),
        codex_session_id: session.codex_session_id.clone(),
        nemotron_session_id: session.nvidia_session_id.clone(),
        gemini_session_id: session.gemini_session_id.clone(),
        openrouter_session_id: session.openrouter_session_id.clone(),
        omniforge_session_id: session.omniforge_session_id.clone(),
        kimi_session_id: session.kimi_session_id.clone(),
        messages: session
            .messages
            .iter()
            .map(|message| ProviderMessage {
                content: message.content.clone(),
                is_user: message.is_user,
                history_content: None,
            })
            .collect(),
    })
}
pub fn to_provider_memory_context(
    memory_context: Option<&MemoryContext>,
) -> Option<ProviderMemoryContext> {
    let memory_context = memory_context?;
    Some(ProviderMemoryContext {
        workspace_id: memory_context.workspace_id.clone(),
        query: memory_context.query.clone(),
        hits: memory_context
            .hits
            .iter()
            .map(|hit| ProviderMemoryHit {
                doc_key: hit.doc_key.clone(),
                chat_id: hit.chat_id.clone(),
                role: hit.role.clone(),
                session_name: hit.session_name.clone(),
                timestamp: hit.timestamp,
                snippet: hit.snippet.clone(),
                title: hit.title.clone(),
                message_index: hit.message_index,
                turn_index: hit.turn_index,
                score: hit.score,
            })
            .collect(),
    })
}
/// MiniMax API key from environment or key file.
pub fn minimax_api_key() -> Option<String> {
    provider_api_key(ModelProvider::MiniMax)
}
