use super::*;
use std::fs;
use std::path::{Path, PathBuf};

fn canonical_shared_root_dir_under_home(home: &Path) -> PathBuf {
    home.join(SHARED_INFRA_DIR)
}

fn legacy_shared_infra_dir_under_home(home: &Path) -> PathBuf {
    home.join(LEGACY_SHARED_INFRA_DIR)
}

pub fn canonical_shared_root_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    canonical_shared_root_dir_under_home(&home)
}

pub fn canonical_config_root_dir() -> PathBuf {
    canonical_shared_root_dir()
}

pub fn shared_infra_dir() -> std::path::PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let canonical = canonical_shared_root_dir_under_home(&home);
    let legacy = legacy_shared_infra_dir_under_home(&home);
    if canonical.exists() || !legacy.exists() {
        canonical
    } else {
        legacy
    }
}

fn dedupe_paths(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::new();
    for path in paths {
        if deduped.iter().all(|existing| existing != &path) {
            deduped.push(path);
        }
    }
    deduped
}

fn repo_fallback_roots() -> Vec<PathBuf> {
    let repo_root = PathBuf::from(repo_root());
    let mut roots = vec![repo_root.clone()];
    if let Some(parent) = repo_root.parent() {
        for sibling in ["rust-chatbot", "chatbot-worker", "auto-runs"] {
            let candidate = parent.join(sibling);
            if candidate != repo_root && candidate.exists() {
                roots.push(candidate);
            }
        }
    }
    dedupe_paths(roots)
}

pub fn legacy_repo_config_paths(relative: impl AsRef<Path>) -> Vec<PathBuf> {
    let relative = relative.as_ref();
    repo_fallback_roots()
        .into_iter()
        .map(|root| root.join(relative))
        .collect()
}

pub fn resolved_global_config_path(relative: impl AsRef<Path>) -> PathBuf {
    let relative = relative.as_ref();
    let canonical = canonical_shared_root_dir().join(relative);
    if canonical.exists() {
        return canonical;
    }

    legacy_repo_config_paths(relative)
        .into_iter()
        .find(|candidate| candidate.exists())
        .unwrap_or(canonical)
}

pub fn resolved_provider_config_dir(provider: crate::model::ModelProvider) -> Option<PathBuf> {
    let name = config_dir_name_for(provider);
    if name.is_empty() {
        None
    } else {
        Some(resolved_global_config_path(name))
    }
}

// ============================================================================
// Catalog-backed config directory helpers
// ============================================================================

/// Resolve the config directory for a provider by name (catalog-backed).
pub fn get_claude_config_dir() -> Option<String> {
    get_config_dir(crate::model::ModelProvider::Claude)
}

pub fn get_minimax_config_dir() -> Option<String> {
    get_config_dir(crate::model::ModelProvider::MiniMax)
}

pub fn get_qwen_config_dir() -> Option<String> {
    get_config_dir(crate::model::ModelProvider::Qwen)
}

pub fn get_nvidia_config_dir() -> Option<String> {
    get_config_dir(crate::model::ModelProvider::Nvidia)
}

pub fn get_openrouter_config_dir() -> Option<String> {
    get_config_dir(crate::model::ModelProvider::OpenRouter)
}

// ============================================================================
// Path helpers (not per-provider — canonical home root with repo fallback)
// ============================================================================

pub fn repo_root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

pub fn checkout_root() -> PathBuf {
    let repo_root = PathBuf::from(repo_root());
    repo_root
        .ancestors()
        .find(|ancestor| ancestor.file_name().and_then(|name| name.to_str()) == Some("git"))
        .map(Path::to_path_buf)
        .or_else(|| repo_root.parent().map(Path::to_path_buf))
        .unwrap_or(repo_root)
}

pub fn claude_json_path() -> Option<String> {
    if let Some(config_dir) = resolved_provider_config_dir(crate::model::ModelProvider::Claude) {
        let config_path = config_dir.join(".claude.json");
        if config_path.exists() {
            return Some(config_path.to_string_lossy().into_owned());
        }
    }
    let home = dirs::home_dir()?;
    let legacy_home = home.join(".claude.json");
    if legacy_home.exists() {
        return Some(legacy_home.to_string_lossy().into_owned());
    }
    Some(
        canonical_shared_root_dir()
            .join(config_dir_name_for(crate::model::ModelProvider::Claude))
            .join(".claude.json")
            .to_string_lossy()
            .into_owned(),
    )
}

pub fn codex_cli_path() -> String {
    if cfg!(test) {
        if let Some(path) = std::env::var_os(RUST_CHATBOT_CODEX_CLI_ENV).filter(|v| !v.is_empty()) {
            return path.to_string_lossy().into_owned();
        }
    }
    "codex".to_string()
}

pub fn nvidia_codex_home_path() -> String {
    if let Some(path) =
        std::env::var_os(RUST_CHATBOT_NVIDIA_CODEX_HOME_ENV).filter(|v| !v.is_empty())
    {
        return path.to_string_lossy().into_owned();
    }
    resolved_provider_config_dir(crate::model::ModelProvider::Nvidia)
        .unwrap_or_else(|| canonical_shared_root_dir().join(".nvidia"))
        .join("codex-home")
        .to_string_lossy()
        .into_owned()
}

pub fn gemini_project_settings_path() -> String {
    resolved_provider_config_dir(crate::model::ModelProvider::Gemini)
        .unwrap_or_else(|| canonical_shared_root_dir().join(".gemini"))
        .join("settings.json")
        .to_string_lossy()
        .into_owned()
}

pub fn codex_toml_path() -> String {
    resolved_provider_config_dir(crate::model::ModelProvider::Codex)
        .unwrap_or_else(|| canonical_shared_root_dir().join(".codex"))
        .join("config.toml")
        .to_string_lossy()
        .into_owned()
}

pub fn mcp_json_path() -> String {
    resolved_global_config_path(".mcp.json")
        .to_string_lossy()
        .into_owned()
}

pub fn qwen_api_key_path() -> String {
    resolved_provider_config_dir(crate::model::ModelProvider::Qwen)
        .unwrap_or_else(|| canonical_shared_root_dir().join(".qwen"))
        .join("api.key")
        .to_string_lossy()
        .into_owned()
}

pub fn qwen_json_path() -> String {
    resolved_provider_config_dir(crate::model::ModelProvider::Qwen)
        .unwrap_or_else(|| canonical_shared_root_dir().join(".qwen"))
        .join(".qwen.json")
        .to_string_lossy()
        .into_owned()
}

pub fn provider_api_key_path(provider: crate::model::ModelProvider) -> Option<String> {
    match provider {
        crate::model::ModelProvider::MiniMax
        | crate::model::ModelProvider::MiniMaxForge
        | crate::model::ModelProvider::GeminiForge
        | crate::model::ModelProvider::Nvidia
        | crate::model::ModelProvider::OpenRouter => resolved_provider_config_dir(provider)
            .map(|config_dir| config_dir.join("api.key").to_string_lossy().into_owned()),
        _ => None,
    }
}

pub fn minimax_api_key_path() -> String {
    provider_api_key_path(crate::model::ModelProvider::MiniMax)
        .expect("MiniMax should define a config dir")
}

pub fn minimax_json_path() -> String {
    resolved_provider_config_dir(crate::model::ModelProvider::MiniMax)
        .unwrap_or_else(|| canonical_shared_root_dir().join(".minimax"))
        .join(".minimax.json")
        .to_string_lossy()
        .into_owned()
}

pub fn nvidia_api_key_path() -> String {
    provider_api_key_path(crate::model::ModelProvider::Nvidia)
        .expect("Nvidia should define a config dir")
}

pub fn openrouter_api_key_path() -> String {
    provider_api_key_path(crate::model::ModelProvider::OpenRouter)
        .expect("OpenRouter should define a config dir")
}

pub fn legacy_openrouter_api_key_path() -> String {
    resolved_global_config_path(".gemma/api.key")
        .to_string_lossy()
        .into_owned()
}

// ============================================================================
// Catalog-backed per-provider metadata helpers
// ============================================================================

/// Data directory name under the shared root for session storage (catalog-backed).
pub fn data_dir_name(provider: crate::model::ModelProvider) -> &'static str {
    crate::provider_catalog::by_id(provider).data_dir_name
}

fn legacy_data_dir_names(provider: crate::model::ModelProvider) -> &'static [&'static str] {
    match provider {
        crate::model::ModelProvider::Nvidia => &[".nemotron-desktop"],
        _ => &[],
    }
}

fn dir_has_json_files(path: &Path) -> bool {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return false,
    };

    entries
        .flatten()
        .any(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
}

fn provider_data_dir_has_state(data_dir: &Path) -> bool {
    data_dir.join("sessions.json").exists()
        || dir_has_json_files(&data_dir.join("sessions"))
        || dir_has_json_files(&data_dir.join("height_cache"))
}

pub fn canonical_provider_data_dir_under_home(
    provider: crate::model::ModelProvider,
    home: &Path,
) -> PathBuf {
    canonical_shared_root_dir_under_home(home).join(data_dir_name(provider))
}

fn legacy_data_dir_paths_under_home(
    provider: crate::model::ModelProvider,
    home: &Path,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let shared_root = canonical_shared_root_dir_under_home(home);

    for legacy_dir_name in legacy_data_dir_names(provider) {
        paths.push(shared_root.join(legacy_dir_name));
    }
    paths.push(home.join(data_dir_name(provider)));
    for legacy_dir_name in legacy_data_dir_names(provider) {
        paths.push(home.join(legacy_dir_name));
    }

    dedupe_paths(paths)
}

pub fn resolve_data_dir_under_home(provider: crate::model::ModelProvider, home: &Path) -> PathBuf {
    let primary = canonical_provider_data_dir_under_home(provider, home);
    if provider_data_dir_has_state(&primary) {
        return primary;
    }

    for legacy in legacy_data_dir_paths_under_home(provider, home) {
        if provider_data_dir_has_state(&legacy) {
            return legacy;
        }
    }

    primary
}

pub fn resolve_data_dir(provider: crate::model::ModelProvider) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(resolve_data_dir_under_home(provider, &home))
}

/// Window / app title for a provider (catalog-backed).
pub fn app_title_for(provider: crate::model::ModelProvider) -> &'static str {
    crate::provider_catalog::by_id(provider).app_title
}

/// Desktop app-id for window matching (catalog-backed).
pub fn app_id_for(provider: crate::model::ModelProvider) -> &'static str {
    crate::provider_catalog::by_id(provider).app_id
}

pub fn window_title(provider: crate::model::ModelProvider, _instance: Option<u32>) -> String {
    let base = app_title_for(provider);
    let dir_name = std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_default();
    if dir_name.is_empty() {
        base.to_string()
    } else {
        format!("{} ~ /{}", base, dir_name)
    }
}

pub fn window_app_id(provider: crate::model::ModelProvider, instance: Option<u32>) -> String {
    match instance {
        Some(n) => format!("{:02}-{}", n, app_id_for(provider)),
        None => app_id_for(provider).to_string(),
    }
}

pub fn has_mcp_servers() -> bool {
    let claude_json_path = match claude_json_path() {
        Some(path) => path,
        None => return false,
    };
    let content = match std::fs::read_to_string(&claude_json_path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let top_level = json
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .map(|m| !m.is_empty())
        .unwrap_or(false);
    if top_level {
        return true;
    }
    json.get("projects")
        .and_then(|p| p.get("/"))
        .and_then(|root| root.get("mcpServers"))
        .and_then(|v| v.as_object())
        .map(|m| !m.is_empty())
        .unwrap_or(false)
}

#[cfg(test)]
mod paths_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn resolve_data_dir_uses_primary_when_it_has_state() {
        let home = TempDir::new().unwrap();
        let primary = canonical_provider_data_dir_under_home(
            crate::model::ModelProvider::Nvidia,
            home.path(),
        );
        let legacy = home.path().join(".nemotron-desktop");
        fs::create_dir_all(primary.join("height_cache")).unwrap();
        fs::create_dir_all(legacy.join("height_cache")).unwrap();
        fs::write(primary.join("height_cache").join("current.json"), "{}").unwrap();
        fs::write(legacy.join("height_cache").join("legacy.json"), "{}").unwrap();

        let resolved =
            resolve_data_dir_under_home(crate::model::ModelProvider::Nvidia, home.path());

        assert_eq!(resolved, primary);
    }

    #[test]
    fn resolve_data_dir_falls_back_to_legacy_when_primary_is_empty() {
        let home = TempDir::new().unwrap();
        let primary = canonical_provider_data_dir_under_home(
            crate::model::ModelProvider::Nvidia,
            home.path(),
        );
        let legacy = home.path().join(".nemotron-desktop");
        fs::create_dir_all(primary.join("height_cache")).unwrap();
        fs::create_dir_all(legacy.join("height_cache")).unwrap();
        fs::write(legacy.join("height_cache").join("legacy.json"), "{}").unwrap();

        let resolved =
            resolve_data_dir_under_home(crate::model::ModelProvider::Nvidia, home.path());

        assert_eq!(resolved, legacy);
    }

    #[test]
    fn resolve_data_dir_defaults_to_primary_when_no_state_exists() {
        let home = TempDir::new().unwrap();
        let primary = canonical_provider_data_dir_under_home(
            crate::model::ModelProvider::Nvidia,
            home.path(),
        );

        let resolved =
            resolve_data_dir_under_home(crate::model::ModelProvider::Nvidia, home.path());

        assert_eq!(resolved, primary);
    }

    #[test]
    fn resolve_data_dir_accepts_legacy_alias_under_shared_root() {
        let home = TempDir::new().unwrap();
        let canonical = canonical_provider_data_dir_under_home(
            crate::model::ModelProvider::Nvidia,
            home.path(),
        );
        let legacy = canonical_shared_root_dir_under_home(home.path()).join(".nemotron-desktop");
        fs::create_dir_all(canonical.join("height_cache")).unwrap();
        fs::create_dir_all(legacy.join("height_cache")).unwrap();
        fs::write(legacy.join("height_cache").join("legacy.json"), "{}").unwrap();

        let resolved =
            resolve_data_dir_under_home(crate::model::ModelProvider::Nvidia, home.path());

        assert_eq!(resolved, legacy);
    }

    #[test]
    fn resolved_global_config_path_prefers_canonical_home_root() {
        let home = TempDir::new().unwrap();
        let original_home = std::env::var_os("HOME");
        std::env::set_var("HOME", home.path());
        let canonical = canonical_shared_root_dir().join(".mcp.json");
        fs::create_dir_all(canonical.parent().unwrap()).unwrap();
        fs::write(&canonical, "{}\n").unwrap();

        let resolved = resolved_global_config_path(".mcp.json");

        match original_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }

        assert_eq!(resolved, canonical);
    }
}
