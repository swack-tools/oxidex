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

# T2 (review of staging/agent-server @ 6bf59f2b): the FILENAME SUFFIX of
# the verdict-store-failure marker gate.sh's store_verdict() writes and
# fleetd's reap loop + durable-warning sweep read back.
#
# WHY THIS IS HERE and not just in gate.sh: the two spellings used to be
# hand-kept in two files with nothing comparing them, so renaming either
# left every test green while fleetd stopped seeing a marker gate.sh was
# still writing -- the failure the marker exists to surface goes back to
# being invisible, silently. Must byte-for-byte match
# tools/fleet/config.py's VERDICT_STORE_FAILED_SUFFIX;
# tests/test_verdict_marker_seam.py::TestTheSuffixIsSpelledInExactlyTwoPlaces
# pins the two against each other, and that file's
# TestGateShAndFleetdAgreeOnTheMarkerPath evaluates gate.sh's own SV= line
# against config.verdict_store_failed_marker().
: "${FLEET_VERDICT_STORE_FAILED_SUFFIX:=.verdict-store-failed}"
export FLEET_VERDICT_STORE_FAILED_SUFFIX

# PLAN Stage 3 task 7: the wrapper supervisor's own log path, shared by
# fleetd-wrapper.sh (writer) and restart-fleetd.sh (reads it back to
# report where to look) -- exactly the class of "two files, one literal"
# duplication the rest of this file exists to close off, so it is
# spelled here once instead of a third time. ~/.keel/log, not
# ~/gatelogs -- SPEC SS2 C7: "logs go under ~/.keel/log/, never /tmp"
# (that row is about the launchd plist, but the same directory is used
# fleet-wide for the runner's own logs, including this wrapper's).
: "${FLEETD_WRAPPER_LOG:=$HOME/.keel/log/runner-wrapper.log}"
export FLEETD_WRAPPER_LOG
