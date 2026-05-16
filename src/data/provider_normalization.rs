use super::*;

fn normalize_loaded_codex_summary(session: &mut ChatSession) {
    let rollout_metadata = codex_rollout_sessions_root().and_then(|root| {
        session
            .codex_session_id
            .as_deref()
            .and_then(|session_id| load_codex_rollout_metadata_from_root(&root, session_id))
    });

    if session.json_summary.is_none() {
        let Some(metadata) = rollout_metadata.as_ref() else {
            return;
        };
        let mut summary = JsonSummary::new();
        summary.session_id = session.codex_session_id.clone();
        apply_codex_rollout_metadata(&mut summary, metadata);
        session.json_summary = Some(summary);
    }

    let Some(summary) = session.json_summary.as_mut() else {
        return;
    };

    if summary.session_id.is_none() {
        summary.session_id = session.codex_session_id.clone();
    }
    if let Some(metadata) = rollout_metadata.as_ref() {
        apply_codex_rollout_metadata(summary, metadata);
    } else if looks_like_legacy_codex_context_guess(summary) {
        summary.context_window_size = 0;
    }

    if !codex_summary_needs_normalization(summary) {
        return;
    }

    summary.context_window_tokens = 0;
    if summary.turn_history.is_empty() {
        return;
    }

    let authoritative_totals = CodexUsageTotals::from_summary(summary);
    let authoritative_duration_ms = summary.total_duration_ms;
    let authoritative_response_count = summary.response_count;
    let canonical = canonicalize_codex_turn_history(
        &summary.turn_history,
        authoritative_totals,
        authoritative_duration_ms,
        authoritative_response_count,
    );

    summary.turn_history = canonical.turns;
    canonical.totals.apply_to_summary(summary);
    summary.total_duration_ms = if authoritative_duration_ms > 0 {
        authoritative_duration_ms
    } else {
        summary
            .turn_history
            .iter()
            .map(|turn| turn.duration_ms)
            .sum()
    };
    if authoritative_response_count == 0 {
        summary.response_count = summary.turn_history.len() as u32;
    }
}

fn normalize_loaded_gemini_summary(session: &mut ChatSession) {
    let Some(summary) = session.json_summary.as_mut() else {
        return;
    };

    let context_window_size = gemini_context_window_size(summary.model.as_deref());
    if context_window_size > 0 && summary.context_window_size == 0 {
        summary.context_window_size = context_window_size;
    }

    if summary.context_window_tokens == 0 {
        if let Some(last_turn) = summary.turn_history.last() {
            if last_turn.context_window_tokens > 0 {
                summary.context_window_tokens = last_turn.context_window_tokens;
            } else if last_turn.input_tokens > 0 {
                summary.context_window_tokens = last_turn.input_tokens;
            }
        }
    }

    if summary.total_cost_usd == 0.0 {
        let turn_cost_sum: f64 = summary.turn_history.iter().map(|turn| turn.cost_usd).sum();
        if turn_cost_sum > 0.0 {
            summary.total_cost_usd = turn_cost_sum;
        } else if summary.total_input_tokens > 0 || summary.total_output_tokens > 0 {
            summary.total_cost_usd = compute_gemini_cost(
                summary.model.as_deref(),
                summary.total_input_tokens,
                summary.cache_read_tokens,
                summary.total_output_tokens,
            );
        }
    }

    if summary.response_count == 0 && !summary.turn_history.is_empty() {
        summary.response_count = summary.turn_history.len() as u32;
    }
}

impl ModelProvider {
    pub fn normalize_loaded_session(self, session: &mut ChatSession) {
        match self {
            ModelProvider::Codex | ModelProvider::Nvidia => normalize_loaded_codex_summary(session),
            ModelProvider::Gemini | ModelProvider::GeminiCliForge => {
                normalize_loaded_gemini_summary(session)
            }
            ModelProvider::Claude
            | ModelProvider::MiniMax
            | ModelProvider::Qwen
            | ModelProvider::OpenRouter
            | ModelProvider::OmniForge
            | ModelProvider::GeminiForge
            | ModelProvider::MiniMaxForge
            | ModelProvider::Kimi
            | ModelProvider::OpenCode
            | ModelProvider::Hermes
            | ModelProvider::Forge
            | ModelProvider::Claw => {}
        }
    }
}

fn codex_summary_needs_normalization(summary: &JsonSummary) -> bool {
    if summary.context_window_size == 0 {
        return true;
    }
    if summary.context_window_tokens > summary.context_window_size {
        return true;
    }
    summary.turn_history.iter().any(|turn| {
        turn.context_window_tokens > 0
            || turn.input_tokens > summary.context_window_size
            || turn.cache_read_tokens > turn.input_tokens
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct CodexRolloutMetadata {
    pub(super) model: Option<String>,
    pub(super) context_window_size: u64,
    pub(super) context_window_tokens: u64,
}

fn codex_rollout_sessions_root() -> Option<PathBuf> {
    let path = dirs::home_dir()?.join(".codex").join("sessions");
    path.is_dir().then_some(path)
}

pub(super) fn load_codex_rollout_metadata_from_root(
    root: &Path,
    session_id: &str,
) -> Option<CodexRolloutMetadata> {
    let rollout_path = find_codex_rollout_file(root, session_id)?;
    read_codex_rollout_metadata(&rollout_path)
}

fn find_codex_rollout_file(root: &Path, session_id: &str) -> Option<PathBuf> {
    let suffix = format!("-{session_id}.jsonl");
    let entries = fs::read_dir(root).ok()?;
    let mut candidates: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Recurse into directories
            if let Some(found) = find_codex_rollout_file(&path, session_id) {
                // Get modification time for comparison
                if let Ok(metadata) = found.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        candidates.push((found, modified));
                    }
                }
            }
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if file_name.ends_with(&suffix) {
            // Get modification time
            if let Ok(metadata) = path.metadata() {
                if let Ok(modified) = metadata.modified() {
                    candidates.push((path, modified));
                }
            }
        }
    }

    // Return the newest (most recent) matching file
    candidates
        .into_iter()
        .max_by_key(|(_, time)| *time)
        .map(|(path, _)| path)
}

fn read_codex_rollout_metadata(path: &Path) -> Option<CodexRolloutMetadata> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut metadata = CodexRolloutMetadata::default();

    for line in reader.lines().map_while(|line| line.ok()) {
        let Ok(json) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        if metadata.model.is_none() {
            metadata.model = json
                .pointer("/payload/model")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned);
        }

        if let Some(window) = json
            .pointer("/payload/info/model_context_window")
            .and_then(|value| value.as_u64())
        {
            metadata.context_window_size = window;
        }

        if let Some(tokens) = json
            .pointer("/payload/info/estimated_token_count")
            .and_then(|value| value.as_u64())
        {
            metadata.context_window_tokens = tokens;
        }
    }

    (metadata.model.is_some()
        || metadata.context_window_size > 0
        || metadata.context_window_tokens > 0)
        .then_some(metadata)
}

pub(super) fn apply_codex_rollout_metadata(
    summary: &mut JsonSummary,
    metadata: &CodexRolloutMetadata,
) {
    if summary.model.is_none() {
        summary.model = metadata.model.clone();
    }
    if metadata.context_window_size > 0 {
        summary.context_window_size = metadata.context_window_size;
    }
    if summary.context_window_tokens == 0 && metadata.context_window_tokens > 0 {
        summary.context_window_tokens = metadata.context_window_tokens;
    }
}

fn looks_like_legacy_codex_context_guess(summary: &JsonSummary) -> bool {
    summary.context_window_size == 1_050_000
        && summary.context_window_tokens == 0
        && summary
            .model
            .as_deref()
            .map(|model| model.to_ascii_lowercase().contains("gpt-5.4"))
            .unwrap_or(false)
}

#[derive(Clone, Copy, Debug, Default)]
struct CodexUsageTotals {
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_tokens: u64,
    cache_read_tokens: u64,
    cost_usd: f64,
}

impl CodexUsageTotals {
    fn from_summary(summary: &JsonSummary) -> Self {
        Self {
            input_tokens: summary.total_input_tokens,
            output_tokens: summary.total_output_tokens,
            cache_creation_tokens: summary.cache_creation_tokens,
            cache_read_tokens: summary.cache_read_tokens,
            cost_usd: summary.total_cost_usd,
        }
    }

    fn from_turn(turn: &TurnUsage) -> Self {
        Self {
            input_tokens: turn.input_tokens,
            output_tokens: turn.output_tokens,
            cache_creation_tokens: turn.cache_creation_tokens,
            cache_read_tokens: turn.cache_read_tokens,
            cost_usd: turn.cost_usd,
        }
    }

    fn is_zero(self) -> bool {
        self.input_tokens == 0
            && self.output_tokens == 0
            && self.cache_creation_tokens == 0
            && self.cache_read_tokens == 0
            && self.cost_usd.abs() < 1e-10
    }

    fn has_any_usage(self) -> bool {
        !self.is_zero()
    }

    fn added(self, other: Self) -> Self {
        Self {
            input_tokens: self.input_tokens.saturating_add(other.input_tokens),
            output_tokens: self.output_tokens.saturating_add(other.output_tokens),
            cache_creation_tokens: self
                .cache_creation_tokens
                .saturating_add(other.cache_creation_tokens),
            cache_read_tokens: self
                .cache_read_tokens
                .saturating_add(other.cache_read_tokens),
            cost_usd: self.cost_usd + other.cost_usd,
        }
    }

    fn saturating_sub(self, previous: Self) -> Self {
        Self {
            input_tokens: self.input_tokens.saturating_sub(previous.input_tokens),
            output_tokens: self.output_tokens.saturating_sub(previous.output_tokens),
            cache_creation_tokens: self
                .cache_creation_tokens
                .saturating_sub(previous.cache_creation_tokens),
            cache_read_tokens: self
                .cache_read_tokens
                .saturating_sub(previous.cache_read_tokens),
            cost_usd: (self.cost_usd - previous.cost_usd).max(0.0),
        }
    }

    fn cumulative_with(self, raw: Self) -> Self {
        Self {
            input_tokens: if raw.input_tokens > 0 {
                raw.input_tokens
            } else {
                self.input_tokens
            },
            output_tokens: if raw.output_tokens > 0 {
                raw.output_tokens
            } else {
                self.output_tokens
            },
            cache_creation_tokens: if raw.cache_creation_tokens > 0 {
                raw.cache_creation_tokens
            } else {
                self.cache_creation_tokens
            },
            cache_read_tokens: if raw.cache_read_tokens > 0 {
                raw.cache_read_tokens
            } else {
                self.cache_read_tokens
            },
            cost_usd: if raw.cost_usd > 0.0 {
                raw.cost_usd
            } else {
                self.cost_usd
            },
        }
    }

    fn apply_to_turn(self, turn: &mut TurnUsage) {
        turn.input_tokens = self.input_tokens;
        turn.output_tokens = self.output_tokens;
        turn.cache_creation_tokens = self.cache_creation_tokens;
        turn.cache_read_tokens = self.cache_read_tokens;
        turn.cost_usd = self.cost_usd;
        turn.context_window_tokens = 0;
        for tool_call in &mut turn.tool_calls {
            tool_call.input_tokens = self.input_tokens;
            tool_call.output_tokens = self.output_tokens;
            tool_call.cache_creation_tokens = self.cache_creation_tokens;
            tool_call.cache_read_tokens = self.cache_read_tokens;
        }
    }

    fn apply_to_summary(self, summary: &mut JsonSummary) {
        summary.total_input_tokens = self.input_tokens;
        summary.total_output_tokens = self.output_tokens;
        summary.cache_creation_tokens = self.cache_creation_tokens;
        summary.cache_read_tokens = self.cache_read_tokens;
        summary.total_cost_usd = self.cost_usd;
    }

    fn exceeds(self, other: Self) -> bool {
        self.input_tokens > other.input_tokens
            || self.output_tokens > other.output_tokens
            || self.cache_creation_tokens > other.cache_creation_tokens
            || self.cache_read_tokens > other.cache_read_tokens
            || self.cost_usd > other.cost_usd + 1e-9
    }

    fn matches(self, other: Self) -> bool {
        self.input_tokens == other.input_tokens
            && self.output_tokens == other.output_tokens
            && self.cache_creation_tokens == other.cache_creation_tokens
            && self.cache_read_tokens == other.cache_read_tokens
            && (self.cost_usd - other.cost_usd).abs() < 1e-9
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodexTurnMode {
    Cumulative,
    Delta,
}

#[derive(Debug)]
struct CanonicalizedCodexHistory {
    turns: Vec<TurnUsage>,
    totals: CodexUsageTotals,
}

fn codex_turn_mode(
    effective: CodexUsageTotals,
    authoritative: CodexUsageTotals,
    raw: CodexUsageTotals,
) -> CodexTurnMode {
    let token_goes_back = (raw.input_tokens > 0 && raw.input_tokens < effective.input_tokens)
        || (raw.output_tokens > 0 && raw.output_tokens < effective.output_tokens)
        || (raw.cache_creation_tokens > 0
            && raw.cache_creation_tokens < effective.cache_creation_tokens)
        || (raw.cache_read_tokens > 0 && raw.cache_read_tokens < effective.cache_read_tokens);

    if token_goes_back {
        return CodexTurnMode::Delta;
    }

    let delta_effective = effective.added(raw);
    let cumulative_effective = effective.cumulative_with(raw);

    let delta_overshoots = authoritative.has_any_usage() && delta_effective.exceeds(authoritative);
    let cumulative_overshoots =
        authoritative.has_any_usage() && cumulative_effective.exceeds(authoritative);

    match (delta_overshoots, cumulative_overshoots) {
        (true, false) => CodexTurnMode::Cumulative,
        (false, true) => CodexTurnMode::Delta,
        _ => CodexTurnMode::Cumulative,
    }
}

fn codex_rollup_turn(
    template: Option<&TurnUsage>,
    totals: CodexUsageTotals,
    duration_ms: u64,
    num_turns: u32,
) -> TurnUsage {
    let mut turn = template.cloned().unwrap_or_default();
    turn.duration_ms = duration_ms;
    turn.num_turns = num_turns;
    turn.tool_calls.clear();
    totals.apply_to_turn(&mut turn);
    turn
}

fn codex_turn_history_totals(turns: &[TurnUsage]) -> CodexUsageTotals {
    turns.iter().fold(CodexUsageTotals::default(), |acc, turn| {
        acc.added(CodexUsageTotals::from_turn(turn))
    })
}

fn canonicalize_codex_turn_history(
    turns: &[TurnUsage],
    authoritative: CodexUsageTotals,
    duration_ms: u64,
    response_count: u32,
) -> CanonicalizedCodexHistory {
    let mut effective = CodexUsageTotals::default();
    let mut normalized = Vec::new();
    let mut had_nonzero_delta_rows = false;

    for raw_turn in turns {
        let raw_totals = CodexUsageTotals::from_turn(raw_turn);
        let has_payload =
            !raw_totals.is_zero() || raw_turn.duration_ms > 0 || !raw_turn.tool_calls.is_empty();
        if !has_payload {
            continue;
        }

        let mode = codex_turn_mode(effective, authoritative, raw_totals);
        if mode == CodexTurnMode::Delta && raw_totals.has_any_usage() {
            had_nonzero_delta_rows = true;
        }

        let delta_totals = match mode {
            CodexTurnMode::Cumulative => raw_totals.saturating_sub(effective),
            CodexTurnMode::Delta => raw_totals,
        };

        let mut normalized_turn = raw_turn.clone();
        delta_totals.apply_to_turn(&mut normalized_turn);
        normalized.push(normalized_turn);

        effective = match mode {
            CodexTurnMode::Cumulative => effective.cumulative_with(raw_totals),
            CodexTurnMode::Delta => effective.added(raw_totals),
        };
    }

    let totals = codex_turn_history_totals(&normalized);
    let authoritative_missing = !authoritative.has_any_usage();
    let fully_cumulative = !had_nonzero_delta_rows;
    let recovered_exact = !authoritative_missing && totals.matches(authoritative);
    let recovered_from_cumulative =
        !authoritative_missing && fully_cumulative && !totals.exceeds(authoritative);

    if normalized.is_empty()
        || (!recovered_exact && !recovered_from_cumulative && !authoritative_missing)
    {
        let rollup_totals = if authoritative_missing {
            totals
        } else {
            authoritative
        };
        let rollup_duration_ms = if duration_ms > 0 {
            duration_ms
        } else {
            turns.iter().map(|turn| turn.duration_ms).sum()
        };
        return CanonicalizedCodexHistory {
            turns: vec![codex_rollup_turn(
                turns.last(),
                rollup_totals,
                rollup_duration_ms,
                response_count,
            )],
            totals: rollup_totals,
        };
    }

    let reconciled_totals = totals;
    CanonicalizedCodexHistory {
        turns: normalized,
        totals: reconciled_totals,
    }
}

fn gemini_context_window_size(model: Option<&str>) -> u64 {
    let normalized = model.unwrap_or_default().to_ascii_lowercase();
    if normalized.contains("gemini-3.1-pro")
        || normalized.contains("gemini-3-pro")
        || normalized.contains("gemini-3.1-flash")
        || normalized.contains("gemini-3-flash")
    {
        1_048_576
    } else {
        0
    }
}

fn compute_gemini_cost(
    model: Option<&str>,
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
) -> f64 {
    let normalized = model.unwrap_or_default().to_ascii_lowercase();
    let (input_rate, cached_input_rate, output_rate) =
        if normalized.contains("gemini-3.1-pro") || normalized.contains("gemini-3-pro") {
            if input_tokens > 200_000 {
                (4.0, 0.4, 18.0)
            } else {
                (2.0, 0.2, 12.0)
            }
        } else if normalized.contains("gemini-3.1-flash") || normalized.contains("gemini-3-flash") {
            (0.5, 0.05, 3.0)
        } else {
            return 0.0;
        };

    let uncached_input_tokens = input_tokens.saturating_sub(cached_input_tokens);
    let m = 1_000_000.0;
    (uncached_input_tokens as f64 / m) * input_rate
        + (cached_input_tokens as f64 / m) * cached_input_rate
        + (output_tokens as f64 / m) * output_rate
}
