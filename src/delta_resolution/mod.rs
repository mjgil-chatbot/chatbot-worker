//! Delta Resolution — external-memory git delta resolution without unbounded in-memory graphs.
//!
//! Implements Task 07 from the git-bounded design: pass-based delta resolution
//! using spill files, bounded memory windows, and streaming delta application.

mod delta;
mod encoding;
mod spill;
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
/// Metadata tracking where a resolved object came from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionOrigin {
    pub pass_number: u32,
    pub source_pack_sequence: Option<u64>,
    pub base_key: Option<BaseKey>,
}
