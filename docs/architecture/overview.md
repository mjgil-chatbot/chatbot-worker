# Current Architecture

This repo owns the worker/runtime surface shared by `rust-chatbot` and
`auto-runs`.

## Main Components

- `chatbot_lib` is the shared Rust library used by both sibling apps.
- `chatbot-worker` is the canonical detached worker binary for chat-response
  execution.
- `chatbot_worker` is the library entrypoint that `chatbot-worker` and the
  thin `rust-chatbot` worker wrapper both call.
- `worker_process` holds generic worker-process bootstrap helpers that
  detached workers can share without pulling UI code across repo boundaries.

## Ownership Boundary

This repo owns:

- provider transport setup and shared runtime config
- worker spool types and atomic spool IO helpers
- worker spool schema-version validation for `request.json`, `state.json`, and
  `result.json`
- shared sync/storage helpers used by multiple binaries
- hook loading/execution and background worker fan-out helpers
- memory/runtime support needed on worker paths
- the layered workspace-memory runtime that now combines pinned manual facts,
  a local `agentic_memory` JSON store, and remote MemMachine episodic search
- provider-specific reasoning defaults for Codex-compatible transports (for example, `Codex`/`Nvidia` and `MiniMax` now track separate knobs)

This repo does not own:

- the `rust-chatbot` desktop UI
- the `auto-runs` coordination engine or its run/task state
- MCP server binaries that remain app-specific

## Shared Runtime Root

The shared runtime now treats a host-managed shared runtime root as the
canonical location for provider settings, provider session data, API keys,
`.mcp.json`, hook config, and the main `config.toml`.

Provider-specific config files and desktop-session state both live under that
root. The worker still honors legacy fallback sources while downstream apps
finish migrating to the shared runtime layout.

MiniMax still runs through the Claude-compatible transport, but its runtime
state is isolated under its own provider-specific subtree instead of sharing
the default Claude state. To keep operator instructions stable across new and
resumed MiniMax sessions, the provider bridge now prepends the global Claude
instructions file when it exists, then appends the supported workspace-local
instruction files before each MiniMax request.

For adapter-backed desktop runs, the worker now overrides MiniMax's provider
executable to a local compatibility wrapper instead of calling the native
`claude` binary directly. That wrapper resolves `chatbot-cli` from sibling
checkout artifacts first and falls back to `PATH`, writes a
temporary MiniMax provider config under isolated XDG config/data roots for the
child process, and now speaks `chatbot acp` over JSON-RPC stdio instead of
parsing `chatbot run --output stream-json`. The bridge sends
`initialize`, `session/create` or `session/resume`, then `prompt`, and maps
ACP notifications such as `tool_call_started`, `tool_call_finished`, `usage`,
and `text_delta` back into the worker's live transcript contract. MiniMax now
uses `https://api.minimax.io/anthropic` consistently across the worker catalog, the
bridge wrapper, and `chatbot-cli`'s provider presets while still sending the
existing Claude-compatible request contract with bearer auth. The shared
Claude-compatible bridge continues to rely on `chatbot-cli`'s canonical
session store for transcript-backed resume instead of maintaining a separate
MiniMax-only transcript shim. Adapter-backed terminal persistence also strips
a leading `<think>...</think>` reasoning block from the finalized assistant
body before it writes the durable spool output, so saved sessions keep the
visible answer or transcript rather than the provider's hidden reasoning
preamble.

During the split transition, path resolution still falls back to legacy
repo-local config files in the sibling repos and legacy host-managed provider
state when the canonical shared-root path is missing.
`scripts/migrate-home-state-to-shared-root.sh` migrates the current machine's
runtime-relevant files and legacy provider data into the shared runtime root
without pulling repo-specific skills or other unrelated tooling into that
root.

## Worker Spool Contract

Detached turns persist into a per-chat worker spool directory under the shared
runtime root.

The stable artifacts are:

- `request.json`: the submitted `WorkerRequest`
- `state.json`: the latest `WorkerJobSnapshot`
- `result.json`: completion metadata including session IDs, summaries, and
  terminal errors
- `events.jsonl`: the structured live transcript used for polling and replay
- `output`: the persisted turn body assembled from transcript preamble plus
  sanitized final output

The worker treats `events.jsonl` as the durable incremental transcript and can
reconstruct visible output from it when the flat `output` file is missing or
empty.

Some failure paths also persist bounded diagnostics beside those core
artifacts:

- `transport-errors.log` for startup-time adapter transport retries
- `git-verifier-context.json` plus raw adapter/provider JSONL captures when a
  git side-effect verification fails
- temporary request-only directories that are cleaned automatically when a
  detached worker never produces `state.json`

The finalized output writer strips a leading hidden-reasoning
`<think>...</think>` block from provider final text, then keeps the transcript
above the clean final answer instead of duplicating the answer in both
sections of the persisted turn.

## Integration Model

`rust-chatbot` keeps a small compatibility library that re-exports this crate's
shared modules plus its remaining app-local modules. Its local
`src/bin/chatbot_worker.rs` is now a thin wrapper around
`chatbot_lib::chatbot_worker::main()`.

Within that shared surface, `src/memory/` owns the workspace-memory layering
used by `rust-chatbot`:

- `manual-memories.json` keeps explicit pinned facts under the shared infra
  root
- `agent-memory-store.json` keeps a rebuilt local `agentic_memory`
  `InMemoryAgentStore` derived from authoritative session JSON
- the `agentic_memory` implementation itself is now vendored under
  `crates/agentic_memory` so public builds stay self-contained
- MemMachine remains the remote episodic backend for workspace-scoped chat
  message search

Retrieval now merges those layers in that order, deduplicating by `doc_key` so
local agent-memory packet hits can override overlapping remote MemMachine
records without mutating the underlying remote episode.

The shared MemMachine client keeps short timeouts on status/search requests so
prompt-time retrieval fails open quickly, while indexing writes use a longer
timeout because large workspace rebuilds can take several minutes server-side.

On each provider-stream session, the worker keeps the most recent provider session
ID seen in stream messages and persists that ID in spool result snapshots, even
when the final stream event does not include a session ID. This avoids dropping
resume metadata between turns and helps keep Codex sessions reusable across
messages.

Adapter-backed runs now also retry one transport-marked provider failure when
the failed attempt emitted no assistant text yet. Those retries are written to
the per-chat spool as `transport-errors.log`, mirrored into the rotating app
log, and summarized into `events.jsonl` so a single transient transport error
does not immediately hard-fail the turn.

Foreground adapter-backed turns now keep retrying those startup-time transport
failures until the provider run succeeds or the user explicitly stops the turn.
The retry classifier accepts both explicit `Transport error` details and the
adapter's transport exit code `8`, which some `agent-adapter-cli` failures
surface only as the generic terminal text `Run command failed`.

When a provider emits a structured raw result payload with `is_error = true`
and an `errors[]` array, the worker now lifts that provider-supplied detail
into the terminal session error instead of letting a later generic
`RunFailed`/stderr wrapper overwrite it. Resume failures such as “No
conversation found with session ID ...” therefore persist into the chat
transcript as the visible `exit_code:<n>:` detail.

The runtime now also exposes `GeminiForge` and `MiniMaxForge` as additive
provider identities. They leave the existing native `Gemini` and `MiniMax`
routes alone and instead use a dedicated OpenRouter-style subprocess transport
family that launches Forge's sibling `rust-chatbot` compatibility binary and
consumes its JSONL `StreamMessage` contract. For those Forge-backed routes, the
worker no longer forces a provider base URL on the subprocess; the compatibility
binary resolves its own default or `chatbot`-config override instead.

`auto-runs` depends on this repo directly for shared worker/runtime modules.
Its `auto-runs-worker` binary remains local because it closes over
`auto-runs` coordination state, but it now uses `worker_process` helpers from
this repo for detached worker bootstrap.

The adapter-backed git side-effect verifier also lives here. It snapshots the
current workspace repo before a mutating run and validates the post-run git
state against explicit `commit` / `push` / `tag` / `merge` / branch-creation
claims. Intent detection now only scans the user-authored request paragraphs
that appear before pasted transcript markers such as `Workspace` or assistant
error blocks, including inline pasted suffixes like `fix this --- 🤖 Assistant:
...`, so quoted examples like `commit and push` or `merge the feature branch`
inside a pasted failure report do not arm the verifier. When the final
assistant text names commit hashes for multiple repos, the verifier scopes hash
checks to commits labeled for the current repo so a sibling repo hash in the
summary does not incorrectly fail the active workspace's verification. Commit
claim extraction also requires standalone 7-40 character hex tokens, which
prevents plain words like `succeeded` from being misread as fake hashes such as
`cceeded`. Narrative local-only notes such as `chatbot-worker local branch contains
an unpushed commit abc1234` also inherit the sibling
repo label from the surrounding sentence, so an unpushed SHA for another repo
does not trip verification in the active workspace.

## Test and Verification Model

This repo carries a large amount of env-sensitive runtime logic, so tests are
designed around deterministic shared-state boundaries rather than assuming a
pristine machine:

- tests that rewrite `HOME`, `PATH`, provider CLI env vars, or shared runtime
  roots serialize through a repo-wide env mutex
- worker spool tests isolate `HOME` when they need to pin canonical versus
  legacy runtime-root behavior
- background dispatcher tests also serialize their fake provider CLI env so
  bounded-concurrency assertions remain meaningful under the full suite
- cgroup helper unit tests run against the test-mode no-op wrapper instead of
  requiring a real `systemd-run` user-session environment

That design keeps the public `cargo test --locked` path deterministic even
when the suite mixes adapter, provider-bridge, memory, and worker-spool
coverage in a single process.

## Verification

This repo also carries `scripts/verify-sibling-builds.sh`, a small operator
script that runs one Cargo verification pass across `chatbot-worker`,
`rust-chatbot`, and `auto-runs` from the extracted worker repo.

The local pre-commit path is now backed by a repo-specific engineering gate:

- `.github/workflows/engineering-gate.yml`
- `contracts/engineering-gate.toml`
- `scripts/verify-engineering-gate.sh`
- `.githooks/pre-commit`

That gate is intentionally repo-aware. It only accepts `chatbot-worker` as the
repo root, matches the staged file set against worker-owned contract globs, and
can dispatch both focused spool-schema tests and the sibling boundary check
without borrowing `rust-chatbot`'s registry.

Remote verification now reuses the same `scripts/verify-engineering-gate.sh`
entrypoint from `.github/workflows/engineering-gate.yml` with a diff range.
That workflow now bootstraps the expected public sibling repositories through
`scripts/bootstrap-ci-siblings.sh` before running the gate so worker-owned
sibling-boundary checks stay intact in CI instead of being weakened for
remote execution.

General build-and-test CI now lives in `.github/workflows/ci.yml`. It uses
that same sibling bootstrap step, then runs the Python release checks, local
Cargo tests/builds, and the targeted worker/desktop sibling-boundary check.

For open-source release hygiene, the public docs intentionally describe runtime
state in boundary terms and direct fresh clones through
`scripts/bootstrap-ci-siblings.sh` instead of hard-coding machine-local hidden
directory names or developer-specific checkout paths.

The sibling-build helper now also assigns a repo-specific `CARGO_TARGET_DIR`
under each checked repo so one repo's verification run does not block or
overwrite another repo's release artifacts.

The gate-facing sibling check now narrows that script to the worker/desktop
pair explicitly, so unrelated `auto-runs` breakage cannot block a
`chatbot-worker`-only contract update.
