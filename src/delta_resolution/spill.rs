//! Pending delta buckets and spill storage.
//!
//! Implements Task 07 §4.3 (Pending Delta Buckets) and §5 (Delta Record Format).

use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use super::{BaseKey, DeltaId, ObjectKind};
/// Error during spill operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpillError {
    /// Spill quota exhausted.
    QuotaExceeded { requested: u64, available: u64 },
    /// I/O error during spill.
    Io(String),
    /// Bucket not found.
    BucketNotFound(u64),
    /// Record too large.
    RecordTooLarge { max: usize, got: usize },
}
/// A single unresolved delta record spilled to external storage.
///
/// Per Task 07 §5, each record contains:
/// - delta_id
/// - source pack sequence
/// - base-key type and value
/// - result object kind
/// - expected result size
/// - delta instruction storage reference
/// - instruction byte length
/// - attempt count
/// - source depth hint if known
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaRecord {
    pub delta_id: DeltaId,
    pub source_pack_sequence: Option<u64>,
    pub base_key: BaseKey,
    pub result_kind: ObjectKind,
    pub expected_result_size: usize,
    /// Reference to where the delta instruction bytes are stored (e.g. file path + offset).
    pub instruction_ref: InstructionRef,
    pub instruction_byte_length: usize,
    /// Starts at 0 for newly spilled deltas; incremented each pass they are retained.
    pub attempt_count: u32,
    pub source_depth_hint: Option<u32>,
}
/// Reference to spilled delta instruction bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstructionRef {
    /// Instructions stored inline (for very small deltas).
    Inline(Vec<u8>),
    /// Instructions stored in a spill file.
    Spilled {
        path: PathBuf,
        offset: u64,
        len: usize,
    },
}
/// Pending delta buckets keyed by deterministic bucket hash.
///
/// Unresolved deltas are spilled into bucket files keyed by a deterministic
/// base key hash. Each bucket file contains append-only delta records.
pub struct PendingDeltaBuckets {
    /// Root directory for all bucket files.
    root: PathBuf,
    /// Per-bucket spill files (append-only).
    buckets: HashMap<u64, BucketHandle>,
    /// Total bytes spilled across all buckets and instruction stores.
    total_spill_bytes: u64,
    /// Maximum allowed spill bytes.
    spill_quota: u64,
    /// Maximum allowed record size.
    max_record_size: usize,
    /// Next generation directory for compacted buckets.
    next_gen_root: PathBuf,
}
struct BucketHandle {
    #[allow(dead_code)]
    path: PathBuf,
    writer: BufWriter<File>,
    record_count: u64,
}
