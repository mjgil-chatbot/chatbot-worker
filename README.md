# chatbot-worker

Shared worker/runtime crate extracted from the chat application and the
autonomous run driver.

It owns:

- the canonical `chatbot-worker` implementation
- provider bridge and background worker runtime code
- worker spool types, sync/shared-storage helpers, hooks, and memory/runtime support
- the vendored `agentic_memory` crate used by worker-side memory layering
- shared runtime config and session resolution used by host applications

See [docs/architecture/overview.md](docs/architecture/overview.md) for the repo
boundary and integration points.

## Runtime Surface

The shared worker runtime centers on three durable contracts:

- adapter/provider execution, including retry classification, live transcript
  shaping, and git side-effect verification for mutating requests
- worker spool persistence under the shared runtime root
- provider/session/config resolution that host applications reuse

The worker spool keeps:

- `request.json` for the submitted turn payload
- `state.json` for the latest worker snapshot
- `result.json` for durable completion metadata such as session IDs or errors
- `events.jsonl` for the structured live transcript
- `output` for the persisted final turn text assembled from transcript plus
  final reply

Some runs also persist bounded diagnostics such as `transport-errors.log`,
git-verifier context, and raw adapter/provider event captures when a retry or
verification failure needs postmortem inspection.

## Public Clone Setup

This repo builds as part of a multi-repo workspace. For a fresh clone or CI
checkout, bootstrap the required public sibling repositories with:

```bash
./scripts/bootstrap-ci-siblings.sh
```

The script clones only missing siblings beside the current checkout. The
`agentic_memory` dependency is vendored under `crates/agentic_memory` so
public CI does not depend on unpublished sources.

## Runtime State

Runtime config and provider session data resolve from a shared root managed by
the host application. During migration, the worker can still fall back to
older repo-local or host-managed state when the canonical shared root is
missing.

To migrate existing state on the current machine, run:

```bash
./scripts/migrate-home-state-to-shared-root.sh
```

The shared root also carries provider-specific config trees, hook config,
workspace memory state, and the worker spool directories used by detached
turns.

## Verification

The full test suite needs Python 3.11 or newer and `ripgrep` (`rg`) on PATH.
The AST-search integration test exercises the real ripgrep fallback. Provider
protocol tests use synthetic executables and a test key; they do not require
installed provider CLIs or personal credentials.

Enable the tracked Git hooks for this clone:

```bash
./scripts/install-hooks.sh
```

For local verification and CI parity, run:

```bash
./scripts/bootstrap-ci-siblings.sh
python3 -m unittest discover -s tests
cargo test --release --locked
cargo build --release --locked
./scripts/verify-sibling-builds.sh --check --release --repo chatbot-worker --repo rust-chatbot
```

Use `--build` when you need a full sibling build pass instead of `cargo check`.

The Rust test suite includes several environment-sensitive groups:

- provider bridge tests that rewrite `HOME`, `PATH`, and provider CLI env vars
- worker spool tests that intentionally exercise canonical-versus-legacy shared
  runtime resolution
- background dispatcher tests that launch fake provider CLIs to assert bounded
  concurrency

Those tests are serialized with shared env locks inside the repo so the full
suite can run deterministically in CI and on contributor machines.

## Additional Docs

- [docs/architecture/overview.md](docs/architecture/overview.md): repo
  boundary, spool/runtime ownership, and CI contract
- [docs/architecture/minimax-acp-bridge.md](docs/architecture/minimax-acp-bridge.md):
  MiniMax Claude-compatible ACP bridge behavior, transcript shaping, and
  provider-tool labeling

## License

Released under the 0BSD license. See [LICENSE](LICENSE).

Third-party components retain their own terms.
