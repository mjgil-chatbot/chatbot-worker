//! Pass-based delta resolution with bounded memory.
//!
//! Implements Task 07 §6 (Parse-Time Ingest), §7 (Pass Algorithm), §9 (Base Lookup),
//! §10 (Thin-Pack Policy), §11 (Worst-Case Memory), §12 (Hostile-Input Limits),
//! §13 (Failure Modes), and §14 (Correctness Criteria).

use std::{collections::HashSet, path::PathBuf};

use super::{
    apply_delta, bucket_key_for, BaseKey, ContentLocation, DeltaApplicationError, DeltaId,
    DeltaRecord, InstructionRef, ObjectKind, OffsetIndex, OffsetIndexEntry, ParsedPackEntry,
    PendingDeltaBuckets, ResolvedBaseIndex, ResolvedObject, GB_UNSUPPORTED_THIN_PACK,
};

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

impl std::fmt::Display for DeltaResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeltaResolveError::UnsupportedThinPack => {
                write!(
                    f,
                    "{GB_UNSUPPORTED_THIN_PACK}: thin packs are not supported in v1"
                )
            }
            DeltaResolveError::MissingBase {
                delta_id,
                base_key,
                pass_number,
            } => {
                write!(
                    f,
                    "missing base for delta {} with key {:?} after pass {}",
                    delta_id.0, base_key, pass_number
                )
            }
            DeltaResolveError::CyclicDependency {
                delta_id,
                pass_number,
            } => {
                write!(
                    f,
                    "cyclic dependency detected for delta {} at pass {}",
                    delta_id.0, pass_number
                )
            }
            DeltaResolveError::DepthLimitExceeded {
                delta_id,
                depth,
                limit,
            } => {
                write!(
                    f,
                    "delta {} depth {} exceeds limit {}",
                    delta_id.0, depth, limit
                )
            }
            DeltaResolveError::MalformedDelta { delta_id, source } => {
                write!(f, "malformed delta {}: {}", delta_id.0, source)
            }
            DeltaResolveError::OutputSizeMismatch {
                delta_id,
                expected,
                got,
            } => {
                write!(
                    f,
                    "delta {} output size mismatch: expected {}, got {}",
                    delta_id.0, expected, got
                )
            }
            DeltaResolveError::SpillQuotaExhausted {
                requested,
                available,
            } => {
                write!(
                    f,
                    "spill quota exhausted: requested {}, available {}",
                    requested, available
                )
            }
            DeltaResolveError::IndexPersistence(msg) => {
                write!(f, "index persistence failed: {}", msg)
            }
            DeltaResolveError::MaxPassesExceeded { remaining_buckets } => {
                write!(
                    f,
                    "maximum passes exceeded with {} remaining buckets",
                    remaining_buckets
                )
            }
            DeltaResolveError::BucketFanInExceeded {
                pass,
                buckets,
                limit,
            } => {
                write!(
                    f,
                    "pass {} bucket fan-in exceeded: {} buckets > limit {}",
                    pass, buckets, limit
                )
            }
            DeltaResolveError::Other(msg) => write!(f, "delta resolve error: {}", msg),
        }
    }
}

impl std::error::Error for DeltaResolveError {}

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

/// Resolve deltas from parsed pack entries using external-memory passes.
///
/// This is the main entry point implementing the Task 07 algorithm:
/// 1. Parse-time ingest: full objects go straight to output; unresolved deltas spill.
/// 2. Pass-based resolution: scan buckets keyed by newly-resolved bases.
/// 3. Streaming delta application with bounded memory windows.
pub fn resolve_deltas(
    entries: Vec<ParsedPackEntry>,
    config: &DeltaResolveConfig,
    spill_root: PathBuf,
) -> Result<ResolutionOutcome, DeltaResolveError> {
    let mut resolved_base_index = ResolvedBaseIndex::new();
    let mut offset_index = OffsetIndex::new();
    let mut outcome = ResolutionOutcome::default();
    let mut next_delta_id: u64 = 0;

    // Ready frontier: bases newly resolved in the current pass.
    let mut current_frontier: Vec<BaseKey> = Vec::new();
    let mut next_frontier: Vec<BaseKey> = Vec::new();

    let mut pending_buckets = PendingDeltaBuckets::open(
        &spill_root,
        config.spill_quota,
        config.max_delta_record_size,
    )
    .map_err(|e| DeltaResolveError::Other(e.to_string()))?;

    // --- Phase 1: Parse-time ingest (Task 07 §6) ---
    for entry in entries {
        match entry {
            ParsedPackEntry::Full {
                object_kind,
                bytes,
                pack_offset,
            } => {
                let object = ResolvedObject {
                    kind: object_kind,
                    logical_size: bytes.len(),
                    bytes,
                    resolved_depth: 0,
                };
                // Compute a simple OID-like hash for indexing.
                let oid = simple_hash_object(&object);
                resolved_base_index.register_resolved(
                    oid,
                    &object,
                    ContentLocation::InMemory(object.bytes.clone()),
                );
                offset_index.insert(
                    pack_offset,
                    OffsetIndexEntry {
                        object_id: oid,
                        object_kind,
                        resolved_depth: 0,
                    },
                );
                current_frontier.push(BaseKey::ObjectId(oid));
                outcome.resolved_objects.push(object);
            }
            ParsedPackEntry::RefDelta {
                base_oid,
                delta_instructions,
                pack_offset,
                result_kind,
                result_size_hint,
            } => {
                if resolved_base_index.lookup(&base_oid).is_some() {
                    // Base already known — resolve immediately (§6.2).
                    let base_entry = resolved_base_index.lookup(&base_oid).unwrap();
                    let base_depth = base_entry.resolved_depth;
                    let new_depth = base_depth + 1;
                    if new_depth > config.max_delta_depth {
                        return Err(DeltaResolveError::DepthLimitExceeded {
                            delta_id: DeltaId(next_delta_id),
                            depth: new_depth,
                            limit: config.max_delta_depth,
                        });
                    }
                    let base_bytes = match &base_entry.content_location {
                        ContentLocation::InMemory(b) => b.clone(),
                        ContentLocation::Spilled { .. } => {
                            // For this implementation, we keep small objects in memory.
                            // Large-object streaming would read from the spill file here.
                            return Err(DeltaResolveError::Other(
                                "streaming from spilled base not yet implemented".into(),
                            ));
                        }
                    };
                    let resolved = apply_delta_in_memory(
                        &base_bytes,
                        &delta_instructions,
                        result_kind,
                        config,
                        DeltaId(next_delta_id),
                        base_depth,
                    )?;
                    let oid = simple_hash_object(&resolved);
                    resolved_base_index.register_resolved(
                        oid,
                        &resolved,
                        ContentLocation::InMemory(resolved.bytes.clone()),
                    );
                    offset_index.insert(
                        pack_offset,
                        OffsetIndexEntry {
                            object_id: oid,
                            object_kind: resolved.kind,
                            resolved_depth: resolved.resolved_depth,
                        },
                    );
                    next_frontier.push(BaseKey::ObjectId(oid));
                    outcome.resolved_objects.push(resolved);
                } else {
                    // Base unresolved — spill to pending bucket (§6.3).
                    let delta_id = DeltaId(next_delta_id);
                    let base_key = BaseKey::ObjectId(base_oid);
                    let record = DeltaRecord {
                        delta_id,
                        source_pack_sequence: Some(pack_offset),
                        base_key: base_key.clone(),
                        result_kind,
                        expected_result_size: result_size_hint,
                        instruction_ref: InstructionRef::Inline(delta_instructions),
                        instruction_byte_length: 0, // set below
                        attempt_count: 0,
                        source_depth_hint: None,
                    };
                    let bucket_key = bucket_key_for(&base_key);
                    pending_buckets
                        .spill(bucket_key, &record)
                        .map_err(|e| match e {
                            super::SpillError::QuotaExceeded {
                                requested,
                                available,
                            } => DeltaResolveError::SpillQuotaExhausted {
                                requested,
                                available,
                            },
                            other => DeltaResolveError::Other(other.to_string()),
                        })?;
                }
                next_delta_id += 1;
            }
            ParsedPackEntry::OfsDelta {
                base_offset,
                delta_instructions,
                pack_offset,
                result_kind,
                result_size_hint,
            } => {
                if let Some(base_entry) =
                    offset_index.resolve_ofs_delta(pack_offset, pack_offset - base_offset)
                {
                    // Base already known via offset index — resolve immediately.
                    let base_oid = base_entry.object_id;
                    let base_entry = resolved_base_index.lookup(&base_oid).unwrap();
                    let base_bytes = match &base_entry.content_location {
                        ContentLocation::InMemory(b) => b.clone(),
                        ContentLocation::Spilled { .. } => {
                            return Err(DeltaResolveError::Other(
                                "streaming from spilled base not yet implemented".into(),
                            ));
                        }
                    };
                    let resolved = apply_delta_in_memory(
                        &base_bytes,
                        &delta_instructions,
                        result_kind,
                        config,
                        DeltaId(next_delta_id),
                        base_entry.resolved_depth,
                    )?;
                    let oid = simple_hash_object(&resolved);
                    resolved_base_index.register_resolved(
                        oid,
                        &resolved,
                        ContentLocation::InMemory(resolved.bytes.clone()),
                    );
                    offset_index.insert(
                        pack_offset,
                        OffsetIndexEntry {
                            object_id: oid,
                            object_kind: resolved.kind,
                            resolved_depth: resolved.resolved_depth,
                        },
                    );
                    next_frontier.push(BaseKey::ObjectId(oid));
                    outcome.resolved_objects.push(resolved);
                } else {
                    // Base unresolved — spill to pending bucket.
                    let delta_id = DeltaId(next_delta_id);
                    let base_key = BaseKey::PackOffset(base_offset);
                    let record = DeltaRecord {
                        delta_id,
                        source_pack_sequence: Some(pack_offset),
                        base_key: base_key.clone(),
                        result_kind,
                        expected_result_size: result_size_hint,
                        instruction_ref: InstructionRef::Inline(delta_instructions),
                        instruction_byte_length: 0,
                        attempt_count: 0,
                        source_depth_hint: None,
                    };
                    let bucket_key = bucket_key_for(&base_key);
                    pending_buckets
                        .spill(bucket_key, &record)
                        .map_err(|e| match e {
                            super::SpillError::QuotaExceeded {
                                requested,
                                available,
                            } => DeltaResolveError::SpillQuotaExhausted {
                                requested,
                                available,
                            },
                            other => DeltaResolveError::Other(other.to_string()),
                        })?;
                }
                next_delta_id += 1;
            }
        }
    }

    // Move next_frontier into current for first pass.
    std::mem::swap(&mut current_frontier, &mut next_frontier);
    next_frontier.clear();

    // --- Phase 2: Pass-based resolution (Task 07 §7) ---
    let mut pass_number: u32 = 0;
    let mut resolving_set: HashSet<DeltaId> = HashSet::new();

    while !current_frontier.is_empty() || !pending_buckets.is_empty() {
        if pass_number >= config.max_passes {
            return Err(DeltaResolveError::MaxPassesExceeded {
                remaining_buckets: pending_buckets.bucket_count(),
            });
        }
        pass_number += 1;
        outcome.passes_completed = pass_number;
        outcome.peak_pending_buckets = outcome
            .peak_pending_buckets
            .max(pending_buckets.bucket_count());

        // Fan-in limit check (§12).
        let bucket_keys: Vec<u64> = pending_buckets.bucket_keys();
        if bucket_keys.len() > config.max_bucket_fan_in_per_pass {
            return Err(DeltaResolveError::BucketFanInExceeded {
                pass: pass_number,
                buckets: bucket_keys.len(),
                limit: config.max_bucket_fan_in_per_pass,
            });
        }

        for base_key in &current_frontier {
            let bucket_key = bucket_key_for(base_key);
            let records = match pending_buckets.read_bucket(bucket_key) {
                Ok(records) => records,
                Err(super::SpillError::BucketNotFound(_)) => continue,
                Err(e) => return Err(DeltaResolveError::Other(e.to_string())),
            };

            let mut remaining: Vec<DeltaRecord> = Vec::new();
            for mut record in records {
                // Cycle detection.
                if !resolving_set.insert(record.delta_id) {
                    return Err(DeltaResolveError::CyclicDependency {
                        delta_id: record.delta_id,
                        pass_number,
                    });
                }

                // Depth check.
                let base_depth = match base_key {
                    BaseKey::ObjectId(oid) => resolved_base_index
                        .lookup(oid)
                        .map(|e| e.resolved_depth)
                        .unwrap_or(0),
                    BaseKey::PackOffset(off) => offset_index
                        .lookup(*off)
                        .map(|e| e.resolved_depth)
                        .unwrap_or(0),
                };
                let new_depth = base_depth + 1;
                if new_depth > config.max_delta_depth {
                    return Err(DeltaResolveError::DepthLimitExceeded {
                        delta_id: record.delta_id,
                        depth: new_depth,
                        limit: config.max_delta_depth,
                    });
                }

                // Try to resolve.
                let base_bytes = match base_key {
                    BaseKey::ObjectId(oid) => {
                        let entry = resolved_base_index.lookup(oid).ok_or_else(|| {
                            DeltaResolveError::MissingBase {
                                delta_id: record.delta_id,
                                base_key: base_key.clone(),
                                pass_number,
                            }
                        })?;
                        match &entry.content_location {
                            ContentLocation::InMemory(b) => b.clone(),
                            ContentLocation::Spilled { .. } => {
                                return Err(DeltaResolveError::Other(
                                    "streaming from spilled base not yet implemented".into(),
                                ));
                            }
                        }
                    }
                    BaseKey::PackOffset(off) => {
                        let entry = offset_index.lookup(*off).ok_or_else(|| {
                            DeltaResolveError::MissingBase {
                                delta_id: record.delta_id,
                                base_key: base_key.clone(),
                                pass_number,
                            }
                        })?;
                        let base_entry =
                            resolved_base_index
                                .lookup(&entry.object_id)
                                .ok_or_else(|| DeltaResolveError::MissingBase {
                                    delta_id: record.delta_id,
                                    base_key: base_key.clone(),
                                    pass_number,
                                })?;
                        match &base_entry.content_location {
                            ContentLocation::InMemory(b) => b.clone(),
                            ContentLocation::Spilled { .. } => {
                                return Err(DeltaResolveError::Other(
                                    "streaming from spilled base not yet implemented".into(),
                                ));
                            }
                        }
                    }
                };

                let instructions = match &record.instruction_ref {
                    InstructionRef::Inline(bytes) => bytes.clone(),
                    InstructionRef::Spilled { .. } => {
                        return Err(DeltaResolveError::Other(
                            "streaming from spilled instructions not yet implemented".into(),
                        ));
                    }
                };

                let delta_id = record.delta_id;
                match apply_delta_in_memory(
                    &base_bytes,
                    &instructions,
                    record.result_kind,
                    config,
                    delta_id,
                    base_depth,
                ) {
                    Ok(resolved) => {
                        let oid = simple_hash_object(&resolved);
                        resolved_base_index.register_resolved(
                            oid,
                            &resolved,
                            ContentLocation::InMemory(resolved.bytes.clone()),
                        );
                        if let Some(seq) = record.source_pack_sequence {
                            offset_index.insert(
                                seq,
                                OffsetIndexEntry {
                                    object_id: oid,
                                    object_kind: resolved.kind,
                                    resolved_depth: resolved.resolved_depth,
                                },
                            );
                        }
                        next_frontier.push(BaseKey::ObjectId(oid));
                        outcome.resolved_objects.push(resolved);
                    }
                    Err(e) => {
                        // If delta application fails, retain for next pass if attempts allow.
                        record.attempt_count += 1;
                        if record.attempt_count >= config.max_passes {
                            return Err(e);
                        }
                        remaining.push(record);
                    }
                }

                resolving_set.remove(&delta_id);
            }

            // Compact or delete bucket (§7.4).
            if remaining.is_empty() {
                pending_buckets
                    .delete_bucket(bucket_key)
                    .map_err(|e| DeltaResolveError::Other(e.to_string()))?;
            } else {
                pending_buckets
                    .compact_bucket(bucket_key, &remaining)
                    .map_err(|e| DeltaResolveError::Other(e.to_string()))?;
            }
        }

        // Pass completion: swap frontiers.
        std::mem::swap(&mut current_frontier, &mut next_frontier);
        next_frontier.clear();
    }

    outcome.total_spill_bytes = pending_buckets.total_spill_bytes();
    Ok(outcome)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_full_entry(bytes: Vec<u8>, offset: u64) -> ParsedPackEntry {
        ParsedPackEntry::Full {
            object_kind: ObjectKind::Blob,
            bytes,
            pack_offset: offset,
        }
    }

    fn make_ref_delta(base_oid: [u8; 20], instructions: Vec<u8>, offset: u64) -> ParsedPackEntry {
        ParsedPackEntry::RefDelta {
            base_oid,
            delta_instructions: instructions,
            pack_offset: offset,
            result_kind: ObjectKind::Blob,
            result_size_hint: 0,
        }
    }

    #[test]
    fn resolve_single_full_object() {
        let entries = vec![make_full_entry(b"hello".to_vec(), 0)];
        let config = DeltaResolveConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let outcome = resolve_deltas(entries, &config, tmp.path().to_path_buf()).unwrap();
        assert_eq!(outcome.resolved_objects.len(), 1);
        assert_eq!(outcome.resolved_objects[0].bytes, b"hello");
        assert_eq!(outcome.passes_completed, 0);
    }

    #[test]
    fn resolve_ref_delta_with_known_base() {
        let base = b"hello world";
        let delta = &[0x0b, 0x05, 0x05, b'h', b'e', b'l', b'l', b'o'];
        let base_oid = simple_hash_object(&ResolvedObject {
            kind: ObjectKind::Blob,
            bytes: base.to_vec(),
            logical_size: base.len(),
            resolved_depth: 0,
        });
        let entries = vec![
            make_full_entry(base.to_vec(), 0),
            make_ref_delta(base_oid, delta.to_vec(), 100),
        ];
        let config = DeltaResolveConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let outcome = resolve_deltas(entries, &config, tmp.path().to_path_buf()).unwrap();
        assert_eq!(outcome.resolved_objects.len(), 2);
        assert_eq!(outcome.resolved_objects[1].bytes, b"hello");
    }

    #[test]
    fn depth_limit_enforced() {
        let base = b"x";
        let base_oid = simple_hash_object(&ResolvedObject {
            kind: ObjectKind::Blob,
            bytes: base.to_vec(),
            logical_size: 1,
            resolved_depth: 0,
        });
        // Chain of 3 deltas, but limit is 1.
        let delta = vec![0x01, 0x01, 0x01, b'y'];
        let entries = vec![
            make_full_entry(base.to_vec(), 0),
            make_ref_delta(base_oid, delta.clone(), 10),
        ];
        let mut config = DeltaResolveConfig::default();
        config.max_delta_depth = 1;
        let tmp = tempfile::tempdir().unwrap();
        let outcome = resolve_deltas(entries, &config, tmp.path().to_path_buf());
        // Should succeed because depth is 1 (base_depth=0 + 1).
        assert!(outcome.is_ok());

        // Now test with a deeper chain.
        let mut config2 = DeltaResolveConfig::default();
        config2.max_delta_depth = 0;
        let entries2 = vec![
            make_full_entry(base.to_vec(), 0),
            make_ref_delta(base_oid, delta, 10),
        ];
        let err = resolve_deltas(entries2, &config2, tmp.path().to_path_buf()).unwrap_err();
        assert!(
            matches!(err, DeltaResolveError::DepthLimitExceeded { .. }),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn max_passes_enforced() {
        let base = b"base";
        let unknown_oid = [0xff; 20];
        let delta = vec![0x04, 0x04, 0x01, b'x'];
        let entries = vec![
            make_full_entry(base.to_vec(), 0),
            make_ref_delta(unknown_oid, delta, 10),
        ];
        let mut config = DeltaResolveConfig::default();
        config.max_passes = 1;
        let tmp = tempfile::tempdir().unwrap();
        let err = resolve_deltas(entries, &config, tmp.path().to_path_buf()).unwrap_err();
        assert!(
            matches!(err, DeltaResolveError::MaxPassesExceeded { .. }),
            "unexpected err: {err:?}"
        );
    }
}
