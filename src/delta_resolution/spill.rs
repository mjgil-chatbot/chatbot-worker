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

impl std::fmt::Display for SpillError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpillError::QuotaExceeded {
                requested,
                available,
            } => {
                write!(
                    f,
                    "spill quota exceeded: requested {requested}, available {available}"
                )
            }
            SpillError::Io(msg) => write!(f, "spill I/O error: {msg}"),
            SpillError::BucketNotFound(key) => write!(f, "spill bucket not found: {key}"),
            SpillError::RecordTooLarge { max, got } => {
                write!(f, "spill record too large: max {max}, got {got}")
            }
        }
    }
}

impl std::error::Error for SpillError {}

impl From<std::io::Error> for SpillError {
    fn from(err: std::io::Error) -> Self {
        SpillError::Io(err.to_string())
    }
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

impl PendingDeltaBuckets {
    /// Open or create pending delta buckets under `root`.
    pub fn open(root: impl AsRef<Path>, spill_quota: u64, max_record_size: usize) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        let next_gen_root = root.join("next-buckets");
        fs::create_dir_all(&next_gen_root)?;
        Ok(Self {
            root,
            buckets: HashMap::new(),
            total_spill_bytes: 0,
            spill_quota,
            max_record_size,
            next_gen_root,
        })
    }

    /// Write a delta record into the appropriate pending bucket.
    pub fn spill(&mut self, bucket_key: u64, record: &DeltaRecord) -> Result<(), SpillError> {
        let record_bytes = bincode::serialize(record)
            .map_err(|e| SpillError::Io(format!("serialize failed: {e}")))?;
        if record_bytes.len() > self.max_record_size {
            return Err(SpillError::RecordTooLarge {
                max: self.max_record_size,
                got: record_bytes.len(),
            });
        }
        let needed = self.total_spill_bytes + record_bytes.len() as u64;
        if needed > self.spill_quota {
            return Err(SpillError::QuotaExceeded {
                requested: needed,
                available: self.spill_quota.saturating_sub(self.total_spill_bytes),
            });
        }

        let handle = self.buckets.entry(bucket_key).or_insert_with(|| {
            let path = self.root.join(format!("bucket-{bucket_key:016x}.delt"));
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .expect("bucket file creation");
            BucketHandle {
                path: path.clone(),
                writer: BufWriter::new(file),
                record_count: 0,
            }
        });

        // Write length-prefixed record.
        let len = record_bytes.len() as u64;
        handle
            .writer
            .write_all(&len.to_le_bytes())
            .map_err(SpillError::from)?;
        handle
            .writer
            .write_all(&record_bytes)
            .map_err(SpillError::from)?;
        handle.record_count += 1;
        self.total_spill_bytes = needed;
        Ok(())
    }

    /// Read all records from a bucket.
    pub fn read_bucket(&self, bucket_key: u64) -> Result<Vec<DeltaRecord>, SpillError> {
        let path = self.root.join(format!("bucket-{bucket_key:016x}.delt"));
        if !path.exists() {
            return Err(SpillError::BucketNotFound(bucket_key));
        }
        let file = File::open(&path).map_err(SpillError::from)?;
        let mut reader = BufReader::new(file);
        let mut records = Vec::new();
        loop {
            let mut len_buf = [0u8; 8];
            match reader.read_exact(&mut len_buf) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(SpillError::from(e)),
            }
            let len = u64::from_le_bytes(len_buf) as usize;
            let mut buf = vec![0u8; len];
            reader.read_exact(&mut buf).map_err(SpillError::from)?;
            let record: DeltaRecord = bincode::deserialize(&buf)
                .map_err(|e| SpillError::Io(format!("deserialize failed: {e}")))?;
            records.push(record);
        }
        Ok(records)
    }

    /// Remove a bucket file after all its records have been resolved or moved.
    pub fn delete_bucket(&mut self, bucket_key: u64) -> Result<(), SpillError> {
        let path = self.root.join(format!("bucket-{bucket_key:016x}.delt"));
        if path.exists() {
            fs::remove_file(&path).map_err(SpillError::from)?;
        }
        if let Some(handle) = self.buckets.remove(&bucket_key) {
            // Flush writer before removal.
            drop(handle.writer);
        }
        Ok(())
    }

    /// Move remaining records from a bucket into the next-generation bucket.
    ///
    /// Used at pass completion when some records in a bucket are still unresolved.
    pub fn compact_bucket(
        &mut self,
        bucket_key: u64,
        remaining: &[DeltaRecord],
    ) -> Result<(), SpillError> {
        // Remove old bucket.
        self.delete_bucket(bucket_key)?;
        // Write next-gen bucket.
        let next_path = self
            .next_gen_root
            .join(format!("bucket-{bucket_key:016x}.delt"));
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&next_path)
            .map_err(SpillError::from)?;
        let mut writer = BufWriter::new(&mut file);
        for record in remaining {
            let record_bytes = bincode::serialize(record)
                .map_err(|e| SpillError::Io(format!("serialize failed: {e}")))?;
            let len = record_bytes.len() as u64;
            writer
                .write_all(&len.to_le_bytes())
                .map_err(SpillError::from)?;
            writer.write_all(&record_bytes).map_err(SpillError::from)?;
        }
        writer.flush().map_err(SpillError::from)?;
        drop(writer);
        // Atomically move next-gen to current.
        let current_path = self.root.join(format!("bucket-{bucket_key:016x}.delt"));
        fs::rename(&next_path, &current_path).map_err(SpillError::from)?;
        Ok(())
    }

    /// List all bucket keys currently present.
    pub fn bucket_keys(&self) -> Vec<u64> {
        self.buckets.keys().copied().collect()
    }

    /// Total bytes spilled so far.
    pub fn total_spill_bytes(&self) -> u64 {
        self.total_spill_bytes
    }

    /// Check if any pending buckets remain.
    pub fn is_empty(&self) -> bool {
        self.buckets.is_empty()
    }

    /// Number of active buckets.
    pub fn bucket_count(&self) -> usize {
        self.buckets.len()
    }

    /// Flush all open bucket writers.
    pub fn flush_all(&mut self) -> Result<(), SpillError> {
        for handle in self.buckets.values_mut() {
            handle.writer.flush().map_err(SpillError::from)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_record(id: u64) -> DeltaRecord {
        DeltaRecord {
            delta_id: DeltaId(id),
            source_pack_sequence: Some(id),
            base_key: BaseKey::ObjectId([0xab; 20]),
            result_kind: ObjectKind::Blob,
            expected_result_size: 1024,
            instruction_ref: InstructionRef::Inline(vec![1, 2, 3]),
            instruction_byte_length: 3,
            attempt_count: 0,
            source_depth_hint: Some(1),
        }
    }

    #[test]
    fn spill_and_read_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let mut buckets = PendingDeltaBuckets::open(tmp.path(), 1024 * 1024, 64 * 1024).unwrap();
        let record = sample_record(1);
        let key = 0x1234;
        buckets.spill(key, &record).unwrap();
        buckets.flush_all().unwrap();

        let read = buckets.read_bucket(key).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0], record);
    }

    #[test]
    fn delete_bucket_removes_records() {
        let tmp = TempDir::new().unwrap();
        let mut buckets = PendingDeltaBuckets::open(tmp.path(), 1024 * 1024, 64 * 1024).unwrap();
        let key = 0x5678;
        buckets.spill(key, &sample_record(1)).unwrap();
        buckets.flush_all().unwrap();
        buckets.delete_bucket(key).unwrap();
        assert!(buckets.read_bucket(key).is_err());
    }

    #[test]
    fn quota_enforced() {
        let tmp = TempDir::new().unwrap();
        let mut buckets = PendingDeltaBuckets::open(tmp.path(), 1, 64 * 1024).unwrap();
        let record = sample_record(1);
        let err = buckets.spill(0, &record).unwrap_err();
        assert!(
            matches!(err, SpillError::QuotaExceeded { .. }),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn compact_bucket_moves_remaining() {
        let tmp = TempDir::new().unwrap();
        let mut buckets = PendingDeltaBuckets::open(tmp.path(), 1024 * 1024, 64 * 1024).unwrap();
        let key = 0x9abc;
        let r1 = sample_record(1);
        let r2 = sample_record(2);
        buckets.spill(key, &r1).unwrap();
        buckets.spill(key, &r2).unwrap();
        buckets.flush_all().unwrap();

        // Compact keeping only r2.
        buckets.compact_bucket(key, &[r2.clone()]).unwrap();
        let read = buckets.read_bucket(key).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0], r2);
    }
}
