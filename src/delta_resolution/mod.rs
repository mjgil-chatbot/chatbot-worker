//! Delta Resolution — external-memory git delta resolution without unbounded in-memory graphs.
//!
//! Implements Task 07 from the git-bounded design: pass-based delta resolution
//! using spill files, bounded memory windows, and streaming delta application.

mod delta;
mod encoding;
mod index;
mod resolve;
mod spill;

pub use delta::{apply_delta, DeltaApplicationError};
pub use encoding::{decode_exact, encode_to_vec, Decode, Decoder, Encode, Encoder, EncodingError};
pub use index::{
    BaseIndexEntry, ContentLocation, OffsetIndex, OffsetIndexEntry, ResolvedBaseIndex,
};
pub use resolve::{resolve_deltas, DeltaResolveConfig, DeltaResolveError, ResolutionOutcome};
pub use spill::{DeltaRecord, InstructionRef, PendingDeltaBuckets, SpillError};

use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for a delta record within the resolution session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeltaId(pub u64);

/// Base key for locating a delta's base object.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BaseKey {
    /// Base identified by object ID (for ref-deltas).
    ObjectId([u8; 20]),
    /// Base identified by pack offset (for ofs-deltas).
    PackOffset(u64),
}

/// Canonical git object kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObjectKind {
    Commit,
    Tree,
    Blob,
    Tag,
}

impl ObjectKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ObjectKind::Commit => "commit",
            ObjectKind::Tree => "tree",
            ObjectKind::Blob => "blob",
            ObjectKind::Tag => "tag",
        }
    }
}

impl fmt::Display for ObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A fully resolved object ready for writing to the object store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedObject {
    pub kind: ObjectKind,
    pub bytes: Vec<u8>,
    pub logical_size: usize,
    pub resolved_depth: u32,
}

/// Parsed pack entry from the upstream pack parser.
#[derive(Clone, Debug)]
pub enum ParsedPackEntry {
    /// A full base object.
    Full {
        object_kind: ObjectKind,
        bytes: Vec<u8>,
        pack_offset: u64,
    },
    /// A ref-delta: base is identified by object ID.
    RefDelta {
        base_oid: [u8; 20],
        delta_instructions: Vec<u8>,
        pack_offset: u64,
        result_kind: ObjectKind,
        result_size_hint: usize,
    },
    /// An ofs-delta: base is identified by pack offset.
    OfsDelta {
        base_offset: u64,
        delta_instructions: Vec<u8>,
        pack_offset: u64,
        result_kind: ObjectKind,
        result_size_hint: usize,
    },
}

/// Stream of resolved objects emitted during resolution.
pub struct ResolvedObjectStream {
    pub objects: Vec<ResolvedObject>,
    pub origin_metadata: ResolutionOrigin,
}

/// Metadata tracking where a resolved object came from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionOrigin {
    pub pass_number: u32,
    pub source_pack_sequence: Option<u64>,
    pub base_key: Option<BaseKey>,
}

/// Error code for unsupported thin packs (v1 rejection).
pub const GB_UNSUPPORTED_THIN_PACK: &str = "GB_UNSUPPORTED_THIN_PACK";

/// Compute a deterministic bucket key from a base key for pending delta storage.
pub fn bucket_key_for(base: &BaseKey) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    base.hash(&mut hasher);
    hasher.finish()
}
