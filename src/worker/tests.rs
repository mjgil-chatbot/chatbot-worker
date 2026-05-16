use super::*;
use std::ffi::OsString;

fn temp_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("chatbot-worker-test-{}-{}", name, nanos))
}

struct HomeGuard {
    previous_home: Option<OsString>,
    temp_home: PathBuf,
}

impl HomeGuard {
    fn set(temp_home: PathBuf) -> Self {
        let previous_home = std::env::var_os("HOME");
        fs::create_dir_all(&temp_home).unwrap();
        std::env::set_var("HOME", &temp_home);
        Self {
            previous_home,
            temp_home,
        }
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        match &self.previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        let _ = fs::remove_dir_all(&self.temp_home);
    }
}

fn age_path(path: &Path, age: Duration) {
    let stale = std::time::SystemTime::now()
        .checked_sub(age)
        .expect("stale timestamp");
    let duration = stale
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix timestamp");
    let ts = libc::timespec {
        tv_sec: duration.as_secs() as libc::time_t,
        tv_nsec: duration.subsec_nanos() as libc::c_long,
    };
    let times = [ts, ts];
    let path = std::ffi::CString::new(path.to_string_lossy().as_bytes()).expect("c path");
    let rc = unsafe { libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), 0) };
    assert_eq!(rc, 0, "failed to age {}", path.to_string_lossy());
}

#[test]
fn worker_job_snapshot_bump_advances_sequence() {
    let mut job = WorkerJobSnapshot::new("chat-1", "claude", 123, 456);
    let first_sequence = job.sequence;
    let first_updated_at = job.updated_at;
    std::thread::sleep(std::time::Duration::from_millis(1));
    job.bump();

    assert!(job.sequence > first_sequence);
    assert!(job.updated_at >= first_updated_at);
}

#[test]
fn write_json_atomic_round_trips_worker_job_snapshot() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let path = temp_path("state.json");
    let job = WorkerJobSnapshot::new("chat-1", "claude", 123, 456);

    write_json_atomic(&path, &job).unwrap();

    let loaded: WorkerJobSnapshot =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(loaded.chat_id, "chat-1");
    assert_eq!(loaded.provider, "claude");
    assert_eq!(loaded.worker_pid, 123);
    let raw: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        raw.get("schema_version").and_then(|value| value.as_u64()),
        Some(WORKER_SPOOL_SCHEMA_VERSION as u64)
    );

    let _ = fs::remove_file(path);
}

#[test]
fn worker_request_deserialize_accepts_missing_schema_version_as_v1() {
    let request: WorkerRequest = serde_json::from_str(
        r#"{"provider":"claude","query":"hi","command":"claude -p hi","chat_id":"chat-1"}"#,
    )
    .expect("request without schema version should default to v1");
    assert_eq!(request.provider, "claude");
    assert_eq!(request.chat_id, "chat-1");
}

#[test]
fn worker_request_deserialize_rejects_unknown_schema_version() {
    let err = serde_json::from_str::<WorkerRequest>(
        r#"{"schema_version":99,"provider":"claude","query":"hi","command":"claude -p hi","chat_id":"chat-1"}"#,
    )
    .expect_err("unsupported request schema version should fail");
    assert!(err
        .to_string()
        .contains("unsupported request.json schema_version"));
}

#[test]
fn worker_result_deserialize_rejects_unknown_schema_version() {
    let err = serde_json::from_str::<WorkerResult>(r#"{"schema_version":99,"error":"boom"}"#)
        .expect_err("unsupported result schema version should fail");
    assert!(err
        .to_string()
        .contains("unsupported result.json schema_version"));
}

#[test]
fn worker_job_snapshot_deserialize_rejects_unknown_schema_version() {
    let err = serde_json::from_str::<WorkerJobSnapshot>(
        r#"{"schema_version":99,"chat_id":"chat-1","provider":"claude","worker_pid":1,"parent_pid":2,"state":"Running","started_at":0,"updated_at":0,"output_len":0,"sequence":1}"#,
    )
    .expect_err("unsupported state schema version should fail");
    assert!(err
        .to_string()
        .contains("unsupported state.json schema_version"));
}

#[test]
fn read_job_state_surfaces_invalid_json() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let chat_id = format!("chat-parse-fail-{}", uuid::Uuid::new_v4());
    let path = crate::config::shared_infra_dir()
        .join("workers")
        .join(&chat_id)
        .join("state.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{not json").unwrap();

    let err = read_job_state(&chat_id).expect_err("invalid state.json should surface an error");
    assert!(err.to_string().contains("failed to parse"));

    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn worker_paths_encode_unsafe_chat_ids() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let path = worker_dir("../escape/../chat");
    assert!(path.starts_with(workers_dir()));
    assert!(!path.to_string_lossy().contains("../"));
}

#[test]
fn list_job_states_keeps_fresh_entry_without_state_json() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = HomeGuard::set(temp_path("worker-home-fresh-missing-state"));
    let chat_id = format!("chat-missing-state-{}", uuid::Uuid::new_v4());
    let dir = worker_dir(&chat_id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(request_path(&chat_id), "{}").unwrap();

    let jobs = list_job_states().expect("listing worker jobs should succeed");
    assert!(
        jobs.iter().all(|job| job.chat_id != chat_id),
        "entries without state.json should be skipped"
    );
    assert!(
        dir.exists(),
        "fresh request-only spool should survive the worker startup gap"
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn list_job_states_cleans_stale_request_only_entry() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = HomeGuard::set(temp_path("worker-home-stale-request"));
    let chat_id = format!("chat-stale-request-{}", uuid::Uuid::new_v4());
    let dir = worker_dir(&chat_id);
    let request = request_path(&chat_id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(&request, "{}").unwrap();
    let stale_age = MISSING_STATE_STARTUP_GRACE + Duration::from_secs(30);
    age_path(&request, stale_age);
    age_path(&dir, stale_age);

    let jobs = list_job_states().expect("listing worker jobs should succeed");
    assert!(
        jobs.iter().all(|job| job.chat_id != chat_id),
        "stale request-only entry should not appear as a worker job"
    );
    assert!(
        !dir.exists(),
        "stale request-only spool should be cleaned automatically"
    );
}

#[test]
fn list_job_states_cleans_stale_empty_entry() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _home = HomeGuard::set(temp_path("worker-home-stale-empty"));
    let chat_id = format!("chat-empty-missing-state-{}", uuid::Uuid::new_v4());
    let dir = worker_dir(&chat_id);
    fs::create_dir_all(&dir).unwrap();
    age_path(&dir, MISSING_STATE_STARTUP_GRACE + Duration::from_secs(30));

    let jobs = list_job_states().expect("listing worker jobs should succeed");
    assert!(
        jobs.iter().all(|job| job.chat_id != chat_id),
        "stale empty entry should not appear as a worker job"
    );
    assert!(
        !dir.exists(),
        "stale empty spool should be cleaned automatically"
    );
}

#[test]
fn list_job_states_skips_corrupt_state_json() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let chat_id = format!("chat-corrupt-state-{}", uuid::Uuid::new_v4());
    let dir = worker_dir(&chat_id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(state_path(&chat_id), "{not valid json}").unwrap();

    let jobs = list_job_states().expect("listing should not bail on corrupt entries");
    assert!(
        jobs.iter().all(|job| job.chat_id != chat_id),
        "entries with corrupt state.json should be skipped"
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn list_job_states_returns_valid_entries_alongside_bad_ones() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let good_id = format!("chat-good-{}", uuid::Uuid::new_v4());
    let bad_id = format!("chat-bad-{}", uuid::Uuid::new_v4());

    let good_dir = worker_dir(&good_id);
    fs::create_dir_all(&good_dir).unwrap();
    let good_job = WorkerJobSnapshot::new(&good_id, "claude", 1, 2);
    write_json_atomic(&state_path(&good_id), &good_job).unwrap();

    let bad_dir = worker_dir(&bad_id);
    fs::create_dir_all(&bad_dir).unwrap();
    fs::write(state_path(&bad_id), "GARBAGE").unwrap();

    let jobs = list_job_states().expect("listing should succeed");
    assert!(
        jobs.iter().any(|job| job.chat_id == good_id),
        "valid entry should still appear even when bad entries exist"
    );
    assert!(
        jobs.iter().all(|job| job.chat_id != bad_id),
        "corrupt entry should not appear"
    );

    let _ = fs::remove_dir_all(good_dir);
    let _ = fs::remove_dir_all(bad_dir);
}

#[test]
fn list_job_states_handles_empty_spool_dir() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let jobs = list_job_states().expect("listing empty spool should succeed");
    assert!(jobs.len() < 10_000, "sanity check on job count");
}

#[test]
fn read_job_state_returns_none_for_missing_chat() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let chat_id = format!("chat-nonexistent-{}", uuid::Uuid::new_v4());
    let result = read_job_state(&chat_id).unwrap();
    assert!(result.is_none());
}

#[test]
fn read_output_returns_empty_for_missing_chat() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let chat_id = format!("chat-no-output-{}", uuid::Uuid::new_v4());
    assert_eq!(read_output(&chat_id), "");
}

#[test]
fn clear_transient_spool_files_removes_stale_stream_artifacts() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let chat_id = format!("chat-clear-stale-{}", uuid::Uuid::new_v4());
    let worker_dir = worker_dir(&chat_id);
    fs::create_dir_all(&worker_dir).unwrap();
    fs::write(output_path(&chat_id), "stale output").unwrap();
    fs::write(result_path(&chat_id), "{\"error\":null}").unwrap();
    fs::write(
        events_path(&chat_id),
        "{\"type\":\"TextDelta\",\"content\":\"x\"}\n",
    )
    .unwrap();
    fs::write(transport_errors_path(&chat_id), "stale transport error\n").unwrap();
    fs::write(request_path(&chat_id), "{\"chat_id\":\"request\"}").unwrap();

    clear_transient_spool_files(&chat_id).unwrap();

    assert!(!output_path(&chat_id).exists());
    assert!(!result_path(&chat_id).exists());
    assert!(!events_path(&chat_id).exists());
    assert!(!transport_errors_path(&chat_id).exists());
    assert!(request_path(&chat_id).exists());

    let _ = fs::remove_dir_all(worker_dir);
}

#[test]
fn validate_chat_id_accepts_normal_ids() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    assert_eq!(validate_chat_id("chat-abc123"), Some("chat-abc123"));
    assert_eq!(validate_chat_id("simple"), Some("simple"));
}

#[test]
fn validate_chat_id_rejects_traversal() {
    assert_eq!(validate_chat_id("../etc/passwd"), None);
    assert_eq!(validate_chat_id("foo/bar"), None);
    assert_eq!(validate_chat_id("foo\\bar"), None);
}

#[test]
fn validate_chat_id_rejects_empty() {
    assert_eq!(validate_chat_id(""), None);
}

#[test]
fn validate_chat_id_rejects_null_byte() {
    assert_eq!(validate_chat_id("foo\0bar"), None);
}

#[test]
fn validate_chat_id_rejects_colon() {
    assert_eq!(validate_chat_id("C:foo"), None);
}

#[test]
fn safe_chat_id_component_passes_through_valid_ids() {
    let result = safe_chat_id_component("chat-123");
    assert!(matches!(result, Cow::Borrowed("chat-123")));
}

#[test]
fn safe_chat_id_component_hex_encodes_unsafe_ids() {
    let result = safe_chat_id_component("../bad");
    assert!(result.starts_with("chat-"));
    assert!(!result.contains(".."));
    assert!(!result.contains('/'));
}

#[test]
fn worker_state_from_u32_roundtrips() {
    for value in 0..=4 {
        let state = WorkerState::from_u32(value);
        let name = state.name();
        assert!(!name.is_empty());
    }
}

#[test]
fn worker_state_unknown_defaults_to_free() {
    assert_eq!(WorkerState::from_u32(99), WorkerState::Free);
    assert_eq!(WorkerState::from_u32(u32::MAX), WorkerState::Free);
}

#[test]
fn worker_state_names_are_uppercase() {
    for value in 0..=4 {
        let name = WorkerState::from_u32(value).name();
        assert_eq!(name, name.to_uppercase(), "state name should be uppercase");
    }
}

#[test]
fn write_text_atomic_creates_file_and_parents() {
    let path = temp_path("subdir").join("nested").join("file.txt");
    write_text_atomic(&path, "hello").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
    let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

#[test]
fn events_path_under_worker_dir() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let path = events_path("chat-42");
    assert!(path.ends_with("events.jsonl"));
    assert!(path.starts_with(workers_dir()));
}

#[test]
fn read_output_prefers_flat_file_over_event_log() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let chat_id = format!("chat-flat-pref-{}", uuid::Uuid::new_v4());
    let dir = worker_dir(&chat_id);
    fs::create_dir_all(&dir).unwrap();

    fs::write(output_path(&chat_id), "flat content").unwrap();
    crate::stream::append_event(
        &events_path(&chat_id),
        &crate::stream::StreamEvent::text_delta("event content"),
    )
    .unwrap();

    let output = read_output(&chat_id);
    assert_eq!(output, "flat content", "flat file should take precedence");

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn read_output_falls_back_to_event_log() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let chat_id = format!("chat-evlog-fb-{}", uuid::Uuid::new_v4());
    let dir = worker_dir(&chat_id);
    fs::create_dir_all(&dir).unwrap();

    crate::stream::append_event(
        &events_path(&chat_id),
        &crate::stream::StreamEvent::text_delta("from events"),
    )
    .unwrap();

    let output = read_output(&chat_id);
    assert!(
        output.ends_with("from events"),
        "should fall back to timestamped event log output, got: {output}"
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn read_output_returns_empty_when_both_missing() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let chat_id = format!("chat-both-missing-{}", uuid::Uuid::new_v4());
    assert_eq!(read_output(&chat_id), "");
}

#[test]
fn read_events_since_tracks_incremental_progress() {
    let _lock = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let chat_id = format!("chat-incr-{}", uuid::Uuid::new_v4());
    let dir = worker_dir(&chat_id);
    fs::create_dir_all(&dir).unwrap();

    crate::stream::append_event(
        &events_path(&chat_id),
        &crate::stream::StreamEvent::text_delta("a"),
    )
    .unwrap();

    let (batch1, off1) = read_events_since(&chat_id, 0);
    assert_eq!(batch1.len(), 1);

    crate::stream::append_event(
        &events_path(&chat_id),
        &crate::stream::StreamEvent::text_delta("b"),
    )
    .unwrap();

    let (batch2, off2) = read_events_since(&chat_id, off1);
    assert_eq!(batch2.len(), 1);
    assert!(off2 > off1);

    let _ = fs::remove_dir_all(dir);
}
