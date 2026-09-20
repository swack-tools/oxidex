#!/bin/bash
# Thin, strict entry point for the genshare-receipt/v3 Python implementation.
set -euo pipefail

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
exec python3 "$script_dir/attribute.py" census "$@"
