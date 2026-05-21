use super::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
mod ingest;
mod manual_memory;
/// Guard that holds the crate-level env lock and overrides HOME to a temp
/// directory for the duration of the test. Keeps the TempDir alive so the
/// path stays valid until the guard is dropped.
pub(super) struct MemTestGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    _home_dir: tempfile::TempDir,
    _prev_home: Option<std::ffi::OsString>,
}
impl MemTestGuard {
    pub(super) fn new() -> Self {
        let lock = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let home_dir = tempfile::TempDir::new().unwrap();
        let prev = std::env::var_os("HOME");
        std::env::set_var("HOME", home_dir.path());
        Self {
            _lock: lock,
            _home_dir: home_dir,
            _prev_home: prev,
        }
    }}
pub(super) fn temp_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("chatbot-memory-test-{}-{}", name, nanos))
}
