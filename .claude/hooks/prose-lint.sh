#!/usr/bin/env bash
# Shared project Vale checker; exit 2 sends lint feedback to the agent.
set -euo pipefail
hook_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
exec python3 "$hook_root/tools/prose_lint.py"
