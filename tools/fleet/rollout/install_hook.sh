#!/usr/bin/env bash
# Install the fleet hub-side hooks on the hub, CHAINING any existing hook
# rather than clobbering it (the production hub already has a post-receive
# that fastchecks every pushed branch). All three hook types this script
# manages (post-receive, pre-receive, update) use stdin/argv appropriately
# for their type and preserve/chain any pre-existing hook of the same type.
#
# DRY-RUN BY DEFAULT. Pass --execute to actually write. Run ON THE HUB as
# the account owning the bare repo:
#
#   tools/fleet/rollout/install_hook.sh <bare-repo-path> [--execute]
#
# Idempotent: re-running detects each wrapper and refreshes only the fleet
# half(ves). Rollback: restore hooks/<type> from hooks/<type>.legacy (the
# verbatim pre-install hook, kept forever) for whichever type you need to
# revert, and delete hooks/<type> or replace it with hooks/<type>.legacy.
#
# ---------------------------------------------------------------------
# R1 (ARCH-FIX-SPEC.md) additions, on top of the original post-receive-only
# installer:
#
#   * pre-receive.fleet / pre-receive (chain wrapper) -- the actual tip
#     protection guard. Denies writes to refs/heads/main and
#     refs/heads/refactor/tag-machinery without a matching
#     `--push-option=train-token=<secret>`, and denies deletion of either
#     ref unconditionally. See tools/fleet/hooks/pre-receive for the full
#     rationale, including why this had to be pre-receive and not update
#     (git never exports push-option env vars to the `update` hook --
#     verified empirically, see that file's header comment).
#
#   * update.fleet / update (chain wrapper) -- a second, independent layer
#     that denies deletion of the two protected refs. Does not check the
#     token (it structurally cannot -- see tools/fleet/hooks/update).
#
#   * train.token -- generated from /dev/urandom (mode 0600) if absent.
#     Its content is the secret the `train-token=` push option must match.
#     Never overwritten if already present (rotating it is a separate,
#     deliberate operator action, not a side effect of a routine
#     reinstall).
#
#   * `receive.advertisePushOptions=true` -- REQUIRED, not optional, for
#     any of this to fire. Without it, git rejects a push that uses
#     `-o ...` at the transport level ("fatal: the receiving end does not
#     support push options") before any hook -- pre-receive included --
#     ever runs. Verified empirically alongside the update-hook finding
#     above.
#
#   * `core.logAllRefUpdates=true` -- turns on the reflog for every ref on
#     the bare repo (bare repos default this off), so an allowed update to
#     a protected ref is durably observable after the fact, independent of
#     this hook's own stderr output.
#
#   * Non-fast-forward denial for the two protected refs -- closes R1's
#     history-rewrite sub-clause. pre-receive's token check and update's
#     deletion-deny stop an ATTACKER without the token from touching a
#     protected ref, but say nothing about a force-push that carries a
#     VALID token and simply rewrites history instead of deleting it
#     outright -- a `--force` push from someone who legitimately has the
#     token can silently discard commits the tip already advanced past,
#     the same tip-integrity failure R1 exists to prevent, just via a
#     non-fast-forward update instead of a delete. This is enforced in
#     `tools/fleet/hooks/update` (installed as `update.fleet`), scoped to
#     PROTECTED_REFS only via `git merge-base --is-ancestor` -- NOT via
#     repo-wide `receive.denyNonFastForwards`, which would deny
#     force-pushes on every ref on the hub, including staging/* and
#     rescued/*, violating R1's non-interference clause (see
#     proof_dnff_scope.sh and proof_dnff_breaks_leases.py, which show the
#     repo-wide setting also breaks every fleetlib payload write, since
#     payload commits are orphans and therefore never fast-forwards).
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

# ======================================================================
# 1. post-receive -- tip-signal bump (T1.3, unchanged from the original
#    version of this script).
# ======================================================================

WRAPPER="$HOOKS/post-receive"
FLEET_HALF="$HOOKS/post-receive.fleet"
LEGACY_HALF="$HOOKS/post-receive.legacy"

if [[ -f "$WRAPPER" ]] && ! grep -q "fleet-hook-wrapper" "$WRAPPER"; then
    say "existing post-receive found; preserving verbatim as post-receive.legacy"
    act cp -p "$WRAPPER" "$LEGACY_HALF"
elif [[ -f "$LEGACY_HALF" ]]; then
    say "legacy post-receive already preserved; leaving it untouched"
else
    say "no pre-existing post-receive hook; wrapper will run the fleet half only"
fi

act cp "$SRC_DIR/hooks/post-receive" "$FLEET_HALF"
act cp "$SRC_DIR/drift.py" "$HOOKS/drift.py"
act cp "$SRC_DIR/fleetlib.py" "$HOOKS/fleetlib.py"
act cp "$SRC_DIR/config.py" "$HOOKS/config.py"   # fleetlib imports its sibling config.py

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
    say "installed post-receive. verify: echo '0000 0000 refs/heads/nosuch' | $WRAPPER"
else
    say "DRY-RUN would: write chain wrapper to $WRAPPER (see script source)"
fi

# ======================================================================
# 2. pre-receive -- R1 tip-protection guard (T3). THE enforcement point:
#    this is the only hook type git gives push-option env vars to before
#    a ref is written. Unlike post-receive's wrapper above, rejection MUST
#    propagate here -- that is the entire point of this hook existing.
# ======================================================================

PR_WRAPPER="$HOOKS/pre-receive"
PR_FLEET_HALF="$HOOKS/pre-receive.fleet"
PR_LEGACY_HALF="$HOOKS/pre-receive.legacy"

if [[ -f "$PR_WRAPPER" ]] && ! grep -q "fleet-hook-wrapper" "$PR_WRAPPER"; then
    say "existing pre-receive found; preserving verbatim as pre-receive.legacy"
    act cp -p "$PR_WRAPPER" "$PR_LEGACY_HALF"
elif [[ -f "$PR_LEGACY_HALF" ]]; then
    say "legacy pre-receive already preserved; leaving it untouched"
else
    say "no pre-existing pre-receive hook; wrapper will run the fleet half only"
fi

act cp "$SRC_DIR/hooks/pre-receive" "$PR_FLEET_HALF"

if [[ "$EXECUTE" == "--execute" ]]; then
    cat > "$PR_WRAPPER" <<'WRAP'
#!/bin/bash
# fleet-hook-wrapper -- installed by tools/fleet/rollout/install_hook.sh.
# Chains the pre-existing pre-receive hook (pre-receive.legacy) with the
# fleet tip-protection guard (pre-receive.fleet). Both read the pushed-refs
# list from stdin AND see the same push-option env vars for this
# invocation, so stdin is captured once and fed to each. UNLIKE the
# post-receive wrapper above, a rejection from EITHER half must propagate:
# this hook runs before anything is written, and its exit code is the
# only thing standing between a tokenless push and the protected refs.
input=$(cat)
hooks_dir="$(cd "$(dirname "$0")" && pwd)"
status=0
if [ -x "$hooks_dir/pre-receive.legacy" ]; then
    printf '%s\n' "$input" | "$hooks_dir/pre-receive.legacy" || status=1
fi
if [ -x "$hooks_dir/pre-receive.fleet" ]; then
    printf '%s\n' "$input" | "$hooks_dir/pre-receive.fleet" || status=1
fi
exit "$status"
WRAP
    chmod +x "$PR_WRAPPER" "$PR_FLEET_HALF" 2>/dev/null || true
    [[ -f "$PR_LEGACY_HALF" ]] && chmod +x "$PR_LEGACY_HALF"
    say "installed pre-receive. verify (expect exit 0, no refs = nothing to deny): printf '' | $PR_WRAPPER; echo \$?"
else
    say "DRY-RUN would: write chain wrapper to $PR_WRAPPER (see script source)"
fi

# ======================================================================
# 3. update -- R1's second, independent layer (T3): denies deletion of
#    the two protected refs without needing push-option access (which
#    this hook type structurally cannot get -- see tools/fleet/hooks/update
#    for the empirical finding). argv-based, not stdin-based, and its
#    rejection must also propagate.
# ======================================================================

UP_WRAPPER="$HOOKS/update"
UP_FLEET_HALF="$HOOKS/update.fleet"
UP_LEGACY_HALF="$HOOKS/update.legacy"

if [[ -f "$UP_WRAPPER" ]] && ! grep -q "fleet-hook-wrapper" "$UP_WRAPPER"; then
    say "existing update hook found; preserving verbatim as update.legacy"
    act cp -p "$UP_WRAPPER" "$UP_LEGACY_HALF"
elif [[ -f "$UP_LEGACY_HALF" ]]; then
    say "legacy update hook already preserved; leaving it untouched"
else
    say "no pre-existing update hook; wrapper will run the fleet half only"
fi

act cp "$SRC_DIR/hooks/update" "$UP_FLEET_HALF"

if [[ "$EXECUTE" == "--execute" ]]; then
    cat > "$UP_WRAPPER" <<'WRAP'
#!/bin/bash
# fleet-hook-wrapper -- installed by tools/fleet/rollout/install_hook.sh.
# Chains the pre-existing update hook (update.legacy) with the fleet
# tip-protection guard (update.fleet), passing the same argv
# (refname oldrev newrev) to each. An update hook's exit code decides
# that ONE ref, so both halves must genuinely be allowed to deny it --
# rejection from either propagates.
hooks_dir="$(cd "$(dirname "$0")" && pwd)"
status=0
if [ -x "$hooks_dir/update.legacy" ]; then
    "$hooks_dir/update.legacy" "$@" || status=1
fi
if [ -x "$hooks_dir/update.fleet" ]; then
    "$hooks_dir/update.fleet" "$@" || status=1
fi
exit "$status"
WRAP
    chmod +x "$UP_WRAPPER" "$UP_FLEET_HALF" 2>/dev/null || true
    [[ -f "$UP_LEGACY_HALF" ]] && chmod +x "$UP_LEGACY_HALF"
    say "installed update. verify (expect exit 0, unprotected ref): $UP_WRAPPER refs/heads/wip/x 0000000000000000000000000000000000000000 1111111111111111111111111111111111111111; echo \$?"
else
    say "DRY-RUN would: write chain wrapper to $UP_WRAPPER (see script source)"
fi

# ======================================================================
# 4. train.token -- the secret the `train-token=` push option must match.
#    Generated once, never rotated by a routine reinstall.
# ======================================================================

TOKEN_FILE="$REPO/train.token"

if [[ -f "$TOKEN_FILE" ]]; then
    say "train.token already present at $TOKEN_FILE; leaving it untouched"
else
    if [[ "$EXECUTE" == "--execute" ]]; then
        ( umask 077 && head -c 32 /dev/urandom | base64 | tr -d '\n' > "$TOKEN_FILE" )
        printf '\n' >> "$TOKEN_FILE"
        chmod 600 "$TOKEN_FILE"
        say "generated $TOKEN_FILE (mode 0600). Read it with 'cat $TOKEN_FILE' before wiring it into" \
            "the train's push-option config -- it is not echoed here to keep secrets out of logs/scrollback."
    else
        say "DRY-RUN would: generate $TOKEN_FILE (0600) from /dev/urandom -- MISSING today means every push" \
            "to a protected ref fails CLOSED until this runs"
    fi
fi

# ======================================================================
# 5. Repo config: push options must be advertised or the transport itself
#    rejects any `-o ...` push before a hook ever runs; logAllRefUpdates
#    makes an allowed protected-ref update durably observable via reflog.
# ======================================================================

act git -C "$REPO" config receive.advertisePushOptions true
act git -C "$REPO" config core.logAllRefUpdates true

say "done. Summary of what changed: post-receive (tip signal, unchanged)," \
    "pre-receive (NEW -- R1 token enforcement), update (NEW -- R1 deletion-deny AND" \
    "non-fast-forward-deny, both scoped to refs/heads/main and refs/heads/refactor/tag-machinery only)," \
    "train.token (created if absent), receive.advertisePushOptions=true, core.logAllRefUpdates=true."
