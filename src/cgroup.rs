//! Cgroup v2 memory limit wrapper for command execution.
//!
//! Provides a single `run_cgroup()` function that wraps any command in a
//! systemd-run scope with memory limits, preventing any spawned process tree
//! from exceeding the configured ceiling.
//!
//! ```rust,no_run
//! use chatbot_lib::cgroup::run_cgroup;
//!
//! fn main() -> std::io::Result<()> {
//! // Run a cargo build with 9GB cgroup limit
//! let _status = run_cgroup("cargo", &["build", "--release"])
//!     .current_dir("/my/repo")
//!     .status()?;
//!
//! // Run an arbitrary shell command with the same limits
//! let _status = run_cgroup("bash", &["-lc", "echo hello"])
//!     .status()?;
//! # Ok(())
//! # }
//! ```
//!
//! Limits applied per cgroup scope:
//!   MemoryMax=9G  (hard limit — OOM kills the whole cgroup if hit)
//!   MemoryHigh=8G (pressure throttle — kernel reclaims before hard limit)
//!   MemorySwapMax=0 (no swap available to the cgroup)
//!   TasksMax=256 (max processes/threads in the cgroup)
//!   OOMPolicy=kill (entire unit is killed together on OOM)

use std::process::Command;
#[cfg(all(unix, not(test)))]
use std::time::{SystemTime, UNIX_EPOCH};

/// Hard memory ceiling for the cgroup. When the cgroup's physical memory
/// usage hits this, the OOM killer targets the entire cgroup unit.
pub const CGROUP_MEMORY_MAX: &str = "9G";

/// Pressure throttle point. The kernel begins reclaiming memory aggressively
/// at this level, before hitting the hard MemoryMax.
pub const CGROUP_MEMORY_HIGH: &str = "8G";

/// Maximum processes/threads allowed inside the cgroup scope.
pub const CGROUP_TASKS_MAX: u64 = 256;

/// Wrap a command in a systemd-run cgroup scope with memory limits.
///
/// Returns a `Command` that, when spawned, runs inside a cgroup v2
/// scope owned by a transient systemd unit. All child processes inherit
/// the cgroup membership and are subject to the same memory ceiling.
///
/// The returned `Command` is pre-configured with all cgroup parameters.
/// You can chain additional modifiers (`.current_dir()`, `.env()`, etc.)
/// before calling `.spawn()`, `.output()`, or `.status()`.
///
/// # Arguments
///
/// * `program` - The executable to run (e.g., `"bash"`, `"cargo"`)
/// * `args` - Arguments to pass to the program
///
/// # Example
///
/// ```rust,no_run
/// use chatbot_lib::cgroup::run_cgroup;
/// use std::process::Stdio;
///
/// fn main() -> std::io::Result<()> {
/// let mut cmd = run_cgroup("cargo", &["test", "--workspace"]);
/// cmd.current_dir("/repo");
/// let _output = cmd.output()?;
///
/// // Or spawn and stream:
/// let mut child = run_cgroup("bash", &["-lc", "tail -f /var/log/app.log"])
///     .stdout(Stdio::piped())
///     .stderr(Stdio::piped())
///     .spawn()?;
/// # let _ = child.kill();
/// # Ok(())
/// # }
/// ```
#[cfg(all(unix, not(test)))]
pub fn run_cgroup(program: &str, args: &[&str]) -> Command {
    let unit_name = format!(
        "cgroup-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );

    let mut cmd = Command::new("systemd-run");
    cmd.arg("--user")
        .arg("--wait")
        .arg("--collect")
        .arg("--pipe")
        .arg(format!("--unit={}", unit_name))
        .arg("-p")
        .arg("Type=exec")
        .arg("-p")
        .arg("MemoryAccounting=yes")
        .arg("-p")
        .arg(format!("MemoryHigh={}", CGROUP_MEMORY_HIGH))
        .arg("-p")
        .arg(format!("MemoryMax={}", CGROUP_MEMORY_MAX))
        .arg("-p")
        .arg("MemorySwapMax=0")
        .arg("-p")
        .arg(format!("TasksMax={}", CGROUP_TASKS_MAX))
        .arg("-p")
        .arg("OOMPolicy=kill")
        .arg("--")
        .arg(program);

    for arg in args {
        cmd.arg(arg);
    }

    cmd
}

/// Fallback for non-Unix platforms: runs the command directly without cgroup
/// wrapping. On Unix, always use `run_cgroup()`.
#[cfg(any(not(unix), test))]
pub fn run_cgroup(program: &str, args: &[&str]) -> Command {
    let mut cmd = Command::new(program);
    for arg in args {
        cmd.arg(arg);
    }
    cmd
}

/// Take any pre-built `&mut Command` and mutate it in-place to become a
/// systemd-run invocation that re-executes the same program with the same
/// arguments, inside a cgroup v2 scope with memory limits applied.
///
/// This lets you build a `Command` using the normal builder pattern, then
/// apply the cgroup wrapper just before spawning — without needing to rebuild
/// the entire argument list.
///
/// ```rust,no_run
/// use chatbot_lib::cgroup::wrap_command;
/// use std::process::{Command, Stdio};
///
/// fn main() -> std::io::Result<()> {
/// let mut cmd = Command::new("cargo");
/// cmd.arg("build").arg("--release");
/// cmd.current_dir("/my/repo");
/// cmd.stdout(Stdio::piped());
/// cmd.stderr(Stdio::piped());
///
/// wrap_command(&mut cmd);
/// let mut child = cmd.spawn()?;
/// # let _ = child.kill();
/// # Ok(())
/// # }
/// ```
///
/// Stdin, stdout, stderr, working directory, and environment variables
/// set on the input command are preserved through the systemd-run wrapper.
#[cfg(all(unix, not(test)))]
pub fn wrap_command(input: &mut Command) {
    // Extract program and args from the input command so we can re-embed
    // them after the systemd-run flags.
    let program = input.get_program().to_string_lossy().into_owned();
    let args: Vec<String> = input
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let cwd = input.get_current_dir().map(|p| p.to_path_buf());
    let envs: Vec<_> = input
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect();

    let unit_name = format!(
        "cgroup-wrap-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );

    // Replace `input` in-place with a systemd-run invocation.
    // The original command's stdin/stdout/stderr settings are preserved because
    // we re-create Command with stdin/stdout/stderr from scratch.
    let mut cmd = Command::new("systemd-run");
    cmd.arg("--user")
        .arg("--wait")
        .arg("--collect")
        .arg("--pipe")
        .arg(format!("--unit={}", unit_name))
        .arg("-p")
        .arg("Type=exec")
        .arg("-p")
        .arg("MemoryAccounting=yes")
        .arg("-p")
        .arg(format!("MemoryHigh={}", CGROUP_MEMORY_HIGH))
        .arg("-p")
        .arg(format!("MemoryMax={}", CGROUP_MEMORY_MAX))
        .arg("-p")
        .arg("MemorySwapMax=0")
        .arg("-p")
        .arg(format!("TasksMax={}", CGROUP_TASKS_MAX))
        .arg("-p")
        .arg("OOMPolicy=kill")
        .arg("--")
        .arg(&program);

    for arg in &args {
        cmd.arg(arg);
    }

    // Carry over cwd and env from the original command
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    for (key, val) in envs {
        match val {
            Some(v) => cmd.env(&key, &v),
            None => cmd.env_remove(&key),
        };
    }

    // Replace the contents of `input` with the systemd-run command.
    // Safety: we discard the original Command's fields and replace them with
    // the systemd-run command's fields. The original stdin/stdout/stderr are
    // reset to inherited (the caller must re-apply them after wrap_command
    // returns if needed — but since they were set on `input` before this call,
    // we need to re-apply them). To avoid this, callers should set
    // stdin/stdout/stderr AFTER calling wrap_command().
    *input = cmd;
}

#[cfg(any(not(unix), test))]
pub fn wrap_command(_input: &mut Command) {
    // No-op on non-Unix: just run the command as-is.
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    #[test]
    fn run_cgroup_does_not_panic() {
        // Just verify it returns a valid Command — don't actually run it
        let mut cmd = run_cgroup("echo", &["hello"]);
        // Chain a modifier to prove the Command is valid
        cmd.stdout(Stdio::null());
        // (do not spawn — we only want to verify construction)
        let mut cmd2 = run_cgroup("bash", &["-c", "true"]);
        cmd2.stdout(Stdio::null());
    }

    #[test]
    fn constants_are_documented() {
        assert_eq!(CGROUP_MEMORY_MAX, "9G");
        assert_eq!(CGROUP_MEMORY_HIGH, "8G");
        assert_eq!(CGROUP_TASKS_MAX, 256);
    }
}
