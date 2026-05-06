const INDEXED_METADATA_MARKERS: &[&str] = &[
    " title: ",
    " uri: ",
    " track: chat-history",
    " labels: chat-message",
    " chat_id: ",
    " command: ",
    " doc_key: ",
    " message_index: ",
    " provider: ",
    " role: ",
    " session_name: ",
    " final_state: ",
];

const SHELL_COMMAND_HEADS: &[&str] = &[
    "ls",
    "pwd",
    "cd",
    "cat",
    "head",
    "tail",
    "find",
    "rg",
    "grep",
    "sed",
    "awk",
    "git",
    "cargo",
    "make",
    "npm",
    "pnpm",
    "yarn",
    "node",
    "python",
    "python3",
    "bash",
    "sh",
    "zsh",
    "docker",
    "kubectl",
    "terraform",
    "env",
    "printenv",
    "ps",
    "kill",
    "tree",
    "du",
    "df",
];

const GREETING_TOKENS: &[&str] = &[
    "hi",
    "hello",
    "hey",
    "howdy",
    "yo",
    "sup",
    "morning",
    "afternoon",
    "evening",
    "there",
];

pub(super) fn sanitize_memory_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn sanitize_retrieved_memory_text(value: &str) -> String {
    let normalized = sanitize_memory_text(value);
    if normalized.is_empty() || !looks_like_indexed_metadata_tail(&normalized) {
        return normalized;
    }

    let cut_at = INDEXED_METADATA_MARKERS
        .iter()
        .filter_map(|marker| normalized.find(marker))
        .min();
    cut_at
        .map(|idx| normalized[..idx].trim().to_string())
        .unwrap_or(normalized)
}

pub(super) fn should_skip_memory_query(query: &str) -> bool {
    if query.trim().is_empty() || query.contains('\n') {
        return false;
    }

    let normalized = sanitize_memory_text(query);
    if normalized.is_empty() || normalized.len() > 120 {
        return false;
    }
    if looks_like_greeting_query(&normalized) {
        return true;
    }

    let candidate = trim_wrapping_backticks(strip_command_lead_in(&normalized));
    let mut tokens = candidate.split_whitespace();
    let Some(first) = tokens.next() else {
        return false;
    };
    if !is_shell_command_head(first) {
        return false;
    }

    tokens.all(is_shell_token)
}

fn looks_like_indexed_metadata_tail(value: &str) -> bool {
    value.contains(" uri: ")
        || value.contains(" track: chat-history")
        || value.contains(" labels: chat-message")
        || value.contains(" doc_key: ")
}

fn strip_command_lead_in(query: &str) -> &str {
    for prefix in ["run ", "Run ", "execute ", "Execute "] {
        if let Some(rest) = query.strip_prefix(prefix) {
            return rest.trim();
        }
    }
    query
}

fn trim_wrapping_backticks(value: &str) -> &str {
    if value.len() >= 2 && value.starts_with('`') && value.ends_with('`') {
        value[1..value.len() - 1].trim()
    } else {
        value
    }
}

fn is_shell_command_head(value: &str) -> bool {
    SHELL_COMMAND_HEADS.contains(&value) || value.starts_with("./") || value.starts_with('/')
}

fn is_shell_token(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '_' | '-' | '.' | '/' | ':' | '=' | ',' | '@' | '%' | '+' | '~' | '*' | '?'
                )
        })
}

fn looks_like_greeting_query(query: &str) -> bool {
    let lowered = query.to_ascii_lowercase();
    let cleaned = lowered
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphabetic() || ch.is_ascii_whitespace() {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>();
    let tokens = cleaned
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if tokens.is_empty() || tokens.len() > 3 {
        return false;
    }
    tokens.iter().all(|token| GREETING_TOKENS.contains(token))
}
