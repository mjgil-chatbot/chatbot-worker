use super::*;
use crate::config;
use crate::storage::atomic::atomic_write;
const PREALLOCATED_SESSION_NAME_PREFIX: &str = "__rust_chatbot_preallocated__:";
impl DataManager {
    fn uses_global_session_index(&self) -> bool {
        dirs::home_dir()
            .map(|home| config::resolve_data_dir_under_home(self.provider, &home))
            .map(|canonical| canonical == self.data_dir)
            .unwrap_or(false)
    }}
impl DataManager {
    pub fn new(provider: ModelProvider) -> Result<Self> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
        let canonical_data_dir = config::canonical_provider_data_dir_under_home(provider, &home);
        let data_dir = config::resolve_data_dir_under_home(provider, &home);

        fs::create_dir_all(&data_dir)?;
        fs::create_dir_all(data_dir.join("active"))?;
        fs::create_dir_all(data_dir.join("sessions"))?;
        fs::create_dir_all(data_dir.join("height_cache"))?;
        if data_dir != canonical_data_dir {
            info!(
                provider=%provider.as_str(),
                canonical_data_dir=%canonical_data_dir.display(),
                resolved_data_dir=%data_dir.display(),
                "using legacy provider data directory"
            );
        }
        info!(data_dir=%data_dir.display(), "data directory ready");

        Ok(Self { data_dir, provider })
    }}
impl DataManager {
    pub fn load_session(&self, session_id: &str) -> Result<ChatSession> {
        self.read_session(session_id)
    }}
impl DataManager {
    fn read_session(&self, session_id: &str) -> Result<ChatSession> {
        let session_path = self.session_path(session_id);
        let session_json = fs::read_to_string(&session_path)?;
        let mut session: ChatSession = serde_json::from_str(&session_json)?;
        self.provider.normalize_loaded_session(&mut session);
        debug!(
            session_id=%session_id,
            session_name=%session.name,
            path=%session_path.display(),
            total_messages=session.messages.len(),
            has_system_prompt=session
                .system_prompt
                .as_deref()
                .map(|prompt| !prompt.trim().is_empty())
                .unwrap_or(false),
            last_message_at=?session.messages.last().map(|message| message.timestamp),
            "session loaded"
        );
        Ok(session)
    }}
impl DataManager {
    fn log_session_write_request(
        &self,
        write_source: &str,
        session: &ChatSession,
        preserve_existing_name: bool,
    ) {
        let snapshot = session_log_snapshot(session);
        debug!(
            write_source,
            session_id=%session.id,
            session_name=%session.name,
            preserve_existing_name,
            total_messages=snapshot.total_messages,
            user_messages=snapshot.user_messages,
            assistant_messages=snapshot.assistant_messages,
            last_message_at=?snapshot.last_message_at,
            last_is_user=session.messages.last().map(|message| message.is_user).unwrap_or(false),
            history_fingerprint=%format!("{:016x}", snapshot.history_fingerprint),
            "session write requested"
        );
    }}
impl DataManager {
    pub fn load_height_cache(&self, session_id: &str) -> Result<Option<SessionHeightCache>> {
        let cache_path = self
            .data_dir
            .join("height_cache")
            .join(format!("{}.json", session_id));
        match fs::read(&cache_path) {
            Ok(cache_bytes) => {
                let cache: SessionHeightCache = serde_json::from_slice(&cache_bytes)?;
                Ok(Some(cache))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }}
impl DataManager {
    pub fn save_height_cache(&self, cache: &SessionHeightCache) -> Result<()> {
        let cache_path = self
            .data_dir
            .join("height_cache")
            .join(format!("{}.json", cache.session_id));
        let cache_json = serde_json::to_vec(cache)?;
        let _ = atomic_write(&cache_path, &cache_json)?;
        debug!(session_id=%cache.session_id, path=%cache_path.display(), "height cache saved");
        Ok(())
    }}
impl DataManager {
    /// Scans the sessions directory for orphaned files not yet in the index
    /// and appends them to the sessions list.
    fn discover_orphaned_sessions(
        &self,
        sessions: &mut Vec<SessionInfo>,
        indexed_ids: &HashSet<String>,
    ) {
        let sessions_dir = self.data_dir.join("sessions");
        let Some(entries) = fs::read_dir(&sessions_dir).ok() else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let file_stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            if indexed_ids.contains(&file_stem) {
                continue;
            }
            // Orphaned file — read it to get session info.
            debug!(path=%path.display(), "found orphaned session file, reading for index");
            let raw = match fs::read_to_string(&path) {
                Ok(raw) => raw,
                Err(_) => continue,
            };
            let mut session: ChatSession = match serde_json::from_str(&raw) {
                Ok(s) => s,
                Err(_) => continue,
            };
            self.provider.normalize_loaded_session(&mut session);
            sessions.push(SessionInfo {
                id: session.id.clone(),
                name: session.name.clone(),
                created_at: session.created_at,
                updated_at: session.updated_at,
                message_count: session.messages.iter().filter(|m| m.is_user).count(),
                hidden: false,
            });
        }
    }}
impl DataManager {
    /// Original list_sessions implementation using metadata index + file scan.
    fn list_sessions_fallback(&self) -> Result<Vec<SessionInfo>> {
        let metadata = self.load_metadata()?;

        // Fast path: use metadata index as the primary source.
        // save_session() already keeps this up-to-date, so we only need
        // to fall back to a full file scan for orphaned session files
        // that were never indexed (e.g. created by older versions).
        if !metadata.sessions.is_empty() {
            let indexed_ids: HashSet<String> = metadata
                .sessions
                .keys()
                .cloned()
                .collect::<HashSet<String>>();
            let mut sessions: Vec<SessionInfo> = metadata
                .sessions
                .values()
                .filter(|info| !info.hidden)
                .cloned()
                .collect();

            // Check for orphaned session files not in the index.
            self.discover_orphaned_sessions(&mut sessions, &indexed_ids);

            sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            debug!(
                count = sessions.len(),
                "sessions listed (from metadata index)"
            );
            return Ok(sessions);
        }

        // Slow path: no metadata index yet — full file scan (first run only).
        let sessions_dir = self.data_dir.join("sessions");
        let mut sessions = Vec::new();

        if let Ok(entries) = fs::read_dir(&sessions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                    continue;
                }

                let raw = match fs::read_to_string(&path) {
                    Ok(raw) => raw,
                    Err(err) => {
                        debug!(path=%path.display(), error=%err, "skipping unreadable session file during list_sessions");
                        continue;
                    }
                };
                let mut session: ChatSession = match serde_json::from_str(&raw) {
                    Ok(session) => session,
                    Err(err) => {
                        debug!(path=%path.display(), error=%err, "skipping invalid session file during list_sessions");
                        continue;
                    }
                };
                self.provider.normalize_loaded_session(&mut session);

                sessions.push(SessionInfo {
                    id: session.id.clone(),
                    name: session.name.clone(),
                    created_at: session.created_at,
                    updated_at: session.updated_at,
                    message_count: session.messages.iter().filter(|m| m.is_user).count(),
                    hidden: false,
                });
            }
        }

        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        debug!(count = sessions.len(), "sessions listed (full scan)");
        Ok(sessions)
    }}
impl DataManager {
    fn load_metadata(&self) -> Result<SessionMetadata> {
        let metadata_path = self.data_dir.join("sessions.json");
        if !metadata_path.exists() {
            return Ok(SessionMetadata::default());
        }

        let metadata_json = fs::read_to_string(&metadata_path)?;
        let metadata: SessionMetadata = serde_json::from_str(&metadata_json)?;
        debug!(path=%metadata_path.display(), "metadata loaded");
        Ok(metadata)
    }}
impl DataManager {
    fn save_metadata(&self, metadata: &SessionMetadata) -> Result<()> {
        let metadata_path = self.data_dir.join("sessions.json");
        let metadata_json = serde_json::to_string_pretty(metadata)?;
        let _ = atomic_write(&metadata_path, metadata_json.as_bytes())?;
        debug!(path=%metadata_path.display(), "metadata saved");
        Ok(())
    }}
impl DataManager {
    fn session_path(&self, session_id: &str) -> PathBuf {
        self.data_dir
            .join("sessions")
            .join(format!("{}.json", session_id))
    }}
impl DataManager {
    fn lock_metadata_exclusive(&self) -> Result<MetadataLock> {
        let lock_path = self.data_dir.join("sessions.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(lock_path)?;
        file.lock_exclusive()?;
        Ok(MetadataLock(file))
    }}
fn search_query_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|term| term.to_lowercase())
        .filter(|term| !term.is_empty())
        .collect()
}
fn preallocated_session_name(session_id: &str) -> String {
    format!("{PREALLOCATED_SESSION_NAME_PREFIX}{session_id}")
}
fn is_preallocated_session_info(info: &SessionInfo) -> bool {
    info.hidden && info.name.starts_with(PREALLOCATED_SESSION_NAME_PREFIX)
}
fn is_preallocated_session(session: &ChatSession) -> bool {
    session.name.starts_with(PREALLOCATED_SESSION_NAME_PREFIX)
        && session.system_prompt.is_none()
        && session.messages.is_empty()
        && session.claude_session_id.is_none()
        && session.minimax_session_id.is_none()
        && session.qwen_session_id.is_none()
        && session.codex_session_id.is_none()
        && session.nvidia_session_id.is_none()
        && session.gemini_session_id.is_none()
        && session.openrouter_session_id.is_none()
}
fn session_matches_search_terms(session: &ChatSession, query_terms: &[String]) -> bool {
    let mut corpus = String::new();
    corpus.push_str(&session.name);
    corpus.push('\n');

    if let Some(system_prompt) = session.system_prompt.as_ref() {
        corpus.push_str(system_prompt);
        corpus.push('\n');
    }

    for message in &session.messages {
        corpus.push_str(&message.content);
        corpus.push('\n');
        if let Some(command) = message.command.as_ref() {
            corpus.push_str(command);
            corpus.push('\n');
        }
    }

    let corpus = corpus.to_lowercase();
    query_terms.iter().all(|term| corpus.contains(term))
}
