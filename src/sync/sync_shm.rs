use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use fs2::FileExt;
use memmap2::MmapMut;
use rust_debug::tracing::{debug, info, warn};
use zerocopy::byteorder::little_endian::U64 as ZcU64;
use zerocopy::{FromBytes, IntoBytes};
use crate::model::ModelProvider;
use crate::AgentRole;
use super::layout::*;
use super::util::is_pid_alive;
use super::util::provider_from_u32;
use super::util::provider_to_u32;
/// Handle to the shared memory region.
///
/// Each application instance creates one of these on startup and drops
/// it on exit.
pub struct SyncShm {
    pub(super) mmap: MmapMut,
    /// Our own PID so we know whether *we* are the writer.
    pub(super) our_pid: u32,
    /// Whether we registered in the instance registry (false for read-only).
    pub(super) registered: bool,
    /// Last sessions_version we observed.
    pub(super) last_seen_sessions_version: u64,
    /// Path or name used for cleanup.
    #[allow(dead_code)]
    pub(super) shm_path: PathBuf,
}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    /// Open the shared memory region in read-only mode (no registration).
    /// Returns `None` if the region doesn't exist or isn't initialized.
    pub fn open_readonly() -> Option<Self> {
        let (mmap, shm_path) = open_existing_shm().ok()?;

        let mut shm = Self {
            mmap,
            our_pid: std::process::id(),
            registered: false,
            last_seen_sessions_version: 0,
            shm_path,
        };

        // Check magic is valid.
        if shm.header().magic.get() != MAGIC {
            return None;
        }

        // Seed from the current value so sessions_changed() doesn't
        // spuriously return true on the very first call.
        shm.last_seen_sessions_version = shm.read_sessions_version();

        Some(shm)
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    pub fn our_pid(&self) -> u32 {
        self.our_pid
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    fn header(&self) -> &ShmHeader {
        ShmHeader::ref_from_prefix(self.mmap.as_ref()).unwrap().0
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    pub(super) fn header_mut(&mut self) -> &mut ShmHeader {
        ShmHeader::mut_from_prefix(self.mmap.as_mut()).unwrap().0
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    fn registry(&self) -> &InstanceRegistryRaw {
        let start = HEADER_SIZE;
        let end = start + REGISTRY_SIZE;
        InstanceRegistryRaw::ref_from_bytes(&self.mmap.as_ref()[start..end]).unwrap()
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    fn registry_mut(&mut self) -> &mut InstanceRegistryRaw {
        let start = HEADER_SIZE;
        let end = start + REGISTRY_SIZE;
        InstanceRegistryRaw::mut_from_bytes(&mut self.mmap.as_mut()[start..end]).unwrap()
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    fn worker_registry(&self) -> &WorkerRegistryRaw {
        let start = WORKER_REGISTRY_OFFSET;
        let end = start + WORKER_REGISTRY_SIZE;
        WorkerRegistryRaw::ref_from_bytes(&self.mmap.as_ref()[start..end]).unwrap()
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    /// Return the magic value from the header.
    pub fn magic(&self) -> u32 {
        self.header().magic.get()
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    /// Return the version from the header.
    pub fn version(&self) -> u32 {
        self.header().version.get()
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    /// Bump the sessions_version counter.  Call this after creating,
    /// deleting, or renaming a session so other instances reload.
    pub fn bump_sessions_version(&mut self) {
        let header = self.header_mut();
        let ptr = &header.sessions_version as *const ZcU64 as *const AtomicU64;
        let new_val = unsafe { &*ptr }.fetch_add(1, Ordering::AcqRel) + 1;
        debug!(sessions_version = new_val, "sync: sessions version bumped");
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    /// Read the current sessions_version from shared memory.
    fn read_sessions_version(&self) -> u64 {
        let header = self.header();
        if header.magic.get() != MAGIC {
            return 0;
        }
        let ptr = &header.sessions_version as *const ZcU64 as *const AtomicU64;
        unsafe { &*ptr }.load(Ordering::Acquire)
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    /// Check whether the sessions list has been modified by another instance.
    /// Returns `true` (and updates internal tracking) if so.
    pub fn sessions_changed(&mut self) -> bool {
        let current = self.read_sessions_version();
        if current != self.last_seen_sessions_version {
            self.last_seen_sessions_version = current;
            true
        } else {
            false
        }
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    /// List all registered instances (including dead ones — caller should
    /// check PIDs).
    pub fn list_instances(&self) -> Vec<InstanceEntry> {
        let reg = self.registry();
        let count = (reg.count.get() as usize).min(MAX_INSTANCES);

        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let e = &reg.entries[i];
            entries.push(InstanceEntry {
                pid: e.pid.get(),
                provider: provider_from_u32(e.provider.get()),
                started_at: e.started_at.get(),
                identity_token: e.identity_token.get(),
                role: role_from_u8(e.role_code),
            });
        }
        entries
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    /// Return only instances whose PID is still alive.
    pub fn live_instances(&self) -> Vec<InstanceEntry> {
        self.list_instances()
            .into_iter()
            .filter(|entry| process_identity_matches(entry.pid, entry.identity_token))
            .collect()
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    pub(super) fn register_instance(&mut self, provider: ModelProvider) {
        let Some(identity_token) = current_process_identity_token(self.our_pid) else {
            warn!(pid = self.our_pid, "failed to read process identity token");
            return;
        };
        let lock = match open_registry_lock_file() {
            Ok(file) => file,
            Err(err) => {
                warn!(pid = self.our_pid, error = %err, "failed to open sync registry lock");
                return;
            }
        };
        if let Err(err) = lock.lock_exclusive() {
            warn!(pid = self.our_pid, error = %err, "failed to lock sync registry");
            return;
        }
        let our_pid = self.our_pid;
        let reg = self.registry_mut();
        let count = (reg.count.get() as usize).min(MAX_INSTANCES);

        // Try to reuse a slot from a dead process first.
        let mut slot: Option<usize> = None;
        for i in 0..count {
            if !process_identity_matches(
                reg.entries[i].pid.get(),
                reg.entries[i].identity_token.get(),
            ) {
                slot = Some(i);
                break;
            }
        }

        let idx = match slot {
            Some(i) => i,
            None => {
                if count >= MAX_INSTANCES {
                    warn!("instance registry full, cannot register");
                    return;
                }
                let i = count;
                reg.count.set((count + 1) as u32);
                i
            }
        };

        let now = chrono::Utc::now().timestamp();
        let entry = &mut reg.entries[idx];
        entry.pid.set(our_pid);
        entry.provider.set(provider_to_u32(provider));
        entry.started_at.set(now);
        entry.identity_token.set(identity_token);
        entry.role_code = ROLE_CODE_NONE; // role is set separately via update_instance_role
        entry._pad = [0u8; 7];

        debug!(pid = our_pid, slot = idx, "sync: instance registered");
        let _ = lock.unlock();
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    pub(super) fn unregister_instance(&mut self) {
        let lock = match open_registry_lock_file() {
            Ok(file) => file,
            Err(err) => {
                warn!(pid = self.our_pid, error = %err, "failed to open sync registry lock");
                return;
            }
        };
        if let Err(err) = lock.lock_exclusive() {
            warn!(pid = self.our_pid, error = %err, "failed to lock sync registry");
            return;
        }
        let our_pid = self.our_pid;
        let reg = self.registry_mut();
        let count = (reg.count.get() as usize).min(MAX_INSTANCES);

        for i in 0..count {
            if reg.entries[i].pid.get() == our_pid
                && reg.entries[i].identity_token.get()
                    == current_process_identity_token(our_pid).unwrap_or(0)
            {
                // Zero the entry using zerocopy — write all zeros.
                let entry_bytes = reg.entries[i].as_mut_bytes();
                entry_bytes.fill(0);
                debug!(pid = our_pid, slot = i, "sync: instance unregistered");
                break;
            }
        }
        let _ = lock.unlock();
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    /// Update this instance's role in the shared-memory registry.
    /// This allows the instance to declare its agent profile (role) after initial registration.
    pub fn update_instance_role(&mut self, role: Option<AgentRole>) {
        let lock = match open_registry_lock_file() {
            Ok(file) => file,
            Err(err) => {
                warn!(pid = self.our_pid, error = %err, "failed to open sync registry lock");
                return;
            }
        };
        if let Err(err) = lock.lock_exclusive() {
            warn!(pid = self.our_pid, error = %err, "failed to lock sync registry");
            return;
        }
        let our_pid = self.our_pid;
        let reg = self.registry_mut();
        let count = (reg.count.get() as usize).min(MAX_INSTANCES);

        for i in 0..count {
            if reg.entries[i].pid.get() == our_pid
                && reg.entries[i].identity_token.get()
                    == current_process_identity_token(our_pid).unwrap_or(0)
            {
                reg.entries[i].role_code = role_to_u8(role);
                debug!(pid = our_pid, slot = i, "sync: instance role updated");
                break;
            }
        }
        let _ = lock.unlock();
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    /// Free a worker slot (zero slot + stream buffer).
    pub fn free_worker_slot(&mut self, slot: usize) {
        let buf = self.mmap.as_mut();
        let base = WORKER_REGISTRY_OFFSET + slot * WORKER_SLOT_SIZE;
        let data_base = worker_stream_offset(slot);
        buf[base..base + WORKER_SLOT_SIZE].fill(0);
        buf[data_base..data_base + WORKER_STREAM_CAPACITY].fill(0);
        debug!(slot, "worker slot freed");
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    fn migrate_to_current(&mut self) {
        let ver = self.header().version.get();
        if ver < VERSION {
            info!(from = ver, to = VERSION, "sync: migrating shm");
            let buf = self.mmap.as_mut();
            buf[WORKER_REGISTRY_OFFSET..SHM_SIZE].fill(0);
            buf[292..304].fill(0);
            self.header_mut().version.set(VERSION);
        }
    }}
impl SyncShm {
    // ── construction ─────────────────────────────────────────────────

    pub(super) fn ensure_initialized(&mut self) {
        if self.header().magic.get() != MAGIC {
            info!("sync: initializing shared memory region");
            // Zero entire region.
            self.mmap.as_mut()[..SHM_SIZE].fill(0);
            let header = self.header_mut();
            header.magic.set(MAGIC);
            header.version.set(VERSION);
        } else {
            // Existing region — check for version migration.
            self.migrate_to_current();
        }
    }}
fn registry_lock_path() -> PathBuf {
    crate::config::shared_infra_dir().join("sync-registry.lock")
}
fn open_registry_lock_file() -> anyhow::Result<std::fs::File> {
    let path = registry_lock_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(Into::into)
}
#[cfg(target_os = "linux")]
fn current_process_identity_token(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rparen = stat.rfind(')')?;
    let rest = stat.get(rparen + 2..)?;
    let field = rest.split_whitespace().nth(19)?;
    field.parse().ok()
}
#[cfg(not(target_os = "linux"))]
fn current_process_identity_token(pid: u32) -> Option<u64> {
    if is_pid_alive(pid) {
        Some(0)
    } else {
        None
    }
}
fn process_identity_matches(pid: u32, identity_token: u64) -> bool {
    if pid == 0 || !is_pid_alive(pid) {
        return false;
    }
    match current_process_identity_token(pid) {
        Some(token) => token == identity_token,
        None => false,
    }
}
impl Drop for SyncShm {
    fn drop(&mut self) {
        if self.registered {
            self.unregister_instance();
        }
        info!(pid = self.our_pid, "sync shm dropped");
    }
}
/// Return the platform-specific path to the shared memory file.
pub fn shm_path() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        PathBuf::from(format!("/dev/shm/{}", SHM_NAME))
    }
    #[cfg(not(target_os = "linux"))]
    {
        // macOS uses shm_open (logical path), fallback uses temp dir.
        std::env::temp_dir().join(SHM_NAME)
    }
}
#[cfg(target_os = "linux")]
fn open_or_create_shm() -> anyhow::Result<(MmapMut, PathBuf)> {
    let path = shm_path();
    let (mmap, path) = open_file_backed_shm(path, true, true)?;
    info!(path=%path.display(), "sync: opened linux shm");
    Ok((mmap, path))
}
#[cfg(target_os = "linux")]
fn open_existing_shm() -> anyhow::Result<(MmapMut, PathBuf)> {
    open_file_backed_shm(shm_path(), false, false)
}
#[cfg(target_os = "macos")]
fn open_or_create_shm() -> anyhow::Result<(MmapMut, PathBuf)> {
    use std::ffi::CString;
    use std::os::fd::FromRawFd;

    let name = CString::new(format!("/{}", SHM_NAME))?;

    // O_CREAT | O_RDWR, mode 0600
    let fd = unsafe { libc::shm_open(name.as_ptr(), libc::O_CREAT | libc::O_RDWR, 0o600) };
    if fd < 0 {
        return Err(anyhow::anyhow!(
            "shm_open failed: {}",
            std::io::Error::last_os_error()
        ));
    }

    let path = PathBuf::from(format!("/dev/shm/{}", SHM_NAME)); // logical only
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    prepare_shm_file(&file, &path, true)?;
    let mmap = map_shm_file(&file)?;
    info!("sync: opened macos shm via shm_open");
    Ok((mmap, path))
}
#[cfg(target_os = "macos")]
fn open_existing_shm() -> anyhow::Result<(MmapMut, PathBuf)> {
    use std::ffi::CString;
    use std::os::fd::FromRawFd;

    let name = CString::new(format!("/{}", SHM_NAME))?;

    let fd = unsafe { libc::shm_open(name.as_ptr(), libc::O_RDWR, 0o600) };
    if fd < 0 {
        return Err(anyhow::anyhow!(
            "shm_open failed: {}",
            std::io::Error::last_os_error()
        ));
    }

    let path = PathBuf::from(format!("/dev/shm/{}", SHM_NAME));
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    prepare_shm_file(&file, &path, false)?;
    let mmap = map_shm_file(&file)?;
    Ok((mmap, path))
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn open_or_create_shm() -> anyhow::Result<(MmapMut, PathBuf)> {
    // Fallback: use a regular file in the user's temp directory.
    let path = std::env::temp_dir().join(SHM_NAME);
    let (mmap, path) = open_file_backed_shm(path, true, true)?;
    warn!(path=%path.display(), "sync: using file-backed fallback shm");
    Ok((mmap, path))
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn open_existing_shm() -> anyhow::Result<(MmapMut, PathBuf)> {
    open_file_backed_shm(std::env::temp_dir().join(SHM_NAME), false, false)
}
pub(super) fn prepare_shm_file(
    file: &std::fs::File,
    path: &std::path::Path,
    allow_resize: bool,
) -> anyhow::Result<()> {
    let meta = file.metadata()?;
    if meta.len() >= SHM_SIZE as u64 {
        return Ok(());
    }

    if allow_resize {
        file.set_len(SHM_SIZE as u64)?;
        return Ok(());
    }

    Err(anyhow::anyhow!(
        "shared memory file too small: {}",
        path.display()
    ))
}
fn map_shm_file(file: &std::fs::File) -> anyhow::Result<MmapMut> {
    Ok(unsafe { MmapMut::map_mut(file)? })
}
pub(super) fn open_file_backed_shm(
    path: PathBuf,
    create: bool,
    allow_resize: bool,
) -> anyhow::Result<(MmapMut, PathBuf)> {
    use std::fs::OpenOptions;

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .open(&path)?;
    prepare_shm_file(&file, &path, allow_resize)?;
    let mmap = map_shm_file(&file)?;
    Ok((mmap, path))
}
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(super) struct RemoteStateEntry {
    pub(super) version: u64,
    pub(super) contents: String,
}
#[cfg_attr(not(test), allow(dead_code))]
type RemoteState = BTreeMap<String, RemoteStateEntry>;
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(super) struct SyncConflict {
    pub(super) key: String,
    pub(super) local: Option<String>,
    pub(super) remote: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(super) enum WorkspacePatch {
    Write { path: PathBuf, contents: String },
    Delete { path: PathBuf },
}
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn merge_remote_state(current: &RemoteState, incoming: &RemoteState) -> RemoteState {
    let mut merged = current.clone();
    for (key, incoming_entry) in incoming {
        let should_replace = merged
            .get(key)
            .map(|existing| incoming_entry.version >= existing.version)
            .unwrap_or(true);
        if should_replace {
            merged.insert(key.clone(), incoming_entry.clone());
        }
    }
    merged
}
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn detect_conflicts(
    base: &RemoteState,
    local: &RemoteState,
    remote: &RemoteState,
) -> Vec<SyncConflict> {
    let keys = base
        .keys()
        .chain(local.keys())
        .chain(remote.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    keys.into_iter()
        .filter_map(|key| {
            let base_contents = base.get(&key).map(|entry| entry.contents.as_str());
            let local_contents = local.get(&key).map(|entry| entry.contents.as_str());
            let remote_contents = remote.get(&key).map(|entry| entry.contents.as_str());

            if local_contents != remote_contents
                && local_contents != base_contents
                && remote_contents != base_contents
            {
                Some(SyncConflict {
                    key,
                    local: local_contents.map(str::to_owned),
                    remote: remote_contents.map(str::to_owned),
                })
            } else {
                None
            }
        })
        .collect()
}
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn apply_patch(workspace_root: &Path, patch: &[WorkspacePatch]) -> anyhow::Result<()> {
    for change in patch {
        apply_patch_change(workspace_root, change)?;
    }
    Ok(())
}
#[cfg_attr(not(test), allow(dead_code))]
fn apply_patch_change(workspace_root: &Path, change: &WorkspacePatch) -> anyhow::Result<()> {
    let path = workspace_root.join(change.relative_path());
    match change {
        WorkspacePatch::Write { contents, .. } => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, contents)?;
        }
        WorkspacePatch::Delete { .. } => {
            if path.exists() {
                std::fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}
#[cfg_attr(not(test), allow(dead_code))]
impl WorkspacePatch {
    fn relative_path(&self) -> &Path {
        match self {
            WorkspacePatch::Write { path, .. } | WorkspacePatch::Delete { path } => path.as_path(),
        }
    }
}
