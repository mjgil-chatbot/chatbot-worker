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

use crate::memory::{build_query_with_memory, MemoryContext};
use crate::provider_bridge::{kimi_cli_config, ProviderRequestOverrides};
use crate::stream::StreamMessage;
use crate::summary::{format_tool_input_preview, JsonSummary, ToolCallRecord};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BridgeStreamFrame {
    response: String,
    command: String,
    session_id: Option<String>,
    summary: Option<JsonSummary>,
    is_final: bool,
}

pub fn maybe_run_bridge_from_args(args: &[String]) -> bool {
    if args.len() == 3 && args[1] == "--kimi-bridge" {
        let path = PathBuf::from(&args[2]);
        if let Err(err) = run_bridge_from_config_path(&path) {
            eprintln!("Kimi bridge failed: {err:#}");
            std::process::exit(1);
        }
        return true;
    }
    false
}

pub fn call_kimi_bridge_stream(
    chat_id: &str,
    query: &str,
    system_prompt: Option<&str>,
    memory_context: Option<&MemoryContext>,
    working_dir: Option<&str>,
    resume_session_id: Option<&str>,
    overrides: Option<&ProviderRequestOverrides>,
    tx: mpsc::Sender<StreamMessage>,
    stop_signal: Arc<AtomicBool>,
) -> Result<()> {
    let config = ensure_bridge(chat_id, working_dir, resume_session_id, overrides)?;
    let mut stream = UnixStream::connect(&config.socket_path).with_context(|| {
        format!(
            "failed to connect to Kimi bridge socket {}",
            config.socket_path.display()
        )
    })?;
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .context("failed to set Kimi bridge read timeout")?;
    let request = BridgePromptRequest {
        prompt: build_bridge_prompt(system_prompt, query, memory_context),
    };
    write_json_line(&mut stream, &request).context("failed to send prompt to Kimi bridge")?;

    let reader_stream = stream
        .try_clone()
        .context("failed to clone Kimi bridge stream")?;
    let mut reader = BufReader::new(reader_stream);
    forward_bridge_stream(&mut reader, &config.pid_path, tx, stop_signal)
}

fn forward_bridge_stream<R: BufRead>(
    reader: &mut R,
    pid_path: &Path,
    tx: mpsc::Sender<StreamMessage>,
    stop_signal: Arc<AtomicBool>,
) -> Result<()> {
    let mut line = String::new();
    loop {
        line.clear();
        if stop_signal.load(Ordering::Acquire) {
            terminate_bridge_process(pid_path);
            bail!("stopped by user");
        }
        let read = match reader.read_line(&mut line) {
            Ok(read) => read,
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(err) => return Err(err).context("failed to read Kimi bridge response"),
        };
        if read == 0 {
            bail!("Kimi bridge closed before sending a final response");
        }
        let msg: BridgeStreamFrame = serde_json::from_str(line.trim_end())
            .context("failed to parse Kimi bridge stream message")?;
        let is_final = msg.is_final;
        tx.send(StreamMessage {
            response: msg.response,
            command: msg.command,
            session_id: msg.session_id,
            summary: msg.summary,
            is_final,
        })
        .map_err(|_| anyhow!("Kimi bridge stream receiver was dropped"))?;
        if is_final {
            return Ok(());
        }
    }
}

fn ensure_bridge(
    chat_id: &str,
    working_dir: Option<&str>,
    resume_session_id: Option<&str>,
    overrides: Option<&ProviderRequestOverrides>,
) -> Result<BridgeConfig> {
    let config = bridge_config(chat_id, working_dir, resume_session_id, overrides)?;
    fs::create_dir_all(bridge_root_dir(chat_id))
        .with_context(|| format!("failed to create bridge dir for chat {chat_id}"))?;
    fs::write(config_path(chat_id), serde_json::to_vec_pretty(&config)?)
        .with_context(|| format!("failed to write Kimi bridge config for chat {chat_id}"))?;

    if bridge_is_live(&config.socket_path) {
        return Ok(config);
    }

    cleanup_stale_bridge(&config);
    spawn_bridge_process(chat_id)?;
    wait_for_bridge(&config.socket_path)?;
    Ok(config)
}

fn bridge_config(
    chat_id: &str,
    working_dir: Option<&str>,
    resume_session_id: Option<&str>,
    overrides: Option<&ProviderRequestOverrides>,
) -> Result<BridgeConfig> {
    let kimi = kimi_cli_config(overrides);
    let root = bridge_root_dir(chat_id);
    let socket_path = root.join("bridge.sock");
    let pid_path = root.join("bridge.pid");
    let session_id = resume_session_id
        .map(str::to_string)
        .unwrap_or_else(|| format!("rust-chatbot-{}", uuid::Uuid::new_v4()));
    Ok(BridgeConfig {
        chat_id: chat_id.to_string(),
        socket_path,
        pid_path,
        cli: kimi.cli,
        model: kimi.model,
        session_id,
        working_dir: working_dir.map(str::to_string),
        extra_env: kimi.extra_env,
    })
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

fn run_bridge_from_config_path(path: &Path) -> Result<()> {
    let config: BridgeConfig = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("failed to read {}", path.display()))?,
    )
    .with_context(|| format!("failed to parse {}", path.display()))?;
    run_bridge_service(config)
}

fn run_bridge_service(config: BridgeConfig) -> Result<()> {
    if let Some(parent) = config.socket_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let _ = fs::remove_file(&config.socket_path);
    let listener = UnixListener::bind(&config.socket_path).with_context(|| {
        format!(
            "failed to bind Kimi bridge socket {}",
            config.socket_path.display()
        )
    })?;
    listener
        .set_nonblocking(true)
        .context("failed to set Kimi bridge listener nonblocking")?;
    fs::write(&config.pid_path, std::process::id().to_string())
        .with_context(|| format!("failed to write {}", config.pid_path.display()))?;

    let mut wire = KimiWireSession::spawn(&config)?;
    let mut last_activity = Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                last_activity = Instant::now();
                if let Err(err) = serve_bridge_client(&mut wire, &config, stream) {
                    eprintln!("Kimi bridge client failed: {err:#}");
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if last_activity.elapsed() >= BRIDGE_IDLE_TIMEOUT {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(err) => return Err(err).context("failed to accept Kimi bridge client"),
        }
    }

    let _ = fs::remove_file(&config.socket_path);
    let _ = fs::remove_file(&config.pid_path);
    Ok(())
}

fn serve_bridge_client(
    wire: &mut KimiWireSession,
    config: &BridgeConfig,
    stream: UnixStream,
) -> Result<()> {
    let request: BridgePromptRequest = {
        let mut reader = BufReader::new(
            stream
                .try_clone()
                .context("failed to clone bridge client stream")?,
        );
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .context("failed to read bridge prompt request")?;
        if read == 0 {
            return Ok(());
        }
        serde_json::from_str(line.trim_end()).context("failed to parse bridge prompt request")?
    };

    let mut writer = stream;
    wire.run_prompt(
        &request.prompt,
        &config.session_id,
        &config.model,
        &mut writer,
    )
}

fn build_bridge_prompt(
    system_prompt: Option<&str>,
    query: &str,
    memory_context: Option<&MemoryContext>,
) -> String {
    let mut sections = Vec::new();
    if let Some(prompt) = system_prompt
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("System instructions:\n{prompt}"));
    }
    let effective_query = build_query_with_memory(query, memory_context);
    if effective_query.contains('\n') {
        sections.push(format!("User:\n{}", effective_query.trim()));
    } else {
        sections.push(format!("User: {}", effective_query.trim()));
    }
    sections.join("\n\n")
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
    }

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
    }

    fn run_prompt(
        &mut self,
        prompt: &str,
        session_id: &str,
        model: &Option<String>,
        writer: &mut UnixStream,
    ) -> Result<()> {
        let prompt_id = self.next_request_id("prompt");
        self.write_rpc(json!({
            "jsonrpc": "2.0",
            "method": "prompt",
            "id": prompt_id,
            "params": { "user_input": prompt }
        }))?;

        let mut state = PromptState::new(model.clone());
        loop {
            let message = self
                .stdout_rx
                .recv_timeout(Duration::from_secs(300))
                .map_err(|_| self.bridge_timeout_error())?;
            if let Some(method) = message.get("method").and_then(|value| value.as_str()) {
                match method {
                    "event" => {
                        state.handle_event(
                            message.get("params").unwrap_or(&Value::Null),
                            session_id,
                            writer,
                        )?;
                    }
                    "request" => {
                        self.handle_request(&message)?;
                    }
                    _ => {}
                }
                continue;
            }

            if message.get("id").and_then(|value| value.as_str()) == Some(prompt_id.as_str()) {
                if let Some(error) = message.get("error") {
                    let response = format!("Kimi command failed.\n\nWire error:\n{error}");
                    let final_msg = state.final_message(response, session_id);
                    write_json_line(writer, &final_msg)?;
                    bail!("Kimi prompt failed: {}", error);
                }
                let final_msg = state.final_message(state.response_view(), session_id);
                write_json_line(writer, &final_msg)?;
                return Ok(());
            }
        }
    }

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
    }

    fn write_rpc(&mut self, value: Value) -> Result<()> {
        write_json_line(&mut self.stdin, &value).context("failed to write Kimi wire message")
    }

    fn next_request_id(&mut self, prefix: &str) -> String {
        let id = format!("{prefix}-{}", self.next_id);
        self.next_id += 1;
        id
    }

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
    }
}

struct PromptState {
    model: Option<String>,
    response_content: String,
    summary_lines: Vec<String>,
    tool_names: HashMap<String, String>,
    summary: JsonSummary,
}

impl PromptState {
    fn new(model: Option<String>) -> Self {
        let mut summary = JsonSummary::new();
        summary.model = model.clone();
        Self {
            model,
            response_content: String::new(),
            summary_lines: Vec::new(),
            tool_names: HashMap::new(),
            summary,
        }
    }

    fn handle_event(
        &mut self,
        params: &Value,
        session_id: &str,
        writer: &mut UnixStream,
    ) -> Result<()> {
        let event_type = params
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let payload = params.get("payload").unwrap_or(&Value::Null);
        self.process_event(event_type, payload);
        let msg = BridgeStreamFrame {
            response: self.response_view(),
            command: "kimi --wire".to_string(),
            session_id: Some(session_id.to_string()),
            summary: Some(self.summary.clone()),
            is_final: false,
        };
        write_json_line(writer, &msg)?;
        Ok(())
    }

    fn process_event(&mut self, event_type: &str, payload: &Value) {
        match event_type {
            "ContentPart" => {
                if payload.get("type").and_then(|value| value.as_str()) == Some("text") {
                    if let Some(text) = payload.get("text").and_then(|value| value.as_str()) {
                        self.response_content.push_str(text);
                    }
                }
            }
            "ToolCall" => {
                let tool_name = payload
                    .get("function")
                    .and_then(|function| function.get("name"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("tool");
                let arguments = payload
                    .get("function")
                    .and_then(|function| function.get("arguments"))
                    .and_then(|value| value.as_str())
                    .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                    .unwrap_or(Value::Null);
                let line = format_tool_call_line(&self.model, tool_name, &arguments);
                push_unique_line(&mut self.summary_lines, line);
                if let Some(id) = payload.get("id").and_then(|value| value.as_str()) {
                    self.tool_names
                        .insert(id.to_string(), tool_name.to_string());
                }
                if !self.summary.tools_used.iter().any(|name| name == tool_name) {
                    self.summary.tools_used.push(tool_name.to_string());
                }
                self.summary.pending_tool_calls.push(ToolCallRecord {
                    name: tool_name.to_string(),
                    input_preview: format_tool_input_preview(tool_name, &arguments),
                    ..Default::default()
                });
            }
            "ToolResult" => {
                let tool_call_id = payload
                    .get("tool_call_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                let tool_name = self
                    .tool_names
                    .get(tool_call_id)
                    .cloned()
                    .unwrap_or_else(|| "tool result".to_string());
                let output_preview = payload
                    .get("return_value")
                    .and_then(|value| value.get("output"))
                    .and_then(extract_output_preview);
                if let Some(preview) = output_preview.filter(|value| !value.trim().is_empty()) {
                    push_unique_line(
                        &mut self.summary_lines,
                        format_tool_result_line(&self.model, &tool_name, &preview),
                    );
                }
            }
            "StatusUpdate" => {
                if let Some(token_usage) = payload.get("token_usage") {
                    self.summary.total_input_tokens = token_usage
                        .get("input_other")
                        .and_then(|value| value.as_u64())
                        .unwrap_or(0);
                    self.summary.total_output_tokens = token_usage
                        .get("output")
                        .and_then(|value| value.as_u64())
                        .unwrap_or(0);
                    self.summary.cache_read_tokens = token_usage
                        .get("input_cache_read")
                        .and_then(|value| value.as_u64())
                        .unwrap_or(0);
                    self.summary.cache_creation_tokens = token_usage
                        .get("input_cache_creation")
                        .and_then(|value| value.as_u64())
                        .unwrap_or(0);
                }
            }
            "SubagentEvent" => {
                if let Some(nested) = payload.get("event") {
                    let nested_type = nested
                        .get("type")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default();
                    let nested_payload = nested.get("payload").unwrap_or(&Value::Null);
                    self.process_event(nested_type, nested_payload);
                }
            }
            _ => {}
        }
    }

    fn response_view(&self) -> String {
        let summary = self.summary_lines.join("\n");
        match (
            summary.trim().is_empty(),
            self.response_content.trim().is_empty(),
        ) {
            (false, false) => format!("{summary}\n\n{}", self.response_content),
            (false, true) => summary,
            (true, false) => self.response_content.clone(),
            (true, true) => String::new(),
        }
    }

    fn final_message(&mut self, response: String, session_id: &str) -> BridgeStreamFrame {
        self.summary.response_count = 1;
        self.summary.session_id = Some(session_id.to_string());
        BridgeStreamFrame {
            response,
            command: "kimi --wire".to_string(),
            session_id: Some(session_id.to_string()),
            summary: Some(self.summary.clone()),
            is_final: true,
        }
    }
}

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

fn format_tool_call_line(model: &Option<String>, tool_name: &str, arguments: &Value) -> String {
    let preview = format_tool_input_preview(tool_name, arguments);
    if preview.is_empty() {
        format!("{} | {}", model_label(model), format_tool_name(tool_name))
    } else {
        format!(
            "{} | {} {}",
            model_label(model),
            format_tool_name(tool_name),
            preview
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::CommandExt;
    use std::process::Command;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;

    #[test]
    fn bridge_prompt_includes_system_prompt_and_memory() {
        let memory = MemoryContext {
            workspace_id: "ws".to_string(),
            query: "fix".to_string(),
            hits: vec![crate::memory::MemoryHit {
                doc_key: "doc".to_string(),
                chat_id: "chat".to_string(),
                role: "assistant".to_string(),
                session_name: "session".to_string(),
                timestamp: chrono::Utc::now(),
                snippet: "remember this".to_string(),
                title: None,
                message_index: None,
                turn_index: None,
                score: None,
            }],
        };
        let prompt =
            build_bridge_prompt(Some("Follow system rules."), "Inspect repo", Some(&memory));
        assert!(prompt.contains("System instructions:\nFollow system rules."));
        assert!(prompt.contains("Retrieved memory:"));
        assert!(prompt.contains("User request:\nInspect repo"));
    }

    #[test]
    fn subagent_tool_events_are_folded_into_live_summary() {
        let mut state = PromptState::new(Some("kimi-code/kimi-for-coding".to_string()));
        state.process_event(
            "SubagentEvent",
            &json!({
                "event": {
                    "type": "ToolCall",
                    "payload": {
                        "id": "tc-1",
                        "function": {
                            "name": "Shell",
                            "arguments": "{\"command\":\"pwd\"}"
                        }
                    }
                }
            }),
        );
        assert!(state.response_view().contains("Kimi | Shell"));
        assert_eq!(state.summary.tools_used, vec!["Shell".to_string()]);
    }

    #[test]
    fn stop_signal_terminates_bridge_process_group() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pid_path = temp.path().join("bridge.pid");
        let mut child = unsafe {
            Command::new("sleep")
                .arg("120")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .pre_exec(|| {
                    libc::setsid();
                    Ok(())
                })
                .spawn()
                .expect("spawn sleep")
        };
        std::fs::write(&pid_path, child.id().to_string()).expect("write pid");

        let (stream_a, _stream_b) = UnixStream::pair().expect("unix stream pair");
        stream_a
            .set_read_timeout(Some(Duration::from_millis(50)))
            .expect("set read timeout");
        let mut reader = BufReader::new(stream_a);
        let (tx, _rx) = mpsc::channel();
        let stop_signal = Arc::new(AtomicBool::new(true));

        let err = forward_bridge_stream(&mut reader, &pid_path, tx, stop_signal)
            .expect_err("stop should abort bridge stream");
        assert!(err.to_string().contains("stopped by user"));

        let status = child.wait().expect("wait for terminated child");
        assert!(
            !status.success(),
            "expected stop to terminate the bridge process group"
        );
    }
}
