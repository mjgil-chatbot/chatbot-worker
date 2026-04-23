from __future__ import annotations

import json
import os
import pathlib
import shutil
import subprocess
import sys
CHATBOT_CLI_ENV = "RUST_CHATBOT_CHATBOT_CLI"
PROVIDER_NAME_ENV = "CHATBOT_CLAUDE_COMPAT_PROVIDER_NAME"
PROVIDER_ID_ENV = "CHATBOT_CLAUDE_COMPAT_PROVIDER_ID"
PROVIDER_TYPE_ENV = "CHATBOT_CLAUDE_COMPAT_PROVIDER_TYPE"
PROVIDER_MODEL_ENV = "CHATBOT_CLAUDE_COMPAT_MODEL"
PROVIDER_BASE_URL_ENV = "CHATBOT_CLAUDE_COMPAT_BASE_URL"
PROVIDER_ENV_KEY_ENV = "CHATBOT_CLAUDE_COMPAT_ENV_KEY"
PROVIDER_AUTH_ENV = "CHATBOT_CLAUDE_COMPAT_AUTH_MODE"
PROVIDER_KEY_FILE_ENV = "CHATBOT_CLAUDE_COMPAT_KEY_FILE"
ACP_PROTOCOL_VERSION = "2024-11-05"
ACP_SESSION_END_METHOD = "session/end"


def main() -> int:
    try:
        options = parse_args(sys.argv[1:])
        return run_chatbot(options)
    except UsageError as err:
        print(f"Error: {err}", file=sys.stderr)
        return 2


class UsageError(Exception):
    pass


def parse_args(argv: list[str]) -> dict[str, object]:
    prompt = None
    model = None
    system_prompt = None
    resume_id = None
    dangerous = False
    safe_mode = False

    i = 0
    while i < len(argv):
        arg = argv[i]
        if arg == "-p":
            i += 1
            prompt = require_value(argv, i, "-p")
        elif arg == "--model":
            i += 1
            model = require_value(argv, i, "--model")
        elif arg == "--append-system-prompt":
            i += 1
            system_prompt = require_value(argv, i, "--append-system-prompt")
        elif arg == "--resume":
            if i + 1 < len(argv) and not argv[i + 1].startswith("-"):
                i += 1
                resume_id = require_value(argv, i, "--resume")
        elif arg == "--dangerously-skip-permissions":
            dangerous = True
        elif arg == "--permission-mode":
            i += 1
            require_value(argv, i, "--permission-mode")
            safe_mode = True
        elif arg in {"--verbose", "--output-format", "--input-format"}:
            if arg in {"--output-format", "--input-format"}:
                i += 1
                require_value(argv, i, arg)
        elif arg in {"-h", "--help"}:
            raise UsageError("help is not supported in adapter mode")
        else:
            raise UsageError(f"unsupported argument: {arg}")
        i += 1

    if prompt is None:
        raise UsageError("missing required -p prompt")

    permission_profile = "full-access" if dangerous and not safe_mode else "workspace-write"
    return {
        "prompt": prompt,
        "model": model,
        "system_prompt": system_prompt,
        "resume_id": resume_id,
        "permission_profile": permission_profile,
    }


def require_value(argv: list[str], index: int, flag: str) -> str:
    if index >= len(argv):
        raise UsageError(f"missing value for {flag}")
    return argv[index]


def run_chatbot(options: dict[str, object]) -> int:
    provider_name = env_required(PROVIDER_NAME_ENV)
    provider_id = env_required(PROVIDER_ID_ENV)
    provider_type = os.environ.get(PROVIDER_TYPE_ENV, "anthropic")
    base_url = env_required(PROVIDER_BASE_URL_ENV)
    auth_env_key = env_required(PROVIDER_ENV_KEY_ENV)
    auth_mode = os.environ.get(PROVIDER_AUTH_ENV, "bearer")
    auth_token = resolve_auth_token(auth_env_key)
    if not auth_token:
        print(
            f"Error: missing {provider_name} API key in {auth_env_key} for chatbot-cli bridge",
            flush=True,
        )
        return 1

    model = str(options["model"] or os.environ.get(PROVIDER_MODEL_ENV) or "").strip()
    if not model:
        print(
            f"Error: missing model for {provider_name} bridge; set {PROVIDER_MODEL_ENV}",
            flush=True,
        )
        return 1

    chatbot = resolve_chatbot_cli()
    resume_id = as_optional_string(options["resume_id"])

    rendered_prompt = build_prompt(
        prompt=str(options["prompt"]),
        system_prompt=as_optional_string(options["system_prompt"]),
    )

    bridge_root = ensure_bridge_root(provider_id)
    env = build_chatbot_env(
        config_root=bridge_root,
        provider_id=provider_id,
        provider_type=provider_type,
        model=model,
        base_url=base_url,
        auth_env_key=auth_env_key,
        auth_mode=auth_mode,
        permission_profile=str(options["permission_profile"]),
        auth_token=auth_token,
    )

    cmd = [chatbot, "acp"]

    proc = subprocess.Popen(
        cmd,
        cwd=os.getcwd(),
        env=env,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    assert proc.stdin is not None
    assert proc.stdout is not None
    assert proc.stderr is not None

    final_text_emitted = False
    final_text = ""
    final_error = None
    response = send_acp_request(
        proc,
        {
            "jsonrpc": "2.0",
            "id": "1",
            "method": "initialize",
            "params": {
                "protocolVersion": ACP_PROTOCOL_VERSION,
                "capabilities": {"tools": True, "permissions": True},
                "clientInfo": {"name": "chatbot-worker-bridge", "version": "1.0.0"},
            },
        },
        final_state={"final_text": final_text, "final_text_emitted": final_text_emitted, "final_error": final_error},
    )
    if response.get("error"):
        final_error = error_message_from_response(response)
        return finish_with_protocol_error(proc, final_error)

    if resume_id:
        if not transcript_exists(bridge_root, resume_id):
            emit_log(
                "warn",
                f"Resume diagnostics: missing bridge transcript for session {resume_id}; attempting provider resume anyway",
            )
        response = send_acp_request(
            proc,
            {
                "jsonrpc": "2.0",
                "id": "2",
                "method": "session/resume",
                "params": {"sessionId": resume_id},
            },
            final_state={"final_text": final_text, "final_text_emitted": final_text_emitted, "final_error": final_error},
        )
        if response.get("error"):
            final_error = error_message_from_response(response)
            emit_log(
                "warn",
                f"Resume fallback: provider resume failed for session {resume_id}: {final_error}; creating a new session",
            )
            final_error = None
            response = send_acp_request(
                proc,
                {
                    "jsonrpc": "2.0",
                    "id": "2b",
                    "method": "session/create",
                    "params": {
                        "model": model,
                        "provider": provider_id,
                        "systemPrompt": as_optional_string(options["system_prompt"]),
                    },
                },
                final_state={"final_text": final_text, "final_text_emitted": final_text_emitted, "final_error": final_error},
            )
    else:
        response = send_acp_request(
            proc,
            {
                "jsonrpc": "2.0",
                "id": "2",
                "method": "session/create",
                "params": {
                    "model": model,
                    "provider": provider_id,
                    "systemPrompt": as_optional_string(options["system_prompt"]),
                },
            },
            final_state={"final_text": final_text, "final_text_emitted": final_text_emitted, "final_error": final_error},
        )
    if response.get("error"):
        final_error = error_message_from_response(response)
        return finish_with_protocol_error(proc, final_error)

    effective_session_id = session_id_from_response(response) or resume_id
    if not effective_session_id:
        return finish_with_protocol_error(
            proc,
            f"missing session identity in {provider_name} ACP response",
        )
    print(
        json.dumps({"type": "session_meta", "session_id": effective_session_id}),
        flush=True,
    )

    response = send_acp_request(
        proc,
        {
            "jsonrpc": "2.0",
            "id": "3",
            "method": "prompt",
            "params": {
                "prompt": rendered_prompt,
                "sessionId": effective_session_id,
            },
        },
        final_state={"final_text": final_text, "final_text_emitted": final_text_emitted, "final_error": final_error},
    )
    if response.get("error"):
        final_error = error_message_from_response(response)
        return finish_with_protocol_error(proc, final_error)

    state = {
        "final_text": final_text,
        "final_text_emitted": final_text_emitted,
        "final_error": final_error,
        "completed": False,
    }
    completion_seen = drain_until_completion(proc, state)
    if completion_seen:
        response = send_acp_request(
            proc,
            {
                "jsonrpc": "2.0",
                "id": "4",
                "method": ACP_SESSION_END_METHOD,
                "params": {"sessionId": effective_session_id},
            },
            state,
        )
        if response.get("error"):
            final_error = error_message_from_response(response)
            state["final_error"] = final_error
            if not state.get("final_text_emitted"):
                emit_error_text(final_error)
                state["final_text_emitted"] = True
        close_stdin(proc)

    final_text = str(state["final_text"])
    final_text_emitted = bool(state["final_text_emitted"])
    final_error = state["final_error"] if isinstance(state["final_error"], str) else None

    try:
        exit_code = proc.wait(timeout=5 if completion_seen else None)
    except subprocess.TimeoutExpired:
        proc.kill()
        exit_code = proc.wait()
        if final_error is None:
            final_error = "ACP bridge did not exit after completion"
            state["final_error"] = final_error
            if not final_text_emitted:
                emit_error_text(final_error)
                final_text_emitted = True
    stderr = proc.stderr.read().strip()
    if stderr:
        emit_log("warn", stderr)
    if exit_code == 0 and final_error is None:
        return 0
    return exit_code or 1


def drain_until_completion(
    proc: subprocess.Popen[str], state: dict[str, object]
) -> bool:
    assert proc.stdout is not None
    while True:
        raw_line = proc.stdout.readline()
        if raw_line == "":
            return bool(state.get("completed"))
        handle_acp_line(raw_line, state)
        if state.get("completed"):
            return True


def close_stdin(proc: subprocess.Popen[str]) -> None:
    try:
        if proc.stdin is not None and not proc.stdin.closed:
            proc.stdin.close()
    except OSError:
        pass


def send_acp_request(
    proc: subprocess.Popen[str],
    request: dict[str, object],
    final_state: dict[str, object],
) -> dict[str, object]:
    assert proc.stdin is not None
    assert proc.stdout is not None
    proc.stdin.write(json.dumps(request) + "\n")
    proc.stdin.flush()
    while True:
        # A TextIOWrapper may already hold a complete reply even when the file
        # descriptor is not readable, or the child has exited. Read buffered
        # lines first and treat EOF, rather than process exit, as missing data.
        raw_line = proc.stdout.readline()
        if raw_line == "":
            message = "ACP bridge closed stdout before responding"
            exit_code = proc.poll()
            if exit_code is not None:
                message += f" (process exited with code {exit_code})"
            return {"error": {"message": message}}

        payload = parse_json_line(raw_line)
        if payload is None:
            emit_log("warn", raw_line.strip())
            continue

        if "id" in payload:
            return payload

        handle_acp_payload(payload, final_state)


def finish_with_protocol_error(proc: subprocess.Popen[str], message: str | None) -> int:
    if message:
        emit_error_text(message)
    try:
        proc.kill()
    except OSError:
        pass
    return 1


def emit_error_text(message: str) -> None:
    print(json.dumps({"type": "text_delta", "text": f"Error: {message}"}), flush=True)


def emit_log(level: str, message: str) -> None:
    print(json.dumps({"type": "log", "level": level, "message": message}), flush=True)


def session_id_from_response(payload: dict[str, object]) -> str | None:
    result = payload.get("result")
    if not isinstance(result, dict):
        return None

    session_id = result.get("sessionId")
    if isinstance(session_id, str):
        session_id = session_id.strip()
        if session_id:
            return session_id

    resume_handle = result.get("resumeHandle")
    if isinstance(resume_handle, str):
        resume_handle = resume_handle.strip()
        if resume_handle:
            return resume_handle.removeprefix("acp:")

    return None


def error_message_from_response(payload: dict[str, object]) -> str:
    error_obj = payload.get("error")
    if isinstance(error_obj, dict):
        message = error_obj.get("message")
        if isinstance(message, str) and message.strip():
            return message.strip()
    return "unknown ACP error"


def handle_acp_line(raw_line: str, state: dict[str, object]) -> None:
    payload = parse_json_line(raw_line)
    if payload is None:
        emit_log("warn", raw_line.strip())
        return
    handle_acp_payload(payload, state)


def parse_json_line(raw_line: str) -> dict[str, object] | None:
    line = raw_line.strip()
    if not line:
        return None
    try:
        payload = json.loads(line)
    except json.JSONDecodeError:
        return None
    if isinstance(payload, dict):
        return payload
    return None


def handle_acp_payload(payload: dict[str, object], state: dict[str, object]) -> None:
    if "id" in payload:
        return

    method = payload.get("method")
    if not isinstance(method, str):
        return

    params = payload.get("params")
    if not isinstance(params, dict):
        params = {}

    if method == "tool_call_started":
        print(
            json.dumps(
                {
                    "type": "tool_use",
                    "id": params.get("id", "tool"),
                    "name": params.get("name", "tool"),
                    "input": params.get("input") or {},
                }
            ),
            flush=True,
        )
        return

    if method == "tool_call_finished":
        translated = {
            "type": "tool_finished",
            "id": params.get("id", "tool"),
        }
        if "error" in params and params.get("error"):
            translated["error"] = params.get("error")
        else:
            translated["output"] = params.get("output", "")
        print(json.dumps(translated), flush=True)
        return

    if method == "usage":
        print(
            json.dumps(
                {
                    "type": "usage",
                    "input_tokens": params.get("inputTokens"),
                    "output_tokens": params.get("outputTokens"),
                }
            ),
            flush=True,
        )
        return

    if method == "text_delta":
        text = params.get("text")
        if isinstance(text, str) and text:
            previous = state.get("final_text")
            prefix = previous if isinstance(previous, str) else ""
            state["final_text"] = prefix + text
            state["final_text_emitted"] = True
            print(json.dumps({"type": "text_delta", "text": text}), flush=True)
        return

    if method == "log":
        level = params.get("level") if isinstance(params.get("level"), str) else "info"
        message = params.get("message") if isinstance(params.get("message"), str) else ""
        if message:
            print(json.dumps({"type": "log", "level": level, "message": message}), flush=True)
        if level == "error" and message:
            state["final_error"] = message
            if not state.get("final_text_emitted"):
                print(json.dumps({"type": "text_delta", "text": f"Error: {message}"}), flush=True)
                state["final_text_emitted"] = True
        return

    if method == "completion":
        state["completed"] = True
        error = params.get("error")
        if isinstance(error, dict):
            message = error.get("message")
            if isinstance(message, str) and message:
                state["final_error"] = message
                if not state.get("final_text_emitted"):
                    emit_error_text(message)
                    state["final_text_emitted"] = True
        elif params.get("finishReason") == "cancelled" and not state.get("final_error"):
            state["final_error"] = "run cancelled"
        return


def as_optional_string(value: object) -> str | None:
    if isinstance(value, str):
        stripped = value.strip()
        return stripped or None
    return None


def env_required(name: str) -> str:
    value = os.environ.get(name, "").strip()
    if not value:
        raise UsageError(f"missing required environment variable {name}")
    return value


def resolve_auth_token(auth_env_key: str) -> str | None:
    direct = os.environ.get(auth_env_key, "").strip()
    if direct:
        return direct

    key_file = os.environ.get(PROVIDER_KEY_FILE_ENV, "").strip()
    if not key_file:
        return None

    try:
        token = pathlib.Path(key_file).read_text(encoding="utf-8").strip()
    except OSError:
        return None
    return token or None


def ensure_bridge_root(provider_id: str) -> pathlib.Path:
    root = pathlib.Path.home() / ".rust-chatbot" / ".chatbot-cli-bridge" / provider_id
    root.mkdir(parents=True, exist_ok=True)
    return root


def transcript_exists(bridge_root: pathlib.Path, session_id: str) -> bool:
    transcript = (
        bridge_root
        / "data"
        / "chatbot-cli"
        / "sessions"
        / "transcripts"
        / f"{session_id}.jsonl"
    )
    return transcript.is_file() and transcript.stat().st_size > 0


def build_prompt(prompt: str, system_prompt: str | None) -> str:
    sections: list[str] = []
    if system_prompt:
        sections.append("System instructions:\n" + system_prompt)
    sections.append("User:\n" + prompt.strip())
    return "\n\n".join(section for section in sections if section).strip()


def build_chatbot_env(
    *,
    config_root: str,
    provider_id: str,
    provider_type: str,
    model: str,
    base_url: str,
    auth_env_key: str,
    auth_mode: str,
    permission_profile: str,
    auth_token: str,
) -> dict[str, str]:
    env = os.environ.copy()
    config_base = pathlib.Path(config_root)
    write_chatbot_config(
        config_base=config_base,
        provider_id=provider_id,
        provider_type=provider_type,
        model=model,
        base_url=base_url,
        auth_env_key=auth_env_key,
        auth_mode=auth_mode,
    )
    env["XDG_CONFIG_HOME"] = str(config_base)
    env["XDG_DATA_HOME"] = str(config_base / "data")
    env["XDG_STATE_HOME"] = str(config_base / "state")
    env["CHATBOT_MODEL"] = f"{provider_id}:{model}"
    env[auth_env_key] = auth_token
    env["CHATBOT_PERMISSION_PROFILE"] = permission_profile
    return env


def write_chatbot_config(
    *,
    config_base: pathlib.Path,
    provider_id: str,
    provider_type: str,
    model: str,
    base_url: str,
    auth_env_key: str,
    auth_mode: str,
) -> None:
    config_text = "\n".join(
        [
            "[model]",
            f'default = "{provider_id}:{model}"',
            "",
            f"[providers.{provider_id}]",
            f'type = "{provider_type}"',
            f'base_url = "{base_url}"',
            f'env_key = "{auth_env_key}"',
            f'auth = "{auth_mode}"',
            "",
        ]
    )
    candidate_paths = [
        config_base / "chatbot-cli" / "chatbot-cli" / "config.toml",
        config_base / "chatbot-cli" / "config.toml",
        config_base / ".chatbot" / "config.toml",
    ]
    for path in candidate_paths:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(config_text, encoding="utf-8")


def _chatbot_version(binary_path: str) -> str | None:
    """Run `chatbot --version` and return the version string, or None on failure.

    Falls back to `chatbot --help` since the chatbot binary does not implement
    `--version`.  A stub binary that immediately exits will fail both checks.
    """
    for flag in ("--version", "--help"):
        try:
            result = subprocess.run(
                [binary_path, flag],
                capture_output=True,
                text=True,
                timeout=5,
            )
            if result.returncode == 0:
                return (
                    result.stdout.strip()
                    or result.stderr.strip()
                    or "ok"
                )
        except (OSError, subprocess.TimeoutExpired):
            pass
    return None


def resolve_chatbot_cli() -> str:
    override = os.environ.get(CHATBOT_CLI_ENV)
    candidates = []
    if override:
        candidates.append(pathlib.Path(override))

    # Discover the chatbot-cli crate's package root via `cargo metadata` so we
    # can reliably locate the built binary at the canonical output path
    # (target/release/chatbot or target/debug/chatbot) regardless of where the
    # source happens to be checked out.
    repo_root = pathlib.Path(__file__).resolve().parents[2]
    metadata_candidates = []
    try:
        result = subprocess.run(
            ["cargo", "metadata", "--format-version=1", "--no-deps"],
            capture_output=True,
            text=True,
            cwd=str(repo_root / "chatbot-cli"),
            timeout=10,
        )
        if result.returncode == 0:
            for line in result.stdout.strip().split("\n"):
                if line.startswith("{"):
                    import json as _json

                    meta = _json.loads(line)
                    for pkg in meta.get("packages", []):
                        if pkg.get("name") == "chatbot-cli":
                            pkg_root = (
                                pathlib.Path(pkg["manifest_path"]).parent
                            )
                            for profile in ("release", "debug"):
                                metadata_candidates.append(
                                    pkg_root.parent.parent
                                    / "target"
                                    / profile
                                    / "chatbot"
                                )
                    break
    except (OSError, subprocess.TimeoutExpired):
        pass

    candidates.extend(metadata_candidates)

    home = pathlib.Path.home()
    candidates.extend(
        [
            repo_root / "chatbot-cli" / "target" / "release" / "chatbot",
            repo_root / "chatbot-cli" / "target" / "debug" / "chatbot",
            home / "git" / "chatbot-cli" / "target" / "release" / "chatbot",
            home / "git" / "chatbot-cli" / "target" / "debug" / "chatbot",
        ]
    )

    # Deduplicate while preserving order
    seen: set[str] = set()
    unique_candidates: list[pathlib.Path] = []
    for c in candidates:
        if c not in seen:
            seen.add(c)
            unique_candidates.append(c)

    # Validate repo candidates before returning them.
    for candidate in unique_candidates:
        if candidate.is_file() and os.access(candidate, os.X_OK):
            version = _chatbot_version(str(candidate))
            if version:
                return str(candidate)

    # Installed binary from PATH — most reliable in production.
    path_hit = shutil.which("chatbot")
    if path_hit:
        return path_hit

    raise UsageError(
        "could not locate chatbot-cli binary; checked RUST_CHATBOT_CHATBOT_CLI, "
        "known sibling checkout build directories, and PATH"
    )


if __name__ == "__main__":
    sys.exit(main())
