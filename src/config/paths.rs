use super::*;
use std::fs;
use std::path::{Path, PathBuf};
fn canonical_shared_root_dir_under_home(home: &Path) -> PathBuf {
    home.join(SHARED_INFRA_DIR)
}
fn legacy_shared_infra_dir_under_home(home: &Path) -> PathBuf {
    home.join(LEGACY_SHARED_INFRA_DIR)
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
