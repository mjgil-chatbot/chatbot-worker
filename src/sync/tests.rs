use super::*;
use crate::model::ModelProvider;
use crate::AgentRole;
use std::collections::BTreeMap;
use std::path::PathBuf;
use tempfile::TempDir;
use zerocopy::FromBytes;
fn sync_env_lock() -> std::sync::MutexGuard<'static, ()> {
    crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}
struct HomeGuard {
    previous: Option<std::ffi::OsString>,
}
impl HomeGuard {
    fn set(path: &std::path::Path) -> Self {
        let previous = std::env::var_os("HOME");
        std::env::set_var("HOME", path);
        Self { previous }
    }}
impl Drop for HomeGuard {
    fn drop(&mut self) {
        match self.previous.as_ref() {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
    }
}
#[test]
fn broker_paths_live_under_shared_infra_dir() {
    let _lock = sync_env_lock();
    let temp_home = TempDir::new().unwrap();
    let _guard = HomeGuard::set(temp_home.path());

    let infra_dir = crate::config::shared_infra_dir();
    assert_eq!(
        broker_socket_path(),
        infra_dir.join(BROKER_SOCKET_FILE_NAME)
    );
    assert_eq!(broker_pid_path(), infra_dir.join(BROKER_PID_FILE_NAME));
}
#[test]
fn test_role_encoding_round_trip() {
    // None (unset) encodes to 0
    assert_eq!(role_to_u8(None), ROLE_CODE_NONE);
    // Orchestrator encodes to 1
    assert_eq!(
        role_to_u8(Some(AgentRole::Orchestrator)),
        ROLE_CODE_ORCHESTRATOR
    );
    // Worker encodes to 2
    assert_eq!(role_to_u8(Some(AgentRole::Worker)), ROLE_CODE_WORKER);
}
#[test]
fn test_role_decoding_round_trip() {
    // 0 decodes to None
    assert_eq!(role_from_u8(ROLE_CODE_NONE), None);
    // 1 decodes to Orchestrator
    assert_eq!(
        role_from_u8(ROLE_CODE_ORCHESTRATOR),
        Some(AgentRole::Orchestrator)
    );
    // 2 decodes to Worker
    assert_eq!(role_from_u8(ROLE_CODE_WORKER), Some(AgentRole::Worker));
    // Unknown codes decode to None (backward compatibility)
    assert_eq!(role_from_u8(99), None);
    assert_eq!(role_from_u8(255), None);
}
#[test]
fn test_instance_entry_raw_layout_preserved() {
    // Verify the struct size remains 32 bytes (backward compatibility)
    assert_eq!(std::mem::size_of::<InstanceEntryRaw>(), INSTANCE_ENTRY_SIZE);
    // Verify role_code is at the expected offset
    assert_eq!(std::mem::offset_of!(InstanceEntryRaw, role_code), 24);
}
#[test]
fn test_instance_registry_raw_layout_preserved() {
    // Verify registry size is unchanged
    assert_eq!(std::mem::size_of::<InstanceRegistryRaw>(), REGISTRY_SIZE);
}
#[test]
fn test_role_encoding_is_stable() {
    // Ensure encoding values are what we expect for shared-memory stability
    assert_eq!(ROLE_CODE_NONE, 0);
    assert_eq!(ROLE_CODE_ORCHESTRATOR, 1);
    assert_eq!(ROLE_CODE_WORKER, 2);
}
#[test]
fn reviewer_and_explorer_encode_as_orchestrator() {
    // Reviewer and Explorer map to ORCHESTRATOR code in shm
    assert_eq!(
        role_to_u8(Some(AgentRole::Reviewer)),
        ROLE_CODE_ORCHESTRATOR
    );
    assert_eq!(
        role_to_u8(Some(AgentRole::Explorer)),
        ROLE_CODE_ORCHESTRATOR
    );
}
#[test]
fn role_round_trip_lossy_for_reviewer_explorer() {
    // Reviewer/Explorer encode to ORCHESTRATOR, which decodes to Orchestrator (lossy)
    let code = role_to_u8(Some(AgentRole::Reviewer));
    assert_eq!(role_from_u8(code), Some(AgentRole::Orchestrator));
}
#[test]
fn provider_to_u32_round_trip_all_variants() {
    for provider in ModelProvider::ALL {
        let code = provider_to_u32(provider);
        let recovered = provider_from_u32(code);
        assert_eq!(recovered, provider, "round-trip failed for {:?}", provider);
    }
}
#[test]
fn provider_from_u32_unknown_defaults_to_claude() {
    assert_eq!(provider_from_u32(99), ModelProvider::Claude);
    assert_eq!(provider_from_u32(u32::MAX), ModelProvider::Claude);
}
