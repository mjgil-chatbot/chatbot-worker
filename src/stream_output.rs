use crate::stream::{compute_output_from_events, StreamEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedTurnOutput {
    pub transcript: String,
    pub transcript_preamble: String,
    pub final_output: String,
    pub combined: String,
}

pub fn build_persisted_turn_output(
    events: &[StreamEvent],
    final_output: &str,
) -> PersistedTurnOutput {
    let sanitized_final_output = strip_leading_think_block(final_output);
    let transcript = compute_output_from_events(events);
    let transcript_preamble = if sanitized_final_output.trim().is_empty() {
        transcript.clone()
    } else {
        trim_transcript_final_output_suffix(events, &transcript, &sanitized_final_output)
    };

    let combined = if sanitized_final_output.trim().is_empty() {
        transcript.clone()
    } else if transcript_preamble.trim().is_empty() {
        sanitized_final_output.clone()
    } else {
        format!("{transcript_preamble}\n\n{sanitized_final_output}")
    };

    PersistedTurnOutput {
        transcript,
        transcript_preamble,
        final_output: sanitized_final_output,
        combined,
    }
}

fn strip_leading_think_block(content: &str) -> String {
    let trimmed = content.trim_start();
    let think_open = "<think>";
    let think_close = "</think>";
    if !trimmed.starts_with(think_open) {
        return content.to_string();
    }

    let Some(close_index) = trimmed.find(think_close) else {
        return content.to_string();
    };
    trimmed[close_index + think_close.len()..]
        .trim_start_matches(['\r', '\n', ' '])
        .to_string()
}

fn trim_transcript_final_output_suffix(
    events: &[StreamEvent],
    transcript: &str,
    final_output: &str,
) -> String {
    if let Some(trimmed) = trim_transcript_text_delta_suffix(events, transcript, final_output) {
        return trimmed;
    }
    trim_transcript_line_suffix(transcript, final_output)
}

fn trim_transcript_text_delta_suffix(
    events: &[StreamEvent],
    transcript: &str,
    final_output: &str,
) -> Option<String> {
    let expected = final_output.trim_end();
    if expected.is_empty() {
        return None;
    }

    let mut accumulated = String::new();
    let mut matched_text_line_count = 0usize;
    let mut saw_rendered_event = false;

    for event in events.iter().rev() {
        match event {
            StreamEvent::TextDelta { content, .. } => {
                saw_rendered_event = true;
                accumulated.insert_str(0, content);
                matched_text_line_count +=
                    content.split('\n').filter(|line| !line.is_empty()).count();

                let accumulated = accumulated.trim_end();
                if accumulated == expected {
                    return Some(trim_trailing_transcript_lines(
                        transcript,
                        matched_text_line_count,
                    ));
                }
                if accumulated.len() >= expected.len() {
                    return None;
                }
            }
            StreamEvent::LiveSummary { .. } => {
                return None;
            }
            _ if saw_rendered_event => continue,
            _ => continue,
        }
    }

    None
}

fn trim_transcript_line_suffix(transcript: &str, final_output: &str) -> String {
    let transcript_lines: Vec<&str> = transcript.lines().collect();
    let final_lines: Vec<&str> = final_output
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect();

    if final_lines.is_empty() || final_lines.len() > transcript_lines.len() {
        return transcript.to_string();
    }

    let suffix_matches = transcript_lines[transcript_lines.len() - final_lines.len()..]
        .iter()
        .map(|line| strip_transcript_timestamp_prefix(line.trim_start()).trim_end())
        .eq(final_lines.iter().copied());

    if suffix_matches {
        trim_trailing_transcript_lines(transcript, final_lines.len())
    } else {
        transcript.to_string()
    }
}

fn trim_trailing_transcript_lines(transcript: &str, line_count: usize) -> String {
    let mut transcript_lines: Vec<&str> = transcript.lines().collect();
    if line_count >= transcript_lines.len() {
        return String::new();
    }
    transcript_lines.truncate(transcript_lines.len() - line_count);
    transcript_lines.join("\n")
}

fn strip_transcript_timestamp_prefix(line: &str) -> &str {
    let bytes = line.as_bytes();
    if bytes.len() < 16 || bytes[0] != b'[' {
        return line;
    }
    if !bytes[1].is_ascii_alphabetic()
        || !bytes[2].is_ascii_alphabetic()
        || !bytes[3].is_ascii_alphabetic()
        || bytes[4] != b' '
    {
        return line;
    }
    let mut index = 5usize;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == 5 || index + 9 > bytes.len() || bytes[index] != b' ' {
        return line;
    }
    if !bytes[index + 1].is_ascii_digit()
        || !bytes[index + 2].is_ascii_digit()
        || bytes[index + 3] != b':'
        || !bytes[index + 4].is_ascii_digit()
        || !bytes[index + 5].is_ascii_digit()
        || !bytes[index + 6].is_ascii_lowercase()
        || !bytes[index + 7].is_ascii_lowercase()
        || bytes[index + 8] != b']'
        || index + 9 >= bytes.len()
        || bytes[index + 9] != b' '
    {
        return line;
    }
    &line[index + 10..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_persisted_turn_output_keeps_transcript_preamble_above_clean_final_reply() {
        let events = vec![
            StreamEvent::TextDelta {
                content: "I’ll inspect the architecture docs first.".to_string(),
                timestamp_ms: 1_779_160_783_628,
            },
            StreamEvent::LiveSummary {
                content: "GPT-5.4 | Bash `sed -n '1,220p' docs/architecture.md`".to_string(),
                timestamp_ms: 1_779_160_783_700,
            },
            StreamEvent::TextDelta {
                content: "Final answer".to_string(),
                timestamp_ms: 1_779_160_783_800,
            },
        ];

        let persisted = build_persisted_turn_output(&events, "Final answer");

        assert!(persisted
            .transcript_preamble
            .contains("I’ll inspect the architecture docs first."));
        assert!(persisted
            .transcript_preamble
            .contains("GPT-5.4 | Bash `sed -n '1,220p' docs/architecture.md`"));
        assert!(persisted.combined.ends_with("\n\nFinal answer"));
        assert_eq!(persisted.combined.matches("Final answer").count(), 1);
    }

    #[test]
    fn build_persisted_turn_output_trims_chunked_text_delta_suffix() {
        let events = vec![
            StreamEvent::TextDelta {
                content: "I’ll inspect the architecture docs first.".to_string(),
                timestamp_ms: 1_779_160_783_628,
            },
            StreamEvent::LiveSummary {
                content: "GPT-5.4 | Bash `git status --short`".to_string(),
                timestamp_ms: 1_779_160_783_700,
            },
            StreamEvent::TextDelta {
                content: "Final ".to_string(),
                timestamp_ms: 1_779_160_783_800,
            },
            StreamEvent::TextDelta {
                content: "answer".to_string(),
                timestamp_ms: 1_779_160_783_801,
            },
            StreamEvent::CostUpdate {
                input_tokens: 1,
                output_tokens: 2,
                timestamp_ms: 1_779_160_783_900,
            },
        ];

        let persisted = build_persisted_turn_output(&events, "Final answer");

        assert!(persisted
            .transcript_preamble
            .contains("I’ll inspect the architecture docs first."));
        assert!(persisted
            .transcript_preamble
            .contains("GPT-5.4 | Bash `git status --short`"));
        assert!(!persisted.transcript_preamble.contains("Final "));
        assert!(persisted.combined.ends_with("\n\nFinal answer"));
        assert_eq!(persisted.combined.matches("Final answer").count(), 1);
    }

    #[test]
    fn build_persisted_turn_output_falls_back_to_final_output_without_transcript() {
        let persisted = build_persisted_turn_output(&[], "Final answer");
        assert!(persisted.transcript.is_empty());
        assert!(persisted.transcript_preamble.is_empty());
        assert_eq!(persisted.final_output, "Final answer");
        assert_eq!(persisted.combined, "Final answer");
    }

    #[test]
    fn build_persisted_turn_output_strips_leading_think_block_from_final_output() {
        let persisted = build_persisted_turn_output(
            &[],
            "<think>Need to inspect the code first.</think>\n\nFinal answer",
        );
        assert_eq!(persisted.final_output, "Final answer");
        assert_eq!(persisted.combined, "Final answer");
    }

    #[test]
    fn build_persisted_turn_output_keeps_transcript_when_final_output_is_think_only() {
        let events = vec![StreamEvent::LiveSummary {
            content: "MiniMax | Bash `pwd`".to_string(),
            timestamp_ms: 1_779_160_783_700,
        }];

        let persisted =
            build_persisted_turn_output(&events, "<think>Need to inspect the code first.</think>");

        assert_eq!(persisted.final_output, "");
        assert_eq!(persisted.combined, persisted.transcript);
        assert!(persisted.combined.contains("MiniMax | Bash `pwd`"));
    }
}
