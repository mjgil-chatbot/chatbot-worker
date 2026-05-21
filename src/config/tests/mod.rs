use super::*;
use std::io::Write as IoWrite;
use tempfile::TempDir;
mod catalog_compat;
mod loading;
/// Shared crate-wide HOME/env mutex. Re-exported under the historic local
/// name so existing call sites compile unchanged. A single mutex across all
/// test modules is required: any module-local mutex would race with HOME
/// mutations elsewhere (provider_bridge tests, hooks, session_runtime_tests).
use crate::TEST_ENV_LOCK as ENV_LOCK;
fn temp_dir() -> TempDir {
    TempDir::new().unwrap()
}
fn write_toml(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = dir.path().join(name);
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(content.as_bytes()).unwrap();
    path
}
