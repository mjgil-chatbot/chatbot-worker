#!/bin/bash

set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/verify-sibling-builds.sh [--check] [--build] [--release] [--locked] [--repo <name>]

Verifies the extracted worker split against the expected sibling repos:
  - chatbot-worker
  - rust-chatbot
  - auto-runs

Defaults to `cargo check` in each repo. Use `--build` to run `cargo build`
instead, `--release` to add the release profile, and `--repo` to restrict the
verification set to specific repos. Repeat `--repo` as needed. Use
`__DESKTOP_REPO__` to target the detected desktop sibling.
EOF
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
PARENT_DIR="$(cd "${REPO_ROOT}/.." && pwd)"
DESKTOP_REPO_NAME=""

MODE="check"
declare -a EXTRA_ARGS
declare -a REQUESTED_REPOS

while (($# > 0)); do
    case "$1" in
        --check)
            MODE="check"
            ;;
        --build)
            MODE="build"
            ;;
        --release|--locked|--offline)
            EXTRA_ARGS+=("$1")
            ;;
        --repo)
            REQUESTED_REPOS+=("${2:-}")
            shift
            ;;
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
    shift
done

declare -a REPOS=(
    "chatbot-worker"
    "__DESKTOP_REPO__"
    "auto-runs"
)
declare -a ACTIVE_REPOS=()

resolve_desktop_repo() {
    if [[ -d "${PARENT_DIR}/rust-chatbot" ]]; then
        echo "rust-chatbot"
        return 0
    fi

    if [[ -d "${PARENT_DIR}/rust-claude-chatbot" ]]; then
        echo "rust-claude-chatbot"
        return 0
    fi

    return 1
}

resolve_repo_name() {
    local repo_name="$1"
    if [[ "${repo_name}" == "__DESKTOP_REPO__" ]]; then
        DESKTOP_REPO_NAME="$(resolve_desktop_repo)" || {
            echo "Missing sibling repo: ${PARENT_DIR}/rust-chatbot (or rust-claude-chatbot)" >&2
            exit 1
        }
        echo "${DESKTOP_REPO_NAME}"
        return 0
    fi

    echo "${repo_name}"
}

require_repo() {
    local repo_name
    repo_name="$(resolve_repo_name "$1")"
    local repo_path="${PARENT_DIR}/${repo_name}"
    if [[ ! -d "${repo_path}" ]]; then
        echo "Missing sibling repo: ${repo_path}" >&2
        exit 1
    fi
}

run_repo() {
    local repo_name="$1"
    local repo_path="${PARENT_DIR}/${repo_name}"
    local manifest_path="${repo_path}/Cargo.toml"
    local mode_suffix="${MODE}"
    local profile_suffix="dev"
    local cargo_target_dir=""

    if [[ " ${EXTRA_ARGS[*]} " == *" --release "* ]]; then
        profile_suffix="release"
    fi
    cargo_target_dir="${repo_path}/target/sibling-verifier/${mode_suffix}-${profile_suffix}"
    mkdir -p "${cargo_target_dir}"

    echo
    echo "==> ${repo_name}"
    echo "    target-dir: ${cargo_target_dir}"

    if [[ "${MODE}" == "build" ]]; then
        if ((${#EXTRA_ARGS[@]})); then
            CARGO_TARGET_DIR="${cargo_target_dir}" cargo build --manifest-path "${manifest_path}" "${EXTRA_ARGS[@]}"
        else
            CARGO_TARGET_DIR="${cargo_target_dir}" cargo build --manifest-path "${manifest_path}"
        fi
    else
        if ((${#EXTRA_ARGS[@]})); then
            CARGO_TARGET_DIR="${cargo_target_dir}" cargo check --manifest-path "${manifest_path}" "${EXTRA_ARGS[@]}"
        else
            CARGO_TARGET_DIR="${cargo_target_dir}" cargo check --manifest-path "${manifest_path}"
        fi
    fi
}

if ((${#REQUESTED_REPOS[@]})); then
    ACTIVE_REPOS=("${REQUESTED_REPOS[@]}")
else
    ACTIVE_REPOS=("${REPOS[@]}")
fi

for repo_name in "${ACTIVE_REPOS[@]}"; do
    require_repo "${repo_name}"
done

for repo_name in "${ACTIVE_REPOS[@]}"; do
    run_repo "$(resolve_repo_name "${repo_name}")"
done
