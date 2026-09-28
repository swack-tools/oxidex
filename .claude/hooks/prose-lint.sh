#!/usr/bin/env bash
# Claude PostToolUse Vale checker; exit 2 sends lint feedback to Claude.
set -euo pipefail

if ! command -v vale >/dev/null 2>&1; then
  echo 'prose-lint: install vale to enable prose linting; skipping hook.' >&2
  exit 0
fi
if ! command -v jq >/dev/null 2>&1; then
  echo 'prose-lint: install jq to read Claude hook input; skipping hook.' >&2
  exit 0
fi

file=$(jq -r '.tool_input.file_path | select(type == "string")')
# Claude Edit/Write paths are absolute. Never treat a filename as an option.
case "$file" in
  /*.md|/*.rs|/*.py|/*.sh|/*.pl) ;;
  *) exit 0 ;;
esac
[[ -f "$file" ]] || exit 0

hook_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd -- "${CLAUDE_PROJECT_DIR:-$hook_root}"
config="$PWD/.vale.ini"
# Google is configured in .vale.ini but its downloaded rules are ignored by Git.
if [[ ! -d .vale/styles/Google ]] && ! output=$(vale --config "$config" sync 2>&1); then
  printf 'prose-lint: vale sync failed: %s\n' "$output" >&2
  exit 2
fi
if ! output=$(vale --config "$config" --output=line -- "$file" 2>&1); then
  printf 'Fix these style errors: %s\n' "$output" >&2
  exit 2
fi
