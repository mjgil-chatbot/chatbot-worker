//! Debug logging initialization and helpers.
//!
//! Logs are written to platform-appropriate, permanent locations:
//! - macOS: ~/Library/Logs/RustChatbot
//! - Linux: $XDG_STATE_HOME/rust-chatbot (or ~/.local/state/rust-chatbot)
//!
//! Log files rotate daily AND when they exceed `MAX_LOG_FILE_SIZE`.

use std::path::PathBuf;

use rust_debug::tracing_appender;
use rust_debug::tracing_appender::non_blocking::WorkerGuard;
use rust_debug::tracing_subscriber;
use rust_debug::tracing_subscriber::{fmt, prelude::*, EnvFilter};
use rust_debug::TracingLayer;

#[cfg(target_os = "macos")]
const APP_LOG_DIR_MAC: &str = "RustChatbot";
#[cfg(target_os = "linux")]
const APP_LOG_DIR_LINUX: &str = "rust-chatbot";
const LOG_FILE_PREFIX: &str = "rust-chatbot.log";

/// Maximum log file size before rotation (10 MB).
const MAX_LOG_FILE_SIZE: u64 = 10 * 1024 * 1024;

pub struct DebugGuard {
    _file_guard: Option<WorkerGuard>,
    _size_checker: Option<std::thread::JoinHandle<()>>,
}

pub fn init() -> DebugGuard {
    let log_dir = log_dir();
    if let Err(err) = std::fs::create_dir_all(&log_dir) {
        eprintln!(
            "Failed to create log directory {}: {}",
            log_dir.display(),
            err
        );
    }

    // Rotate any existing oversized log files before starting
    rotate_oversized_logs(&log_dir);

    let file_appender = tracing_appender::rolling::daily(&log_dir, LOG_FILE_PREFIX);
    let (non_blocking, file_guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"));

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_target(true)
        .with_file(true)
        .with_line_number(true);

    let stderr_enabled = std::env::var("CLAUDE_LOG_STDERR")
        .map(|value| {
            let value = value.to_lowercase();
            value == "1" || value == "true" || value == "yes"
        })
        .unwrap_or(false);

    let debug_layer = TracingLayer::from_env();

    let init_result = if stderr_enabled {
        let stderr_layer = fmt::layer()
            .with_writer(std::io::stderr)
            .with_ansi(true)
            .with_target(true)
            .with_file(true)
            .with_line_number(true);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(file_layer)
            .with(stderr_layer)
            .with(debug_layer)
            .try_init()
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(file_layer)
            .with(debug_layer)
            .try_init()
    };

    if init_result.is_err() {
        return DebugGuard {
            _file_guard: None,
            _size_checker: None,
        };
    }

    std::panic::set_hook(Box::new(|panic_info| {
        rust_debug::tracing::error!(panic=%panic_info, "panic");
    }));

    rust_debug::tracing::info!(log_dir=%log_dir.display(), "logging initialized");

    // Clean up old log files (keep last 3 days)
    cleanup_old_logs(&log_dir, 3);

    // Spawn a background thread to periodically check log file sizes
    let size_check_dir = log_dir.clone();
    let size_checker = std::thread::Builder::new()
        .name("log-size-checker".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
            rotate_oversized_logs(&size_check_dir);
        })
        .ok();

    DebugGuard {
        _file_guard: Some(file_guard),
        _size_checker: size_checker,
    }
}

/// Truncate any log files that exceed `MAX_LOG_FILE_SIZE`.
///
/// Since `tracing-appender` holds the file handle, we can't rename the active
/// file out from under it. Instead we truncate it in place — this resets the
/// file size to zero while the appender keeps writing to the same fd.
fn rotate_oversized_logs(log_dir: &std::path::Path) {
    if let Ok(entries) = std::fs::read_dir(log_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_log = path
                .file_name()
                .map(|n| n.to_string_lossy().starts_with(LOG_FILE_PREFIX))
                .unwrap_or(false);
            if !is_log {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                if meta.len() > MAX_LOG_FILE_SIZE {
                    // Truncate the file to zero bytes
                    if let Ok(file) = std::fs::OpenOptions::new()
                        .write(true)
                        .truncate(true)
                        .open(&path)
                    {
                        drop(file);
                        eprintln!(
                            "Log file {} exceeded {}MB, truncated",
                            path.display(),
                            MAX_LOG_FILE_SIZE / (1024 * 1024)
                        );
                    }
                }
            }
        }
    }
}

/// Remove log files older than `keep_days` days.
fn cleanup_old_logs(log_dir: &std::path::Path, keep_days: u64) {
    let cutoff =
        std::time::SystemTime::now() - std::time::Duration::from_secs(keep_days * 24 * 60 * 60);
    if let Ok(entries) = std::fs::read_dir(log_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none()
                || !path
                    .file_name()
                    .map(|n| n.to_string_lossy().starts_with(LOG_FILE_PREFIX))
                    .unwrap_or(false)
            {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if modified < cutoff {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub fn log_dir() -> PathBuf {
    let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("Library").join("Logs").join(APP_LOG_DIR_MAC)
}

#[cfg(target_os = "linux")]
pub fn log_dir() -> PathBuf {
    if let Ok(state) = std::env::var("XDG_STATE_HOME") {
        return PathBuf::from(state).join(APP_LOG_DIR_LINUX);
    }

    if let Some(home) = dirs::home_dir() {
        return home.join(".local").join("state").join(APP_LOG_DIR_LINUX);
    }

    PathBuf::from(".").join(APP_LOG_DIR_LINUX)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn log_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rust-chatbot")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn log_dir_returns_non_empty_path() {
        let dir = log_dir();
        assert!(
            !dir.as_os_str().is_empty(),
            "log_dir must return a non-empty path"
        );
        // On Linux the path should contain the app slug
        #[cfg(target_os = "linux")]
        assert!(
            dir.to_string_lossy().contains("rust-chatbot"),
            "Linux log dir should contain 'rust-chatbot': {}",
            dir.display()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn log_dir_respects_xdg_state_home() {
        let tmp = tempdir().unwrap();
        let xdg_val = tmp.path().to_string_lossy().to_string();

        // Temporarily set XDG_STATE_HOME
        let prev = std::env::var_os("XDG_STATE_HOME");
        std::env::set_var("XDG_STATE_HOME", &xdg_val);
        let dir = log_dir();
        match prev {
            Some(v) => std::env::set_var("XDG_STATE_HOME", v),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }

        assert_eq!(
            dir,
            PathBuf::from(&xdg_val).join("rust-chatbot"),
            "log_dir should use XDG_STATE_HOME when set"
        );
    }

    #[test]
    fn rotate_oversized_logs_truncates_large_files() {
        let tmp = tempdir().unwrap();
        let log_path = tmp.path().join("rust-chatbot.log.2025-01-01");

        // Write a file larger than MAX_LOG_FILE_SIZE
        let oversized = vec![b'x'; (MAX_LOG_FILE_SIZE + 1) as usize];
        fs::write(&log_path, &oversized).unwrap();
        assert!(fs::metadata(&log_path).unwrap().len() > MAX_LOG_FILE_SIZE);

        rotate_oversized_logs(tmp.path());

        let size_after = fs::metadata(&log_path).unwrap().len();
        assert_eq!(
            size_after, 0,
            "oversized log file should be truncated to 0 bytes"
        );
    }

    #[test]
    fn rotate_oversized_logs_leaves_small_files_alone() {
        let tmp = tempdir().unwrap();
        let log_path = tmp.path().join("rust-chatbot.log.2025-01-01");
        let content = b"small log content";
        fs::write(&log_path, content).unwrap();

        rotate_oversized_logs(tmp.path());

        let size_after = fs::metadata(&log_path).unwrap().len();
        assert_eq!(
            size_after,
            content.len() as u64,
            "small log file should not be truncated"
        );
    }

    #[test]
    fn rotate_oversized_logs_ignores_non_log_files() {
        let tmp = tempdir().unwrap();
        let other_path = tmp.path().join("not-a-log.txt");
        let oversized = vec![b'x'; (MAX_LOG_FILE_SIZE + 1) as usize];
        fs::write(&other_path, &oversized).unwrap();

        rotate_oversized_logs(tmp.path());

        let size_after = fs::metadata(&other_path).unwrap().len();
        assert!(
            size_after > MAX_LOG_FILE_SIZE,
            "non-log file should not be truncated"
        );
    }

    #[test]
    fn rotate_oversized_logs_handles_missing_directory() {
        // Should not panic on a nonexistent directory
        rotate_oversized_logs(std::path::Path::new("/nonexistent/path/for/test"));
    }

    #[test]
    fn cleanup_old_logs_keeps_recent_files() {
        let tmp = tempdir().unwrap();
        let recent_log = tmp.path().join("rust-chatbot.log.2025-01-01");
        fs::write(&recent_log, b"recent log").unwrap();
        // File modification time is "now" by default, well within 3 days

        cleanup_old_logs(tmp.path(), 3);

        assert!(recent_log.exists(), "recent log file should be kept");
    }

    #[test]
    fn cleanup_old_logs_skips_files_without_extension() {
        let tmp = tempdir().unwrap();
        // File with no extension — cleanup_old_logs skips these
        let no_ext = tmp.path().join("rust-chatbot-noext");
        fs::write(&no_ext, b"no extension").unwrap();

        // Use keep_days=0 so the cutoff is "now" and the file is old enough
        cleanup_old_logs(tmp.path(), 0);

        assert!(
            no_ext.exists(),
            "file without extension should not be removed"
        );
    }

    #[test]
    fn cleanup_old_logs_removes_with_zero_keep_days() {
        let tmp = tempdir().unwrap();
        let log_file = tmp.path().join("rust-chatbot.log.2025-01-01");
        fs::write(&log_file, b"old log").unwrap();

        // Sleep briefly so the file mtime is strictly before cutoff
        std::thread::sleep(std::time::Duration::from_millis(50));

        // keep_days=0 means cutoff is "now", so all files are "old"
        cleanup_old_logs(tmp.path(), 0);

        assert!(
            !log_file.exists(),
            "log file should be removed with keep_days=0"
        );
    }

    #[test]
    fn cleanup_old_logs_handles_missing_directory() {
        // Should not panic on nonexistent directory
        cleanup_old_logs(std::path::Path::new("/nonexistent/path/for/test"), 3);
    }
}
