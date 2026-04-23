#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/bootstrap-ci-siblings.sh

Clone the public sibling repositories expected by local Cargo verification and
the sibling-boundary checks when they are missing beside this repo.
EOF
}

if (($# > 0)); then
    case "$1" in
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
PARENT_DIR="$(cd "${REPO_ROOT}/.." && pwd)"

clone_if_missing() {
    local repo_name="$1"
    local repo_url="$2"
    local repo_path="${PARENT_DIR}/${repo_name}"

    if [[ -d "${repo_path}/.git" ]]; then
        echo "bootstrap-ci-siblings: keeping existing ${repo_name}"
        return 0
    fi

    echo "bootstrap-ci-siblings: cloning ${repo_name}"
    git clone --depth 1 "${repo_url}" "${repo_path}"
}

clone_if_missing "ai-providers" "https://github.com/mjgil/ai-providers.git"
clone_if_missing "the-markdown-commons" "https://github.com/mjgil/the-markdown-commons.git"
clone_if_missing "chatbot-adapter" "https://github.com/mjgil/chatbot-adapter.git"
clone_if_missing "auto-runs" "https://github.com/mjgil/auto-runs.git"
clone_if_missing "rust-chatbot" "https://github.com/mjgil/rust-chatbot.git"
clone_if_missing "chatbot-ui" "https://github.com/mjgil/chatbot-ui.git"
clone_if_missing "chatbot-history" "https://github.com/mjgil/chatbot-history.git"
clone_if_missing "history-auditor" "https://github.com/mjgil/history-auditor.git"
