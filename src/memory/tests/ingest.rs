use super::super::*;
use super::temp_path;
use chrono::{TimeZone, Utc};
use std::collections::BTreeMap;
use std::fs;
#[test]
fn ingest_state_round_trips() {
    let dir = temp_path("ingest-state");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(INGEST_STATE_FILE_NAME);

    let mut state = IngestState::default();
    state.sessions.insert(
        "chat-1".to_string(),
        SessionIngestState {
            indexed_message_count: 3,
            history_fingerprint: "abc123".to_string(),
            updated_at: Utc::now(),
        },
    );
    state.invalidated_doc_keys.insert(
        "chat-1:2:assistant".to_string(),
        MemoryInvalidation {
            invalidated_at: Utc::now(),
            reason: Some("stale memory".to_string()),
        },
    );

    save_ingest_state(&path, &state).unwrap();
    let loaded = load_ingest_state(&path).unwrap();
    assert_eq!(loaded.sessions.len(), 1);
    assert_eq!(loaded.invalidated_doc_keys.len(), 1);

    let _ = fs::remove_dir_all(dir);
}
