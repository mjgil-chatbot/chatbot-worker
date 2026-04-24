use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
#[path = "adapter_git_verifier_text.rs"]
mod text;
use text::{
    commit_claim_matches_repo, extract_probable_commit_ids, intent_query_text,
    mentions_branch_creation, mentions_git_commit, mentions_git_merge, mentions_git_push,
    mentions_git_tag,
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
struct GitSideEffectIntent {
    require_commit: bool,
    require_push: bool,
    require_tag: bool,
    require_merge: bool,
    require_branch_create: bool,
}
impl GitSideEffectIntent {
    fn from_query(query: &str) -> Option<Self> {
        let lower = intent_query_text(query).to_ascii_lowercase();
        let intent = Self {
            require_commit: mentions_git_commit(&lower),
            require_push: mentions_git_push(&lower),
            require_tag: mentions_git_tag(&lower),
            require_merge: mentions_git_merge(&lower),
            require_branch_create: mentions_branch_creation(&lower),
        };
        (!intent.is_empty()).then_some(intent)
    }}
impl GitSideEffectIntent {
    fn is_empty(self) -> bool {
        !self.require_commit
            && !self.require_push
            && !self.require_tag
            && !self.require_merge
            && !self.require_branch_create
    }}
impl GitSideEffectIntent {
    fn describe(self) -> String {
        let mut parts = Vec::new();
        if self.require_commit {
            parts.push("commit");
        }
        if self.require_push {
            parts.push("push");
        }
        if self.require_tag {
            parts.push("tag");
        }
        if self.require_merge {
            parts.push("merge");
        }
        if self.require_branch_create {
            parts.push("branch creation");
        }
        if parts.is_empty() {
            return "git side effects".to_string();
        }
        parts.join(", ")
    }}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct GitRepoSnapshot {
    repo_root: PathBuf,
    head_sha: Option<String>,
    branch: Option<String>,
    upstream_ref: Option<String>,
    upstream_sha: Option<String>,
    tags: BTreeSet<String>,
    local_branches: BTreeSet<String>,
}
impl GitRepoSnapshot {
    fn capture(cwd: &Path) -> Result<Option<Self>> {
        let Some(repo_root) = git_optional_stdout(cwd, &["rev-parse", "--show-toplevel"])? else {
            return Ok(None);
        };
        let repo_root = PathBuf::from(repo_root);
        let head_sha = git_optional_stdout(&repo_root, &["rev-parse", "--verify", "HEAD"])?;
        let branch =
            git_optional_stdout(&repo_root, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        let upstream_ref = git_optional_stdout(
            &repo_root,
            &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
        )?
        .or_else(|| {
            branch
                .as_ref()
                .and_then(|branch| fallback_origin_ref(&repo_root, branch))
        });
        let upstream_sha = upstream_ref
            .as_deref()
            .map(|upstream_ref| {
                git_optional_stdout(&repo_root, &["rev-parse", "--verify", upstream_ref])
            })
            .transpose()?
            .flatten();
        let tags = git_ref_set(&repo_root, &["tag", "--list"])?;
        let local_branches = git_ref_set(
            &repo_root,
            &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
        )?;

        Ok(Some(Self {
            repo_root,
            head_sha,
            branch,
            upstream_ref,
            upstream_sha,
            tags,
            local_branches,
        }))
    }}
fn fallback_origin_ref(repo_root: &Path, branch: &str) -> Option<String> {
    let ref_name = format!("refs/remotes/origin/{branch}");
    git_optional_stdout(repo_root, &["rev-parse", "--verify", &ref_name])
        .ok()
        .flatten()
        .map(|_| ref_name)
}
#[cfg(test)]
fn git_stdout(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(
            "git {} failed in {}: {}",
            args.join(" "),
            cwd.display(),
            stderr
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
fn git_optional_stdout(cwd: &Path, args: &[&str]) -> Result<Option<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!stdout.is_empty()).then_some(stdout))
}
fn git_ref_set(cwd: &Path, args: &[&str]) -> Result<BTreeSet<String>> {
    let stdout = git_optional_stdout(cwd, args)?.unwrap_or_default();
    Ok(stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}
fn commit_exists(repo_root: &Path, sha: &str) -> Result<bool> {
    Ok(git_optional_stdout(repo_root, &["rev-parse", "--verify", sha])?.is_some())
}
fn head_changed(before: Option<&GitRepoSnapshot>, after: &GitRepoSnapshot) -> bool {
    match (
        before.and_then(|snapshot| snapshot.head_sha.as_deref()),
        after.head_sha.as_deref(),
    ) {
        (Some(before_head), Some(after_head)) => before_head != after_head,
        (None, Some(_)) => true,
        _ => false,
    }
}
fn short_sha(sha: &str) -> &str {
    let end = sha.len().min(7);
    &sha[..end]
}
#[derive(Clone, Debug, Default)]
pub(crate) struct RawJsonlCapture {
    total_bytes: usize,
    lines: VecDeque<String>,
}
impl RawJsonlCapture {
    const MAX_CAPTURE_BYTES: usize = 256 * 1024;}
impl RawJsonlCapture {
    pub(crate) fn push_line(&mut self, line: impl Into<String>) {
        let line = line.into();
        self.total_bytes = self.total_bytes.saturating_add(line.len() + 1);
        self.lines.push_back(line);
        while self.total_bytes > Self::MAX_CAPTURE_BYTES {
            let Some(removed) = self.lines.pop_front() else {
                break;
            };
            self.total_bytes = self.total_bytes.saturating_sub(removed.len() + 1);
        }
    }}
impl RawJsonlCapture {
    pub(crate) fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }}
impl RawJsonlCapture {
    pub(crate) fn as_text(&self) -> String {
        if self.lines.is_empty() {
            return String::new();
        }
        let mut text = self.lines.iter().cloned().collect::<Vec<_>>().join("\n");
        text.push('\n');
        text
    }}
pub(crate) fn raw_adapter_events_path(spool_dir: &Path) -> PathBuf {
    spool_dir.join("adapter-events.raw.jsonl")
}
pub(crate) fn raw_provider_events_path(spool_dir: &Path) -> PathBuf {
    spool_dir.join("provider-events.raw.jsonl")
}
pub(crate) fn git_verifier_context_path(spool_dir: &Path) -> PathBuf {
    spool_dir.join("git-verifier-context.json")
}
#[derive(Debug, Serialize)]
struct GitVerifierContextArtifact<'a> {
    request_query: &'a str,
    adapter_prompt: &'a str,
    cwd: &'a Path,
    intent: GitSideEffectIntent,
    before: Option<&'a GitRepoSnapshot>,
}
pub(crate) fn persist_failed_verification_artifacts(
    spool_dir: &Path,
    request_query: &str,
    adapter_prompt: &str,
    guard: &GitSideEffectGuard,
    raw_adapter_events: &RawJsonlCapture,
    raw_provider_events: &RawJsonlCapture,
) -> Result<()> {
    let context = GitVerifierContextArtifact {
        request_query,
        adapter_prompt,
        cwd: &guard.cwd,
        intent: guard.intent,
        before: guard.before.as_ref(),
    };
    crate::worker::write_text_atomic(
        &git_verifier_context_path(spool_dir),
        &serde_json::to_string_pretty(&context)?,
    )
    .with_context(|| {
        format!(
            "failed to persist {}",
            git_verifier_context_path(spool_dir).display()
        )
    })?;

    if !raw_adapter_events.is_empty() {
        crate::worker::write_text_atomic(
            &raw_adapter_events_path(spool_dir),
            &raw_adapter_events.as_text(),
        )
        .with_context(|| {
            format!(
                "failed to persist {}",
                raw_adapter_events_path(spool_dir).display()
            )
        })?;
    }

    if !raw_provider_events.is_empty() {
        crate::worker::write_text_atomic(
            &raw_provider_events_path(spool_dir),
            &raw_provider_events.as_text(),
        )
        .with_context(|| {
            format!(
                "failed to persist {}",
                raw_provider_events_path(spool_dir).display()
            )
        })?;
    }

    Ok(())
}
#[derive(Clone, Debug)]
pub(crate) struct GitSideEffectGuard {
    intent: GitSideEffectIntent,
    before: Option<GitRepoSnapshot>,
    cwd: PathBuf,
}
impl GitSideEffectGuard {
    pub(crate) fn prepare(query: &str, cwd: &Path) -> Result<Option<Self>> {
        let Some(intent) = GitSideEffectIntent::from_query(query) else {
            return Ok(None);
        };
        Ok(Some(Self {
            intent,
            before: GitRepoSnapshot::capture(cwd)?,
            cwd: cwd.to_path_buf(),
        }))
    }}
impl GitSideEffectGuard {
    pub(crate) fn verify_success(&self, output: &str) -> Result<()> {
        let after = GitRepoSnapshot::capture(&self.cwd)?;
        let Some(after) = after else {
            bail!(
                "git side-effect verification failed: requested {} but {} is not a git repository after the run",
                self.intent.describe(),
                self.cwd.display()
            );
        };

        let mut problems = Vec::new();
        if self.intent.require_commit {
            match (&self.before, &after.head_sha) {
                (_, None) => {
                    problems.push("repository has no HEAD commit after the run".to_string())
                }
                (Some(before), Some(after_head))
                    if before.head_sha.as_deref() == Some(after_head.as_str()) =>
                {
                    problems.push(format!("HEAD stayed at {}", short_sha(after_head)));
                }
                _ => {}
            }
        }

        if self.intent.require_merge && !head_changed(self.before.as_ref(), &after) {
            if let Some(after_head) = after.head_sha.as_deref() {
                problems.push(format!(
                    "merge claim not verified because HEAD stayed at {}",
                    short_sha(after_head)
                ));
            } else {
                problems.push(
                    "merge claim not verified because the repository has no HEAD after the run"
                        .to_string(),
                );
            }
        }

        if self.intent.require_push {
            match (&after.head_sha, &after.upstream_ref, &after.upstream_sha) {
                (None, _, _) => {
                    problems.push("repository has no HEAD commit after the run".to_string());
                }
                (_, None, _) => {
                    problems.push("no upstream ref is available to verify the push".to_string());
                }
                (Some(head_sha), Some(upstream_ref), Some(upstream_sha))
                    if upstream_sha != head_sha =>
                {
                    problems.push(format!(
                        "upstream {} points to {} while HEAD is {}",
                        upstream_ref,
                        short_sha(upstream_sha),
                        short_sha(head_sha)
                    ));
                }
                (Some(_), Some(_), Some(_)) => {
                    if let Some(before) = &self.before {
                        if before.head_sha == after.head_sha
                            && before.upstream_sha == after.upstream_sha
                        {
                            problems
                                .push("HEAD and upstream ref both stayed unchanged".to_string());
                        }
                    }
                }
                _ => {
                    problems.push("upstream ref could not be resolved after the run".to_string());
                }
            }
        }

        if self.intent.require_tag
            && self
                .before
                .as_ref()
                .is_some_and(|before| before.tags == after.tags)
        {
            problems.push("tag claim not verified because no new tag was created".to_string());
        }

        if self.intent.require_branch_create
            && self
                .before
                .as_ref()
                .is_some_and(|before| before.local_branches == after.local_branches)
        {
            problems.push(
                "branch creation claim not verified because no new local branch was created"
                    .to_string(),
            );
        }

        let missing_claims = extract_probable_commit_ids(output)
            .into_iter()
            .filter(|claim| commit_claim_matches_repo(claim, &after.repo_root))
            .filter_map(|claim| match commit_exists(&after.repo_root, &claim.sha) {
                Ok(true) => None,
                Ok(false) => Some(Ok(claim.sha)),
                Err(err) => Some(Err(err)),
            })
            .collect::<Result<Vec<_>>>()?;
        if !missing_claims.is_empty() {
            problems.push(format!(
                "assistant referenced missing commit(s): {}",
                missing_claims.join(", ")
            ));
        }

        if problems.is_empty() {
            return Ok(());
        }

        Err(anyhow!(
            "git side-effect verification failed for requested {} in {}: {}",
            self.intent.describe(),
            after.repo_root.display(),
            problems.join("; ")
        ))
    }}
