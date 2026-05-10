fn write_file_atomically(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("missing parent directory for {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;

    let file_name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".to_string());
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    for attempt in 0..32u32 {
        let temp_path = parent.join(format!(
            ".{}.{}.{}.tmp",
            file_name,
            std::process::id(),
            nonce + u128::from(attempt)
        ));

        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(mut file) => {
                let write_result = (|| -> Result<()> {
                    file.write_all(contents)?;
                    file.sync_all()?;
                    Ok(())
                })();
                if let Err(err) = write_result {
                    let _ = std::fs::remove_file(&temp_path);
                    return Err(err)
                        .with_context(|| format!("failed to write {}", temp_path.display()));
                }
                drop(file);
                if let Err(err) = std::fs::rename(&temp_path, path) {
                    let _ = std::fs::remove_file(&temp_path);
                    return Err(err)
                        .with_context(|| format!("failed to replace {}", path.display()));
                }
                return Ok(());
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("failed to create temporary config for {}", path.display())
                });
            }
        }
    }

    anyhow::bail!(
        "failed to allocate temporary config path for {}",
        path.display()
    );
}
fn toml_basic_string(value: &str) -> Result<String> {
    serde_json::to_string(value).map_err(Into::into)
}
fn resolved_mcp_json_path(overrides: Option<&ProviderRequestOverrides>) -> PathBuf {
    path_override(
        overrides.and_then(|overrides| overrides.mcp_json_path.as_ref()),
        TEST_MCP_JSON_PATH_ENV,
        || Some(PathBuf::from(config::mcp_json_path())),
    )
    .unwrap_or_else(|| PathBuf::from(config::mcp_json_path()))
}
fn resolved_claude_json_path(overrides: Option<&ProviderRequestOverrides>) -> Option<PathBuf> {
    path_override(
        overrides.and_then(|overrides| overrides.claude_json_path.as_ref()),
        TEST_CLAUDE_JSON_PATH_ENV,
        || config::claude_json_path().map(PathBuf::from),
    )
}
fn resolved_minimax_compat_json_paths(
    overrides: Option<&ProviderRequestOverrides>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(claude_json) = resolved_claude_json_path(overrides) {
        if !paths.contains(&claude_json) {
            paths.push(claude_json);
        }
    }

    let config_dir = overrides
        .and_then(|overrides| overrides.config_dir.as_ref())
        .map(PathBuf::from)
        .or_else(|| config::get_minimax_config_dir().map(PathBuf::from));

    if let Some(config_dir) = config_dir {
        let path = config_dir.join(".claude.json");
        if !paths.contains(&path) {
            paths.push(path);
        }
    }

    paths
}
fn gemini_runsc_settings_sidecar_path(source: &Path) -> PathBuf {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&source.to_string_lossy(), &mut hasher);
    let source_hash = std::hash::Hasher::finish(&hasher);
    config::shared_infra_dir()
        .join("provider-bridge-gemini-settings")
        .join(format!("{source_hash:016x}.json"))
}
const MCP_COORDINATOR_SERVER_NAME: &str = "mcp-coordinator";
const MCP_PROVIDER_PROFILE_CLAUDE: &str = "claude";
const MCP_PROVIDER_PROFILE_CODEX: &str = "codex";
const MCP_PROVIDER_PROFILE_GEMINI: &str = "gemini";
fn provider_mcp_profile(provider: ModelProvider) -> Option<&'static str> {
    match provider {
        ModelProvider::Claude | ModelProvider::MiniMax | ModelProvider::MiniMaxForge => {
            Some(MCP_PROVIDER_PROFILE_CLAUDE)
        }
        ModelProvider::Codex | ModelProvider::Nvidia => Some(MCP_PROVIDER_PROFILE_CODEX),
        ModelProvider::Gemini
        | ModelProvider::OmniForge
        | ModelProvider::GeminiForge
        | ModelProvider::GeminiCliForge => {
            Some(MCP_PROVIDER_PROFILE_GEMINI)
        }
        _ => None,
    }
}
fn load_mcp_json_value(path: &Path) -> Option<serde_json::Value> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}
fn configured_mcp_servers<'a>(
    parsed: &'a serde_json::Value,
    provider_profile: Option<&str>,
) -> Option<&'a serde_json::Map<String, serde_json::Value>> {
    provider_profile
        .and_then(|profile| {
            parsed
                .get("providers")
                .and_then(|value| value.as_object())
                .and_then(|providers| providers.get(profile))
                .and_then(|value| value.as_object())
                .and_then(|provider| provider.get("mcpServers"))
                .and_then(|value| value.as_object())
        })
        .or_else(|| parsed.get("mcpServers").and_then(|value| value.as_object()))
}
fn resolve_binary_on_path(binary: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
}
fn implicit_fff_server_spec() -> Option<serde_json::Value> {
    let command = resolve_binary_on_path("fff-mcp")?;
    Some(serde_json::json!({
        "command": command.to_string_lossy().into_owned()
    }))
}
fn effective_configured_mcp_servers(
    parsed: &serde_json::Value,
    provider_profile: Option<&str>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut servers = configured_mcp_servers(parsed, provider_profile)
        .cloned()
        .unwrap_or_default();
    if !servers.contains_key("fff") {
        if let Some(spec) = implicit_fff_server_spec() {
            servers.insert("fff".to_string(), spec);
        }
    }
    servers
}
fn filtered_profile_mcp_servers(
    parsed: &serde_json::Value,
    provider_profile: Option<&str>,
    overrides: Option<&ProviderRequestOverrides>,
) -> serde_json::Map<String, serde_json::Value> {
    filtered_mcp_servers(
        &effective_configured_mcp_servers(parsed, provider_profile),
        overrides,
    )
}
fn mcp_coordinator_binary_path() -> PathBuf {
    let repo_root = Path::new(config::repo_root());
    let base = repo_root.parent().unwrap_or(repo_root);
    let binary_name = if cfg!(windows) {
        "mcp-coordinator.exe"
    } else {
        "mcp-coordinator"
    };
    base.join("mcp-coordinator")
        .join("target")
        .join("release")
        .join(binary_name)
}
fn coordinator_server_spec(
    upstream_config_path: &Path,
    provider_profile: &str,
) -> serde_json::Value {
    serde_json::json!({
        "command": mcp_coordinator_binary_path().to_string_lossy().into_owned(),
        "args": [],
        "env": {
            "MCP_COORDINATOR_PROVIDER": provider_profile,
            "MCP_COORDINATOR_SELF_NAME": MCP_COORDINATOR_SERVER_NAME,
            "MCP_COORDINATOR_UPSTREAM_CONFIG_PATH": upstream_config_path.to_string_lossy().into_owned()
        }
    })
}
fn ensure_json_object<'a>(
    object: &'a mut serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<&'a mut serde_json::Map<String, serde_json::Value>> {
    if !object.contains_key(key) {
        object.insert(
            key.to_string(),
            serde_json::Value::Object(serde_json::Map::new()),
        );
    }
    object.get_mut(key)?.as_object_mut()
}
fn provider_upstream_json(
    provider_profile: &str,
    servers: &serde_json::Map<String, serde_json::Value>,
) -> serde_json::Value {
    let mut provider = serde_json::Map::new();
    provider.insert(
        "mcpServers".to_string(),
        serde_json::Value::Object(servers.clone()),
    );
    let mut providers = serde_json::Map::new();
    providers.insert(
        provider_profile.to_string(),
        serde_json::Value::Object(provider),
    );
    let mut root = serde_json::Map::new();
    root.insert(
        "providers".to_string(),
        serde_json::Value::Object(providers),
    );
    serde_json::Value::Object(root)
}
fn runsc_mcp_restrictions_enabled(overrides: Option<&ProviderRequestOverrides>) -> bool {
    overrides
        .and_then(|overrides| overrides.runsc_image.as_ref())
        .is_some_and(|image| !image.trim().is_empty())
        || std::env::var("RUST_CHATBOT_BACKGROUND_EXECUTION_ENV")
            .ok()
            .as_deref()
            == Some("runsc")
        || std::env::var("RUST_CHATBOT_AUTONOMOUS_EXECUTION_ENV")
            .ok()
            .as_deref()
            == Some("runsc")
        || std::env::var("RUST_CHATBOT_AUTONOMOUS_INSIDE_RUNSC")
            .ok()
            .as_deref()
            == Some("1")
}
const HOST_FILESYSTEM_ROOT: &str = "/";
fn targets_trusted_workspace_root(arg: &str) -> bool {
    if arg == HOST_FILESYSTEM_ROOT {
        return true;
    }

    let path = std::path::Path::new(arg);
    crate::config::trusted_workspace_roots()
        .iter()
        .any(|root| path == root || path.starts_with(root))
}
fn mcp_server_targets_host_filesystem(entry: &serde_json::Value) -> bool {
    let Some(object) = entry.as_object() else {
        return false;
    };
    let command_mentions_filesystem = object
        .get("command")
        .and_then(|value| value.as_str())
        .is_some_and(|command| command.contains("server-filesystem"));
    let args = object
        .get("args")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let args_mention_filesystem = args.iter().any(|value| {
        value
            .as_str()
            .is_some_and(|arg| arg.contains("@modelcontextprotocol/server-filesystem"))
    });
    let targets_host_root = args
        .iter()
        .filter_map(|value| value.as_str())
        .any(targets_trusted_workspace_root);

    targets_host_root && (command_mentions_filesystem || args_mention_filesystem)
}
fn mcp_server_allowed_in_current_execution(
    name: &str,
    entry: &serde_json::Value,
    overrides: Option<&ProviderRequestOverrides>,
) -> bool {
    if !runsc_mcp_restrictions_enabled(overrides) {
        return true;
    }

    if matches!(name, "bash" | "mcp-server-commands") {
        return false;
    }

    !mcp_server_targets_host_filesystem(entry)
}
fn filtered_mcp_servers(
    servers: &serde_json::Map<String, serde_json::Value>,
    overrides: Option<&ProviderRequestOverrides>,
) -> serde_json::Map<String, serde_json::Value> {
    servers
        .iter()
        .filter(|(name, entry)| mcp_server_allowed_in_current_execution(name, entry, overrides))
        .map(|(name, entry)| (name.clone(), entry.clone()))
        .collect()
}
fn merge_mcp_json_with_coordinator(
    path: &Path,
    provider_profile: &str,
    upstream_servers: &serde_json::Map<String, serde_json::Value>,
    coordinator_config_path: &Path,
) -> Option<serde_json::Value> {
    let mut existing: serde_json::Value = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).ok()?,
        Err(_) => serde_json::Value::Object(serde_json::Map::new()),
    };

    let obj = existing.as_object_mut()?;
    let mcp_servers = ensure_json_object(obj, "mcpServers")?;
    for name in upstream_servers.keys() {
        mcp_servers.remove(name);
    }
    mcp_servers.remove(MCP_COORDINATOR_SERVER_NAME);
    if !upstream_servers.is_empty() {
        mcp_servers.insert(
            MCP_COORDINATOR_SERVER_NAME.to_string(),
            coordinator_server_spec(coordinator_config_path, provider_profile),
        );
    }

    let providers = ensure_json_object(obj, "providers")?;
    if upstream_servers.is_empty() {
        providers.remove(provider_profile);
    } else {
        let provider_config = ensure_json_object(providers, provider_profile)?;
        provider_config.insert(
            "mcpServers".to_string(),
            serde_json::Value::Object(upstream_servers.clone()),
        );
    }

    Some(existing)
}
fn merge_gemini_settings_for_runsc(
    source_path: &Path,
    upstream_servers: &serde_json::Map<String, serde_json::Value>,
    coordinator_config_path: &Path,
) -> Option<serde_json::Value> {
    let mut existing: serde_json::Value = match std::fs::read(source_path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
        Err(_) => serde_json::Value::Object(serde_json::Map::new()),
    };

    let obj = existing.as_object_mut()?;
    let mcp_servers = ensure_json_object(obj, "mcpServers")?;
    for name in upstream_servers.keys() {
        mcp_servers.remove(name);
    }
    mcp_servers.remove(MCP_COORDINATOR_SERVER_NAME);
    if !upstream_servers.is_empty() {
        mcp_servers.insert(
            MCP_COORDINATOR_SERVER_NAME.to_string(),
            coordinator_server_spec(coordinator_config_path, MCP_PROVIDER_PROFILE_GEMINI),
        );
    }
    let remove_providers = obj
        .get_mut("providers")
        .and_then(|value| value.as_object_mut())
        .map(|providers| {
            providers.remove(MCP_PROVIDER_PROFILE_GEMINI);
            providers.is_empty()
        })
        .unwrap_or(false);
    if remove_providers {
        obj.remove("providers");
    }
    Some(existing)
}
fn codex_local_upstream_name(name: &str) -> &str {
    match name {
        "task-broker" => "taskserver",
        _ => name,
    }
}
fn json_string_array(value: &toml::Value) -> Option<Vec<serde_json::Value>> {
    let values = value
        .as_array()?
        .iter()
        .map(|entry| entry.as_str().map(|item| serde_json::Value::String(item.to_string())))
        .collect::<Option<Vec<_>>>()?;
    Some(values)
}
fn json_string_map(value: &toml::Value) -> Option<serde_json::Map<String, serde_json::Value>> {
    let table = value.as_table()?;
    let mut map = serde_json::Map::new();
    for (key, entry) in table {
        let string = entry.as_str()?;
        map.insert(key.clone(), serde_json::Value::String(string.to_string()));
    }
    Some(map)
}
fn extract_codex_local_mcp_servers(
    toml_content: &str,
) -> serde_json::Map<String, serde_json::Value> {
    let Ok(parsed) = toml::from_str::<toml::Value>(toml_content) else {
        return serde_json::Map::new();
    };
    let Some(mcp_servers) = parsed.get("mcp_servers").and_then(|value| value.as_table()) else {
        return serde_json::Map::new();
    };

    let mut servers = serde_json::Map::new();
    for (name, value) in mcp_servers {
        if name == MCP_COORDINATOR_SERVER_NAME {
            continue;
        }
        let Some(table) = value.as_table() else {
            continue;
        };
        let Some(command) = table.get("command").and_then(|value| value.as_str()) else {
            continue;
        };

        let mut server = serde_json::Map::new();
        server.insert(
            "command".to_string(),
            serde_json::Value::String(command.to_string()),
        );
        if let Some(args) = table.get("args").and_then(json_string_array) {
            server.insert("args".to_string(), serde_json::Value::Array(args));
        }
        if let Some(env) = table.get("env").and_then(json_string_map) {
            server.insert("env".to_string(), serde_json::Value::Object(env));
        }
        if let Some(cwd) = table.get("cwd").and_then(|value| value.as_str()) {
            server.insert("cwd".to_string(), serde_json::Value::String(cwd.to_string()));
        }
        if let Some(timeout) = table.get("toolTimeoutSec").and_then(|value| value.as_integer()) {
            server.insert(
                "toolTimeoutSec".to_string(),
                serde_json::Value::Number(timeout.into()),
            );
        }
        if let Some(enabled_tools) = table.get("enabledTools").and_then(json_string_array) {
            server.insert(
                "enabledTools".to_string(),
                serde_json::Value::Array(enabled_tools),
            );
        }
        if let Some(disabled_tools) = table.get("disabledTools").and_then(json_string_array) {
            server.insert(
                "disabledTools".to_string(),
                serde_json::Value::Array(disabled_tools),
            );
        }

        servers.insert(
            codex_local_upstream_name(name).to_string(),
            serde_json::Value::Object(server),
        );
    }
    servers
}
fn parse_mcp_server_section_name(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("[mcp_servers.")?;
    let name = rest.strip_suffix(']')?;
    Some(name.trim().trim_matches('"').to_string())
}
fn strip_codex_mcp_sections(toml_content: &str) -> String {
    enum BlockKind {
        Root,
        Mcp,
        Other,
    }

    fn flush(output: &mut String, buffer: &str, kind: &BlockKind) {
        if matches!(kind, BlockKind::Mcp) {
            return;
        }
        output.push_str(buffer);
    }

    let mut output = String::new();
    let mut buffer = String::new();
    let mut kind = BlockKind::Root;

    for line in toml_content.split_inclusive('\n') {
        if line.trim_start().starts_with('[') {
            flush(&mut output, &buffer, &kind);
            buffer.clear();
            kind = parse_mcp_server_section_name(line)
                .map(|_| BlockKind::Mcp)
                .unwrap_or(BlockKind::Other);
        }
        buffer.push_str(line);
    }
    flush(&mut output, &buffer, &kind);

    output.trim_end().to_string()
}
fn coordinator_sidecar_path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("mcp-coordinator-upstream.json")
}
fn gemini_coordinator_sidecar_path(settings_path: &Path) -> PathBuf {
    let parent = settings_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = settings_path
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "settings".to_string());
    parent.join(format!("{stem}.mcp-coordinator-upstream.json"))
}
fn render_coordinator_codex_toml(sidecar_path: &Path) -> Result<String> {
    let mut servers = serde_json::Map::new();
    servers.insert(
        MCP_COORDINATOR_SERVER_NAME.to_string(),
        coordinator_server_spec(sidecar_path, MCP_PROVIDER_PROFILE_CODEX),
    );
    render_codex_mcp_toml(&servers)
}
fn render_codex_mcp_toml(servers: &serde_json::Map<String, serde_json::Value>) -> Result<String> {
    let mut toml = String::new();
    for (name, entry) in servers {
        toml.push_str(&format!("[mcp_servers.{}]\n", toml_basic_string(name)?));
        if let Some(cmd) = entry.get("command").and_then(|value| value.as_str()) {
            toml.push_str(&format!("command = {}\n", toml_basic_string(cmd)?));
        }
        if let Some(args) = entry.get("args").and_then(|value| value.as_array()) {
            let arg_list: Result<Vec<String>> = args
                .iter()
                .filter_map(|value| value.as_str())
                .map(toml_basic_string)
                .collect();
            toml.push_str(&format!("args = [{}]\n", arg_list?.join(", ")));
        }
        if let Some(env) = entry.get("env").and_then(|value| value.as_object()) {
            let env_entries: Result<Vec<String>> = env
                .iter()
                .filter_map(|(key, value)| value.as_str().map(|value| (key.as_str(), value)))
                .map(|(key, value)| {
                    Ok(format!(
                        "{} = {}",
                        toml_basic_string(key)?,
                        toml_basic_string(value)?
                    ))
                })
                .collect();
            toml.push_str(&format!("env = {{ {} }}\n", env_entries?.join(", ")));
        }
        toml.push('\n');
    }

    Ok(toml)
}
