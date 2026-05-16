use zerocopy::byteorder::little_endian::{I64, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::model::ModelProvider;
use crate::AgentRole;

// ── layout structs ──────────────────────────────────────────────────

/// Shared memory header (512 bytes).
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct ShmHeader {
    pub magic: U32,            // 0..4
    pub version: U32,          // 4..8
    _reserved1: [u8; 288],     // 8..296
    pub sessions_version: U64, // 296..304  (accessed atomically at runtime)
    _reserved2: [u8; 208],     // 304..512
}

/// A single instance entry in the registry (32 bytes).
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct InstanceEntryRaw {
    pub pid: U32,             // 0..4
    pub provider: U32,        // 4..8
    pub started_at: I64,      // 8..16
    pub identity_token: U64,  // 16..24
    pub role_code: u8,        // 24..25  (0=unset, 1=Orchestrator, 2=Worker)
    pub(super) _pad: [u8; 7], // 25..32
}

/// Instance registry section: count followed by entries (260 bytes).
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct InstanceRegistryRaw {
    pub count: U32,                                 // 0..4
    pub entries: [InstanceEntryRaw; MAX_INSTANCES], // 4..260
}

/// A single worker slot (128 bytes).
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct WorkerSlotRaw {
    pub worker_pid: U32,   // 0..4
    pub parent_pid: U32,   // 4..8
    pub provider: U32,     // 8..12
    pub state: U32,        // 12..16
    pub chat_id_len: U32,  // 16..20
    pub chat_id: [u8; 64], // 20..84
    _pad1: [u8; 4],        // 84..88  (alignment padding for i64)
    pub started_at: I64,   // 88..96
    pub updated_at: I64,   // 96..104
    pub output_len: U64,   // 104..112
    pub sequence: U64,     // 112..120
    _reserved: [u8; 8],    // 120..128
}

/// Worker registry: array of worker slots (1024 bytes).
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct WorkerRegistryRaw {
    pub slots: [WorkerSlotRaw; MAX_WORKERS],
}

// ── compile-time size assertions ────────────────────────────────────

const _: () = assert!(std::mem::size_of::<ShmHeader>() == HEADER_SIZE);
const _: () = assert!(std::mem::size_of::<InstanceEntryRaw>() == INSTANCE_ENTRY_SIZE);
const _: () = assert!(std::mem::size_of::<InstanceRegistryRaw>() == REGISTRY_SIZE);
const _: () = assert!(std::mem::size_of::<WorkerSlotRaw>() == WORKER_SLOT_SIZE);
const _: () = assert!(std::mem::size_of::<WorkerRegistryRaw>() == WORKER_REGISTRY_SIZE);

// sessions_version must be 8-byte aligned within ShmHeader so the
// U64→AtomicU64 cast in bump_sessions_version / read_sessions_version is safe.
const _: () = assert!(std::mem::offset_of!(ShmHeader, sessions_version) % 8 == 0);

// Pin the byte offsets used for atomic reads in read_worker_slots().
// If WorkerSlotRaw ever changes, these assertions will catch the mismatch.
const _: () = assert!(std::mem::offset_of!(WorkerSlotRaw, output_len) == 104);
const _: () = assert!(std::mem::offset_of!(WorkerSlotRaw, sequence) == 112);

// ── constants ────────────────────────────────────────────────────────

pub const SHM_NAME: &str = "claude-desktop-chat-sync";

pub const MAGIC: u32 = 0xC1A0_DE50; // "claude-s(ync)"
pub const VERSION: u32 = 6;

pub const HEADER_SIZE: usize = 512;
pub const CHAT_ID_MAX: usize = 256;

pub const MAX_INSTANCES: usize = 8;
pub const INSTANCE_ENTRY_SIZE: usize = 32; // pid(4) + provider(4) + started_at(8) + identity(8) + pad(8)
pub const REGISTRY_SIZE: usize = 4 + MAX_INSTANCES * INSTANCE_ENTRY_SIZE; // count + entries

// ── worker registry constants ────────────────────────────────────────
pub const MAX_WORKERS: usize = 8;
pub const WORKER_SLOT_SIZE: usize = 128;
pub const WORKER_CHAT_ID_MAX: usize = 64;
pub const WORKER_REGISTRY_SIZE: usize = MAX_WORKERS * WORKER_SLOT_SIZE; // 1024
const WORKER_REGISTRY_ALIGNMENT: usize = 8;
const WORKER_REGISTRY_PADDING: usize = (WORKER_REGISTRY_ALIGNMENT
    - ((HEADER_SIZE + REGISTRY_SIZE) % WORKER_REGISTRY_ALIGNMENT))
    % WORKER_REGISTRY_ALIGNMENT;
pub const WORKER_REGISTRY_OFFSET: usize = HEADER_SIZE + REGISTRY_SIZE + WORKER_REGISTRY_PADDING; // 776

pub const DATA_OFFSET: usize = WORKER_REGISTRY_OFFSET + WORKER_REGISTRY_SIZE; // 1800
/// Total shared memory size: 4 MiB (plenty for accumulated response text).
pub const SHM_SIZE: usize = 4 * 1024 * 1024;
pub const DATA_CAPACITY: usize = SHM_SIZE - DATA_OFFSET;
pub const WORKER_STREAM_CAPACITY: usize = DATA_CAPACITY / MAX_WORKERS;

#[inline]
pub(super) fn worker_stream_offset(slot: usize) -> usize {
    DATA_OFFSET + slot * WORKER_STREAM_CAPACITY
}

const _: () = assert!(WORKER_REGISTRY_OFFSET % 8 == 0);
const _: () =
    assert!((WORKER_REGISTRY_OFFSET + std::mem::offset_of!(WorkerSlotRaw, output_len)) % 8 == 0);
const _: () =
    assert!((WORKER_REGISTRY_OFFSET + std::mem::offset_of!(WorkerSlotRaw, sequence)) % 8 == 0);

// Keep legacy header field offsets for backward compat (chatbot_ctl etc.)
pub const OFF_MAGIC: usize = 0;
pub const OFF_VERSION: usize = 4;
pub const OFF_SESSIONS_VERSION: usize = 296;
pub const REG_OFF_COUNT: usize = 0;
pub const REG_OFF_ENTRIES: usize = 4;

// ── instance entry ──────────────────────────────────────────────────

/// Compact role codes stored in shared memory.
/// These must remain stable as they are persisted in the shared memory region.
pub const ROLE_CODE_NONE: u8 = 0;
pub const ROLE_CODE_ORCHESTRATOR: u8 = 1;
pub const ROLE_CODE_WORKER: u8 = 2;

/// Encode an AgentRole as a compact byte code for shared-memory storage.
pub fn role_to_u8(role: Option<AgentRole>) -> u8 {
    match role {
        None => ROLE_CODE_NONE,
        Some(AgentRole::Orchestrator | AgentRole::Reviewer | AgentRole::Explorer) => {
            ROLE_CODE_ORCHESTRATOR
        }
        Some(AgentRole::Worker | AgentRole::DeepWorker) => ROLE_CODE_WORKER,
    }
}

/// Decode a compact byte code back to an AgentRole.
/// Returns None for the unset/legacy code (0).
pub fn role_from_u8(code: u8) -> Option<AgentRole> {
    match code {
        ROLE_CODE_ORCHESTRATOR => Some(AgentRole::Orchestrator),
        ROLE_CODE_WORKER => Some(AgentRole::Worker),
        _ => None, // 0 or unknown codes mean no role set
    }
}

/// Assert that role_code is at offset 24 within InstanceEntryRaw.
/// This offset must be stable for shared-memory compatibility.
const _: () = assert!(std::mem::offset_of!(InstanceEntryRaw, role_code) == 24);

#[derive(Debug, Clone)]
pub struct InstanceEntry {
    pub pid: u32,
    pub provider: ModelProvider,
    pub started_at: i64,
    pub identity_token: u64,
    /// The agent role of this instance, if set.
    pub role: Option<AgentRole>,
}
