use super::*;
use crate::adapter_tool_call_schema::{
    ToolCallLiveUpdate, ToolCallLiveUpdateKind, ToolCallLiveUpdatePhase,
};
use serde_json::json;

fn sample_summary_timestamp_ms() -> i64 {
    Local
        .with_ymd_and_hms(2026, 5, 19, 0, 2, 0)
        .single()
        .expect("local timestamp")
        .timestamp_millis()
}

fn temp_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("chatbot-stream-test-{}-{}", name, nanos))
}

#[test]
fn stream_event_text_delta_round_trips() {
    let event = StreamEvent::TextDelta {
        content: "hello world".into(),
        timestamp_ms: 1234567890,
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, parsed);
}

#[test]
fn stream_event_tool_start_round_trips() {
    let event = StreamEvent::ToolStart {
        tool: "read_file".into(),
        input: json!({"path": "/tmp/foo"}),
        live_update: None,
        timestamp_ms: 100,
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, parsed);
}

#[test]
fn stream_event_tool_end_round_trips() {
    let event = StreamEvent::ToolEnd {
        tool: "read_file".into(),
        output: json!({"content": "data"}),
        live_update: None,
        timestamp_ms: 200,
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, parsed);
}

#[test]
fn stream_event_tool_end_preserves_live_update_schema() {
    let event = StreamEvent::ToolEnd {
        tool: "tool-1".into(),
        output: json!({"output": "done"}),
        live_update: Some(ToolCallLiveUpdate {
            schema_version: 1,
            call_id: "tool-1".to_string(),
            phase: ToolCallLiveUpdatePhase::Finished,
            kind: ToolCallLiveUpdateKind::Bash,
            raw_name: "bash".to_string(),
            label: "Bash".to_string(),
            detail: Some("`pwd`".to_string()),
            result_preview: Some("/tmp/project".to_string()),
        }),
        timestamp_ms: 300,
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, parsed);
}

#[test]
fn stream_event_thinking_round_trips() {
    let start = StreamEvent::ThinkingStart { timestamp_ms: 10 };
    let end = StreamEvent::ThinkingEnd { timestamp_ms: 20 };
    for event in [start, end] {
        let json = serde_json::to_string(&event).unwrap();
        let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, parsed);
    }
}

#[test]
fn stream_event_cost_update_round_trips() {
    let event = StreamEvent::CostUpdate {
        input_tokens: 100,
        output_tokens: 200,
        timestamp_ms: 50,
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, parsed);
}

#[test]
fn stream_event_error_round_trips() {
    let event = StreamEvent::Error {
        message: "something went wrong".into(),
        timestamp_ms: 99,
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, parsed);
}

#[test]
fn stream_event_live_summary_round_trips() {
    let event = StreamEvent::LiveSummary {
        content: "GPT-5.4 | Bash `git status --short`".into(),
        timestamp_ms: 101,
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, parsed);
}

#[test]
fn all_variants_tagged_serde_round_trip() {
    let events = vec![
        StreamEvent::text_delta("hi"),
        StreamEvent::live_summary("Codex | Bash `git status`"),
        StreamEvent::tool_start("grep", json!({"q": "foo"})),
        StreamEvent::tool_end("grep", json!({"matches": 3})),
        StreamEvent::ThinkingStart { timestamp_ms: 0 },
        StreamEvent::ThinkingEnd { timestamp_ms: 0 },
        StreamEvent::cost_update(10, 20),
        StreamEvent::error("oops"),
    ];
    for event in &events {
        let json = serde_json::to_string(event).unwrap();
        let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(*event, parsed);
    }
}

#[test]
fn append_and_read_events_round_trip() {
    let dir = temp_path("append-read");
    let path = dir.join("events.jsonl");

    let events = vec![
        StreamEvent::text_delta("hello "),
        StreamEvent::text_delta("world"),
        StreamEvent::error("boom"),
    ];

    for event in &events {
        append_event(&path, event).unwrap();
    }

    let loaded = read_events(&path);
    assert_eq!(loaded.len(), 3);
    if let StreamEvent::TextDelta { content, .. } = &loaded[0] {
        assert_eq!(content, "hello ");
    } else {
        panic!("expected TextDelta");
    }
    if let StreamEvent::Error { message, .. } = &loaded[2] {
        assert_eq!(message, "boom");
    } else {
        panic!("expected Error");
    }

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn append_event_truncates_large_tool_payloads_before_spooling() {
    let dir = temp_path("truncate-tool-output");
    let path = dir.join("events.jsonl");
    let large_output = "x".repeat(MAX_TOOL_VALUE_BYTES * 4);
    let event = StreamEvent::ToolEnd {
        tool: "ListDir".to_string(),
        output: json!({ "output": large_output }),
        live_update: None,
        timestamp_ms: 321,
    };

    append_event(&path, &event).unwrap();

    let bytes = fs::read(&path).unwrap();
    assert!(
        bytes.len() < MAX_SERIALIZED_EVENT_BYTES,
        "sanitized event should stay below the hard event cap"
    );

    let loaded = read_events(&path);
    match &loaded[0] {
        StreamEvent::ToolEnd { output, .. } => {
            assert_eq!(output["truncated"], json!(true));
            assert!(output["original_bytes"].as_u64().unwrap() > MAX_TOOL_VALUE_BYTES as u64);
            assert!(output["preview"].as_str().unwrap().contains("[truncated"));
        }
        other => panic!("expected ToolEnd, got {other:?}"),
    }

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn append_event_hard_caps_large_text_delta_lines() {
    let dir = temp_path("truncate-text-delta");
    let path = dir.join("events.jsonl");
    let event = StreamEvent::TextDelta {
        content: "y".repeat(MAX_SERIALIZED_EVENT_BYTES * 2),
        timestamp_ms: 456,
    };

    append_event(&path, &event).unwrap();

    let bytes = fs::read(&path).unwrap();
    assert!(
        bytes.len() < MAX_SERIALIZED_EVENT_BYTES,
        "oversized text deltas should be bounded before writing the spool line"
    );

    let loaded = read_events(&path);
    match &loaded[0] {
        StreamEvent::TextDelta { content, .. } => {
            assert!(content.contains("[truncated"));
            assert!(content.len() < MAX_SERIALIZED_EVENT_BYTES);
        }
        other => panic!("expected TextDelta, got {other:?}"),
    }

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn read_events_returns_empty_for_missing_file() {
    let path = temp_path("nonexistent").join("events.jsonl");
    let events = read_events(&path);
    assert!(events.is_empty());
}

#[test]
fn read_events_skips_malformed_lines() {
    let dir = temp_path("malformed");
    let path = dir.join("events.jsonl");
    fs::create_dir_all(&dir).unwrap();

    let good = StreamEvent::text_delta("ok");
    let good_json = serde_json::to_string(&good).unwrap();
    let content = format!("{}\n{{not json}}\n{}\n", good_json, good_json);
    fs::write(&path, content).unwrap();

    let events = read_events(&path);
    assert_eq!(events.len(), 2, "malformed line should be skipped");

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn read_events_since_tracks_offset() {
    let dir = temp_path("incremental");
    let path = dir.join("events.jsonl");

    append_event(&path, &StreamEvent::text_delta("a")).unwrap();
    append_event(&path, &StreamEvent::text_delta("b")).unwrap();

    let (batch1, offset1) = read_events_since(&path, 0);
    assert_eq!(batch1.len(), 2);
    assert!(offset1 > 0);

    append_event(&path, &StreamEvent::text_delta("c")).unwrap();

    let (batch2, offset2) = read_events_since(&path, offset1);
    assert_eq!(batch2.len(), 1);
    assert!(offset2 > offset1);

    let (batch3, offset3) = read_events_since(&path, offset2);
    assert!(batch3.is_empty());
    assert_eq!(offset3, offset2);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn read_events_since_returns_empty_for_missing_file() {
    let path = temp_path("missing-incremental").join("events.jsonl");
    let (events, offset) = read_events_since(&path, 0);
    assert!(events.is_empty());
    assert_eq!(offset, 0);
}

#[test]
fn compute_output_formats_text_deltas_as_timestamped_lines() {
    let timestamp_ms = sample_summary_timestamp_ms();
    let events = vec![
        StreamEvent::LiveSummary {
            content: "Codex | Bash `git status`".to_string(),
            timestamp_ms,
        },
        StreamEvent::TextDelta {
            content: "Hello".to_string(),
            timestamp_ms,
        },
        StreamEvent::tool_start("grep", json!({})),
        StreamEvent::TextDelta {
            content: "world".to_string(),
            timestamp_ms,
        },
        StreamEvent::error("ignored for output"),
        StreamEvent::TextDelta {
            content: "!".to_string(),
            timestamp_ms,
        },
    ];
    let output = compute_output_from_events(&events);
    assert_eq!(
        output,
        format!(
            "{}\n{}\n{}\n{}",
            format_live_summary_line(timestamp_ms, "Codex | Bash `git status`"),
            format_timestamped_transcript_line(timestamp_ms, "Hello"),
            format_timestamped_transcript_line(timestamp_ms, "world"),
            format_timestamped_transcript_line(timestamp_ms, "!")
        )
    );
}

#[test]
fn compute_output_empty_for_no_text_events() {
    let events = vec![
        StreamEvent::tool_start("x", json!({})),
        StreamEvent::tool_end("x", json!({})),
    ];
    assert_eq!(compute_output_from_events(&events), "");
}

#[test]
fn compute_output_uses_summary_when_no_text_is_available() {
    let timestamp_ms = sample_summary_timestamp_ms();
    let events = vec![StreamEvent::LiveSummary {
        content: "Kimi | Read main.rs".to_string(),
        timestamp_ms,
    }];
    assert_eq!(
        compute_output_from_events(&events),
        format_live_summary_line(timestamp_ms, "Kimi | Read main.rs")
    );
}

#[test]
fn append_output_from_events_dedupes_summary_across_batches() {
    let timestamp_ms = sample_summary_timestamp_ms();
    let summary = StreamEvent::LiveSummary {
        content: "Kimi | Read main.rs".to_string(),
        timestamp_ms,
    };
    let delta = StreamEvent::TextDelta {
        content: "alpha".to_string(),
        timestamp_ms,
    };

    let mut transcript = String::new();
    append_output_from_events(&mut transcript, std::slice::from_ref(&summary));
    append_output_from_events(&mut transcript, &[summary, delta]);

    assert_eq!(
        transcript,
        format!(
            "{}\n{}",
            format_live_summary_line(timestamp_ms, "Kimi | Read main.rs"),
            format_timestamped_transcript_line(timestamp_ms, "alpha")
        )
    );
}

#[test]
fn events_jsonl_produces_timestamped_live_transcript() {
    let dir = temp_path("compat");
    let event_path = dir.join("events.jsonl");
    let timestamp_ms = sample_summary_timestamp_ms();

    let chunks = vec!["The answer is", "42", "."];
    append_event(
        &event_path,
        &StreamEvent::LiveSummary {
            content: "GPT-5.4 | Bash `git status --short`".to_string(),
            timestamp_ms,
        },
    )
    .unwrap();
    for chunk in &chunks {
        append_event(
            &event_path,
            &StreamEvent::TextDelta {
                content: (*chunk).to_string(),
                timestamp_ms,
            },
        )
        .unwrap();
    }
    append_event(&event_path, &StreamEvent::cost_update(10, 20)).unwrap();

    let events = read_events(&event_path);
    let computed = compute_output_from_events(&events);
    assert_eq!(
        computed,
        format!(
            "{}\n{}\n{}\n{}",
            format_live_summary_line(timestamp_ms, "GPT-5.4 | Bash `git status --short`"),
            format_timestamped_transcript_line(timestamp_ms, "The answer is"),
            format_timestamped_transcript_line(timestamp_ms, "42"),
            format_timestamped_transcript_line(timestamp_ms, ".")
        )
    );

    let flat_output = chunks.join("");
    assert_ne!(computed, flat_output);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn compute_output_splits_multiline_text_deltas_into_timestamped_lines() {
    let timestamp_ms = sample_summary_timestamp_ms();
    let events = vec![StreamEvent::TextDelta {
        content: "alpha\nbeta\n".to_string(),
        timestamp_ms,
    }];
    assert_eq!(
        compute_output_from_events(&events),
        format!(
            "{}\n{}",
            format_timestamped_transcript_line(timestamp_ms, "alpha"),
            format_timestamped_transcript_line(timestamp_ms, "beta")
        )
    );
}

#[test]
fn events_path_joins_correctly() {
    let dir = PathBuf::from("/tmp/workers/chat-123");
    assert_eq!(
        events_path(&dir),
        PathBuf::from("/tmp/workers/chat-123/events.jsonl")
    );
}

#[test]
fn compute_output_deduplicates_repeated_timestamped_summary_lines() {
    let timestamp_ms = sample_summary_timestamp_ms();
    let events = vec![
        StreamEvent::LiveSummary {
            content: "MM-2.7 | Glob `tests/*.rs`".to_string(),
            timestamp_ms,
        },
        StreamEvent::LiveSummary {
            content: "MM-2.7 | Glob `tests/*.rs`".to_string(),
            timestamp_ms,
        },
    ];
    assert_eq!(
        compute_output_from_events(&events),
        format_live_summary_line(timestamp_ms, "MM-2.7 | Glob `tests/*.rs`")
    );
}
