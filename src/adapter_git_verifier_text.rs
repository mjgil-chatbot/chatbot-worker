use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CommitClaim {
    pub(super) sha: String,
    repo_label: Option<String>,
}

pub(super) fn intent_query_text(query: &str) -> String {
    let mut paragraphs = Vec::new();

    for paragraph in query.split("\n\n") {
        let trimmed = paragraph.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_transcript_paragraph(trimmed) {
            break;
        }
        let trimmed = trim_before_inline_transcript_marker(trimmed);
        if trimmed.is_empty() {
            break;
        }
        let saw_inline_transcript = trimmed.len() != paragraph.trim().len();
        paragraphs.push(trimmed);
        if saw_inline_transcript {
            break;
        }
    }

    if paragraphs.is_empty() {
        query.trim().to_string()
    } else {
        paragraphs.join("\n\n")
    }
}

pub(super) fn mentions_branch_creation(lower: &str) -> bool {
    [
        "create branch",
        "create a branch",
        "new branch",
        "branch off",
        "checkout -b",
        "switch -c",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

pub(super) fn mentions_git_tag(lower: &str) -> bool {
    const REQUEST_CUES: &[&str] = &[
        "please", "can", "could", "should", "must", "need", "needs", "needed", "want", "wants",
        "wanted", "just", "then", "now", "also", "to", "add", "create",
    ];
    const DIRECT_OBJECTS: &[&str] = &[
        "it", "this", "that", "the", "my", "our", "these", "those", "release", "releases",
        "version", "versions", "commit", "commits",
    ];
    const ARTICLE_OBJECTS: &[&str] = &["release", "version", "commit", "changes", "change"];
    const EXCLUDED_OBJECTS: &[&str] = &[
        "helper",
        "helpers",
        "rendering",
        "parser",
        "parsing",
        "html",
        "markdown",
        "selector",
        "selectors",
        "attribute",
        "attributes",
    ];
    let tokens = query_tokens(lower);

    for (idx, token) in tokens.iter().enumerate() {
        if *token != "tag" {
            continue;
        }
        if tokens.get(idx.wrapping_sub(1)) == Some(&"git") {
            return true;
        }
        let prev = idx.checked_sub(1).and_then(|prev_idx| tokens.get(prev_idx));
        let next = tokens.get(idx + 1);
        if next.is_some_and(|next| EXCLUDED_OBJECTS.contains(next)) {
            continue;
        }
        if prev.is_some_and(|prev| *prev == "git") {
            return true;
        }
        if next.is_some_and(|next| DIRECT_OBJECTS.contains(next)) {
            return true;
        }
        if next.is_some_and(|next| *next == "and") && tokens.get(idx + 2) == Some(&"push") {
            return true;
        }
        if next.is_some_and(|next| *next == "the" || *next == "a" || *next == "an")
            && tokens
                .get(idx + 2)
                .is_some_and(|after_article| ARTICLE_OBJECTS.contains(after_article))
        {
            return true;
        }
        if tokens.len() == 1 {
            return true;
        }
        if prev.is_some_and(|prev| REQUEST_CUES.contains(prev)) && next.is_none() {
            return true;
        }
    }

    false
}

pub(super) fn mentions_git_commit(lower: &str) -> bool {
    const REQUEST_CUES: &[&str] = &[
        "please", "can", "could", "should", "must", "need", "needs", "needed", "want", "wants",
        "wanted", "just", "then", "now", "also", "to",
    ];
    const DIRECT_OBJECTS: &[&str] = &[
        "and", "it", "this", "that", "the", "my", "our", "these", "those", "changes", "change",
        "fix", "fixes", "work", "patch", "diff", "result", "results",
    ];
    const ARTICLE_OBJECTS: &[&str] = &["changes", "change", "fix", "patch", "diff", "work"];
    const EXCLUDED_OBJECTS: &[&str] = &[
        "message",
        "messages",
        "hash",
        "hashes",
        "sha",
        "id",
        "ids",
        "history",
        "histories",
        "log",
        "logs",
    ];
    let tokens = query_tokens(lower);

    for (idx, token) in tokens.iter().enumerate() {
        if *token != "commit" {
            continue;
        }
        if tokens.get(idx.wrapping_sub(1)) == Some(&"git") {
            return true;
        }
        let prev = idx.checked_sub(1).and_then(|prev_idx| tokens.get(prev_idx));
        let next = tokens.get(idx + 1);
        if next.is_some_and(|next| EXCLUDED_OBJECTS.contains(next)) {
            continue;
        }
        if next.is_some_and(|next| DIRECT_OBJECTS.contains(next)) {
            return true;
        }
        if next.is_some_and(|next| *next == "and") && tokens.get(idx + 2) == Some(&"push") {
            return true;
        }
        if next.is_some_and(|next| *next == "the" || *next == "a" || *next == "an")
            && tokens
                .get(idx + 2)
                .is_some_and(|after_article| ARTICLE_OBJECTS.contains(after_article))
        {
            return true;
        }
        if tokens.len() == 1 {
            return true;
        }
        if prev.is_some_and(|prev| REQUEST_CUES.contains(prev)) && next.is_none() {
            return true;
        }
    }

    false
}

pub(super) fn mentions_git_push(lower: &str) -> bool {
    const REQUEST_CUES: &[&str] = &[
        "please", "can", "could", "should", "must", "need", "needs", "needed", "want", "wants",
        "wanted", "just", "then", "now", "also", "to",
    ];
    const DIRECT_OBJECTS: &[&str] = &[
        "it", "this", "that", "the", "my", "our", "these", "those", "to", "upstream", "origin",
        "main", "master", "branch", "branches", "commit", "commits",
    ];
    const ARTICLE_OBJECTS: &[&str] = &["branch", "branches", "changes", "change", "commit"];
    const EXCLUDED_OBJECTS: &[&str] = &[
        "notification",
        "notifications",
        "state",
        "states",
        "event",
        "events",
        "message",
        "messages",
        "gateway",
        "gateways",
    ];
    let tokens = query_tokens(lower);

    for (idx, token) in tokens.iter().enumerate() {
        if *token != "push" {
            continue;
        }
        if tokens.get(idx.wrapping_sub(1)) == Some(&"git") {
            return true;
        }
        let prev = idx.checked_sub(1).and_then(|prev_idx| tokens.get(prev_idx));
        let next = tokens.get(idx + 1);
        if next.is_some_and(|next| EXCLUDED_OBJECTS.contains(next)) {
            continue;
        }
        if next.is_some_and(|next| DIRECT_OBJECTS.contains(next)) {
            return true;
        }
        if prev.is_some_and(|prev| *prev == "and")
            && idx
                .checked_sub(2)
                .and_then(|prev_prev_idx| tokens.get(prev_prev_idx))
                .is_some_and(|prev_prev| {
                    matches!(*prev_prev, "commit" | "tag" | "merge" | "branch")
                })
        {
            return true;
        }
        if next.is_some_and(|next| *next == "the" || *next == "a" || *next == "an")
            && tokens
                .get(idx + 2)
                .is_some_and(|after_article| ARTICLE_OBJECTS.contains(after_article))
        {
            return true;
        }
        if tokens.len() == 1 {
            return true;
        }
        if prev.is_some_and(|prev| REQUEST_CUES.contains(prev)) && next.is_none() {
            return true;
        }
    }

    false
}

pub(super) fn mentions_git_merge(lower: &str) -> bool {
    const DIRECT_OBJECTS: &[&str] = &[
        "branch", "branches", "pr", "prs", "change", "changes", "request", "requests", "into",
        "to", "main", "master", "it", "this", "that",
    ];
    const ARTICLE_OBJECTS: &[&str] = &[
        "branch", "pr", "pull", "request", "change", "changes", "commit", "commits",
    ];
    let tokens = query_tokens(lower);

    for (idx, token) in tokens.iter().enumerate() {
        if *token != "merge" {
            continue;
        }
        if tokens.get(idx.wrapping_sub(1)) == Some(&"git") {
            return true;
        }
        if let Some(next) = tokens.get(idx + 1) {
            if DIRECT_OBJECTS.contains(next) {
                return true;
            }
            if (*next == "the" || *next == "a" || *next == "an")
                && (tokens
                    .get(idx + 2)
                    .is_some_and(|after_article| ARTICLE_OBJECTS.contains(after_article))
                    || tokens
                        .get(idx + 3)
                        .is_some_and(|after_article| ARTICLE_OBJECTS.contains(after_article)))
            {
                return true;
            }
            if *next == "pull" && tokens.get(idx + 2) == Some(&"request") {
                return true;
            }
        }
    }

    false
}

pub(super) fn extract_probable_commit_ids(output: &str) -> Vec<CommitClaim> {
    let bytes = output.as_bytes();
    let mut ids = Vec::new();
    let mut start = None;

    for idx in 0..=bytes.len() {
        let is_hex = idx < bytes.len() && bytes[idx].is_ascii_hexdigit();
        match (start, is_hex) {
            (None, true) => start = Some(idx),
            (Some(begin), false) => {
                let current = &output[begin..idx];
                if has_commit_boundaries(output, begin, idx)
                    && (7..=40).contains(&current.len())
                    && current.chars().any(|ch| ch.is_ascii_alphabetic())
                    && !ids
                        .iter()
                        .any(|existing: &CommitClaim| existing.sha == current)
                {
                    ids.push(CommitClaim {
                        sha: current.to_string(),
                        repo_label: repo_label_before(output, begin),
                    });
                }
                start = None;
            }
            _ => {}
        }
    }

    ids
}

pub(super) fn commit_claim_matches_repo(claim: &CommitClaim, repo_root: &Path) -> bool {
    let Some(label) = claim.repo_label.as_deref() else {
        return true;
    };

    repo_label_matches_repo(label, repo_root)
}

fn query_tokens(lower: &str) -> Vec<&str> {
    lower
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect()
}

fn trim_before_inline_transcript_marker(paragraph: &str) -> &str {
    let marker_index = inline_transcript_markers()
        .iter()
        .filter_map(|marker| paragraph.find(marker))
        .min();
    match marker_index {
        Some(index) => paragraph[..index].trim_end(),
        None => paragraph,
    }
}

fn inline_transcript_markers() -> &'static [&'static str] {
    &[
        "\nWorkspace",
        "\n🤖 Assistant:",
        "\nAssistant:",
        "\nError:",
        "\nI will abide by",
        "\n[May ",
        " --- 🤖 Assistant:",
        " --- Assistant:",
        " --- Error:",
        " --- [May ",
    ]
}

fn is_transcript_paragraph(paragraph: &str) -> bool {
    let Some(first_line) = paragraph
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_ascii_lowercase)
    else {
        return false;
    };

    first_line == "workspace"
        || first_line.starts_with("🤖 assistant:")
        || first_line.starts_with("assistant:")
        || first_line.starts_with("error:")
        || first_line.starts_with("i will abide by")
        || first_line.starts_with("[may ")
}

fn has_commit_boundaries(output: &str, start: usize, end: usize) -> bool {
    let prev = output[..start].chars().next_back();
    let next = output[end..].chars().next();
    is_commit_boundary(prev) && is_commit_boundary(next)
}

fn is_commit_boundary(ch: Option<char>) -> bool {
    ch.is_none_or(|ch| !ch.is_ascii_alphanumeric())
}

fn repo_label_before(output: &str, commit_start: usize) -> Option<String> {
    adjacent_repo_label_before(output, commit_start)
        .or_else(|| repo_label_from_local_commit_context(output, commit_start))
}

fn is_repo_label_separator(ch: char) -> bool {
    ch.is_ascii_whitespace() || matches!(ch, '`' | '"' | '\'' | ':' | ';' | ',' | ')' | ']' | '}')
}

fn is_repo_label_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/')
}

fn looks_like_repo_label(token: &str) -> bool {
    token.contains('/')
        || token.contains('-')
        || token.contains('_')
        || token.ends_with(".git")
        || token.starts_with('.')
}

fn adjacent_repo_label_before(output: &str, commit_start: usize) -> Option<String> {
    let prefix = &output[..commit_start];
    let mut chars = prefix.char_indices().rev().peekable();

    while let Some((_, ch)) = chars.peek().copied() {
        if is_repo_label_separator(ch) {
            chars.next();
        } else {
            break;
        }
    }

    let mut start = None;
    let mut end = None;
    while let Some((idx, ch)) = chars.peek().copied() {
        if is_repo_label_char(ch) {
            start = Some(idx);
            end.get_or_insert(idx + ch.len_utf8());
            chars.next();
        } else {
            break;
        }
    }

    let (start, end) = match (start, end) {
        (Some(start), Some(end)) => (start, end),
        _ => return None,
    };
    let token = prefix[start..end].trim_matches(|ch: char| !is_repo_label_char(ch));
    normalized_repo_context_label(token)
}

fn repo_label_from_local_commit_context(output: &str, commit_start: usize) -> Option<String> {
    let sentence_start = output[..commit_start]
        .rmatch_indices(['\n', '.', '!', '?'])
        .next()
        .map(|(idx, _)| idx + 1)
        .unwrap_or(0);
    let sentence = output[sentence_start..commit_start].trim();
    let lower = sentence.to_ascii_lowercase();
    if !has_local_commit_context(&lower) {
        return None;
    }

    sentence
        .split_whitespace()
        .rev()
        .filter_map(normalized_repo_context_label)
        .next()
}

fn has_local_commit_context(lower: &str) -> bool {
    [
        "local commit",
        "one commit ahead",
        "local main",
        "did not push",
        "didn't push",
        "left local",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn normalized_repo_context_label(raw: &str) -> Option<String> {
    let token = raw.trim_matches(|ch: char| !is_repo_label_char(ch));
    if token.is_empty() || !looks_like_repo_label(token) || is_non_repo_context_label(token) {
        return None;
    }
    Some(token.to_string())
}

fn is_non_repo_context_label(token: &str) -> bool {
    matches!(
        token,
        "origin/main" | "origin/master" | "upstream/main" | "upstream/master"
    )
}

fn repo_label_matches_repo(label: &str, repo_root: &Path) -> bool {
    let normalized = normalize_repo_label(label);
    let Some(repo_name) = repo_root.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    normalize_repo_label(repo_name) == normalized
}

fn normalize_repo_label(label: &str) -> String {
    label
        .trim_end_matches(".git")
        .rsplit('/')
        .next()
        .unwrap_or(label)
        .trim_matches(|ch: char| !is_repo_label_char(ch))
        .to_string()
}
