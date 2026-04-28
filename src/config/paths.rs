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
