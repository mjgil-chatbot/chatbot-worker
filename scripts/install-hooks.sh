#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
git -C "$repo_root" config core.hooksPath .githooks

scaffold_dir="$(cd "$repo_root/.." && pwd)/rust-project-scaffold/.git-template"
if [ -d "$scaffold_dir" ]; then
  git config --global init.templateDir "$scaffold_dir"
  echo "Configured global init.templateDir to $scaffold_dir"
fi

echo "Configured Git hooks path to .githooks for $repo_root"
