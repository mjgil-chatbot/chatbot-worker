use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::{config, worker};
const BRIDGE_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const BRIDGE_CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const BRIDGE_CONNECT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const STDERR_TAIL_MAX_LINES: usize = 120;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct BridgeConfig {
    chat_id: String,
    socket_path: PathBuf,
    pid_path: PathBuf,
    cli: String,
    model: Option<String>,
    session_id: String,
    working_dir: Option<String>,
    extra_env: Vec<(String, String)>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BridgePromptRequest {
    prompt: String,
}
fn bridge_root_dir(chat_id: &str) -> PathBuf {
    let safe_name = worker::worker_dir(chat_id)
        .file_name()
        .map(|value| value.to_owned())
        .unwrap_or_else(|| std::ffi::OsString::from(chat_id));
    config::shared_infra_dir()
        .join("kimi-bridges")
        .join(PathBuf::from(safe_name))
}
fn config_path(chat_id: &str) -> PathBuf {
    bridge_root_dir(chat_id).join("config.json")
}
fn bridge_is_live(socket_path: &Path) -> bool {
    UnixStream::connect(socket_path).is_ok()
}
fn cleanup_stale_bridge(config: &BridgeConfig) {
    terminate_bridge_process(&config.pid_path);
    let _ = fs::remove_file(&config.socket_path);
    let _ = fs::remove_file(&config.pid_path);
}
fn terminate_bridge_process(pid_path: &Path) {
    if let Some(pid) = fs::read_to_string(pid_path)
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok())
    {
        terminate_process_group(pid);
    }
}
fn terminate_process_group(pid: i32) {
    unsafe {
        libc::killpg(pid, libc::SIGTERM);
    }
}
fn spawn_bridge_process(chat_id: &str) -> Result<()> {
    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    let cfg_path = config_path(chat_id);
    let mut child = unsafe {
        Command::new(exe)
            .arg("--kimi-bridge")
            .arg(&cfg_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .pre_exec(|| {
                libc::setsid();
                Ok(())
            })
            .spawn()
    }
    .with_context(|| format!("failed to spawn Kimi bridge for chat {chat_id}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
fn wait_for_bridge(socket_path: &Path) -> Result<()> {
    let start = Instant::now();
    while start.elapsed() < BRIDGE_CONNECT_TIMEOUT {
        if bridge_is_live(socket_path) {
            return Ok(());
        }
        std::thread::sleep(BRIDGE_CONNECT_POLL_INTERVAL);
    }
    bail!(
        "timed out waiting for Kimi bridge socket {}",
        socket_path.display()
    );
}
struct KimiWireSession {
    child: Child,
    stdin: ChildStdin,
    stdout_rx: mpsc::Receiver<Value>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    next_id: u64,
}
impl KimiWireSession {
    fn spawn(config: &BridgeConfig) -> Result<Self> {
        let mut cmd = Command::new(&config.cli);
        if let Some(working_dir) = config.working_dir.as_deref() {
            cmd.current_dir(working_dir);
            cmd.arg("--work-dir").arg(working_dir);
        }
        for (key, value) in &config.extra_env {
            cmd.env(key, value);
        }
        cmd.arg("--wire").arg("--yolo");
        if let Some(model) = config.model.as_deref() {
            cmd.arg("--model").arg(model);
        }
        cmd.arg("--resume").arg(&config.session_id);

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to spawn Kimi wire process")?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("failed to capture Kimi stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to capture Kimi stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("failed to capture Kimi stderr"))?;

        let (stdout_tx, stdout_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(json) = serde_json::from_str::<Value>(&line) {
                    if stdout_tx.send(json).is_err() {
                        break;
                    }
                }
            }
        });

        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_MAX_LINES)));
        let stderr_tail_for_thread = Arc::clone(&stderr_tail);
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                let mut tail = stderr_tail_for_thread
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if tail.len() == STDERR_TAIL_MAX_LINES {
                    tail.pop_front();
                }
                tail.push_back(line);
            }
        });

        let mut session = Self {
            child,
            stdin,
            stdout_rx,
            stderr_tail,
            next_id: 1,
        };
        session.initialize()?;
        Ok(session)
    }}
impl KimiWireSession {
    fn initialize(&mut self) -> Result<()> {
        let id = self.next_request_id("initialize");
        self.write_rpc(json!({
            "jsonrpc": "2.0",
            "method": "initialize",
            "id": id,
            "params": {
                "protocol_version": "1.10",
                "client": { "name": "rust-chatbot", "version": "1.0.0" },
                "capabilities": {
                    "supports_question": false,
                    "supports_plan_mode": false
                }
            }
        }))?;
        loop {
            let message = self
                .stdout_rx
                .recv_timeout(Duration::from_secs(10))
                .map_err(|_| anyhow!("timed out waiting for Kimi initialize response"))?;
            if message.get("id").and_then(|value| value.as_str()) == Some(id.as_str()) {
                if let Some(error) = message.get("error") {
                    bail!("Kimi wire initialize failed: {}", error);
                }
                return Ok(());
            }
        }
    }}
impl KimiWireSession {
    fn handle_request(&mut self, message: &Value) -> Result<()> {
        let Some(id) = message.get("id").and_then(|value| value.as_str()) else {
            return Ok(());
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let request_type = params
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        match request_type {
            "ApprovalRequest" => {
                let request_id = params
                    .get("payload")
                    .and_then(|payload| payload.get("id"))
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                self.write_rpc(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "request_id": request_id, "response": "approve" }
                }))?;
            }
            _ => {
                self.write_rpc(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("unsupported Kimi bridge request type: {request_type}")
                    }
                }))?;
            }
        }
        Ok(())
    }}
impl KimiWireSession {
    fn write_rpc(&mut self, value: Value) -> Result<()> {
        write_json_line(&mut self.stdin, &value).context("failed to write Kimi wire message")
    }}
impl KimiWireSession {
    fn next_request_id(&mut self, prefix: &str) -> String {
        let id = format!("{prefix}-{}", self.next_id);
        self.next_id += 1;
        id
    }}
impl KimiWireSession {
    fn bridge_timeout_error(&mut self) -> anyhow::Error {
        let stderr_tail = self
            .stderr_tail
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        let status = self.child.try_wait().ok().flatten();
        anyhow!(
            "timed out waiting for Kimi bridge output (child={status:?}){}",
            if stderr_tail.is_empty() {
                String::new()
            } else {
                format!("\n\nRecent stderr output:\n{stderr_tail}")
            }
        )
    }}
fn write_json_line<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}
fn push_unique_line(lines: &mut Vec<String>, line: String) {
    if line.is_empty() || lines.last().is_some_and(|existing| existing == &line) {
        return;
    }
    lines.push(line);
}
fn extract_output_preview(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(truncate_preview(text)),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| {
                    if part.get("type").and_then(|value| value.as_str()) == Some("text") {
                        part.get("text")
                            .and_then(|value| value.as_str())
                            .map(str::to_string)
                    } else {
                        None
                    }
                })
                .collect::<String>();
            (!text.is_empty()).then(|| truncate_preview(&text))
        }
        _ => None,
    }
}
fn format_tool_result_line(model: &Option<String>, tool_name: &str, output: &str) -> String {
    format!(
        "{} | {} -> {}",
        model_label(model),
        format_tool_name(tool_name),
        truncate_preview(output)
    )
}
fn model_label(model: &Option<String>) -> &str {
    if model.as_deref().unwrap_or_default().is_empty() {
        "Kimi"
    } else {
        "Kimi"
    }
}
fn format_tool_name(name: &str) -> String {
    let formatted = name
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if c.is_uppercase() && i > 0 {
                format!(" {}", c)
            } else if c == '_' {
                " ".to_string()
            } else {
                c.to_string()
            }
        })
        .collect::<String>();
    formatted.trim().to_string()
}
fn truncate_preview(text: &str) -> String {
    const MAX_CHARS: usize = 80;
    if text.chars().count() <= MAX_CHARS {
        return text.to_string();
    }
    text.chars().take(MAX_CHARS - 3).collect::<String>() + "..."
}
