#!/usr/bin/env bash
# Preserve the public entry point. Generation and builds use private clones;
# only a fully verified live run promotes manifest outputs and the pin.
# Run --help for options; --recover checks an interrupted promotion's journal.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec python3 "$HERE/upgrade_transaction.py" "$@"
