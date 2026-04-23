# MiniMax ACP Bridge

MiniMax desktop adapter runs use `scripts/minimax-chatbot-cli-adapter.py`,
which delegates to the shared `scripts/claude-compat-chatbot-cli-adapter.py`
ACP bridge.

The adapter now sets `CHATBOT_CLAUDE_COMPAT_BASE_URL` to MiniMax's documented
Anthropic-compatible endpoint, `https://api.minimax.io/anthropic`, so the ACP
bridge and the worker catalog use the same transport base URL.

## Request Flow

Each MiniMax desktop run uses the Claude-compatible ACP bridge in this order:

1. launch `chatbot acp` with isolated provider-specific config/data roots
2. send `initialize`
3. send `session/create` for new turns or `session/resume` for resumed turns
4. send `prompt`
5. stream ACP notifications back into the worker transcript/spool contract
6. send `session/end` during normal cleanup

The worker maps ACP notifications such as `tool_call_started`,
`tool_call_finished`, `usage`, `text_delta`, and terminal completion/failure
events into the same `events.jsonl` schema used by other adapter-backed
providers.

## Session Identity

The bridge no longer keeps a locally generated placeholder session ID after
`session/create`. Instead it adopts the `sessionId` returned by `chatbot acp`
and emits that value in the adapter `session_meta` event.

That same ACP-issued session ID is then reused for:

- later `session/resume` requests from `rust-chatbot`
- the follow-up `prompt` request that actually launches the turn
- `session/end` cleanup after the run completes

This keeps MiniMax multi-turn desktop chats aligned with the transcript that
`chatbot-cli` actually persisted, instead of resuming a non-existent local
placeholder session.

If `chatbot acp` rejects `session/resume` with a normal JSON-RPC error such as
`Session not found`, the bridge now keeps using that same ACP process for the
documented `session/create` fallback instead of treating the resume miss as a
transport-fatal condition.

Provider-supplied structured errors also win over later generic wrapper text
when the worker persists the terminal failure, so a real MiniMax resume or
auth error remains visible in the saved session instead of being collapsed to a
generic adapter exit string.

## Live Transcript Labels

MiniMax desktop runs now normalize shell-wrapped code-search commands in the
shared live summary layer before they are persisted into the session
transcript.

- bare `semble ...` and `fff ...` commands keep their search-specific labels
- direct local-launch forms from a sibling `semble-rs` checkout also collapse
  to `Semble` while preserving the truncated command preview shown in the
  transcript row

This keeps prompt-debug and saved-session transcript rows aligned with the
policy-layer code-search preference, even when MiniMax executes `semble`
through the shared local-checkout wrapper.

The shared runtime now ensures a shell-level `semble` wrapper is available
automatically. The wrapper prefers a sibling `semble-rs` checkout release
binary and falls back to launching the local checkout through Cargo when that
binary has not been built yet. MCP sync still injects an `fff` server when an
`fff-mcp` binary is already present on `PATH`. That means Semble is exposed
through the shell-tool path, while FFF is exposed through the provider-facing
MCP inventory rather than being assumed to exist as a standalone shell binary.

The shared worker-side tool-call schema also now normalizes Claude-compatible
tool names like `Read File` and `Write File` onto the same `Read` / `Write`
labels used by other providers. If a Claude-compatible `Shell` event arrives
without an `input.command` payload, the current fallback intentionally renders
it as bare `Bash` with no truncated preview because there is no command string
available to classify as `Semble` or `FFF`.
For remaining adapter events that still arrive with the raw name `tool`, the
worker now performs one more shape-based inference pass before it persists the
transcript row: generic command inputs are rewritten onto the same `Bash` /
`Semble` / `FFF` path, `pattern` plus `path` inputs collapse to `Grep`,
pattern-only inputs collapse to `Glob`, path-only inputs collapse to `Read`,
and content-bearing path inputs collapse to `Write` or `Edit`. If no stable
kind can be inferred, the worker still records the bounded input preview under
a generic `Tool` label instead of persisting a blank or lowercase `tool` row.

## Persistence Notes

MiniMax adapter-backed desktop turns share the same worker-spool contract as
other adapter-backed providers:

- the live transcript is appended to `events.jsonl`
- the final visible answer is persisted through the shared output composer
- startup-time transport retries are logged to `transport-errors.log`
- git side-effect verification failures persist bounded raw captures and
  verifier context beside the normal spool files

That keeps MiniMax postmortems and saved-session replay aligned with Codex,
Claude-compatible, and Forge-backed desktop runs instead of introducing a
MiniMax-only persistence path.
