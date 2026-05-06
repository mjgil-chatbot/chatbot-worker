fn resolved_mcp_json_path(overrides: Option<&ProviderRequestOverrides>) -> PathBuf {
    path_override(
        overrides.and_then(|overrides| overrides.mcp_json_path.as_ref()),
        TEST_MCP_JSON_PATH_ENV,
        || Some(PathBuf::from(config::mcp_json_path())),
    )
    .unwrap_or_else(|| PathBuf::from(config::mcp_json_path()))
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
