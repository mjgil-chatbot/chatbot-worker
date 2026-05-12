//! Crash-safe atomic write primitive shared by authoritative file writers.

use anyhow::{Context, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

/// Writes bytes to a temp file, fsyncs it, renames it into place, then fsyncs
/// the parent directory so the rename is durable across crashes.
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<u64> {
    let parent = path.parent().context("path has no parent")?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("path has no file name")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;

    let tmp_path = parent.join(format!(".{}.tmp-{}", file_name, std::process::id()));
    let mut tmp_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmp_path)
        .with_context(|| format!("failed to create {}", tmp_path.display()))?;
    tmp_file
        .write_all(content)
        .with_context(|| format!("failed to write {}", tmp_path.display()))?;
    tmp_file
        .flush()
        .with_context(|| format!("failed to flush {}", tmp_path.display()))?;
    tmp_file
        .sync_all()
        .with_context(|| format!("failed to sync {}", tmp_path.display()))?;
    drop(tmp_file);

    fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "failed to rename {} to {}",
            tmp_path.display(),
            path.display()
        )
    })?;

    let parent_file =
        fs::File::open(parent).with_context(|| format!("failed to open {}", parent.display()))?;
    parent_file
        .sync_all()
        .with_context(|| format!("failed to sync {}", parent.display()))?;

    Ok(content.len() as u64)
}

pub fn write_string(path: &Path, content: &str) -> Result<u64> {
    atomic_write(path, content.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_file_with_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.json");
        atomic_write(&path, b"hello world").unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "hello world");
    }

    #[test]
    fn atomic_write_overwrites_existing_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.json");
        std::fs::write(&path, b"old content").unwrap();
        atomic_write(&path, b"new content").unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "new content");
    }

    #[test]
    fn atomic_write_fails_on_parentless_path() {
        let result = atomic_write(std::path::Path::new(""), b"data");
        assert!(result.is_err());
    }

    #[test]
    fn write_string_creates_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.txt");
        write_string(&path, "hello world").unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "hello world");
    }
}
