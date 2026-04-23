#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/.." && pwd)"
workspace_root="$(cd -- "${repo_root}/.." && pwd)"
target_root="${RUST_CHATBOT_SHARED_ROOT:-${RUST_CHATBOT_CONFIG_HOME:-${HOME}/.rust-chatbot}}"

declare -a source_roots=()
for repo_name in rust-chatbot chatbot-worker auto-runs; do
  candidate="${workspace_root}/${repo_name}"
  if [[ -d "${candidate}" ]]; then
    source_roots+=("${candidate}")
  fi
done

if [[ "${#source_roots[@]}" -eq 0 ]]; then
  echo "no sibling repos found under ${workspace_root}" >&2
  exit 1
fi

copy_repo_file_if_missing() {
  local source_rel="$1"
  local target_rel="$2"
  local source_root=""
  local source_path=""
  local target_path="${target_root}/${target_rel}"

  for source_root in "${source_roots[@]}"; do
    source_path="${source_root}/${source_rel}"
    if [[ -f "${source_path}" ]]; then
      mkdir -p "$(dirname -- "${target_path}")"
      if [[ -e "${target_path}" ]]; then
        echo "skip ${target_rel} (already exists)"
      else
        cp -p "${source_path}" "${target_path}"
        echo "copied ${source_rel} from ${source_root} -> ${target_rel}"
      fi
      return 0
    fi
  done

  return 0
}

merge_directory_contents() {
  local source_dir="$1"
  local target_dir="$2"
  mkdir -p "${target_dir}"

  if command -v rsync >/dev/null 2>&1; then
    rsync -a --update "${source_dir}/" "${target_dir}/"
  else
    cp -a "${source_dir}/." "${target_dir}/"
  fi

  rm -rf "${source_dir}"
  echo "moved ${source_dir} -> ${target_dir}"
}

move_file_if_newer() {
  local source_path="$1"
  local target_path="$2"
  mkdir -p "$(dirname -- "${target_path}")"

  if [[ ! -e "${target_path}" || "${source_path}" -nt "${target_path}" ]]; then
    cp -p "${source_path}" "${target_path}"
  fi

  rm -f "${source_path}"
  echo "moved ${source_path} -> ${target_path}"
}

move_entry() {
  local source_path="$1"
  local target_path="$2"

  if [[ -d "${source_path}" ]]; then
    merge_directory_contents "${source_path}" "${target_path}"
    return 0
  fi

  if [[ -f "${source_path}" || -L "${source_path}" ]]; then
    move_file_if_newer "${source_path}" "${target_path}"
    return 0
  fi

  if [[ -S "${source_path}" ]]; then
    echo "skip live socket ${source_path}"
    return 0
  fi

  if [[ -e "${source_path}" ]]; then
    echo "skip unsupported path ${source_path}"
  fi
}

split_legacy_claude_desktop() {
  local source_dir="${HOME}/.claude-desktop"
  [[ -d "${source_dir}" ]] || return 0

  while IFS= read -r -d '' entry; do
    local name
    name="$(basename -- "${entry}")"
    case "${name}" in
      backup-last-success.txt|broker.pid|coordination|index.db|logs|memory|provider-bridge-output-schemas|runsc-sidecars|sync-registry.lock|sync.sock|workers)
        move_entry "${entry}" "${target_root}/${name}"
        ;;
      *)
        move_entry "${entry}" "${target_root}/.claude-desktop/${name}"
        ;;
    esac
  done < <(find "${source_dir}" -mindepth 1 -maxdepth 1 -print0)

  rmdir "${source_dir}" 2>/dev/null || true
}

migrate_home_desktop_dirs() {
  local source_dir=""
  local base=""

  split_legacy_claude_desktop

  for source_dir in "${HOME}"/.*-desktop; do
    [[ -d "${source_dir}" ]] || continue
    base="$(basename -- "${source_dir}")"
    [[ "${base}" == ".claude-desktop" ]] && continue
    merge_directory_contents "${source_dir}" "${target_root}/${base}"
  done
}

mkdir -p "${target_root}"

copy_repo_file_if_missing "chatbot-config.toml" "config.toml"
copy_repo_file_if_missing ".mcp.json" ".mcp.json"
copy_repo_file_if_missing ".rust-chatbot/hooks.json" "hooks.json"

copy_repo_file_if_missing ".claude-desktop/.claude.json" ".claude-desktop/.claude.json"
copy_repo_file_if_missing ".claude-desktop/.credentials.json" ".claude-desktop/.credentials.json"
copy_repo_file_if_missing ".claude-desktop/settings.json" ".claude-desktop/settings.json"
copy_repo_file_if_missing ".claude-desktop/hooks.json" ".claude-desktop/hooks.json"

copy_repo_file_if_missing ".codex/config.toml" ".codex/config.toml"
copy_repo_file_if_missing ".codex/auth.json" ".codex/auth.json"

copy_repo_file_if_missing ".gemini/settings.json" ".gemini/settings.json"
copy_repo_file_if_missing ".gemini/settings.local.json" ".gemini/settings.local.json"
copy_repo_file_if_missing ".gemini/google_accounts.json" ".gemini/google_accounts.json"
copy_repo_file_if_missing ".gemini/installation_id" ".gemini/installation_id"
copy_repo_file_if_missing ".gemini/mcp-oauth-tokens-v2.json" ".gemini/mcp-oauth-tokens-v2.json"
copy_repo_file_if_missing ".gemini/oauth_creds.json" ".gemini/oauth_creds.json"
copy_repo_file_if_missing ".gemini/projects.json" ".gemini/projects.json"
copy_repo_file_if_missing ".gemini/state.json" ".gemini/state.json"
copy_repo_file_if_missing ".gemini/trustedFolders.json" ".gemini/trustedFolders.json"

copy_repo_file_if_missing ".minimax/.claude.json" ".minimax/.claude.json"
copy_repo_file_if_missing ".minimax/.minimax.json" ".minimax/.minimax.json"
copy_repo_file_if_missing ".minimax/api.key" ".minimax/api.key"
copy_repo_file_if_missing ".minimax/settings.json" ".minimax/settings.json"

copy_repo_file_if_missing ".nvidia/api.key" ".nvidia/api.key"

copy_repo_file_if_missing ".openrouter/api.key" ".openrouter/api.key"
copy_repo_file_if_missing ".gemma/api.key" ".gemma/api.key"

copy_repo_file_if_missing ".qwen/settings.json" ".qwen/settings.json"
copy_repo_file_if_missing ".qwen/oauth_creds.json" ".qwen/oauth_creds.json"
copy_repo_file_if_missing ".qwen/installation_id" ".qwen/installation_id"
copy_repo_file_if_missing ".qwen/source.json" ".qwen/source.json"
copy_repo_file_if_missing ".qwen/api.key" ".qwen/api.key"
copy_repo_file_if_missing ".qwen/.qwen.json" ".qwen/.qwen.json"

copy_repo_file_if_missing ".xiaomi/.claude.json" ".xiaomi/.claude.json"

migrate_home_desktop_dirs

echo "shared root migration complete under ${target_root}"
