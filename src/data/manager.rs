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
