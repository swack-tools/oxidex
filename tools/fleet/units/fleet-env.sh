#!/bin/sh
# fleet-env.sh -- single source of truth (shell) for fleet-wide runtime
# defaults that must never be hardcoded ad hoc across tools/fleet (R6,
# review of staging/agent-server @ 99f06cb3). tools/fleet/config.py is
# the Python mirror; both must agree on the same literal, never
# reassemble it from pieces.
#
# WHY THIS EXISTS. gate.sh used to do:
#     : "${_OXIDEX_CACHE_BASENAME:=oxidex-exiftool-cache}"
#     export EXIFTOOL_CACHE_DIR="${EXIFTOOL_CACHE_DIR:-/tmp/$_OXIDEX_CACHE_BASENAME}"
# split across a basename variable and a "/tmp/$var" reconstruction so
# that "/tmp/oxidex-exiftool-cache" never appears as one contiguous
# literal in the file -- a "two-piece assembly" that produces the exact
# same runtime value while defeating a naive substring search for the
# full path (tests/test_no_hardcoded_hosts.py). That is the same bug as
# spelling the literal out again: the default lived in a second place,
# just one that reads as different from the others at a glance. This
# file removes the need for either shape: source it, and stop.
#
# USAGE.
#     . "$(dirname "$0")/units/fleet-env.sh"       # from a script in tools/fleet
#     . "$SELF_DIR/units/fleet-env.sh"             # gate.sh already computes SELF_DIR
#
# Only sets a default -- an EXIFTOOL_CACHE_DIR already present in the
# environment (from a unit template, a hand-run export, or a caller
# testing an override) is left untouched. Safe to source more than once.
#
# Must byte-for-byte match tools/fleet/config.py's
# DEFAULT_EXIFTOOL_CACHE_DIR -- tests/test_no_hardcoded_hosts.py pins the
# two against each other.
: "${EXIFTOOL_CACHE_DIR:=/tmp/oxidex-exiftool-cache}"
export EXIFTOOL_CACHE_DIR
