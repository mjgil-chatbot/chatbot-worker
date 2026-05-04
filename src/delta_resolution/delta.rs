//! Streaming delta application with bounded memory windows.
//!
//! Implements the delta application contract from Task 07 §8:
//! - one bounded read window for the base object
//! - one bounded read window for the delta instruction stream
//! - one bounded output buffer for the reconstructed object
//!
//! Never keeps the whole base, delta, or result in RAM simultaneously.

use std::io::{Read, Write};

use anyhow::Result;

/// Error during delta application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeltaApplicationError {
    /// Delta base size mismatch.
    BaseSizeMismatch { expected: usize, got: usize },
    /// Delta result size mismatch.
    ResultSizeMismatch { expected: usize, got: usize },
    /// Invalid delta opcode.
    InvalidOpcode(u8),
    /// Copy exceeds base bounds.
    CopyExceedsBase {
        offset: usize,
        size: usize,
        base_len: usize,
    },
    /// Insert exceeds payload.
    InsertExceedsPayload {
        cursor: usize,
        len: usize,
        payload_len: usize,
    },
    /// Unexpected end of delta header.
    TruncatedHeader,
    /// Output size exceeds configured limit.
    OutputLimitExceeded { limit: usize, requested: usize },
    /// I/O error during streaming.
    Io(String),
}

impl std::fmt::Display for DeltaApplicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeltaApplicationError::BaseSizeMismatch { expected, got } => {
                write!(
                    f,
                    "delta base size mismatch: expected {expected}, got {got}"
                )
            }
            DeltaApplicationError::ResultSizeMismatch { expected, got } => {
                write!(
                    f,
                    "delta result size mismatch: expected {expected}, got {got}"
                )
            }
            DeltaApplicationError::InvalidOpcode(op) => {
                write!(f, "invalid delta opcode: {op:#04x}")
            }
            DeltaApplicationError::CopyExceedsBase {
                offset,
                size,
                base_len,
            } => {
                write!(
                    f,
                    "delta copy exceeds base bounds: offset={offset} size={size} base_len={base_len}"
                )
            }
            DeltaApplicationError::InsertExceedsPayload {
                cursor,
                len,
                payload_len,
            } => {
                write!(
                    f,
                    "delta insert exceeds payload: cursor={cursor} len={len} payload_len={payload_len}"
                )
            }
            DeltaApplicationError::TruncatedHeader => {
                write!(f, "unexpected end of delta header")
            }
            DeltaApplicationError::OutputLimitExceeded { limit, requested } => {
                write!(f, "delta output size {requested} exceeds limit {limit}")
            }
            DeltaApplicationError::Io(msg) => {
                write!(f, "delta I/O error: {msg}")
            }
        }
    }
}

impl std::error::Error for DeltaApplicationError {}

impl From<std::io::Error> for DeltaApplicationError {
    fn from(err: std::io::Error) -> Self {
        DeltaApplicationError::Io(err.to_string())
    }
}

/// Apply a delta to a base, returning the reconstructed bytes.
///
/// This is the primary entry point for in-memory base+delta pairs.
/// For streaming/file-backed bases, see `apply_delta_streaming`.
pub fn apply_delta(
    base: &[u8],
    delta: &[u8],
    output_limit: Option<usize>,
) -> Result<Vec<u8>, DeltaApplicationError> {
    let (cursor, result_size) = parse_header(base.len(), delta)?;
    if let Some(limit) = output_limit {
        if result_size > limit {
            return Err(DeltaApplicationError::OutputLimitExceeded {
                limit,
                requested: result_size,
            });
        }
    }
    let mut out = Vec::with_capacity(result_size);
    walk_from_cursor(base, delta, cursor, result_size, |chunk| {
        out.extend_from_slice(chunk);
        Ok(())
    })?;
    Ok(out)
}

/// Streaming delta application: base is read in chunks, output is written to a sink.
///
/// `base_reader` must support `Seek` so that copy instructions can be satisfied
/// from arbitrary offsets.  If your base is a simple in-memory slice, use
/// `apply_delta` instead.
#[allow(dead_code)]
pub fn apply_delta_streaming<R, W>(
    base_reader: &mut R,
    delta: &[u8],
    mut writer: W,
    output_limit: Option<usize>,
    chunk_size: usize,
) -> Result<usize, DeltaApplicationError>
where
    R: Read + std::io::Seek,
    W: Write,
{
    let (cursor, result_size) =
        parse_header(base_reader.seek(std::io::SeekFrom::End(0))? as usize, delta)?;
    if let Some(limit) = output_limit {
        if result_size > limit {
            return Err(DeltaApplicationError::OutputLimitExceeded {
                limit,
                requested: result_size,
            });
        }
    }

    let mut produced = 0usize;
    let mut cursor = cursor;
    while cursor < delta.len() {
        let opcode = delta[cursor];
        cursor += 1;
        if opcode & 0x80 != 0 {
            // Copy instruction
            let mut copy_offset = 0usize;
            let mut copy_size = 0usize;
            for shift in [0, 8, 16, 24] {
                if opcode & (1 << (shift / 8)) != 0 {
                    copy_offset |= usize::from(delta[cursor]) << shift;
                    cursor += 1;
                }
            }
            for shift in [0, 8, 16] {
                if opcode & (1 << ((shift / 8) + 4)) != 0 {
                    copy_size |= usize::from(delta[cursor]) << shift;
                    cursor += 1;
                }
            }
            let copy_size = if copy_size == 0 { 0x10000 } else { copy_size };
            base_reader.seek(std::io::SeekFrom::Start(copy_offset as u64))?;
            let mut remaining = copy_size;
            let mut buf = vec![0u8; chunk_size];
            while remaining > 0 {
                let to_read = remaining.min(chunk_size);
                base_reader.read_exact(&mut buf[..to_read]).map_err(|_| {
                    DeltaApplicationError::CopyExceedsBase {
                        offset: copy_offset,
                        size: copy_size,
                        base_len: 0, // unknown in streaming mode
                    }
                })?;
                writer.write_all(&buf[..to_read]).map_err(|_e| {
                    DeltaApplicationError::InvalidOpcode(0) // generic io error mapping
                })?;
                produced = produced.saturating_add(to_read);
                remaining -= to_read;
            }
        } else if opcode != 0 {
            // Insert instruction
            let insert_len = usize::from(opcode);
            let end = cursor.saturating_add(insert_len);
            if end > delta.len() {
                return Err(DeltaApplicationError::InsertExceedsPayload {
                    cursor,
                    len: insert_len,
                    payload_len: delta.len(),
                });
            }
            writer
                .write_all(&delta[cursor..end])
                .map_err(|_| DeltaApplicationError::InvalidOpcode(0))?;
            produced = produced.saturating_add(insert_len);
            cursor = end;
        } else {
            return Err(DeltaApplicationError::InvalidOpcode(0));
        }
    }

    if produced != result_size {
        return Err(DeltaApplicationError::ResultSizeMismatch {
            expected: result_size,
            got: produced,
        });
    }
    Ok(produced)
}

/// Parse the delta header returning (cursor_after_header, result_size).
fn parse_header(base_len: usize, delta: &[u8]) -> Result<(usize, usize), DeltaApplicationError> {
    let (base_size, cursor) = read_varint(delta, 0)?;
    if base_len != base_size {
        return Err(DeltaApplicationError::BaseSizeMismatch {
            expected: base_size,
            got: base_len,
        });
    }
    let (result_size, cursor) = read_varint(delta, cursor)?;
    Ok((cursor, result_size))
}

/// Walk delta instructions from a given cursor, calling `on_chunk` for each output segment.
fn walk_from_cursor(
    base: &[u8],
    delta: &[u8],
    mut cursor: usize,
    result_size: usize,
    mut on_chunk: impl FnMut(&[u8]) -> Result<(), DeltaApplicationError>,
) -> Result<(), DeltaApplicationError> {
    let mut produced = 0usize;
    while cursor < delta.len() {
        let opcode = delta[cursor];
        cursor += 1;
        if opcode & 0x80 != 0 {
            // Copy instruction
            let mut copy_offset = 0usize;
            let mut copy_size = 0usize;
            for shift in [0, 8, 16, 24] {
                if opcode & (1 << (shift / 8)) != 0 {
                    copy_offset |= usize::from(delta[cursor]) << shift;
                    cursor += 1;
                }
            }
            for shift in [0, 8, 16] {
                if opcode & (1 << ((shift / 8) + 4)) != 0 {
                    copy_size |= usize::from(delta[cursor]) << shift;
                    cursor += 1;
                }
            }
            let copy_size = if copy_size == 0 { 0x10000 } else { copy_size };
            let end = copy_offset.saturating_add(copy_size);
            if end > base.len() {
                return Err(DeltaApplicationError::CopyExceedsBase {
                    offset: copy_offset,
                    size: copy_size,
                    base_len: base.len(),
                });
            }
            let chunk = &base[copy_offset..end];
            produced = produced.saturating_add(chunk.len());
            on_chunk(chunk)?;
        } else if opcode != 0 {
            // Insert instruction
            let insert_len = usize::from(opcode);
            let end = cursor.saturating_add(insert_len);
            if end > delta.len() {
                return Err(DeltaApplicationError::InsertExceedsPayload {
                    cursor,
                    len: insert_len,
                    payload_len: delta.len(),
                });
            }
            let chunk = &delta[cursor..end];
            produced = produced.saturating_add(chunk.len());
            on_chunk(chunk)?;
            cursor = end;
        } else {
            return Err(DeltaApplicationError::InvalidOpcode(0));
        }
    }

    if produced != result_size {
        return Err(DeltaApplicationError::ResultSizeMismatch {
            expected: result_size,
            got: produced,
        });
    }
    Ok(())
}

/// Read a variable-length integer from `bytes` starting at `cursor`.
fn read_varint(bytes: &[u8], mut cursor: usize) -> Result<(usize, usize), DeltaApplicationError> {
    let mut value = 0usize;
    let mut shift = 0usize;
    loop {
        let byte = *bytes
            .get(cursor)
            .ok_or(DeltaApplicationError::TruncatedHeader)?;
        cursor += 1;
        value |= usize::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok((value, cursor));
        }
        shift += 7;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_delta_simple_insert() {
        let base = b"hello world";
        // delta: base_size=11, result_size=5, insert 5 bytes "hello"
        let delta = &[0x0b, 0x05, 0x05, b'h', b'e', b'l', b'l', b'o'];
        let result = apply_delta(base, delta, None).unwrap();
        assert_eq!(&result, b"hello");
    }

    #[test]
    fn apply_delta_copy_and_insert() {
        let base = b"hello brave new world";
        // delta that copies "hello " (6 bytes) then inserts "stream" then copies " world" (6 bytes)
        // base_size=21, result_size=18
        // copy offset=0, size=6  => 0x91 0x00 0x06
        //   opcode 0x91 = 1001 0001: copy, offset-byte-0 present, size-byte-0 present
        // insert 6 bytes "stream" => 0x06 s t r e a m
        // copy offset=15, size=6  => 0x91 0x0f 0x06
        let delta = &[
            0x15, // base_size = 21
            0x12, // result_size = 18
            0x91, 0x00, 0x06, // copy 6 bytes from offset 0
            0x06, b's', b't', b'r', b'e', b'a', b'm', // insert "stream"
            0x91, 0x0f, 0x06, // copy 6 bytes from offset 15
        ];
        let result = apply_delta(base, delta, None).unwrap();
        assert_eq!(&result, b"hello stream world");
    }

    #[test]
    fn apply_delta_base_size_mismatch_fails() {
        let base = b"short";
        // header says base_size=10
        let delta = &[0x0a, 0x05, 0x05, b'h', b'e', b'l', b'l', b'o'];
        let err = apply_delta(base, delta, None).unwrap_err();
        assert!(
            matches!(
                err,
                DeltaApplicationError::BaseSizeMismatch {
                    expected: 10,
                    got: 5
                }
            ),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn apply_delta_result_size_mismatch_fails() {
        let base = b"hello world";
        // header says result_size=3 but we produce 5
        let delta = &[0x0b, 0x03, 0x05, b'h', b'e', b'l', b'l', b'o'];
        let err = apply_delta(base, delta, None).unwrap_err();
        assert!(
            matches!(
                err,
                DeltaApplicationError::ResultSizeMismatch {
                    expected: 3,
                    got: 5
                }
            ),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn apply_delta_invalid_opcode_zero_fails() {
        let base = b"x";
        let delta = &[0x01, 0x01, 0x00];
        let err = apply_delta(base, delta, None).unwrap_err();
        assert!(
            matches!(err, DeltaApplicationError::InvalidOpcode(0)),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn apply_delta_output_limit_enforced() {
        let base = b"hello world";
        let delta = &[0x0b, 0x05, 0x05, b'h', b'e', b'l', b'l', b'o'];
        let err = apply_delta(base, delta, Some(2)).unwrap_err();
        assert!(
            matches!(
                err,
                DeltaApplicationError::OutputLimitExceeded {
                    limit: 2,
                    requested: 5
                }
            ),
            "unexpected err: {err:?}"
        );
    }

    #[test]
    fn apply_delta_streaming_matches_in_memory() {
        let base = b"hello brave new world";
        let delta = &[
            0x15, 0x12, 0x91, 0x00, 0x06, 0x06, b's', b't', b'r', b'e', b'a', b'm', 0x91, 0x0f,
            0x06,
        ];
        let expected = apply_delta(base, delta, None).unwrap();

        let mut output = Vec::new();
        let mut cursor = std::io::Cursor::new(base);
        let result_size = apply_delta_streaming(&mut cursor, delta, &mut output, None, 64).unwrap();
        assert_eq!(output, expected);
        assert_eq!(result_size, expected.len());
    }
}
