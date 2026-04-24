use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result};
use chrono::Utc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::process::{Child, ChildStderr};
use tokio::task::JoinHandle;
use tokio::time::Duration;

use crate::worker;

pub(crate) const STDERR_TAIL_MAX_CHARS: usize = 8 * 1024;
pub(crate) const TRANSPORT_RETRY_BACKOFF: Duration = Duration::from_millis(750);
pub(crate) const STOP_TERM_GRACE_PERIOD: Duration = Duration::from_secs(2);
pub(crate) const STOP_KILL_GRACE_PERIOD: Duration = Duration::from_secs(2);

pub(crate) fn should_retry_transport_error(output: &str, error: &str) -> bool {
    if !output.trim().is_empty() {
        return false;
    }

    let normalized = error.trim().to_ascii_lowercase();
    normalized.contains("transport error")
        || normalized.starts_with("exit_code:8:")
        || normalized.contains("stream disconnected before completion")
        || normalized.contains("websocket closed by server before response.completed")
}

pub(crate) fn transport_retry_notice(attempt: usize, error: &str) -> String {
    format!("transport error attempt {attempt} failed: {error}; retrying")
}

pub(crate) fn append_transport_error_log(chat_id: &str, message: &str) -> Result<PathBuf> {
    let path = worker::transport_errors_path(chat_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    writeln!(file, "{} {}", Utc::now().to_rfc3339(), message)
        .with_context(|| format!("failed to append {}", path.display()))?;
    Ok(path)
}

pub(crate) async fn finish_stderr_task(stderr_task: JoinHandle<String>) -> String {
    match stderr_task.await {
        Ok(stderr_tail) => stderr_tail,
        Err(err) => {
            eprintln!("Failed to join adapter stderr reader: {err}");
            String::new()
        }
    }
}

pub(crate) fn push_stderr_tail(stderr_tail: &mut String, line: &str) {
    if !stderr_tail.is_empty() {
        stderr_tail.push('\n');
    }
    stderr_tail.push_str(line);

    let total_chars = stderr_tail.chars().count();
    if total_chars <= STDERR_TAIL_MAX_CHARS {
        return;
    }

    let chars_to_trim = total_chars.saturating_sub(STDERR_TAIL_MAX_CHARS);
    let cutoff = stderr_tail
        .char_indices()
        .nth(chars_to_trim)
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    stderr_tail.drain(..cutoff);
}

pub(crate) fn terminal_adapter_error_message(
    code: Option<i32>,
    output: &str,
    error_message: Option<&str>,
) -> String {
    let detail = preferred_terminal_adapter_error_detail(error_message, Some(output))
        .unwrap_or_else(|| "unknown error".to_string());
    format!("exit_code:{}: {}", code.unwrap_or(1), detail)
}

pub(crate) fn preferred_terminal_adapter_error_detail(
    primary: Option<&str>,
    fallback: Option<&str>,
) -> Option<String> {
    let primary = primary.and_then(normalize_terminal_adapter_error_detail);
    let fallback = fallback.and_then(normalize_terminal_adapter_error_detail);

    match (primary, fallback) {
        (Some(primary), Some(fallback))
            if terminal_adapter_error_detail_is_generic(&primary)
                && !terminal_adapter_error_detail_is_generic(&fallback) =>
        {
            Some(fallback)
        }
        (Some(primary), _) => Some(primary),
        (None, Some(fallback)) => Some(fallback),
        (None, None) => None,
    }
}

fn normalize_terminal_adapter_error_detail(value: &str) -> Option<String> {
    let mut detail = value.trim();
    if detail.is_empty() {
        return None;
    }

    while let Some(stripped) = detail.strip_prefix("Error:") {
        let stripped = stripped.trim();
        if stripped.is_empty() {
            break;
        }
        detail = stripped;
    }

    Some(detail.to_string())
}

fn terminal_adapter_error_detail_is_generic(detail: &str) -> bool {
    let normalized = detail.trim().to_ascii_lowercase();
    normalized == "run command failed"
        || normalized.starts_with("provider exited with code ")
        || normalized.ends_with(" exited with code some(1)")
        || normalized.contains(" exited with code some(")
        || normalized.contains(" exited with code ")
}

pub(crate) async fn wait_for_retry_backoff(stop_signal: &AtomicBool) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(TRANSPORT_RETRY_BACKOFF) => false,
        _ = crate::adapter_worker::wait_for_stop_signal(stop_signal) => true,
    }
}

pub(crate) fn configure_subprocess_group(cmd: &mut TokioCommand) {
    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

pub(crate) fn signal_process_group(pid: Option<u32>, signal: libc::c_int) -> Result<()> {
    let pid = pid.context("no pid for child")?;
    let pgid = -(pid as libc::pid_t);
    if unsafe { libc::kill(pgid, signal) } == 0 {
        return Ok(());
    }

    if unsafe { libc::kill(pid as libc::pid_t, signal) } == 0 {
        return Ok(());
    }

    let err = io::Error::last_os_error();
    match err.raw_os_error() {
        Some(libc::ESRCH) => Ok(()),
        _ => Err(err).context("signal adapter subprocess"),
    }
}

pub(crate) async fn wait_for_child_exit(
    child: &mut Child,
    stop_requested: bool,
) -> Result<std::process::ExitStatus> {
    if !stop_requested {
        return child.wait().await.context("wait for child");
    }

    match tokio::time::timeout(STOP_TERM_GRACE_PERIOD, child.wait()).await {
        Ok(status) => return status.context("wait for child after SIGTERM"),
        Err(_) => {
            let _ = signal_process_group(child.id(), libc::SIGKILL);
        }
    }

    tokio::time::timeout(STOP_KILL_GRACE_PERIOD, child.wait())
        .await
        .context("wait for child after SIGKILL")?
        .context("wait for child after SIGKILL")
}

pub(crate) async fn drain_stderr(stderr: ChildStderr) -> String {
    let mut reader = BufReader::new(stderr).lines();
    let mut stderr_tail = String::new();

    loop {
        match reader.next_line().await {
            Ok(Some(line)) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                eprintln!("[agent-adapter stderr] {trimmed}");
                push_stderr_tail(&mut stderr_tail, trimmed);
            }
            Ok(None) => break,
            Err(err) => {
                eprintln!("Failed to read adapter stderr: {err}");
                break;
            }
        }
    }

    stderr_tail
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_retry_requires_empty_output_and_transport_signal() {
        assert!(should_retry_transport_error(
            "",
            "provider: Transport error: HTTP error: 400 Bad Request"
        ));
        assert!(should_retry_transport_error(
            "",
            "exit_code:8: Run command failed"
        ));
        assert!(should_retry_transport_error(
            "",
            "stream disconnected before completion: websocket closed by server before response.completed"
        ));
        assert!(should_retry_transport_error(
            "",
            "websocket closed by server before response.completed"
        ));
        assert!(!should_retry_transport_error(
            "partial reply",
            "provider: Transport error: HTTP error: 400 Bad Request"
        ));
        assert!(!should_retry_transport_error(
            "partial reply",
            "stream disconnected before completion: websocket closed by server before response.completed"
        ));
        assert!(!should_retry_transport_error(
            "partial reply",
            "exit_code:8: Run command failed"
        ));
        assert!(!should_retry_transport_error(
            "",
            "provider error: validation failed"
        ));
    }

    #[test]
    fn terminal_adapter_error_message_normalizes_fallback_error_prefixes() {
        assert_eq!(
            terminal_adapter_error_message(Some(8), "", Some("Error: Run command failed")),
            "exit_code:8: Run command failed"
        );
        assert_eq!(
            terminal_adapter_error_message(Some(8), "", Some("Error: Error: Run command failed")),
            "exit_code:8: Run command failed"
        );
    }

    #[test]
    fn terminal_adapter_error_message_prefers_explicit_error_over_partial_output() {
        assert_eq!(
            terminal_adapter_error_message(
                Some(1),
                "I’m checking the actual failing build path first.",
                Some("codex exited with code Some(1)")
            ),
            "exit_code:1: I’m checking the actual failing build path first."
        );
    }

    #[test]
    fn preferred_terminal_adapter_error_detail_prefers_specific_fallback() {
        assert_eq!(
            preferred_terminal_adapter_error_detail(
                Some("Error: Run command failed"),
                Some("No conversation found with session ID: 799b06e2"),
            ),
            Some("No conversation found with session ID: 799b06e2".to_string())
        );
        assert_eq!(
            preferred_terminal_adapter_error_detail(
                Some("claude exited with code Some(1)"),
                Some("provider-specific failure"),
            ),
            Some("provider-specific failure".to_string())
        );
    }
}
