//! Durable Record Payload Encoding — Task 16 implementation.
//!
//! Implements the binary payload encoding rules for framed durable records
//! defined in the git-bounded design doc `16-durable-record-payload-encoding.md`.
//!
//! General rules (v1):
//! - all fixed-width integers are little-endian
//! - all booleans are encoded as one byte: `0x00` false, `0x01` true
//! - all strings are UTF-8 without implicit terminator
//! - all variable-length byte arrays are length-prefixed (`u32`)
//! - field order is exactly the field order documented by the owning schema
//! - object ids are exactly 20 raw bytes
//! - closed enums use a `u16` discriminant followed by variant payload fields
//! - optional values use `present_u8` (`0x00` absent, `0x01` present) + payload
//! - vectors use `count_u32` + items in logical order

use std::path::PathBuf;

use super::{BaseKey, DeltaId, DeltaRecord, InstructionRef, ObjectKind, ResolutionOrigin};

// ── Error type ─────────────────────────────────────────────────────────────

/// Error during durable payload encoding or decoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodingError {
    /// Unexpected end of input.
    UnexpectedEof,
    /// Invalid boolean tag (not 0x00 or 0x01).
    InvalidBool(u8),
    /// Invalid option tag (not 0x00 or 0x01).
    InvalidOptionTag(u8),
    /// Invalid enum discriminant.
    InvalidDiscriminant(u16),
    /// Invalid UTF-8 in string field.
    InvalidUtf8,
    /// Object id has wrong length (not 20 bytes).
    InvalidObjectIdLength(u32),
    /// Length prefix exceeds remaining frame bytes.
    LengthExceedsFrame { len: u64, remaining: usize },
    /// Trailing undecoded bytes for a fixed-layout payload.
    TrailingBytes(usize),
    /// I/O or other encoding failure.
    Other(String),
}

impl std::fmt::Display for EncodingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncodingError::UnexpectedEof => write!(f, "unexpected end of durable payload"),
            EncodingError::InvalidBool(v) => write!(f, "invalid bool tag: 0x{v:02x}"),
            EncodingError::InvalidOptionTag(v) => write!(f, "invalid option tag: 0x{v:02x}"),
            EncodingError::InvalidDiscriminant(d) => write!(f, "invalid enum discriminant: {d}"),
            EncodingError::InvalidUtf8 => write!(f, "invalid UTF-8 in durable string field"),
            EncodingError::InvalidObjectIdLength(len) => {
                write!(f, "invalid object id length: {len} (expected 20)")
            }
            EncodingError::LengthExceedsFrame { len, remaining } => {
                write!(
                    f,
                    "length prefix {len} exceeds remaining frame bytes {remaining}"
                )
            }
            EncodingError::TrailingBytes(n) => {
                write!(f, "trailing undecoded bytes: {n}")
            }
            EncodingError::Other(msg) => write!(f, "encoding error: {msg}"),
        }
    }
}

impl std::error::Error for EncodingError {}

// ── Stable enum discriminants (v1) ─────────────────────────────────────────

// BaseKey
const DISCRIMINANT_BASE_KEY_OBJECT_ID: u16 = 1;
const DISCRIMINANT_BASE_KEY_PACK_OFFSET: u16 = 2;

// ObjectKind
const DISCRIMINANT_OBJECT_KIND_COMMIT: u16 = 1;
const DISCRIMINANT_OBJECT_KIND_TREE: u16 = 2;
const DISCRIMINANT_OBJECT_KIND_BLOB: u16 = 3;
const DISCRIMINANT_OBJECT_KIND_TAG: u16 = 4;

// InstructionRef
const DISCRIMINANT_INSTRUCTION_REF_INLINE: u16 = 1;
const DISCRIMINANT_INSTRUCTION_REF_SPILLED: u16 = 2;

// ── Encoder ────────────────────────────────────────────────────────────────

/// Stateful encoder that appends bytes to an internal buffer.
pub struct Encoder {
    buf: Vec<u8>,
}

impl Encoder {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    // -- scalars --

    pub fn write_u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn write_u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn write_u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn write_u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn write_bool(&mut self, v: bool) {
        self.buf.push(if v { 0x01 } else { 0x00 });
    }

    /// Encode `usize` as `u64` for cross-platform portability.
    pub fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }

    // -- length-prefixed values --

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.write_u32(bytes.len() as u32);
        self.buf.extend_from_slice(bytes);
    }

    pub fn write_string(&mut self, s: &str) {
        self.write_bytes(s.as_bytes());
    }

    // -- option --

    pub fn write_option<T>(&mut self, opt: Option<T>, encode_some: impl FnOnce(&mut Self, T)) {
        if let Some(v) = opt {
            self.write_u8(0x01);
            encode_some(self, v);
        } else {
            self.write_u8(0x00);
        }
    }

    // -- vector --

    pub fn write_vec<T>(&mut self, items: &[T], encode_item: impl FnMut(&mut Self, &T)) {
        self.write_u32(items.len() as u32);
        let mut encode_item = encode_item;
        for item in items {
            encode_item(self, item);
        }
    }
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}

// ── Decoder ────────────────────────────────────────────────────────────────

/// Stateful decoder that reads from a byte slice.
pub struct Decoder<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Decoder<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    fn need(&self, n: usize) -> Result<(), EncodingError> {
        if self.remaining() < n {
            Err(EncodingError::UnexpectedEof)
        } else {
            Ok(())
        }
    }

    fn advance(&mut self, n: usize) -> &'a [u8] {
        let start = self.pos;
        self.pos += n;
        &self.buf[start..self.pos]
    }

    // -- scalars --

    pub fn read_u8(&mut self) -> Result<u8, EncodingError> {
        self.need(1)?;
        Ok(self.advance(1)[0])
    }

    pub fn read_u16(&mut self) -> Result<u16, EncodingError> {
        self.need(2)?;
        Ok(u16::from_le_bytes(self.advance(2).try_into().unwrap()))
    }

    pub fn read_u32(&mut self) -> Result<u32, EncodingError> {
        self.need(4)?;
        Ok(u32::from_le_bytes(self.advance(4).try_into().unwrap()))
    }

    pub fn read_u64(&mut self) -> Result<u64, EncodingError> {
        self.need(8)?;
        Ok(u64::from_le_bytes(self.advance(8).try_into().unwrap()))
    }

    pub fn read_bool(&mut self) -> Result<bool, EncodingError> {
        match self.read_u8()? {
            0x00 => Ok(false),
            0x01 => Ok(true),
            v => Err(EncodingError::InvalidBool(v)),
        }
    }

    /// Decode `usize` from a `u64`.
    pub fn read_usize(&mut self) -> Result<usize, EncodingError> {
        let v = self.read_u64()?;
        Ok(v as usize)
    }

    // -- length-prefixed values --

    pub fn read_bytes(&mut self) -> Result<Vec<u8>, EncodingError> {
        let len = self.read_u32()? as usize;
        self.need(len)?;
        Ok(self.advance(len).to_vec())
    }

    pub fn read_string(&mut self) -> Result<String, EncodingError> {
        let bytes = self.read_bytes()?;
        String::from_utf8(bytes).map_err(|_| EncodingError::InvalidUtf8)
    }

    // -- option --

    pub fn read_option<T>(
        &mut self,
        decode_some: impl FnOnce(&mut Self) -> Result<T, EncodingError>,
    ) -> Result<Option<T>, EncodingError> {
        match self.read_u8()? {
            0x00 => Ok(None),
            0x01 => Ok(Some(decode_some(self)?)),
            v => Err(EncodingError::InvalidOptionTag(v)),
        }
    }

    // -- vector --

    pub fn read_vec<T>(
        &mut self,
        mut decode_item: impl FnMut(&mut Self) -> Result<T, EncodingError>,
    ) -> Result<Vec<T>, EncodingError> {
        let count = self.read_u32()? as usize;
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(decode_item(self)?);
        }
        Ok(out)
    }

    /// Verify no trailing bytes remain (for fixed-layout payloads).
    pub fn expect_eof(&self) -> Result<(), EncodingError> {
        let remaining = self.remaining();
        if remaining == 0 {
            Ok(())
        } else {
            Err(EncodingError::TrailingBytes(remaining))
        }
    }
}

// ── Encode / Decode traits ─────────────────────────────────────────────────

pub trait Encode {
    fn encode(&self, enc: &mut Encoder);
}

pub trait Decode: Sized {
    fn decode(dec: &mut Decoder<'_>) -> Result<Self, EncodingError>;
}

// ── Implementations for delta-resolution types ────────────────────────────

impl Encode for DeltaId {
    fn encode(&self, enc: &mut Encoder) {
        enc.write_u64(self.0);
    }
}

impl Decode for DeltaId {
    fn decode(dec: &mut Decoder<'_>) -> Result<Self, EncodingError> {
        Ok(DeltaId(dec.read_u64()?))
    }
}

impl Encode for BaseKey {
    fn encode(&self, enc: &mut Encoder) {
        match self {
            BaseKey::ObjectId(oid) => {
                enc.write_u16(DISCRIMINANT_BASE_KEY_OBJECT_ID);
                enc.buf.extend_from_slice(oid);
            }
            BaseKey::PackOffset(offset) => {
                enc.write_u16(DISCRIMINANT_BASE_KEY_PACK_OFFSET);
                enc.write_u64(*offset);
            }
        }
    }
}

impl Decode for BaseKey {
    fn decode(dec: &mut Decoder<'_>) -> Result<Self, EncodingError> {
        let discriminant = dec.read_u16()?;
        match discriminant {
            DISCRIMINANT_BASE_KEY_OBJECT_ID => {
                dec.need(20)?;
                let mut oid = [0u8; 20];
                oid.copy_from_slice(dec.advance(20));
                Ok(BaseKey::ObjectId(oid))
            }
            DISCRIMINANT_BASE_KEY_PACK_OFFSET => Ok(BaseKey::PackOffset(dec.read_u64()?)),
            d => Err(EncodingError::InvalidDiscriminant(d)),
        }
    }
}

impl Encode for ObjectKind {
    fn encode(&self, enc: &mut Encoder) {
        let d = match self {
            ObjectKind::Commit => DISCRIMINANT_OBJECT_KIND_COMMIT,
            ObjectKind::Tree => DISCRIMINANT_OBJECT_KIND_TREE,
            ObjectKind::Blob => DISCRIMINANT_OBJECT_KIND_BLOB,
            ObjectKind::Tag => DISCRIMINANT_OBJECT_KIND_TAG,
        };
        enc.write_u16(d);
    }
}

impl Decode for ObjectKind {
    fn decode(dec: &mut Decoder<'_>) -> Result<Self, EncodingError> {
        let discriminant = dec.read_u16()?;
        match discriminant {
            DISCRIMINANT_OBJECT_KIND_COMMIT => Ok(ObjectKind::Commit),
            DISCRIMINANT_OBJECT_KIND_TREE => Ok(ObjectKind::Tree),
            DISCRIMINANT_OBJECT_KIND_BLOB => Ok(ObjectKind::Blob),
            DISCRIMINANT_OBJECT_KIND_TAG => Ok(ObjectKind::Tag),
            d => Err(EncodingError::InvalidDiscriminant(d)),
        }
    }
}

impl Encode for InstructionRef {
    fn encode(&self, enc: &mut Encoder) {
        match self {
            InstructionRef::Inline(bytes) => {
                enc.write_u16(DISCRIMINANT_INSTRUCTION_REF_INLINE);
                enc.write_bytes(bytes);
            }
            InstructionRef::Spilled { path, offset, len } => {
                enc.write_u16(DISCRIMINANT_INSTRUCTION_REF_SPILLED);
                enc.write_string(&path.to_string_lossy());
                enc.write_u64(*offset);
                enc.write_usize(*len);
            }
        }
    }
}

impl Decode for InstructionRef {
    fn decode(dec: &mut Decoder<'_>) -> Result<Self, EncodingError> {
        let discriminant = dec.read_u16()?;
        match discriminant {
            DISCRIMINANT_INSTRUCTION_REF_INLINE => {
                let bytes = dec.read_bytes()?;
                Ok(InstructionRef::Inline(bytes))
            }
            DISCRIMINANT_INSTRUCTION_REF_SPILLED => {
                let path_str = dec.read_string()?;
                let offset = dec.read_u64()?;
                let len = dec.read_usize()?;
                Ok(InstructionRef::Spilled {
                    path: PathBuf::from(path_str),
                    offset,
                    len,
                })
            }
            d => Err(EncodingError::InvalidDiscriminant(d)),
        }
    }
}

impl Encode for DeltaRecord {
    fn encode(&self, enc: &mut Encoder) {
        // Field order exactly as documented by Task 07 §5:
        // 1. delta_id
        // 2. source_pack_sequence
        // 3. base_key
        // 4. result_kind
        // 5. expected_result_size
        // 6. instruction_ref
        // 7. instruction_byte_length
        // 8. attempt_count
        // 9. source_depth_hint
        self.delta_id.encode(enc);
        enc.write_option(self.source_pack_sequence, |enc, v| enc.write_u64(v));
        self.base_key.encode(enc);
        self.result_kind.encode(enc);
        enc.write_usize(self.expected_result_size);
        self.instruction_ref.encode(enc);
        enc.write_usize(self.instruction_byte_length);
        enc.write_u32(self.attempt_count);
        enc.write_option(self.source_depth_hint, |enc, v| enc.write_u32(v));
    }
}

impl Decode for DeltaRecord {
    fn decode(dec: &mut Decoder<'_>) -> Result<Self, EncodingError> {
        Ok(DeltaRecord {
            delta_id: DeltaId::decode(dec)?,
            source_pack_sequence: dec.read_option(|d| d.read_u64())?,
            base_key: BaseKey::decode(dec)?,
            result_kind: ObjectKind::decode(dec)?,
            expected_result_size: dec.read_usize()?,
            instruction_ref: InstructionRef::decode(dec)?,
            instruction_byte_length: dec.read_usize()?,
            attempt_count: dec.read_u32()?,
            source_depth_hint: dec.read_option(|d| d.read_u32())?,
        })
    }
}

impl Encode for ResolutionOrigin {
    fn encode(&self, enc: &mut Encoder) {
        enc.write_u32(self.pass_number);
        enc.write_option(self.source_pack_sequence, |enc, v| enc.write_u64(v));
        enc.write_option(self.base_key.as_ref(), |enc, v| v.encode(enc));
    }
}

impl Decode for ResolutionOrigin {
    fn decode(dec: &mut Decoder<'_>) -> Result<Self, EncodingError> {
        Ok(ResolutionOrigin {
            pass_number: dec.read_u32()?,
            source_pack_sequence: dec.read_option(|d| d.read_u64())?,
            base_key: dec.read_option(|d| BaseKey::decode(d))?,
        })
    }
}

// ── Convenience free functions ─────────────────────────────────────────────

/// Encode a value to a byte vector.
pub fn encode_to_vec<T: Encode>(value: &T) -> Vec<u8> {
    let mut enc = Encoder::new();
    value.encode(&mut enc);
    enc.into_vec()
}

/// Decode a value from a byte slice, verifying no trailing bytes remain.
pub fn decode_exact<T: Decode>(buf: &[u8]) -> Result<T, EncodingError> {
    let mut dec = Decoder::new(buf);
    let value = T::decode(&mut dec)?;
    dec.expect_eof()?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_roundtrip() {
        let mut enc = Encoder::new();
        enc.write_u8(0x42);
        enc.write_u16(0x1234);
        enc.write_u32(0xDEADBEEF);
        enc.write_u64(0x123456789ABCDEF0);
        enc.write_bool(true);
        enc.write_bool(false);
        enc.write_usize(0xABCD);

        let buf = enc.into_vec();
        let mut dec = Decoder::new(&buf);
        assert_eq!(dec.read_u8().unwrap(), 0x42);
        assert_eq!(dec.read_u16().unwrap(), 0x1234);
        assert_eq!(dec.read_u32().unwrap(), 0xDEADBEEF);
        assert_eq!(dec.read_u64().unwrap(), 0x123456789ABCDEF0);
        assert_eq!(dec.read_bool().unwrap(), true);
        assert_eq!(dec.read_bool().unwrap(), false);
        assert_eq!(dec.read_usize().unwrap(), 0xABCD);
        dec.expect_eof().unwrap();
    }

    #[test]
    fn invalid_bool_rejected() {
        let mut enc = Encoder::new();
        enc.write_u8(0x02);
        let buf = enc.into_vec();
        let mut dec = Decoder::new(&buf);
        assert!(matches!(
            dec.read_bool(),
            Err(EncodingError::InvalidBool(0x02))
        ));
    }

    #[test]
    fn invalid_option_tag_rejected() {
        let mut enc = Encoder::new();
        enc.write_u8(0x02);
        let buf = enc.into_vec();
        let mut dec = Decoder::new(&buf);
        assert!(matches!(
            dec.read_option(|_| Ok(())),
            Err(EncodingError::InvalidOptionTag(0x02))
        ));
    }

    #[test]
    fn string_roundtrip() {
        let mut enc = Encoder::new();
        enc.write_string("hello world");
        let buf = enc.into_vec();
        let mut dec = Decoder::new(&buf);
        assert_eq!(dec.read_string().unwrap(), "hello world");
        dec.expect_eof().unwrap();
    }

    #[test]
    fn bytes_roundtrip() {
        let mut enc = Encoder::new();
        enc.write_bytes(&[1, 2, 3, 4, 5]);
        let buf = enc.into_vec();
        let mut dec = Decoder::new(&buf);
        assert_eq!(dec.read_bytes().unwrap(), vec![1, 2, 3, 4, 5]);
        dec.expect_eof().unwrap();
    }

    #[test]
    fn option_roundtrip() {
        let mut enc = Encoder::new();
        enc.write_option(Some(42u64), |enc, v| enc.write_u64(v));
        let buf = enc.into_vec();
        let mut dec = Decoder::new(&buf);
        assert_eq!(dec.read_option(|d| d.read_u64()).unwrap(), Some(42));

        let mut enc = Encoder::new();
        enc.write_option::<u64>(None, |enc, v| enc.write_u64(v));
        let buf = enc.into_vec();
        let mut dec = Decoder::new(&buf);
        assert_eq!(dec.read_option(|d| d.read_u64()).unwrap(), None);
    }

    #[test]
    fn vec_roundtrip() {
        let items = vec![1u16, 2, 3];
        let mut enc = Encoder::new();
        enc.write_vec(&items, |enc, v| enc.write_u16(*v));
        let buf = enc.into_vec();
        let mut dec = Decoder::new(&buf);
        assert_eq!(dec.read_vec(|d| d.read_u16()).unwrap(), vec![1, 2, 3]);
        dec.expect_eof().unwrap();
    }

    #[test]
    fn base_key_object_id_roundtrip() {
        let key = BaseKey::ObjectId([0xab; 20]);
        let buf = encode_to_vec(&key);
        assert_eq!(buf.len(), 2 + 20); // discriminant + 20 bytes
        let decoded = decode_exact::<BaseKey>(&buf).unwrap();
        assert_eq!(decoded, key);
    }

    #[test]
    fn base_key_pack_offset_roundtrip() {
        let key = BaseKey::PackOffset(0x123456789ABCDEF0);
        let buf = encode_to_vec(&key);
        let decoded = decode_exact::<BaseKey>(&buf).unwrap();
        assert_eq!(decoded, key);
    }

    #[test]
    fn invalid_base_key_discriminant_rejected() {
        let mut enc = Encoder::new();
        enc.write_u16(99);
        let buf = enc.into_vec();
        let mut dec = Decoder::new(&buf);
        assert!(matches!(
            BaseKey::decode(&mut dec),
            Err(EncodingError::InvalidDiscriminant(99))
        ));
    }

    #[test]
    fn object_kind_roundtrip() {
        for kind in [
            ObjectKind::Commit,
            ObjectKind::Tree,
            ObjectKind::Blob,
            ObjectKind::Tag,
        ] {
            let buf = encode_to_vec(&kind);
            let decoded = decode_exact::<ObjectKind>(&buf).unwrap();
            assert_eq!(decoded, kind);
        }
    }

    #[test]
    fn instruction_ref_inline_roundtrip() {
        let iref = InstructionRef::Inline(vec![1, 2, 3, 4, 5]);
        let buf = encode_to_vec(&iref);
        let decoded = decode_exact::<InstructionRef>(&buf).unwrap();
        assert_eq!(decoded, iref);
    }

    #[test]
    fn instruction_ref_spilled_roundtrip() {
        let iref = InstructionRef::Spilled {
            path: PathBuf::from("/tmp/instructions.bin"),
            offset: 1024,
            len: 4096,
        };
        let buf = encode_to_vec(&iref);
        let decoded = decode_exact::<InstructionRef>(&buf).unwrap();
        assert_eq!(decoded, iref);
    }

    #[test]
    fn delta_record_roundtrip() {
        let record = DeltaRecord {
            delta_id: DeltaId(0x1234),
            source_pack_sequence: Some(42),
            base_key: BaseKey::ObjectId([0xcd; 20]),
            result_kind: ObjectKind::Blob,
            expected_result_size: 1024,
            instruction_ref: InstructionRef::Inline(vec![0xde, 0xad, 0xbe, 0xef]),
            instruction_byte_length: 4,
            attempt_count: 3,
            source_depth_hint: Some(7),
        };
        let buf = encode_to_vec(&record);
        let decoded = decode_exact::<DeltaRecord>(&buf).unwrap();
        assert_eq!(decoded, record);
    }

    #[test]
    fn delta_record_with_none_options_roundtrip() {
        let record = DeltaRecord {
            delta_id: DeltaId(0),
            source_pack_sequence: None,
            base_key: BaseKey::PackOffset(0),
            result_kind: ObjectKind::Commit,
            expected_result_size: 0,
            instruction_ref: InstructionRef::Spilled {
                path: PathBuf::from(""),
                offset: 0,
                len: 0,
            },
            instruction_byte_length: 0,
            attempt_count: 0,
            source_depth_hint: None,
        };
        let buf = encode_to_vec(&record);
        let decoded = decode_exact::<DeltaRecord>(&buf).unwrap();
        assert_eq!(decoded, record);
    }

    #[test]
    fn resolution_origin_roundtrip() {
        let origin = ResolutionOrigin {
            pass_number: 2,
            source_pack_sequence: Some(99),
            base_key: Some(BaseKey::ObjectId([0xef; 20])),
        };
        let buf = encode_to_vec(&origin);
        let decoded = decode_exact::<ResolutionOrigin>(&buf).unwrap();
        assert_eq!(decoded, origin);
    }

    #[test]
    fn trailing_bytes_rejected() {
        let mut enc = Encoder::new();
        enc.write_u64(42);
        enc.write_u8(0xFF); // trailing garbage
        let buf = enc.into_vec();
        let result = decode_exact::<DeltaId>(&buf);
        assert!(matches!(result, Err(EncodingError::TrailingBytes(1))));
    }

    #[test]
    fn unexpected_eof_rejected() {
        let buf = [0x01, 0x02]; // too short for any meaningful read
        let mut dec = Decoder::new(&buf);
        assert!(matches!(dec.read_u64(), Err(EncodingError::UnexpectedEof)));
    }
}
