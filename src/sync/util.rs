use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use rust_debug::tracing::warn;

use crate::model::ModelProvider;
use crate::AgentRole;

use super::sync_shm::SyncShm;

pub const BROKER_SOCKET_FILE_NAME: &str = "sync.sock";
pub const BROKER_PID_FILE_NAME: &str = "broker.pid";

// ── little-endian byte helpers (kept for atomic reads) ──────────────

#[inline]
pub fn read_u32(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(buf[off..off + 4].try_into().unwrap())
}

#[inline]
pub fn write_u32(buf: &mut [u8], off: usize, val: u32) {
    buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
}

#[inline]
pub fn read_i64(buf: &[u8], off: usize) -> i64 {
    i64::from_le_bytes(buf[off..off + 8].try_into().unwrap())
}

#[inline]
pub fn write_i64(buf: &mut [u8], off: usize, val: i64) {
    buf[off..off + 8].copy_from_slice(&val.to_le_bytes());
}

/// Read a u64 using plain byte-level LE decoding (non-atomic, for diagnostics).
#[inline]
pub fn read_u64(buf: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(buf[off..off + 8].try_into().unwrap())
}

/// Read a u64 with Acquire ordering to pair with the writer's Release.
#[inline]
pub fn read_u64_acquire(buf: &[u8], off: usize) -> u64 {
    let ptr = buf[off..].as_ptr() as *const AtomicU64;
    unsafe { &*ptr }.load(Ordering::Acquire)
}

// ── provider ↔ u32 helpers ──────────────────────────────────────────

pub fn provider_to_u32(p: ModelProvider) -> u32 {
    p.shm_code()
}

pub fn provider_from_u32(v: u32) -> ModelProvider {
    match ModelProvider::from_shm_code(v) {
        Some(p) => p,
        None => {
            warn!(
                code = v,
                "unknown provider code in shared memory, defaulting to Claude"
            );
            ModelProvider::Claude
        }
    }
}

// ── process liveness check ──────────────────────────────────────────

pub fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // kill(pid, 0) checks if the process exists without sending a signal.
    // Checked cast guards against sign-wrap on hypothetically large PIDs.
    let Ok(signed_pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    unsafe { libc::kill(signed_pid, 0) == 0 }
}

pub fn broker_socket_path() -> PathBuf {
    crate::config::shared_infra_dir().join(BROKER_SOCKET_FILE_NAME)
}

pub fn broker_pid_path() -> PathBuf {
    crate::config::shared_infra_dir().join(BROKER_PID_FILE_NAME)
}

/// Look up an instance's role by PID from the shared-memory registry.
/// Returns the role if the instance is found and has a role set, or None.
/// This is a convenience helper that opens shared memory in read-only mode.
pub fn get_instance_role_by_pid(pid: u32) -> Option<AgentRole> {
    let Some(shm) = SyncShm::open_readonly() else {
        return None;
    };
    shm.list_instances()
        .into_iter()
        .find(|e| e.pid == pid)
        .and_then(|e| e.role)
}
