use super::layout::worker_stream_offset;
use super::sync_shm::{
    apply_patch, detect_conflicts, merge_remote_state, open_file_backed_shm, prepare_shm_file,
    RemoteStateEntry, SyncConflict, WorkspacePatch,
};
use super::*;
use crate::model::ModelProvider;
use crate::AgentRole;
use std::collections::BTreeMap;
use std::path::PathBuf;
use tempfile::TempDir;
use zerocopy::FromBytes;

// Alias the crate-level lock so all HOME-mutating tests coordinate.
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
    }
}

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

// ── Slice 3: sync pure-logic seam tests ─────────────────────────

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

#[test]
fn worker_stream_offset_is_contiguous() {
    // Each worker's stream buffer should be contiguous and non-overlapping
    for slot in 0..MAX_WORKERS {
        let start = worker_stream_offset(slot);
        assert_eq!(start, DATA_OFFSET + slot * WORKER_STREAM_CAPACITY);
    }
    // Last slot ends within SHM_SIZE
    let last_end = worker_stream_offset(MAX_WORKERS - 1) + WORKER_STREAM_CAPACITY;
    assert!(last_end <= SHM_SIZE, "worker streams exceed SHM_SIZE");
}

#[test]
fn shm_layout_sizes_are_consistent() {
    // REGISTRY_SIZE = 4 (count) + MAX_INSTANCES * INSTANCE_ENTRY_SIZE
    assert_eq!(REGISTRY_SIZE, 4 + MAX_INSTANCES * INSTANCE_ENTRY_SIZE);
    // WORKER_REGISTRY_SIZE = MAX_WORKERS * WORKER_SLOT_SIZE
    assert_eq!(WORKER_REGISTRY_SIZE, MAX_WORKERS * WORKER_SLOT_SIZE);
    // DATA_CAPACITY fills the rest of SHM_SIZE
    assert_eq!(DATA_CAPACITY, SHM_SIZE - DATA_OFFSET);
    // WORKER_STREAM_CAPACITY evenly divides DATA_CAPACITY
    assert_eq!(WORKER_STREAM_CAPACITY, DATA_CAPACITY / MAX_WORKERS);
}

#[test]
fn worker_registry_offset_is_aligned() {
    assert_eq!(WORKER_REGISTRY_OFFSET % 8, 0, "must be 8-byte aligned");
}

#[test]
fn read_write_u32_round_trip() {
    let mut buf = vec![0u8; 16];
    write_u32(&mut buf, 4, 0xDEAD_BEEF);
    assert_eq!(read_u32(&buf, 4), 0xDEAD_BEEF);
    // Surrounding bytes are unchanged
    assert_eq!(read_u32(&buf, 0), 0);
}

#[test]
fn read_write_i64_round_trip() {
    let mut buf = vec![0u8; 16];
    write_i64(&mut buf, 0, -42);
    assert_eq!(read_i64(&buf, 0), -42);
    write_i64(&mut buf, 8, i64::MAX);
    assert_eq!(read_i64(&buf, 8), i64::MAX);
}

#[test]
fn read_u64_plain_and_acquire_agree() {
    // Both variants should read the same value from a locally-written buffer
    let mut buf = vec![0u8; 16];
    let val: u64 = 0x0102_0304_0506_0708;
    buf[0..8].copy_from_slice(&val.to_le_bytes());
    assert_eq!(read_u64(&buf, 0), val);
    assert_eq!(read_u64_acquire(&buf, 0), val);
}

#[test]
fn is_pid_alive_zero_returns_false() {
    assert!(!is_pid_alive(0));
}

#[test]
fn merge_remote_state_prefers_newer_overlapping_keys() {
    let current = BTreeMap::from([
        (
            "alpha".to_string(),
            RemoteStateEntry {
                version: 1,
                contents: "old-alpha".to_string(),
            },
        ),
        (
            "shared".to_string(),
            RemoteStateEntry {
                version: 2,
                contents: "old-shared".to_string(),
            },
        ),
    ]);
    let incoming = BTreeMap::from([
        (
            "shared".to_string(),
            RemoteStateEntry {
                version: 3,
                contents: "new-shared".to_string(),
            },
        ),
        (
            "beta".to_string(),
            RemoteStateEntry {
                version: 1,
                contents: "new-beta".to_string(),
            },
        ),
    ]);

    let merged = merge_remote_state(&current, &incoming);

    assert_eq!(merged.len(), 3);
    assert_eq!(merged["alpha"].contents, "old-alpha");
    assert_eq!(merged["shared"].contents, "new-shared");
    assert_eq!(merged["shared"].version, 3);
    assert_eq!(merged["beta"].contents, "new-beta");
}

#[test]
fn detect_conflicts_reports_when_local_and_remote_diverge() {
    let base = BTreeMap::from([(
        "shared".to_string(),
        RemoteStateEntry {
            version: 1,
            contents: "base".to_string(),
        },
    )]);
    let local = BTreeMap::from([(
        "shared".to_string(),
        RemoteStateEntry {
            version: 2,
            contents: "local".to_string(),
        },
    )]);
    let remote = BTreeMap::from([(
        "shared".to_string(),
        RemoteStateEntry {
            version: 2,
            contents: "remote".to_string(),
        },
    )]);

    let conflicts = detect_conflicts(&base, &local, &remote);

    assert_eq!(
        conflicts,
        vec![SyncConflict {
            key: "shared".to_string(),
            local: Some("local".to_string()),
            remote: Some("remote".to_string()),
        }]
    );
}

#[test]
fn apply_patch_updates_tempdir_workspace() {
    let workspace = TempDir::new().unwrap();
    let stale_file = workspace.path().join("stale.txt");
    std::fs::write(&stale_file, "stale").unwrap();

    apply_patch(
        workspace.path(),
        &[
            WorkspacePatch::Write {
                path: PathBuf::from("nested/state.txt"),
                contents: "patched".to_string(),
            },
            WorkspacePatch::Delete {
                path: PathBuf::from("stale.txt"),
            },
        ],
    )
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(workspace.path().join("nested/state.txt")).unwrap(),
        "patched"
    );
    assert!(!stale_file.exists());
}

#[test]
fn sync_shm_registers_updates_role_and_unregisters_via_temp_backing_file() {
    let _lock = sync_env_lock();
    let home = TempDir::new().unwrap();
    let _home_guard = HomeGuard::set(home.path());
    let shm_dir = TempDir::new().unwrap();
    let shm_file = shm_dir.path().join("sync-shm.bin");
    let (mmap, path) = open_file_backed_shm(shm_file, true, true).unwrap();
    let mut shm = SyncShm {
        mmap,
        our_pid: std::process::id(),
        registered: false,
        last_seen_sessions_version: 0,
        shm_path: path,
    };

    shm.ensure_initialized();
    assert_eq!(shm.magic(), MAGIC);
    assert_eq!(shm.version(), VERSION);
    assert!(!shm.sessions_changed());

    shm.register_instance(ModelProvider::Codex);
    let entries = shm.list_instances();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].pid, std::process::id());
    assert_eq!(entries[0].provider, ModelProvider::Codex);
    assert_eq!(entries[0].role, None);
    assert_eq!(shm.live_instances().len(), 1);

    shm.update_instance_role(Some(AgentRole::DeepWorker));
    let updated = shm.list_instances();
    assert_eq!(updated[0].role, Some(AgentRole::Worker));

    shm.bump_sessions_version();
    assert!(shm.sessions_changed());
    assert!(!shm.sessions_changed());

    shm.unregister_instance();
    assert!(shm.live_instances().is_empty());
}

#[test]
fn sync_shm_reads_and_frees_worker_slots_from_temp_backing_file() {
    let _lock = sync_env_lock();
    let home = TempDir::new().unwrap();
    let _home_guard = HomeGuard::set(home.path());
    let shm_dir = TempDir::new().unwrap();
    let shm_file = shm_dir.path().join("worker-shm.bin");
    let (mmap, path) = open_file_backed_shm(shm_file, true, true).unwrap();
    let mut shm = SyncShm {
        mmap,
        our_pid: std::process::id(),
        registered: false,
        last_seen_sessions_version: 0,
        shm_path: path,
    };
    shm.ensure_initialized();

    {
        let start = WORKER_REGISTRY_OFFSET;
        let end = start + WORKER_REGISTRY_SIZE;
        let wreg = WorkerRegistryRaw::mut_from_bytes(&mut shm.mmap.as_mut()[start..end]).unwrap();
        let slot = &mut wreg.slots[0];
        slot.worker_pid.set(3001);
        slot.parent_pid.set(2001);
        slot.provider.set(provider_to_u32(ModelProvider::Gemini));
        slot.state.set(1);
        slot.chat_id_len.set(6);
        slot.chat_id[..6].copy_from_slice(b"chat-1");
        slot.started_at.set(11);
        slot.updated_at.set(22);
        slot.output_len.set(44);
        slot.sequence.set(55);
    }
    let stream_offset = worker_stream_offset(0);
    shm.mmap.as_mut()[stream_offset..stream_offset + 5].copy_from_slice(b"hello");

    let slots = shm.read_worker_slots();
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].worker_pid, 3001);
    assert_eq!(slots[0].parent_pid, 2001);
    assert_eq!(slots[0].chat_id, "chat-1");
    assert_eq!(slots[0].output_len, 44);
    assert_eq!(slots[0].sequence, 55);

    shm.free_worker_slot(0);
    assert!(shm.read_worker_slots().is_empty());
    assert!(shm.mmap.as_ref()[stream_offset..stream_offset + 5]
        .iter()
        .all(|byte| *byte == 0));
}

#[test]
fn ensure_initialized_migrates_older_temp_backed_regions() {
    let _lock = sync_env_lock();
    let home = TempDir::new().unwrap();
    let _home_guard = HomeGuard::set(home.path());
    let shm_dir = TempDir::new().unwrap();
    let shm_file = shm_dir.path().join("migrate-shm.bin");
    let (mmap, path) = open_file_backed_shm(shm_file, true, true).unwrap();
    let mut shm = SyncShm {
        mmap,
        our_pid: std::process::id(),
        registered: false,
        last_seen_sessions_version: 0,
        shm_path: path,
    };
    shm.ensure_initialized();
    shm.header_mut().version.set(VERSION - 1);
    shm.mmap.as_mut()[WORKER_REGISTRY_OFFSET..WORKER_REGISTRY_OFFSET + 16].fill(0xAB);
    shm.mmap.as_mut()[292..304].fill(0xCD);

    shm.ensure_initialized();

    assert_eq!(shm.version(), VERSION);
    assert!(
        shm.mmap.as_ref()[WORKER_REGISTRY_OFFSET..WORKER_REGISTRY_OFFSET + 16]
            .iter()
            .all(|byte| *byte == 0)
    );
    assert!(shm.mmap.as_ref()[292..304].iter().all(|byte| *byte == 0));
}

#[test]
fn prepare_shm_file_resizes_when_allowed_and_rejects_small_existing_files() {
    let dir = TempDir::new().unwrap();
    let grow_path = dir.path().join("grow.bin");
    let grow_file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&grow_path)
        .unwrap();

    prepare_shm_file(&grow_file, &grow_path, true).unwrap();
    assert_eq!(grow_file.metadata().unwrap().len(), SHM_SIZE as u64);

    let small_path = dir.path().join("small.bin");
    std::fs::write(&small_path, [0u8; 8]).unwrap();
    let small_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&small_path)
        .unwrap();

    let err = prepare_shm_file(&small_file, &small_path, false)
        .expect_err("small file should be rejected when resize is disabled");
    assert!(err.to_string().contains("shared memory file too small"));
}
