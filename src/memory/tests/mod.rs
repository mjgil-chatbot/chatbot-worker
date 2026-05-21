use super::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};

mod agentic_memory;
mod ingest;
mod manual_memory;
mod retrieval;

/// Guard that holds the crate-level env lock and overrides HOME to a temp
/// directory for the duration of the test. Keeps the TempDir alive so the
/// path stays valid until the guard is dropped.
pub(super) struct MemTestGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    _home_dir: tempfile::TempDir,
    _prev_home: Option<std::ffi::OsString>,
}

impl MemTestGuard {
    pub(super) fn new() -> Self {
        let lock = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let home_dir = tempfile::TempDir::new().unwrap();
        let prev = std::env::var_os("HOME");
        std::env::set_var("HOME", home_dir.path());
        Self {
            _lock: lock,
            _home_dir: home_dir,
            _prev_home: prev,
        }
    }
}

impl Drop for MemTestGuard {
    fn drop(&mut self) {
        match self._prev_home.as_ref() {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}

pub(super) fn temp_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("chatbot-memory-test-{}-{}", name, nanos))
}

pub(super) fn make_session(
    workspace: &Path,
    id: &str,
    name: &str,
    messages: Vec<(&str, bool, Option<&str>)>,
) -> ChatSession {
    let created_at = Utc.with_ymd_and_hms(2026, 3, 25, 12, 0, 0).unwrap();
    let mut session = ChatSession {
        id: id.to_string(),
        name: name.to_string(),
        system_prompt: None,
        json_summary: None,
        claude_session_id: Some("claude-session".to_string()),
        minimax_session_id: None,
        qwen_session_id: None,
        codex_session_id: None,
        nvidia_session_id: None,
        gemini_session_id: None,
        openrouter_session_id: None,
        omniforge_session_id: None,
        omniforge_model_id: None,
        kimi_session_id: None,
        opencode_session_id: None,
        hermes_session_id: None,
        forge_session_id: None,
        claw_session_id: None,
        launched_from: Some(workspace.to_string_lossy().into_owned()),
        runsc_dirs: Vec::new(),
        runsc_image: None,
        created_at,
        updated_at: created_at,
        messages: Vec::new(),
    };

    for (index, (content, is_user, command)) in messages.into_iter().enumerate() {
        let timestamp = created_at + chrono::Duration::minutes(index as i64);
        session.messages.push(ChatMessage {
            content: content.to_string(),
            is_user,
            timestamp,
            command: command.map(str::to_string),
            retrieved_memory: None,
        });
        session.updated_at = timestamp;
    }

    session
}

#[test]
fn format_retrieved_memory_preview_matches_prompt_lines() {
    let context = MemoryContext {
        workspace_id: "ws".to_string(),
        query: "fix".to_string(),
        hits: vec![MemoryHit {
            doc_key: "chat:0:assistant".to_string(),
            chat_id: "chat".to_string(),
            role: "assistant".to_string(),
            session_name: "session".to_string(),
            timestamp: Utc.with_ymd_and_hms(2026, 5, 19, 12, 0, 0).unwrap(),
            snippet: "remember the final retry behavior".to_string(),
            title: None,
            message_index: None,
            turn_index: None,
            score: None,
        }],
    };

    assert_eq!(
        format_retrieved_memory_preview(Some(&context)).as_deref(),
        Some(
            "- [session=session role=assistant time=2026-05-19T12:00:00+00:00] remember the final retry behavior"
        )
    );
}

#[derive(Clone)]
struct StoredEpisode {
    uid: String,
    content: String,
    created_at: String,
    producer_id: String,
    producer_role: String,
    metadata: BTreeMap<String, Value>,
}

#[derive(Default)]
struct MockMemMachineState {
    next_uid: usize,
    projects: HashMap<String, Vec<StoredEpisode>>,
}

pub(super) struct MockMemMachineServer {
    state: std::sync::Arc<std::sync::Mutex<MockMemMachineState>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    join_handle: Option<std::thread::JoinHandle<()>>,
    pub(super) base_url: String,
}

impl MockMemMachineServer {
    pub(super) fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let base_url = format!("http://{}", address);
        write_memmachine_config(&base_url, "memory-tests");

        let state = std::sync::Arc::new(std::sync::Mutex::new(MockMemMachineState::default()));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_state = std::sync::Arc::clone(&state);
        let thread_stop = std::sync::Arc::clone(&stop);
        let join_handle = std::thread::spawn(move || {
            while !thread_stop.load(std::sync::atomic::Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        // Accepted sockets inherit O_NONBLOCK on macOS.
                        stream.set_nonblocking(false).unwrap();
                        handle_connection(stream, &thread_state);
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            state,
            stop,
            join_handle: Some(join_handle),
            base_url,
        }
    }

    pub(super) fn episode_count(&self, project_id: &str) -> usize {
        self.state
            .lock()
            .unwrap()
            .projects
            .get(project_id)
            .map(Vec::len)
            .unwrap_or(0)
    }
}

impl Drop for MockMemMachineServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = TcpStream::connect(self.base_url.trim_start_matches("http://"))
            .and_then(|stream| stream.shutdown(Shutdown::Both));
        if let Some(join_handle) = self.join_handle.take() {
            let _ = join_handle.join();
        }
    }
}

fn write_memmachine_config(base_url: &str, org_id: &str) {
    let home = PathBuf::from(std::env::var("HOME").unwrap());
    let config_dir = home.join(".rust-chatbot");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        format!(
            "[memory]\nmemmachine_base_url = \"{base_url}\"\nmemmachine_org_id = \"{org_id}\"\n"
        ),
    )
    .unwrap();
    crate::config::reset_config();
}

fn handle_connection(
    mut stream: TcpStream,
    state: &std::sync::Arc<std::sync::Mutex<MockMemMachineState>>,
) {
    let Ok((path, body)) = read_request(&mut stream) else {
        return;
    };
    let request = if body.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str::<Value>(&body).unwrap_or_else(|_| json!({}))
    };

    match path.as_str() {
        "/api/v2/projects/get" => {
            let project_id = request["project_id"].as_str().unwrap_or_default();
            let exists = state.lock().unwrap().projects.contains_key(project_id);
            if exists {
                write_json_response(
                    &mut stream,
                    200,
                    json!({
                        "org_id": request["org_id"].as_str().unwrap_or_default(),
                        "project_id": project_id,
                        "description": "",
                        "config": {"embedder": "", "reranker": ""}
                    }),
                );
            } else {
                write_json_response(&mut stream, 404, json!({"message": "missing"}));
            }
        }
        "/api/v2/projects" => {
            let project_id = request["project_id"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            state
                .lock()
                .unwrap()
                .projects
                .entry(project_id.clone())
                .or_default();
            write_json_response(
                &mut stream,
                201,
                json!({
                    "org_id": request["org_id"].as_str().unwrap_or_default(),
                    "project_id": project_id,
                    "description": request["description"].as_str().unwrap_or_default(),
                    "config": {"embedder": "", "reranker": ""}
                }),
            );
        }
        "/api/v2/projects/delete" => {
            let project_id = request["project_id"].as_str().unwrap_or_default();
            state.lock().unwrap().projects.remove(project_id);
            write_empty_response(&mut stream, 204);
        }
        "/api/v2/memories" => {
            let project_id = request["project_id"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let mut guard = state.lock().unwrap();
            let mut results = Vec::new();
            for message in request["messages"].as_array().cloned().unwrap_or_default() {
                guard.next_uid += 1;
                let uid = format!("ep-{}", guard.next_uid);
                guard
                    .projects
                    .entry(project_id.clone())
                    .or_default()
                    .push(StoredEpisode {
                        uid: uid.clone(),
                        content: message["content"].as_str().unwrap_or_default().to_string(),
                        created_at: message["timestamp"]
                            .as_str()
                            .unwrap_or("1970-01-01T00:00:00Z")
                            .to_string(),
                        producer_id: message["producer"].as_str().unwrap_or("user").to_string(),
                        producer_role: message["role"].as_str().unwrap_or("user").to_string(),
                        metadata: message["metadata"]
                            .as_object()
                            .map(|entries| {
                                entries
                                    .iter()
                                    .map(|(key, value)| (key.clone(), value.clone()))
                                    .collect::<BTreeMap<_, _>>()
                            })
                            .unwrap_or_default(),
                    });
                results.push(json!({ "uid": uid }));
            }
            write_json_response(&mut stream, 200, json!({ "results": results }));
        }
        "/api/v2/memories/search" => {
            let project_id = request["project_id"].as_str().unwrap_or_default();
            let query = request["query"].as_str().unwrap_or_default();
            let guard = state.lock().unwrap();
            let Some(project) = guard.projects.get(project_id) else {
                write_json_response(&mut stream, 404, json!({"message": "missing"}));
                return;
            };
            let query_terms = query
                .to_ascii_lowercase()
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            let episodes = project
                .iter()
                .filter(|episode| {
                    let content = episode.content.to_ascii_lowercase();
                    query_terms.iter().all(|term| content.contains(term))
                })
                .map(|episode| {
                    json!({
                        "uid": episode.uid,
                        "content": episode.content,
                        "created_at": episode.created_at,
                        "producer_id": episode.producer_id,
                        "producer_role": episode.producer_role,
                        "metadata": episode.metadata,
                        "score": 0.9
                    })
                })
                .collect::<Vec<_>>();
            write_json_response(
                &mut stream,
                200,
                json!({
                    "status": 0,
                    "content": {
                        "episodic_memory": {
                            "long_term_memory": { "episodes": episodes },
                            "short_term_memory": { "episodes": [], "episode_summary": [] }
                        }
                    }
                }),
            );
        }
        _ => write_json_response(&mut stream, 404, json!({"message": "unsupported"})),
    }
}

fn read_request(stream: &mut TcpStream) -> Result<(String, String), std::io::Error> {
    let mut buffer = Vec::new();
    let mut headers_end = None;
    while headers_end.is_none() {
        let mut chunk = [0_u8; 1024];
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..count]);
        headers_end = buffer.windows(4).position(|window| window == b"\r\n\r\n");
    }

    let headers_end = headers_end.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "incomplete mock HTTP headers",
        )
    })?;
    let head = String::from_utf8_lossy(&buffer[..headers_end]).to_string();
    let path = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string();
    let content_length = head
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(key, value)| {
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
        })
        .unwrap_or(0);
    let body_offset = headers_end.saturating_add(4);
    let mut body = buffer[body_offset..].to_vec();
    while body.len() < content_length {
        let mut chunk = vec![0_u8; content_length - body.len()];
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..count]);
    }
    Ok((path, String::from_utf8_lossy(&body).to_string()))
}

fn write_json_response(stream: &mut TcpStream, status: u16, body: Value) {
    let payload = body.to_string();
    let response = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        payload.len(),
        payload
    );
    let _ = stream.write_all(response.as_bytes());
}

fn write_empty_response(stream: &mut TcpStream, status: u16) {
    let response =
        format!("HTTP/1.1 {status} OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let _ = stream.write_all(response.as_bytes());
}

#[test]
fn mock_memory_server_waits_for_delayed_request_bytes() {
    let _guard = MemTestGuard::new();
    let server = MockMemMachineServer::start();
    let mut client = TcpStream::connect(server.base_url.trim_start_matches("http://")).unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    let body = r#"{"project_id":"delayed"}"#;
    write!(client, "POST /api/v2/projects/get HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 404"), "{response}");
}

#[test]
fn mock_memory_reader_rejects_empty_shutdown_connections() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let (mut accepted, _) = listener.accept().unwrap();
    assert_eq!(
        read_request(&mut accepted).unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
}
