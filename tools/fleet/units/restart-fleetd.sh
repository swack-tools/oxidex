#!/bin/bash
# restart-fleetd.sh -- sanctioned, host-appropriate restart of fleetd.
#
# Picks the right mechanism for THIS host rather than assuming one:
#   - systemd --user unit installed (i7 `server`, and any other Linux
#     host with `~/.config/systemd/user/fleetd.service` -- see
#     fleetd.service)              -> `systemctl --user restart fleetd`
#   - launchd agent installed (macOS: M4 `oldair`, m5 -- see
#     com.oxidex.fleetd.plist)     -> `launchctl kickstart -k`
#   - neither (T7, ARCH-FIX-SPEC.md R8): the work2 pod, a k8s container
#     with no systemd/launchd      -> stop+relaunch fleetd-wrapper.sh
#
# The third case replaces the old bare `nohup ... fleetd.py &` restart
# (kill whatever process matched, hand-relaunch, hope nothing else was
# watching): NEVER TOUCHES RUNNING GATES. This script only ever signals
# the wrapper it started via its recorded pidfile, and the wrapper in
# turn only ever signals the fleetd child IT started (see
# fleetd-wrapper.sh's own header) -- fleetd's SIGTERM handler drains
# rather than kills live workers, so nothing this script does can reach a
# running gate.
set -u

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PIDFILE="${FLEETD_WRAPPER_PIDFILE:-$HOME/.fleetd/wrapper.pid}"
LOG="${FLEETD_WRAPPER_LOG:-$HOME/gatelogs/fleetd-wrapper.log}"
STOP_TIMEOUT_S="${FLEETD_RESTART_STOP_TIMEOUT_S:-30}"

has_systemd_unit() {
  command -v systemctl >/dev/null 2>&1 \
    && systemctl --user list-unit-files 2>/dev/null | grep -q '^fleetd\.service'
}

has_launchd_agent() {
  command -v launchctl >/dev/null 2>&1 \
    && launchctl print "gui/$(id -u)/com.oxidex.fleetd" >/dev/null 2>&1
}

restart_via_wrapper() {
  mkdir -p "$(dirname "$PIDFILE")" "$(dirname "$LOG")"
  if [ -f "$PIDFILE" ]; then
    OLD_PID=$(cat "$PIDFILE" 2>/dev/null || true)
    if [ -n "$OLD_PID" ] && kill -0 "$OLD_PID" 2>/dev/null; then
      echo "restart-fleetd: stopping wrapper pid $OLD_PID (SIGTERM) ..."
      kill -TERM "$OLD_PID" 2>/dev/null
      waited=0
      while kill -0 "$OLD_PID" 2>/dev/null; do
        if [ "$waited" -ge "$STOP_TIMEOUT_S" ]; then
          echo "restart-fleetd: wrapper pid $OLD_PID did not stop within ${STOP_TIMEOUT_S}s -- giving up, NOT force-killing it (that risks a mid-drain worker; investigate manually)" >&2
          exit 1
        fi
        sleep 1
        waited=$((waited + 1))
      done
      echo "restart-fleetd: wrapper pid $OLD_PID stopped cleanly"
    fi
  fi
  echo "restart-fleetd: starting fleetd-wrapper.sh"
  nohup "$SELF_DIR/fleetd-wrapper.sh" >> "$LOG" 2>&1 &
  disown
  sleep 1
  NEW_PID=$(cat "$PIDFILE" 2>/dev/null || true)
  echo "restart-fleetd: wrapper started, pid ${NEW_PID:-<unknown -- check $LOG>}"
}

if has_systemd_unit; then
  echo "restart-fleetd: systemd --user unit found -- systemctl --user restart fleetd"
  systemctl --user restart fleetd
elif has_launchd_agent; then
  echo "restart-fleetd: launchd agent found -- launchctl kickstart -k gui/$(id -u)/com.oxidex.fleetd"
  launchctl kickstart -k "gui/$(id -u)/com.oxidex.fleetd"
else
  echo "restart-fleetd: no systemd/launchd supervisor found for fleetd -- using fleetd-wrapper.sh (R8)"
  restart_via_wrapper
fi
