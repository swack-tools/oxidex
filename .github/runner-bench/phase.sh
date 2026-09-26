#!/usr/bin/env bash
# Times one benchmark phase and records its duration and exit status under
# <key> in the JSON object at $PHASES. Always exits 0, so every phase still
# runs when one fails on a runner; summarize.py flags failures instead.
#
#   bash phase.sh <key> <command...>
set -uo pipefail

key=$1
shift
start=$(date +%s%N)
"$@"
status=$?
ms=$((($(date +%s%N) - start) / 1000000))

[ -f "$PHASES" ] || echo '{}' >"$PHASES"
jq --arg k "$key" --argjson s "$status" --argjson ms "$ms" \
  '.[$k] = {seconds: ($ms / 1000), exit: $s}' "$PHASES" >"$PHASES.tmp"
mv "$PHASES.tmp" "$PHASES"
if [ "$status" -ne 0 ]; then echo "::warning::phase $key exited $status"; fi
