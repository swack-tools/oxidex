#!/bin/bash
# fleetd-wrapper.sh -- persistent supervisor for keel-runner
# (ARCH-FIX-SPEC.md R8), for hosts with NO systemd/launchd to restart it.
# PLAN Stage 3 task 7: re-pointed from fleetd.py at keel/runner.py (SPEC
# SS2 C7); every "fleetd" below still names the CLI/env/exit-code
# contract the runner keeps unchanged (rc==0 graceful stop, "drain,
# don't kill" live workers, etc.), not a stale reference -- only the
# script path launched and the default log location changed. (Historically the
# work2 pod: a k8s container with neither `systemctl --user` nor
# `launchctl` and a generated `work2box-<hash>` hostname -- see
# fleetd.py's `host_identity()` comment; removed from the fleet
# 2026-08-22 with the rest of the ryzen. No current host needs this
# path, but the supervisor semantics are seam-tested and kept.) Linux
# hosts WITH systemd (i7 `server`) use fleetd.service instead
# (`Restart=always`); macOS hosts use com.oxidex.fleetd.plist
# (`KeepAlive`).
#
# This replaces the bare `nohup ... fleetd.py &` the pod was launched
# with before: that gave fleetd no restart-on-crash at all (a dead
# process just stayed dead until someone noticed) and no guard against a
# second hand-launch running a second scheduler alongside a live one.
#
# Loop: run fleetd in the foreground; if it exits non-zero (crash,
# fleetd.py's own rc=3 "another instance holds the singleton claim", or
# rc=4 "host lease lost"), log it and restart after a short backoff; if
# it exits ZERO -- a deliberate SIGTERM/SIGINT, e.g. from
# restart-fleetd.sh or an operator draining this host -- the wrapper's
# job is done and it exits too, rather than immediately reviving a
# daemon someone just asked to stop. See fleetd.py's own `main()`: rc==0
# is exactly and only the graceful-stop path (`stop["flag"]` set, host
# lease NOT lost); every other exit is either a startup precondition
# failure or something to retry.
#
# NEVER TOUCHES RUNNING GATES. This script only ever signals the fleetd
# PROCESS it started (SIGTERM to ask it to stop; nothing else). It holds
# no claim, greps no process list, and kills nothing beyond that one
# child. fleetd itself already knows to drain rather than kill live
# workers on the way out (see fleetd.py's "drain, don't kill" comment in
# main()'s finally block) -- this wrapper only restarts the RECONCILER,
# never reaches into what it's reconciling.
#
# Pid-file guard against double-start: checked before anything else,
# including before fleetd's own singleton claim would even run (that
# claim is a network round-trip to the hub; this is a same-host,
# no-network check that fails fast).
set -u

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FLEET_DIR="$(cd "$SELF_DIR/.." && pwd)"
# PLAN Stage 3 task 7: FLEETD_WRAPPER_LOG's default (~/.keel/log/
# runner-wrapper.log, not the old ~/gatelogs) lives in fleet-env.sh, the
# ONE place shared with restart-fleetd.sh -- see that file's own comment.
. "$SELF_DIR/fleet-env.sh"
PIDFILE="${FLEETD_WRAPPER_PIDFILE:-$HOME/.fleetd/wrapper.pid}"
LOG="$FLEETD_WRAPPER_LOG"
RETRY_SLEEP_S="${FLEETD_WRAPPER_RETRY_S:-5}"
PYTHON="${FLEETD_WRAPPER_PYTHON:-python3}"
# PLAN Stage 3 task 7: keel/runner.py, not fleetd.py -- SPEC SS2 C7.
RUNNER_SCRIPT="$FLEET_DIR/keel/runner.py"

mkdir -p "$(dirname "$PIDFILE")" "$(dirname "$LOG")"

log() {
  printf '%s fleetd-wrapper[%s]: %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$$" "$1" >> "$LOG"
}

# --- pid-file guard ---
# A stale pidfile (left by a crash, a `kill -9`, or a host reboot) is
# distinguished from a live one with `kill -0` on the recorded pid, never
# trusted blindly -- and never by matching a command line (`pgrep -f
# fleetd-wrapper.sh` would match THIS check's own subshell too, the same
# self-match class of bug `pgrep -c` caused elsewhere on this fleet).
if [ -f "$PIDFILE" ]; then
  OLD_PID=$(cat "$PIDFILE" 2>/dev/null || true)
  if [ -n "$OLD_PID" ] && kill -0 "$OLD_PID" 2>/dev/null; then
    log "refusing to start: wrapper already running as pid $OLD_PID ($PIDFILE)"
    echo "fleetd-wrapper: already running as pid $OLD_PID ($PIDFILE)" >&2
    exit 0
  fi
  log "stale pidfile for dead pid ${OLD_PID:-<empty>} -- replacing"
fi
echo "$$" > "$PIDFILE"
cleanup() {
  # Only ever removes ITS OWN pidfile, and only if it still owns it.
  [ "$(cat "$PIDFILE" 2>/dev/null || true)" = "$$" ] && rm -f "$PIDFILE"
}
trap cleanup EXIT

# Forward a stop signal to the CHILD fleetd, never to anything fleetd
# itself started. fleetd's own SIGTERM/SIGINT handler sets its stop
# flag, finishes the current reconcile step, releases its host-singleton
# claim, and exits 0 WITHOUT killing live workers. This wrapper then
# sees rc==0 in the loop below and treats that as the deliberate-stop
# case: exit, don't restart.
CHILD_PID=""
forward_term() {
  log "received stop signal -- forwarding to fleetd pid ${CHILD_PID:-<none>}"
  [ -n "$CHILD_PID" ] && kill -TERM "$CHILD_PID" 2>/dev/null
}
trap forward_term TERM INT

log "starting (hub=${FLEET_HUB_URL:-<unset>}, pid=$$)"
while true; do
  "$PYTHON" "$RUNNER_SCRIPT" "$@" &
  CHILD_PID=$!
  # `wait` on a specific pid returns EARLY, with the trapped signal's own
  # 128+signum status (143 for SIGTERM), the instant forward_term's trap
  # runs -- not the child's real eventual exit code, which may not exist
  # yet. Bash reports the interrupted wait's status here, not the
  # process's. Re-`wait` on the same pid until it actually reports the
  # child as gone (`kill -0` fails): the FIRST such completed wait is the
  # real exit status. Confirmed empirically: without this loop, a clean
  # `sys.exit(0)` stop was misreported as rc=143 and treated as a crash to
  # restart -- exactly the "deliberate stop" case this script exists to
  # get right.
  while true; do
    wait "$CHILD_PID"
    RC=$?
    kill -0 "$CHILD_PID" 2>/dev/null || break
  done
  CHILD_PID=""
  if [ "$RC" -eq 0 ]; then
    log "fleetd exited 0 (deliberate stop) -- wrapper exiting, not restarting"
    exit 0
  fi
  log "fleetd exited $RC -- restarting in ${RETRY_SLEEP_S}s"
  sleep "$RETRY_SLEEP_S"
done
