//! Pass-based delta resolution with bounded memory.
//!
//! Implements Task 07 §6 (Parse-Time Ingest), §7 (Pass Algorithm), §9 (Base Lookup),
//! §10 (Thin-Pack Policy), §11 (Worst-Case Memory), §12 (Hostile-Input Limits),
//! §13 (Failure Modes), and §14 (Correctness Criteria).

use std::{collections::HashSet, path::PathBuf};
use super::apply_delta;
use super::BaseKey;
use super::DeltaApplicationError;
use super::DeltaId;
use super::ObjectKind;
use super::ResolvedObject;
/// Configuration for delta resolution, defining all hostile-input limits.
#[derive(Clone, Debug)]
pub struct DeltaResolveConfig {
    /// Maximum delta chain depth before rejection.
    pub max_delta_depth: u32,
    /// Maximum size of an unresolved delta record (metadata + instructions).
    pub max_delta_record_size: usize,
    /// Maximum size of a delta instruction stream.
    pub max_instruction_stream_size: usize,
    /// Maximum reconstructed object size.
    pub max_reconstructed_object_size: usize,
    /// Maximum number of resolution passes.
    pub max_passes: u32,
    /// Maximum number of buckets to scan per pass (fan-in limit).
    pub max_bucket_fan_in_per_pass: usize,
    /// Maximum spill bytes for pending deltas.
    pub spill_quota: u64,
    /// Whether thin packs are accepted (v1: always false).
    pub accept_thin_packs: bool,
}
impl Default for DeltaResolveConfig {
    fn default() -> Self {
        Self {
            max_delta_depth: 50,
            max_delta_record_size: 64 * 1024,
            max_instruction_stream_size: 1024 * 1024,
            max_reconstructed_object_size: 100 * 1024 * 1024,
            max_passes: 100,
            max_bucket_fan_in_per_pass: 10_000,
            spill_quota: 1024 * 1024 * 1024,
            accept_thin_packs: false,
        }
    }
}
/// Error during delta resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeltaResolveError {
    /// Thin pack rejected (v1 policy).
    UnsupportedThinPack,
    /// Missing base after maximum allowed passes.
    MissingBase {
        delta_id: DeltaId,
        base_key: BaseKey,
        pass_number: u32,
    },
    /// Cyclic dependency detected.
    CyclicDependency { delta_id: DeltaId, pass_number: u32 },
    /// Delta chain depth limit exceeded.
    DepthLimitExceeded {
        delta_id: DeltaId,
        depth: u32,
        limit: u32,
    },
    /// Malformed delta instruction stream.
    MalformedDelta { delta_id: DeltaId, source: String },
    /// Output size mismatch after delta application.
    OutputSizeMismatch {
        delta_id: DeltaId,
        expected: usize,
        got: usize,
    },
    /// Spill quota exhausted.
    SpillQuotaExhausted { requested: u64, available: u64 },
    /// Unable to persist index state.
    IndexPersistence(String),
    /// Maximum passes exceeded.
    MaxPassesExceeded { remaining_buckets: usize },
    /// Bucket fan-in per pass exceeded.
    BucketFanInExceeded {
        pass: u32,
        buckets: usize,
        limit: usize,
    },
    /// Generic resolution error with context.
    Other(String),
}
/// Outcome of a delta resolution run.
#[derive(Clone, Debug, Default)]
pub struct ResolutionOutcome {
    /// All fully resolved objects.
    pub resolved_objects: Vec<ResolvedObject>,
    /// Total number of resolution passes performed.
    pub passes_completed: u32,
    /// Number of deltas that were rejected as invalid/unsupported.
    pub rejected_count: u32,
    /// Peak number of pending buckets.
    pub peak_pending_buckets: usize,
    /// Total spill bytes used.
    pub total_spill_bytes: u64,
}
/// Apply a delta in memory with all configured limits checked.
fn apply_delta_in_memory(
    base: &[u8],
    delta: &[u8],
    result_kind: ObjectKind,
    config: &DeltaResolveConfig,
    delta_id: DeltaId,
    base_depth: u32,
) -> Result<ResolvedObject, DeltaResolveError> {
    if delta.len() > config.max_instruction_stream_size {
        return Err(DeltaResolveError::MalformedDelta {
            delta_id,
            source: format!(
                "instruction stream size {} exceeds limit {}",
                delta.len(),
                config.max_instruction_stream_size
            ),
        });
    }

    let bytes = apply_delta(base, delta, Some(config.max_reconstructed_object_size)).map_err(
        |e| match e {
            DeltaApplicationError::BaseSizeMismatch { expected, got } => {
                DeltaResolveError::MalformedDelta {
                    delta_id,
                    source: format!("base size mismatch: expected {expected}, got {got}"),
                }
            }
            DeltaApplicationError::ResultSizeMismatch { expected, got } => {
                DeltaResolveError::OutputSizeMismatch {
                    delta_id,
                    expected,
                    got,
                }
            }
            DeltaApplicationError::InvalidOpcode(op) => DeltaResolveError::MalformedDelta {
                delta_id,
                source: format!("invalid opcode {op:#04x}"),
            },
            DeltaApplicationError::CopyExceedsBase {
                offset,
                size,
                base_len,
            } => DeltaResolveError::MalformedDelta {
                delta_id,
                source: format!(
                    "copy exceeds base: offset={offset} size={size} base_len={base_len}"
                ),
            },
            DeltaApplicationError::InsertExceedsPayload {
                cursor,
                len,
                payload_len,
            } => DeltaResolveError::MalformedDelta {
                delta_id,
                source: format!(
                    "insert exceeds payload: cursor={cursor} len={len} payload_len={payload_len}"
                ),
            },
            DeltaApplicationError::TruncatedHeader => DeltaResolveError::MalformedDelta {
                delta_id,
                source: "truncated delta header".into(),
            },
            DeltaApplicationError::OutputLimitExceeded { limit, requested } => {
                DeltaResolveError::MalformedDelta {
                    delta_id,
                    source: format!("output size {requested} exceeds limit {limit}"),
                }
            }
            DeltaApplicationError::Io(msg) => {
                DeltaResolveError::Other(format!("delta I/O error: {msg}"))
            }
        },
    )?;

    Ok(ResolvedObject {
        kind: result_kind,
        logical_size: bytes.len(),
        bytes,
        resolved_depth: base_depth + 1,
    })
}
/// Simple deterministic hash for resolved objects (not a real SHA-1, but stable for indexing).
fn simple_hash_object(object: &ResolvedObject) -> [u8; 20] {
    // FNV-1a 64-bit for determinism (DefaultHasher uses a random seed per process).
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in object.kind.as_str().as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    for byte in &object.bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let mut result = [0u8; 20];
    result[..8].copy_from_slice(&hash.to_le_bytes());
    // Mix in length as a second hash value.
    let mut len_hash: u64 = 0xcbf29ce484222325;
    let len_bytes = (object.bytes.len() as u64).to_le_bytes();
    for byte in &len_bytes {
        len_hash ^= *byte as u64;
        len_hash = len_hash.wrapping_mul(0x100000001b3);
    }
    result[8..16].copy_from_slice(&len_hash.to_le_bytes());
    result
}
