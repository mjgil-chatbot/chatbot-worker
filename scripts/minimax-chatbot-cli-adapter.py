#!/usr/bin/env python3

import os
import pathlib
import runpy
import sys


DEFAULT_MODEL = "MiniMax-M2.7"
MINIMAX_BASE_URL = "https://api.minimax.io/v1"
BRIDGE_NAME = "claude-compat-chatbot-cli-adapter.py"


def main() -> int:
    auth_token = (
        os.environ.get("CHATBOT_PROVIDER_MINIMAX_API_KEY")
        or os.environ.get("ANTHROPIC_AUTH_TOKEN")
        or os.environ.get("MINIMAX_API_KEY")
    )
    if auth_token:
        os.environ.setdefault("CHATBOT_PROVIDER_MINIMAX_API_KEY", auth_token)

    os.environ.setdefault("CHATBOT_CLAUDE_COMPAT_PROVIDER_NAME", "MiniMax")
    os.environ.setdefault("CHATBOT_CLAUDE_COMPAT_PROVIDER_ID", "minimax")
    os.environ.setdefault("CHATBOT_CLAUDE_COMPAT_PROVIDER_TYPE", "openai")
    os.environ.setdefault("CHATBOT_CLAUDE_COMPAT_MODEL", DEFAULT_MODEL)
    os.environ.setdefault("CHATBOT_CLAUDE_COMPAT_BASE_URL", MINIMAX_BASE_URL)
    os.environ.setdefault("CHATBOT_CLAUDE_COMPAT_ENV_KEY", "CHATBOT_PROVIDER_MINIMAX_API_KEY")
    os.environ.setdefault("CHATBOT_CLAUDE_COMPAT_AUTH_MODE", "bearer")
    os.environ.setdefault(
        "CHATBOT_CLAUDE_COMPAT_KEY_FILE",
        str(pathlib.Path.home() / ".rust-chatbot" / ".minimax" / "api.key"),
    )

    bridge_path = pathlib.Path(__file__).with_name(BRIDGE_NAME)
    if not bridge_path.is_file():
        print(f"Error: missing shared bridge {bridge_path}", file=sys.stderr)
        return 1
    runpy.run_path(str(bridge_path), run_name="__main__")
    return 0


if __name__ == "__main__":
    sys.exit(main())
