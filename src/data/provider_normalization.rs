use super::*;
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
#[derive(Clone, Copy, Debug, Default)]
struct CodexUsageTotals {
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_tokens: u64,
    cache_read_tokens: u64,
    cost_usd: f64,
}
impl CodexUsageTotals {
    fn is_zero(self) -> bool {
        self.input_tokens == 0
            && self.output_tokens == 0
            && self.cache_creation_tokens == 0
            && self.cache_read_tokens == 0
            && self.cost_usd.abs() < 1e-10
    }}
impl CodexUsageTotals {
    fn has_any_usage(self) -> bool {
        !self.is_zero()
    }}
impl CodexUsageTotals {
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
    }}
impl CodexUsageTotals {
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
    }}
impl CodexUsageTotals {
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
    }}
impl CodexUsageTotals {
    fn exceeds(self, other: Self) -> bool {
        self.input_tokens > other.input_tokens
            || self.output_tokens > other.output_tokens
            || self.cache_creation_tokens > other.cache_creation_tokens
            || self.cache_read_tokens > other.cache_read_tokens
            || self.cost_usd > other.cost_usd + 1e-9
    }}
impl CodexUsageTotals {
    fn matches(self, other: Self) -> bool {
        self.input_tokens == other.input_tokens
            && self.output_tokens == other.output_tokens
            && self.cache_creation_tokens == other.cache_creation_tokens
            && self.cache_read_tokens == other.cache_read_tokens
            && (self.cost_usd - other.cost_usd).abs() < 1e-9
    }}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodexTurnMode {
    Cumulative,
    Delta,
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
