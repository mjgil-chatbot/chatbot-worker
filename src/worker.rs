//! Worker types and helpers shared between the UI and chatbot-worker binary.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;
/// Atomically write text content inside the worker spool directory.
pub fn write_text_atomic(path: &Path, content: &str) -> io::Result<()> {
    write_bytes_atomic(path, content.as_bytes())
}
fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    fs::create_dir_all(parent)?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let tmp_path = parent.join(format!(".{}.tmp-{}", file_name, std::process::id()));

    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}
