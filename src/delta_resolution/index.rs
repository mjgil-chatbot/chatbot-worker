//! Persistent indexes for delta resolution.
//!
//! Implements Task 07 §4.1 (Resolved Base Index) and §4.2 (Offset Index).

use std::collections::HashMap;

use super::{ObjectKind, ResolvedObject};

/// Persisted index of objects already available as full byte streams.
///
/// Each entry stores:
/// - object identity key
/// - content location (for streaming retrieval)
/// - object kind
/// - full size
/// - resolved depth
#[derive(Clone, Debug, Default)]
pub struct ResolvedBaseIndex {
    entries: HashMap<[u8; 20], BaseIndexEntry>,
}

/// Entry in the resolved base index.
#[derive(Clone, Debug)]
pub struct BaseIndexEntry {
    /// Where the full object bytes can be read from (e.g. a file path or offset).
    pub content_location: ContentLocation,
    /// Canonical object kind.
    pub object_kind: ObjectKind,
    /// Full uncompressed size.
    pub full_size: usize,
    /// How many delta applications were needed to resolve this object.
    pub resolved_depth: u32,
}

/// Location where resolved object content is stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentLocation {
    /// In-memory bytes (for small objects during early passes).
    InMemory(Vec<u8>),
    /// Spilled to a file at the given path, starting at offset, with given length.
    Spilled {
        path: std::path::PathBuf,
        offset: u64,
        len: usize,
    },
}

/// Persisted mapping from pack offsets to resolved object identities.
///
/// Required for ofs-delta resolution (Task 07 §4.2).
#[derive(Clone, Debug, Default)]
pub struct OffsetIndex {
    entries: HashMap<u64, OffsetIndexEntry>,
}

/// Entry in the offset index.
#[derive(Clone, Debug)]
pub struct OffsetIndexEntry {
    /// Resolved object ID (20-byte SHA-1).
    pub object_id: [u8; 20],
    /// Object kind.
    pub object_kind: ObjectKind,
    /// Resolved depth.
    pub resolved_depth: u32,
}

impl ResolvedBaseIndex {
    /// Create a new empty resolved base index.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Insert a resolved object into the index.
    pub fn insert(&mut self, object_id: [u8; 20], entry: BaseIndexEntry) {
        self.entries.insert(object_id, entry);
    }

    /// Look up a base by object ID.
    pub fn lookup(&self, object_id: &[u8; 20]) -> Option<&BaseIndexEntry> {
        self.entries.get(object_id)
    }

    /// Remove an entry (used when the object is no longer needed as a base).
    pub fn remove(&mut self, object_id: &[u8; 20]) -> Option<BaseIndexEntry> {
        self.entries.remove(object_id)
    }

    /// Number of entries in the index.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over all entries.
    pub fn iter(&self) -> impl Iterator<Item = (&[u8; 20], &BaseIndexEntry)> {
        self.entries.iter()
    }

    /// Register a fully resolved object.
    pub fn register_resolved(
        &mut self,
        object_id: [u8; 20],
        object: &ResolvedObject,
        location: ContentLocation,
    ) {
        self.insert(
            object_id,
            BaseIndexEntry {
                content_location: location,
                object_kind: object.kind,
                full_size: object.logical_size,
                resolved_depth: object.resolved_depth,
            },
        );
    }
}

impl OffsetIndex {
    /// Create a new empty offset index.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Map a pack offset to a resolved object identity.
    pub fn insert(&mut self, pack_offset: u64, entry: OffsetIndexEntry) {
        self.entries.insert(pack_offset, entry);
    }

    /// Look up a resolved object by its original pack offset.
    pub fn lookup(&self, pack_offset: u64) -> Option<&OffsetIndexEntry> {
        self.entries.get(&pack_offset)
    }

    /// Remove an entry.
    pub fn remove(&mut self, pack_offset: u64) -> Option<OffsetIndexEntry> {
        self.entries.remove(&pack_offset)
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Translate a relative ofs-delta offset to an absolute pack offset,
    /// then look up the resolved object.
    ///
    /// For ofs-deltas, the base is at `current_offset - delta_distance`.
    pub fn resolve_ofs_delta(
        &self,
        current_offset: u64,
        delta_distance: u64,
    ) -> Option<&OffsetIndexEntry> {
        let base_offset = current_offset.checked_sub(delta_distance)?;
        self.lookup(base_offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_base_index_roundtrip() {
        let mut index = ResolvedBaseIndex::new();
        let oid = [0xab; 20];
        let entry = BaseIndexEntry {
            content_location: ContentLocation::InMemory(vec![1, 2, 3]),
            object_kind: ObjectKind::Blob,
            full_size: 3,
            resolved_depth: 0,
        };
        index.insert(oid, entry.clone());
        assert_eq!(index.len(), 1);
        let looked_up = index.lookup(&oid).unwrap();
        assert_eq!(looked_up.object_kind, ObjectKind::Blob);
        assert_eq!(looked_up.full_size, 3);
        assert_eq!(looked_up.resolved_depth, 0);
    }

    #[test]
    fn offset_index_roundtrip() {
        let mut index = OffsetIndex::new();
        let entry = OffsetIndexEntry {
            object_id: [0xcd; 20],
            object_kind: ObjectKind::Commit,
            resolved_depth: 1,
        };
        index.insert(1234, entry);
        assert_eq!(index.len(), 1);
        let looked_up = index.lookup(1234).unwrap();
        assert_eq!(looked_up.object_kind, ObjectKind::Commit);
        assert_eq!(looked_up.resolved_depth, 1);
    }

    #[test]
    fn offset_index_resolve_ofs_delta() {
        let mut index = OffsetIndex::new();
        let entry = OffsetIndexEntry {
            object_id: [0xef; 20],
            object_kind: ObjectKind::Tree,
            resolved_depth: 2,
        };
        index.insert(1000, entry);
        let resolved = index.resolve_ofs_delta(1500, 500).unwrap();
        assert_eq!(resolved.object_kind, ObjectKind::Tree);
        assert_eq!(resolved.resolved_depth, 2);
    }

    #[test]
    fn offset_index_resolve_ofs_delta_underflow() {
        let index = OffsetIndex::new();
        assert!(index.resolve_ofs_delta(100, 200).is_none());
    }
}
