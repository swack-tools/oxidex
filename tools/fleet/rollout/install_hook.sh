#!/usr/bin/env bash
# Install the fleet tip-signal post-receive hook on the hub, CHAINING any
# existing hook rather than clobbering it (the production hub already has
# one: it fastchecks every pushed branch). Both hooks read stdin, so the
# wrapper captures the ref list once and feeds it to each.
#
# DRY-RUN BY DEFAULT. Pass --execute to actually write. Run ON THE HUB as
# the account owning the bare repo:
#
#   tools/fleet/rollout/install_hook.sh <bare-repo-path> [--execute]
#
# Idempotent: re-running detects the wrapper and refreshes only the fleet
# half. Rollback: restore hooks/post-receive from hooks/post-receive.legacy
# (the verbatim pre-install hook, kept forever).
set -euo pipefail

REPO="${1:?usage: install_hook.sh <bare-repo-path> [--execute]}"
EXECUTE="${2:-}"
HOOKS="$REPO/hooks"
SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"  # tools/fleet

[[ -d "$HOOKS" ]] || { echo "no hooks dir at $HOOKS -- is $REPO a bare repo?" >&2; exit 1; }

say() { echo "install_hook: $*"; }
act() {
    if [[ "$EXECUTE" == "--execute" ]]; then "$@"; else say "DRY-RUN would: $*"; fi
}

WRAPPER="$HOOKS/post-receive"
FLEET_HALF="$HOOKS/post-receive.fleet"
LEGACY_HALF="$HOOKS/post-receive.legacy"

# 1. Preserve any existing non-wrapper hook as the legacy half, once.
if [[ -f "$WRAPPER" ]] && ! grep -q "fleet-hook-wrapper" "$WRAPPER"; then
    say "existing post-receive found; preserving verbatim as post-receive.legacy"
    act cp -p "$WRAPPER" "$LEGACY_HALF"
elif [[ -f "$LEGACY_HALF" ]]; then
    say "legacy half already preserved; leaving it untouched"
else
    say "no pre-existing hook; wrapper will run the fleet half only"
fi

# 2. The fleet half + its imports (drift.py imports fleetlib.py by path).
act cp "$SRC_DIR/hooks/post-receive" "$FLEET_HALF"
act cp "$SRC_DIR/drift.py" "$HOOKS/drift.py"
act cp "$SRC_DIR/fleetlib.py" "$HOOKS/fleetlib.py"

# 3. The wrapper: capture stdin once, feed both halves. The fleet half
# must not be able to block a push (it already guards itself, and the
# wrapper ignores its exit); the legacy half keeps its old best-effort
# semantics (it always exits 0 itself).
if [[ "$EXECUTE" == "--execute" ]]; then
    cat > "$WRAPPER" <<'WRAP'
#!/bin/bash
# fleet-hook-wrapper -- installed by tools/fleet/rollout/install_hook.sh.
# Chains the pre-fleet hook (post-receive.legacy) with the fleet
# tip-signal hook (post-receive.fleet). Both read the pushed-refs list
# from stdin, so capture it once.
input=$(cat)
hooks_dir="$(cd "$(dirname "$0")" && pwd)"
[ -x "$hooks_dir/post-receive.legacy" ] && printf '%s\n' "$input" | "$hooks_dir/post-receive.legacy"
[ -x "$hooks_dir/post-receive.fleet"  ] && printf '%s\n' "$input" | "$hooks_dir/post-receive.fleet"
exit 0
WRAP
    chmod +x "$WRAPPER" "$FLEET_HALF" 2>/dev/null || true
    [[ -f "$LEGACY_HALF" ]] && chmod +x "$LEGACY_HALF"
    say "installed. verify: echo '0000 0000 refs/heads/nosuch' | $WRAPPER"
else
    say "DRY-RUN would: write chain wrapper to $WRAPPER (see script source)"
fi
