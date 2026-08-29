#!/bin/sh
# fleet-toolchain.sh -- the SHELL half of the one toolchain resolver
# (`tools/fleet/toolchain.py`). Sourced by gate.sh; nothing else in the
# tree is allowed to compute a rustc_id/platform_id of its own.
#
# WHY THIS EXISTS -- the Keel Stage 1 LIVE incident, 2026-08-27/28.
# fleetd's host claim on the i7 recorded platform_id b2bdf493... while
# the gate fleetd itself had just spawned wrote its verdict under
# b6613b19..., same host, same minute. `platform_id` is a third of the
# verdict cache key, so the scheduler that PAID for the gate could not
# read the gate's own PASS: `classify_branch` never returned
# AWAITING_TRAIN and the host re-gated the identical merge tree forever.
# The two ids differed by one trailing newline -- `$(rustc -vV)` strips
# it, `subprocess.run(...).stdout` does not -- not by which compiler.
# See toolchain.py's module docstring for the measurement.
#
# So the DIGEST FORMULA is computed in exactly one place (toolchain.py)
# and this file only carries it into shell.
#
# USAGE, from a script in tools/fleet:
#     . "$SELF_DIR/units/fleet-toolchain.sh"
#     fleet_toolchain_ids
#     [ -n "$FLEET_TOOLCHAIN_ERROR" ] && <abort loudly>
#
# Sourcing sets FLEET_TOOLCHAIN_PATH_PREFIX (see below). A successful
# call then sets:
#     RUSTC_PATH   -- the rustc that was measured
#     RUSTC_ID     -- sha256 of `rustc -vV` with `^host:` lines dropped
#     PLATFORM_ID  -- sha256 of `rustc -vV` with the `host:` line KEPT
# Both digests are over the text as `RUSTC_VV=$(rustc -vV)` yields it --
# trailing newline stripped. That normalization is not a detail; it is
# the entire incident above, and toolchain.py owns it.
# FLEET_TOOLCHAIN_ERROR is empty on success. On failure the three are left
# EMPTY and FLEET_TOOLCHAIN_ERROR carries the reason -- never a partial
# or guessed id, because a guessed platform_id is exactly the bug above.
#
# THE PATH PREFIX IS THE ONE THING SPELLED TWICE, on purpose. This file
# must be able to give gate.sh a PATH without first succeeding at a
# python3 subprocess: if it could not, a broken python3 would silently
# drop ~/.cargo/bin from the gate's PATH and build the entire gate with
# whatever rustc the system happens to carry -- a much worse failure
# than the one being fixed. It must byte-for-byte match
# toolchain.py's TOOLCHAIN_PATH_PREFIX_REL, joined onto $HOME;
# tests/test_toolchain_seam.py pins the two against each other exactly
# the way test_no_hardcoded_hosts.py pins config.py against
# units/fleet-env.sh.
: "${FLEET_TOOLCHAIN_PATH_PREFIX:=$HOME/.cargo/bin:$HOME/.local/bin}"
export FLEET_TOOLCHAIN_PATH_PREFIX

# Overridable so a fixture can point the helper at a stand-in resolver
# (and so a host with python3 somewhere unusual can say so) without this
# file growing a search of its own.
: "${FLEET_TOOLCHAIN_PY:=}"
: "${FLEET_TOOLCHAIN_PYTHON:=python3}"

fleet_toolchain_ids() {
  FLEET_TOOLCHAIN_ERROR=""
  RUSTC_PATH=""
  RUSTC_ID=""
  PLATFORM_ID=""

  _ft_py="$FLEET_TOOLCHAIN_PY"
  if [ -z "$_ft_py" ]; then
    # This file lives in tools/fleet/units/; toolchain.py is its parent.
    # `$0` is wrong for a sourced file, so derive from the caller's
    # SELF_DIR when it set one and fall back to this file's own path.
    if [ -n "${SELF_DIR:-}" ] && [ -f "$SELF_DIR/toolchain.py" ]; then
      _ft_py="$SELF_DIR/toolchain.py"
    else
      _ft_py="$(dirname "$0")/../toolchain.py"
    fi
  fi
  if [ ! -f "$_ft_py" ]; then
    FLEET_TOOLCHAIN_ERROR="toolchain resolver not found at $_ft_py"
    return 1
  fi

  # `if ! var=$(cmd)` and not `var=$(cmd); [ $? -ne 0 ]`: the status being
  # tested has to be unambiguously the resolver's own (this fleet has
  # already been bitten once by `cmd | tail -1 && echo ok` reporting
  # `tail`'s status five failed pushes in a row).
  if ! _ft_out=$("$FLEET_TOOLCHAIN_PYTHON" "$_ft_py" ids --format sh 2>&1); then
    FLEET_TOOLCHAIN_ERROR="toolchain resolver failed: $_ft_out"
    return 1
  fi

  # Parsed with a `case`, never `eval`: this is subprocess output being
  # turned into shell variables, and `eval` would execute anything that
  # ever ended up on that stream.
  while IFS= read -r _ft_line; do
    case "$_ft_line" in
      RUSTC_PATH=*)  RUSTC_PATH="${_ft_line#RUSTC_PATH=}" ;;
      RUSTC_ID=*)    RUSTC_ID="${_ft_line#RUSTC_ID=}" ;;
      PLATFORM_ID=*) PLATFORM_ID="${_ft_line#PLATFORM_ID=}" ;;
      FLEET_TOOLCHAIN_PATH_PREFIX=*) : ;;  # already defaulted above
      *) ;;                                 # ignore anything unrecognised
    esac
  done <<EOF
$_ft_out
EOF

  if [ -z "$RUSTC_ID" ] || [ -z "$PLATFORM_ID" ]; then
    FLEET_TOOLCHAIN_ERROR="toolchain resolver produced no ids (output: $_ft_out)"
    RUSTC_PATH=""; RUSTC_ID=""; PLATFORM_ID=""
    return 1
  fi
  return 0
}
