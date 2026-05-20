use super::*;
use crate::config;
use crate::omniforge;
use crate::storage;
use crate::storage::atomic::atomic_write;

const PREALLOCATED_SESSION_NAME_PREFIX: &str = "__rust_chatbot_preallocated__:";

impl DataManager {
    fn uses_global_session_index(&self) -> bool {
        dirs::home_dir()
            .map(|home| config::resolve_data_dir_under_home(self.provider, &home))
            .map(|canonical| canonical == self.data_dir)
            .unwrap_or(false)
    }

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
    }

    pub fn create_session(
        &self,
        name: String,
        system_prompt: Option<String>,
    ) -> Result<ChatSession> {
        if let Some(session) =
            self.checkout_preallocated_session(name.clone(), system_prompt.clone())?
        {
            return Ok(session);
        }

        let now = Utc::now();
        let omniforge_model_id = (self.provider == ModelProvider::OmniForge)
            .then(|| omniforge::DEFAULT_MODEL_ID.to_string());
        let session = ChatSession {
            id: Uuid::new_v4().to_string(),
            name,
            system_prompt,
            json_summary: None,
            claude_session_id: None,
            minimax_session_id: None,
            qwen_session_id: None,
            codex_session_id: None,
            nvidia_session_id: None,
            gemini_session_id: None,
            openrouter_session_id: None,
            omniforge_session_id: None,
            omniforge_model_id,
            kimi_session_id: None,
            opencode_session_id: None,
            hermes_session_id: None,
            forge_session_id: None,
            claw_session_id: None,
            launched_from: std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().into_owned()),
            runsc_dirs: Vec::new(),
            runsc_image: None,
            created_at: now,
            updated_at: now,
            messages: Vec::new(),
        };

        let snapshot = session_log_snapshot(&session);
        let history_fingerprint = format!("{:016x}", snapshot.history_fingerprint);
        info!(
            session_id=%session.id,
            session_name=%session.name,
            total_messages=snapshot.total_messages,
            user_messages=snapshot.user_messages,
            assistant_messages=snapshot.assistant_messages,
            first_message_at=?snapshot.first_message_at,
            last_message_at=?snapshot.last_message_at,
            history_fingerprint=%history_fingerprint,
            "creating session"
        );
        self.save_session(&session)?;
        Ok(session)
    }

    pub fn ensure_preallocated_sessions(&self, minimum_available: usize) -> Result<()> {
        if minimum_available == 0 {
            return Ok(());
        }

        let _lock = self.lock_metadata_exclusive()?;
        let mut metadata = self.load_metadata()?;
        let mut available = 0usize;

        for session_id in metadata
            .sessions
            .values()
            .filter(|info| is_preallocated_session_info(info))
            .map(|info| info.id.clone())
        {
            let is_valid = self
                .load_session(&session_id)
                .map(|session| is_preallocated_session(&session))
                .unwrap_or(false);
            if is_valid {
                available += 1;
            }
        }

        while available < minimum_available {
            let now = Utc::now();
            let session_id = Uuid::new_v4().to_string();
            let omniforge_model_id = (self.provider == ModelProvider::OmniForge)
                .then(|| omniforge::DEFAULT_MODEL_ID.to_string());
            let session = ChatSession {
                id: session_id.clone(),
                name: preallocated_session_name(&session_id),
                system_prompt: None,
                json_summary: None,
                claude_session_id: None,
                minimax_session_id: None,
                qwen_session_id: None,
                codex_session_id: None,
                nvidia_session_id: None,
                gemini_session_id: None,
                openrouter_session_id: None,
                omniforge_session_id: None,
                omniforge_model_id,
                kimi_session_id: None,
                opencode_session_id: None,
                hermes_session_id: None,
                forge_session_id: None,
                claw_session_id: None,
                launched_from: None,
                runsc_dirs: Vec::new(),
                runsc_image: None,
                created_at: now,
                updated_at: now,
                messages: Vec::new(),
            };

            info!(session_id=%session.id, "creating preallocated hidden session");
            self.persist_session_locked(&session, &mut metadata, true, false)?;
            available += 1;
        }

        Ok(())
    }

    pub fn checkout_preallocated_session(
        &self,
        name: String,
        system_prompt: Option<String>,
    ) -> Result<Option<ChatSession>> {
        let _lock = self.lock_metadata_exclusive()?;
        let mut metadata = self.load_metadata()?;
        let mut candidates: Vec<SessionInfo> = metadata
            .sessions
            .values()
            .filter(|info| is_preallocated_session_info(info))
            .cloned()
            .collect();
        candidates.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        for info in candidates {
            let Ok(mut session) = self.load_session(&info.id) else {
                continue;
            };
            if !is_preallocated_session(&session) {
                continue;
            }

            let now = Utc::now();
            session.name = name.clone();
            session.system_prompt = system_prompt.clone();
            session.created_at = now;
            session.updated_at = now;
            session.launched_from = std::env::current_dir()
                .ok()
                .map(|path| path.to_string_lossy().into_owned());
            if self.provider == ModelProvider::OmniForge && session.omniforge_model_id.is_none() {
                session.omniforge_model_id = Some(omniforge::DEFAULT_MODEL_ID.to_string());
            }

            info!(session_id=%session.id, session_name=%session.name, "checked out preallocated session");
            self.persist_session_locked(&session, &mut metadata, false, false)?;
            return Ok(Some(session));
        }

        Ok(None)
    }

    pub fn save_session(&self, session: &ChatSession) -> Result<()> {
        self.save_session_with_source(session, true, "chatbot_worker.save_session")
    }

    pub fn save_session_with_source(
        &self,
        session: &ChatSession,
        preserve_existing_name: bool,
        write_source: &'static str,
    ) -> Result<()> {
        let _lock = self.lock_metadata_exclusive()?;
        let mut metadata = self.load_metadata()?;
        self.log_session_write_request(write_source, session, preserve_existing_name);
        self.persist_session_locked(session, &mut metadata, false, preserve_existing_name)
    }

    pub fn mutate_session<R, F>(
        &self,
        session_id: &str,
        preserve_existing_name: bool,
        mutate: F,
    ) -> Result<R>
    where
        F: FnOnce(&mut ChatSession) -> R,
    {
        self.mutate_session_with_source(
            session_id,
            preserve_existing_name,
            "chatbot_worker.mutate_session",
            mutate,
        )
    }

    pub fn mutate_session_with_source<R, F>(
        &self,
        session_id: &str,
        preserve_existing_name: bool,
        write_source: &'static str,
        mutate: F,
    ) -> Result<R>
    where
        F: FnOnce(&mut ChatSession) -> R,
    {
        let _lock = self.lock_metadata_exclusive()?;
        let mut metadata = self.load_metadata()?;
        let mut session = self.read_session(session_id)?;
        let result = mutate(&mut session);
        self.log_session_write_request(write_source, &session, preserve_existing_name);
        self.persist_session_locked(&session, &mut metadata, false, preserve_existing_name)?;
        Ok(result)
    }

    pub fn load_session(&self, session_id: &str) -> Result<ChatSession> {
        self.read_session(session_id)
    }

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
    }

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
    }

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
    }

    pub fn save_height_cache(&self, cache: &SessionHeightCache) -> Result<()> {
        let cache_path = self
            .data_dir
            .join("height_cache")
            .join(format!("{}.json", cache.session_id));
        let cache_json = serde_json::to_vec(cache)?;
        let _ = atomic_write(&cache_path, &cache_json)?;
        debug!(session_id=%cache.session_id, path=%cache_path.display(), "height cache saved");
        Ok(())
    }

    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        let _lock = self.lock_metadata_exclusive()?;
        let mut metadata = self.load_metadata()?;
        if let Some(session_info) = metadata.sessions.get_mut(session_id) {
            session_info.hidden = true;
            info!(session_id=%session_id, session_name=%session_info.name, "session hidden (soft delete)");
        } else {
            info!(session_id=%session_id, "session not found in metadata during soft delete");
        }
        self.save_metadata(&metadata)?;

        // Dual-write hidden flag to SQLite index.
        if self.uses_global_session_index() {
            if let Err(err) = storage::hide_session(session_id) {
                warn!(
                    session_id=%session_id,
                    error=%err,
                    "failed to mark session hidden in SQLite (file write succeeded)"
                );
            }
        }

        Ok(())
    }

    pub fn rename_session(&self, session_id: &str, new_name: String) -> Result<()> {
        let mut session = self.load_session(session_id)?;
        session.name = new_name;
        session.updated_at = Utc::now();
        info!(session_id=%session_id, "session renamed");
        self.save_session_with_source(&session, false, "chatbot_worker.rename_session")?;
        Ok(())
    }

    pub fn search_session_content(&self, query: &str) -> Result<HashSet<String>> {
        let query_terms = search_query_terms(query);
        if query_terms.is_empty() {
            return Ok(HashSet::new());
        }

        // Fast path: try SQLite FTS index first.
        if self.uses_global_session_index() {
            if let Some(index) = storage::get_index() {
                if let Ok(conn) = index.conn.lock() {
                    match storage::index::search_sessions_fts(&conn, query) {
                        Ok(ids) => {
                            debug!(
                                query=%query,
                                count=ids.len(),
                                "search_session_content: FTS index hit"
                            );
                            return Ok(ids.into_iter().collect());
                        }
                        Err(err) => {
                            warn!(
                                error=%err,
                                "search_session_content: FTS query failed, falling back to file scan"
                            );
                        }
                    }
                }
            }
        }

        // Fallback: scan session files directly.
        self.search_session_content_fallback(&query_terms)
    }

    fn search_session_content_fallback(&self, query_terms: &[String]) -> Result<HashSet<String>> {
        let sessions_dir = self.data_dir.join("sessions");
        let mut matched = HashSet::new();
        let mut errors = Vec::new();

        let entries = match std::fs::read_dir(&sessions_dir) {
            Ok(e) => e,
            Err(err) => {
                warn!(
                    "search_session_content: failed to read sessions dir: {}",
                    err
                );
                return Ok(HashSet::new());
            }
        };
        for entry in entries {
            match entry {
                Ok(entry) => {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("json") {
                        continue;
                    }
                    let raw = match std::fs::read_to_string(&path) {
                        Ok(r) => r,
                        Err(err) => {
                            errors.push(format!("failed to read {}: {}", path.display(), err));
                            continue;
                        }
                    };
                    let session = match serde_json::from_str::<ChatSession>(&raw) {
                        Ok(s) => s,
                        Err(err) => {
                            errors.push(format!("failed to parse {}: {}", path.display(), err));
                            continue;
                        }
                    };
                    let mut session = session;
                    self.provider.normalize_loaded_session(&mut session);
                    if session_matches_search_terms(&session, query_terms) {
                        matched.insert(session.id.clone());
                    }
                }
                Err(err) => {
                    errors.push(format!("failed to read directory entry: {}", err));
                }
            }
        }

        // Log any errors that occurred during search
        for err in &errors {
            warn!("search_session_content: {}", err);
        }
        if !errors.is_empty() {
            warn!(
                "search_session_content: {} error(s) during search, returning partial results",
                errors.len()
            );
        }

        debug!(
            query_terms=?query_terms,
            count=matched.len(),
            "search_session_content: fallback scan complete"
        );
        Ok(matched)
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        // Fast path: try SQLite index first.
        if self.uses_global_session_index() {
            if let Some(index) = storage::get_index() {
                if let Ok(conn) = index.conn.lock() {
                    match storage::index::list_sessions_from_index(&conn, self.provider.as_str()) {
                        Ok(mut sessions) => {
                            // SQLite doesn't know about orphaned session files on disk,
                            // so scan for any that aren't in the index yet.
                            let indexed_ids: HashSet<String> = sessions
                                .iter()
                                .map(|s| s.id.clone())
                                .collect::<HashSet<String>>();
                            self.discover_orphaned_sessions(&mut sessions, &indexed_ids);
                            sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
                            debug!(count = sessions.len(), "list_sessions: SQLite index hit");
                            return Ok(sessions);
                        }
                        Err(err) => {
                            warn!(
                                error=%err,
                                "list_sessions: SQLite query failed, falling back to metadata"
                            );
                        }
                    }
                }
            }
        }

        // Fallback: use metadata index + orphaned file discovery (original logic).
        self.list_sessions_fallback()
    }

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
    }

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
    }

    fn load_metadata(&self) -> Result<SessionMetadata> {
        let metadata_path = self.data_dir.join("sessions.json");
        if !metadata_path.exists() {
            return Ok(SessionMetadata::default());
        }

        let metadata_json = fs::read_to_string(&metadata_path)?;
        let metadata: SessionMetadata = serde_json::from_str(&metadata_json)?;
        debug!(path=%metadata_path.display(), "metadata loaded");
        Ok(metadata)
    }

    fn save_metadata(&self, metadata: &SessionMetadata) -> Result<()> {
        let metadata_path = self.data_dir.join("sessions.json");
        let metadata_json = serde_json::to_string_pretty(metadata)?;
        let _ = atomic_write(&metadata_path, metadata_json.as_bytes())?;
        debug!(path=%metadata_path.display(), "metadata saved");
        Ok(())
    }

    fn session_path(&self, session_id: &str) -> PathBuf {
        self.data_dir
            .join("sessions")
            .join(format!("{}.json", session_id))
    }

    fn lock_metadata_exclusive(&self) -> Result<MetadataLock> {
        let lock_path = self.data_dir.join("sessions.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(lock_path)?;
        file.lock_exclusive()?;
        Ok(MetadataLock(file))
    }

    fn persist_session_locked(
        &self,
        session: &ChatSession,
        metadata: &mut SessionMetadata,
        hidden: bool,
        preserve_existing_name: bool,
    ) -> Result<()> {
        let mut session = session.clone();
        if preserve_existing_name {
            if let Some(session_info) = metadata
                .sessions
                .get(&session.id)
                .filter(|info| !info.hidden)
            {
                if session_info.name != session.name {
                    info!(
                        session_id=%session.id,
                        incoming_session_name=%session.name,
                        persisted_session_name=%session_info.name,
                        "preserving persisted session name during save"
                    );
                    session.name = session_info.name.clone();
                }
            }
        }

        let session_path = self.session_path(&session.id);
        let session_json = serde_json::to_string_pretty(&session)?;
        let _ = atomic_write(&session_path, session_json.as_bytes())?;

        if self.uses_global_session_index() {
            let provider_name = self.provider.as_str();
            if let Err(err) = storage::index_session(&self.data_dir, &session, provider_name) {
                warn!(
                    session_id=%session.id,
                    error=%err,
                    "failed to index session in SQLite (file write succeeded)"
                );
            } else if hidden {
                if let Err(err) = storage::hide_session(&session.id) {
                    warn!(
                        session_id=%session.id,
                        error=%err,
                        "failed to mark session hidden in SQLite (file write succeeded)"
                    );
                }
            }
        }

        let snapshot = session_log_snapshot(&session);
        let history_fingerprint = format!("{:016x}", snapshot.history_fingerprint);
        debug!(
            session_id=%session.id,
            session_name=%session.name,
            path=%session_path.display(),
            total_messages=snapshot.total_messages,
            user_messages=snapshot.user_messages,
            assistant_messages=snapshot.assistant_messages,
            first_message_at=?snapshot.first_message_at,
            last_message_at=?snapshot.last_message_at,
            history_fingerprint=%history_fingerprint,
            hidden,
            "session saved"
        );

        metadata.sessions.insert(
            session.id.clone(),
            SessionInfo {
                id: session.id.clone(),
                name: session.name.clone(),
                created_at: session.created_at,
                updated_at: session.updated_at,
                message_count: session.messages.iter().filter(|m| m.is_user).count(),
                hidden,
            },
        );
        self.save_metadata(metadata)?;
        Ok(())
    }
}

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
