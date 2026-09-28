#!/usr/bin/env bash
# Shared project Vale checker; exit 2 sends lint feedback to the agent.
set -euo pipefail

# Check optional dependencies before loading the shared file-selection helper.
if ! command -v vale >/dev/null 2>&1; then
  echo 'prose-lint: install vale to enable prose linting; skipping hook.' >&2
  exit 0
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo 'prose-lint: install python3 to run the Vale hook; skipping hook.' >&2
  exit 0
fi

hook_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
exec python3 "$hook_root/tools/prose_lint.py"
