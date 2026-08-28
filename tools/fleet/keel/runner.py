#!/usr/bin/env python3
"""keel-runner -- the per-host runner: fleetd's LOCAL half, re-homed
(SPEC §2 C7, §9 "fleetd.py"; PLAN Stage 3 task 1).

This is the process the `units/*` supervisors launch (they were
re-pointed at this file by Stage 3 task 7). It keeps fleetd's CLI, env
and exit-code contract byte-for-byte -- rc 0 deliberate stop, rc 3
singleton refused, rc 4 host lease lost, rc 5 cannot rebuild worker
state, rc 6 hub unusable -- and its behaviour with NO server configured
is exactly fleetd's today: singleton, adoption, reconcile, drain-never-
kill, lost-lease kill by process group. That offline default is the
property this stage must preserve (PLAN Stage 3: "a runner works with
no server at all"), and `tests/test_runner_core.py` pins it.

THE ONE NEW THING here is the hub wiring. `build_hub()` returns

    FallbackHub(ServerHub(<server url>), Hub(<state repo>))   -- server configured
    Hub(<state repo>)                                         -- no server (default)

so every lease acquire/renew/release, every heartbeat and every claim
read below goes through the server when one is configured and answers,
and directly at the state repo when it is not (SPEC §4.3: reads always
fall back; writes fall back only on a before-send failure; an ambiguous
write raises, which `claim._note_renew_failure` already tolerates).
Nothing in the moved code knows which shape it got -- that is the point
of the eight-method contract (SPEC §4.1).

WHAT IS MOVED vs SHARED -- the split's exact boundary (SPEC §9 row
"fleetd.py": "process ownership is a runner concern; selection is a
server concern"):

MOVED (now live HERE; `fleetd.py` re-imports them from this module, so
`fleetd.<name>` remains valid for every existing consumer and test):

  * process/host primitives: `host_identity`, `owning_user`,
    `free_disk_gb`, `free_mem_gb`, `live_pgids`, `session_of`,
    `_ps_env`, `_pgid_alive`
  * workers: `Worker`, `default_gate_command`, `WORKER_MARKERS`,
    `worker_markers`, `FLEET_SCOPE_PREFIX`, `fleet_scope_token`,
    `_scoped_worker_in_group`, `fleet_worker_pgids`
  * claim-before-launch spawns: `_spawn_env`, `start_gate`,
    `start_agent`
  * the only kill: `kill_process_group`, `kill_worker`, `KILL_GRACE_S`
  * adoption: `AdoptionResult`, `adopt_workers`, `_release_claim_ref`
    (scope tokens, kin sparing, identity-verified adoption,
    unreadable-disarms-the-sweep)
  * host singleton: `singleton_ttl_s`, `FLEETD_MARKER`,
    `fleetd_marker_in_group`, `reap_dead_same_host_singleton`
  * host warnings: `HostWarnings`, `_verdict_store_failed_marker`
  * heartbeat writing: `write_heartbeat`, `HOSTS_PREFIX`,
    `PUSH_RETRIES`, `PUSH_BACKOFF_S`
  * limits/oracle probes: `_limits_ok`, `_exiftool_cache_dir`,
    `_oracle_ok`, `_gate_version`
  * the daemon shell: `run_daemon` (singleton + adoption + the
    bounded-failure loop, verbatim from `fleetd.main`'s body;
    `fleetd.main` now parses its argv and delegates here), and
    `RECONCILE_HUB_FAILURE_LIMIT`/`LOOP_SECONDS`

SHARED (still live in `fleetd.py`; this module delegates): the
SELECTION half -- `reconcile_once` itself, `ReconcileResult`,
`classify_branch`, the verdict-aware machinery (`_TreeResolver`,
`_VerdictIndex`, `_scan_gatelogs_memo`), `dispatch_agents` and
`_AGENT_RC_OUTCOMES`. PLAN Stage 4 moves those into
`keel/scheduler.py`; moving them twice (here first) would create a
third copy of the fleet's most safety-critical step function for one
stage's convenience. `reconcile_once` below is the runner's entry point
to it and documents the ORDER invariant the runner owns either way
(SPEC I5: local reap + lost-lease kill BEFORE any hub read).

NOT HERE YET (sibling Stage 3 tasks, landing separately): the job
journal + offline start (`keel/journal.py`), register/heartbeat/
long-poll/spool (the outbound protocol), election, and
`autonomous_when_serverless`.

The historical `work2`/ryzen host anecdotes in the moved docstrings
were de-specified to "a pod host" in the move: the ryzen was removed
from the fleet on 2026-08-22 and a NEW file must not re-document it as
live topology. The mechanisms those anecdotes motivated (`FLEET_HOST`
vs hostname, holder_host threading) are unchanged.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import signal
import socket
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Optional, Sequence, Union

_KEEL_DIR = Path(__file__).resolve().parent
_FLEET_DIR = _KEEL_DIR.parent
for _p in (_FLEET_DIR, _KEEL_DIR):
    if str(_p) not in sys.path:
        sys.path.insert(0, str(_p))

import claim as claim_mod  # noqa: E402
import config  # noqa: E402
from claim import Claim  # noqa: E402
from fleetlib import Hub, HubError, HubUnreachableError  # noqa: E402

# Qualified `keel.<name>` imports, not bare ones -- see serverhub.py's
# module docstring: a bare `import fallbackhub` here would risk a second,
# distinct module object (and a second, distinct exception class)
# alongside what every other file imports qualified.
from keel.fallbackhub import FallbackHub  # noqa: E402
from keel.serverhub import ServerHub  # noqa: E402
from keel import runner_toml  # noqa: E402

# --------------------------------------------------------------------- #
# Constants (moved from fleetd.py; FLEET_PLAN.md "Shared contracts" is
# the authority)
# --------------------------------------------------------------------- #

LOOP_SECONDS = 15
HOSTS_PREFIX = "refs/fleet/hosts/"

# Transient-push retry: rapid consecutive pushes to the real hub fail and
# then succeed on a spaced retry (observed repeatedly on 2026-08-14; the
# 1Password ssh agent drops signature requests under bursts, and the hub's
# post-receive hook holds a lock per push). Single pushes almost always
# succeed, so a short backoff ladder is enough.
PUSH_RETRIES = 3
PUSH_BACKOFF_S = 4

# Seconds between SIGTERM and SIGKILL when tearing down a worker whose
# lease was lost. Short by design: the window we are closing is "two hosts
# running the same gate", and every second of grace is a second of that.
KILL_GRACE_S = float(os.environ.get("FLEET_KILL_GRACE_S", "10"))

def singleton_ttl_s() -> float:
    """The host-singleton lease's TTL. Shorter than the default gate/agent
    LEASE_TTL (600s) on purpose: this is not a claim on paid work, it is
    one fleetd asserting "I am the scheduler for this host" -- the longer
    that lease outlives a hard-killed holder, the longer a host runs with
    no scheduler, no heartbeat, watching nothing (ARCH-FIX-SPEC.md R6/T6).
    `reap_dead_same_host_singleton` already collapses the *provably-dead,
    same-host* case to a `ps`-listing round trip, but the TTL is still the
    backstop for every case that guard cannot decide (payload lost, `ps`
    unavailable, cross-host) -- 120s bounds that window instead of the
    full 600s. `Claim.__init__`'s clamp (`renew_interval <= ttl / 2`)
    turns this into a renew cadence of at most 60s, same as this file's
    `RENEW` default.

    `claim_mod.TTL_ENV` (`FLEET_TEST_TTL_S`) is checked FIRST and, if set,
    wins outright: every fixture-hub test compresses ALL lease TTLs --
    gate, agent, and singleton alike -- to one shared value so a held
    claim's lifetime stays proportional across the suite (test_adoption.py,
    test_seams.py). Passing an explicit `ttl=` to `Claim()` bypasses that
    env lookup entirely (see `Claim.__init__`), so reading `TTL_ENV` here
    ourselves is what keeps hermetic tests hermetic instead of quietly
    switching the singleton to a real 120s while gate claims stay
    compressed to single-digit seconds -- exactly the kind of seam this
    effort exists to close. Only when `FLEET_TEST_TTL_S` is unset (real
    daemon, and the seam suite's uncompressed slow run) does
    `FLEET_SINGLETON_TTL_S` (default 120) apply. Both reads go through
    `claim_mod._env_seconds`, which never raises: a malformed override
    leaves the TTL at its default rather than taking the daemon down at
    import.
    """
    if os.environ.get(claim_mod.TTL_ENV, "").strip():
        return claim_mod._env_seconds(claim_mod.TTL_ENV, 120)
    return claim_mod._env_seconds("FLEET_SINGLETON_TTL_S", 120)

# How many CONSECUTIVE reconcile steps may die on a `HubError` before the
# daemon stops and lets its supervisor deal with it.
#
# `reconcile_once` reads five or six hub refs and `workqueue.Queue.compute`
# reads every claim payload, and until now not one of those calls had a
# `try` around it, in `reconcile_once` or in `main`. One `HubError` --
# `fleetlib.Hub.read`'s ls-remote/fetch race (fixed in fleetlib, but the
# class of failure is not: a dropped ssh signature, a hub mid-`gc`, a full
# disk in the object cache) -- did not degrade a queue, it EXITED THE
# DAEMON, taking the scheduler for a host with live gates on it.
#
# So one bad step costs one step. But the opposite failure is just as real
# and much quieter: a daemon that swallows every hub error forever looks
# alive, logs cheerfully, starts nothing, renews nothing it did not already
# hold, and reports a heartbeat only because that write is inside the step
# that is failing. A host wedged that way is indistinguishable from an idle
# one until someone reads the log. The cap is the difference: transient
# failures are absorbed, a persistently unreachable hub still surfaces as a
# nonzero exit for the supervisor (units/fleetd-wrapper.sh) to act on.
#
# Five at the 15s loop is ~75 seconds of hub trouble tolerated, which is an
# order of magnitude longer than any blip observed on this fleet and well
# inside a 600s lease TTL -- the daemon gives up long before the claims it
# is failing to renew could be reaped out from under it.
RECONCILE_HUB_FAILURE_LIMIT = int(os.environ.get("FLEET_RECONCILE_HUB_FAILURE_LIMIT", "5"))


def host_identity() -> str:
    """The fleet host name, overridable for tests and for hosts whose
    hostname is not their fleet name (a pod host's hostname is a
    generator-assigned `<name>-<hash>`)."""
    return os.environ.get("FLEET_HOST") or socket.gethostname().split(".")[0]


def owning_user() -> str:
    return os.environ.get("FLEET_USER") or os.environ.get("USER") or "unknown"

# --------------------------------------------------------------------- #
# Host probes -- injectable so tests never depend on real disk/mem
# --------------------------------------------------------------------- #


def free_disk_gb(path: str = None) -> float:
    usage = shutil.disk_usage(path or str(Path.home()))
    return usage.free / 1e9


def free_mem_gb() -> float:
    """Best-effort available memory in GB; -1 if unknowable (macOS without
    psutil). A probe that cannot answer must say so rather than guess --
    the limits check treats -1 as 'unknown, do not block on memory'."""
    try:
        if sys.platform == "linux":
            with open("/proc/meminfo") as f:
                for line in f:
                    if line.startswith("MemAvailable:"):
                        return int(line.split()[1]) / 1e6
        elif sys.platform == "darwin":
            out = subprocess.run(
                ["vm_stat"], capture_output=True, text=True, errors="replace", timeout=10
            ).stdout
            page_size = 16384
            free_pages = 0
            for line in out.splitlines():
                if "page size of" in line:
                    page_size = int("".join(c for c in line.split("page size of")[1] if c.isdigit()))
                for key in ("Pages free:", "Pages inactive:"):
                    if line.startswith(key):
                        free_pages += int(line.split(":")[1].strip().rstrip("."))
            return free_pages * page_size / 1e9
    except (OSError, ValueError, subprocess.TimeoutExpired):
        pass
    return -1.0


def live_pgids() -> set:
    """Process groups alive on this host, by listing -- the instrument is
    `ps -eo pgid=`, never pgrep."""
    try:
        out = subprocess.run(
            ["ps", "-eo", "pgid=,stat="], capture_output=True, text=True, errors="replace", timeout=10
        ).stdout
        pgids = set()
        for line in out.splitlines():
            parts = line.split()
            if len(parts) >= 2 and parts[0].isdigit() and not parts[1].startswith("Z"):
                pgids.add(int(parts[0]))
        return pgids
    except (OSError, subprocess.TimeoutExpired):
        return set()

# --------------------------------------------------------------------- #
# Gate workers
# --------------------------------------------------------------------- #


@dataclass
class Worker:
    """A gate this fleetd instance started and still tracks. The claim is
    the durable record; this object only exists for the lifetime of the
    daemon and is rebuilt from claims + live pgids after a restart."""

    branch: str
    tag: str
    pgid: int
    claim: Claim
    popen: Optional[subprocess.Popen] = None
    kind: str = "gate"

    def alive(self, pgids: Optional[set] = None) -> bool:
        # For workers we spawned, poll() is the truth AND reaps the child:
        # without it a finished gate stays a zombie, zombies still appear
        # in `ps -eo pgid`, and the worker reads as alive forever (observed:
        # a PASSed gate held its fleetd slot as <defunct> until restart).
        if self.popen is not None:
            return self.popen.poll() is None
        return self.pgid in (pgids if pgids is not None else live_pgids())


def default_gate_command(repo_root: Path) -> list:
    return [str(repo_root / "tools" / "fleet" / "gate.sh")]


# Substrings that identify a process-group LEADER as a fleet worker, for the
# orphan sweep in `adopt_workers`. Overridable via FLEET_WORKER_MARKERS
# (comma-separated) so a host whose gate lives at a non-default path -- and
# the fixture tests, whose "gate" is a stub script -- can say so without
# this list growing guesses.
WORKER_MARKERS = ("tools/fleet/gate.sh", "tools/fleet/agentworker.py")


def worker_markers() -> tuple:
    raw = os.environ.get("FLEET_WORKER_MARKERS")
    if raw and raw.strip():
        return tuple(m for m in (p.strip() for p in raw.split(",")) if m)
    return WORKER_MARKERS


# The scope token: an inert extra argv element stamped onto every worker this
# daemon spawns, derived from the daemon's own hub URL. The orphan sweep may
# only kill a marker-matched group that ALSO carries the sweeping daemon's
# token -- a marker alone proves a process is gate-SHAPED, not that it is
# OURS to signal.
#
# Why this exists (incident, 2026-08-20): `TestFleetdSingletonRenews` spawns
# a real `fleetd.py` against a throwaway fixture hub with production
# WORKER_MARKERS. That daemon's startup sweep read its (empty) fixture hub,
# concluded every gate on the host was unleased, and SIGKILLed a live,
# manually-launched gate mid-run -- no verdict, no journal trace, and the
# gate's own fleet-tests stage was what ran the murdering test. Scoping the
# kill to the daemon's own hub makes that impossible for ANY fixture daemon,
# and as a stated consequence makes out-of-band workers (a human running
# gate.sh by hand carries no token) unsweepable: absence of provenance now
# reads "not ours", never "orphan".
FLEET_SCOPE_PREFIX = "fleet-scope="


def fleet_scope_token(hub_url: str) -> str:
    """`fleet-scope=<first 12 hex of sha256(hub_url)>`. Same URL string
    (modulo a trailing slash), same token -- which is exactly the guarantee
    a supervisor restart needs, since the unit file respawns the daemon with
    the identical --hub argument. Two spellings of one hub (ssh vs local
    path) yield different tokens; that costs a sweep its kill, never a live
    worker its life, so it is the acceptable direction."""
    digest = hashlib.sha256(
        str(hub_url).rstrip("/").encode("utf-8", "surrogateescape")
    ).hexdigest()[:12]
    return FLEET_SCOPE_PREFIX + digest

def _ps_env() -> dict:
    """Env for `ps` calls whose COMMAND column must not be truncated. procps
    documents that piped output width is 'undefined' and that an exported
    COLUMNS 'may be used to exactly determine the width' -- i.e. a stray
    COLUMNS from the invoking terminal can silently truncate command lines
    even through a pipe. The scope token rides at the END of a worker's
    argv, so truncation eats the kill entitlement first and the sweep
    no-ops silently. `-ww` (unlimited width) plus a scrubbed COLUMNS closes
    both ends; both are supported by procps and macOS ps."""
    env = dict(os.environ)
    env.pop("COLUMNS", None)
    return env


def session_of(pid: int) -> Optional[int]:
    """Session id of `pid` via the getsid(2) SYSCALL, or None if the pid is
    gone (or the platform refuses). NOT a `ps` column on purpose: macOS ps
    prints `sess=` as 0 for everything (it is a kernel struct address
    there, masked), so a listing-based session map silently no-ops on
    darwin -- measured 2026-08-20, the same day this was written.

    The orphan sweep uses this to recognize KIN: gate.sh's
    wall-clock-timeout stage runs `( ... ) &` under `set -m`, which makes
    the stage subshell a process-group LEADER of its own whose argv is the
    gate's (marker and scope token included, since a forked bash keeps its
    parent's argv) -- but no claim ever names that transient pgid. Its
    SESSION id is the main gate's pid (start_new_session at spawn), which
    IS the claimed pgid. POSIX: a process group never spans sessions, so
    the leader's sid speaks for the whole group."""
    try:
        return os.getsid(pid)
    except (OSError, AttributeError):
        return None


def _scoped_worker_in_group(pgid: int, markers: Optional[Sequence[str]],
                            scope_token: Optional[str]) -> Optional[str]:
    """Command line of a live, same-uid member of group `pgid` that carries
    BOTH a worker marker and `scope_token` -- or None. This is adoption's
    identity check: `fleetd_marker_in_group`'s refusal to trust a recycled
    pgid, generalized to gate/agent claims (see the adoption block). Any
    member counts, not only the leader -- the leader IS the worker here,
    but a worker mid-exec can momentarily present oddly and its children
    carry the same argv."""
    if scope_token is None:
        return None
    markers = tuple(markers) if markers else worker_markers()
    try:
        out = subprocess.run(
            ["ps", "-wweo", "pgid=,uid=,command="],
            capture_output=True, text=True, errors="replace", timeout=10,
            env=_ps_env(),
        ).stdout
    except (OSError, subprocess.TimeoutExpired):
        return None
    try:
        uid = os.getuid()
    except AttributeError:
        uid = None
    for line in out.splitlines():
        parts = line.split(None, 2)
        if len(parts) < 3 or not (parts[0].isdigit() and parts[1].isdigit()):
            continue
        if int(parts[0]) != pgid:
            continue
        if uid is not None and int(parts[1]) != uid:
            continue
        command = parts[2]
        if any(m in command for m in markers) and scope_token in command:
            return command
    return None


def fleet_worker_pgids(markers: Optional[Sequence[str]] = None) -> dict:
    """`{pgid: command}` for fleet-worker process groups on this host.

    By LISTING (`ps -eo`), never `pgrep` -- the rule this daemon's docstring
    already states, for the reason it states.

    Three filters, each of which exists to stop this function reporting
    something the orphan sweep would then kill:

      * same uid only. `ps -e` shows every user's processes, and another
        user's fleetd running its own gates on a shared box is not ours to
        signal.
      * group LEADER only (`pid == pgid`). Workers are spawned with
        `start_new_session`, so the leader is the gate/agent itself; the
        cargo and rustc children underneath it share the pgid and would
        otherwise each report the same group again.
      * an explicit command marker. A pgid is not evidence of anything on
        its own, and the sweep SIGTERMs what this returns.

    Consequence of the leader filter, stated rather than hidden: a group
    whose leader has already exited while children linger is not reported,
    so it is not swept. That is the safe direction -- the alternative is a
    matcher that fires on `rustc` command lines.
    """
    markers = tuple(markers) if markers else worker_markers()
    try:
        out = subprocess.run(
            ["ps", "-wweo", "pgid=,pid=,uid=,command="],
            capture_output=True, text=True, errors="replace", timeout=10,
            env=_ps_env(),
        ).stdout
    except (OSError, subprocess.TimeoutExpired):
        return {}
    try:
        uid = os.getuid()
    except AttributeError:
        uid = None
    found: dict = {}
    for line in out.splitlines():
        parts = line.split(None, 3)
        if len(parts) < 4:
            continue
        spgid, spid, suid, command = parts
        if not (spgid.isdigit() and spid.isdigit() and suid.isdigit()):
            continue
        pgid, pid, puid = int(spgid), int(spid), int(suid)
        if uid is not None and puid != uid:
            continue
        if pid != pgid:
            continue
        if not any(m in command for m in markers):
            continue
        found[pgid] = command
    return found


# --------------------------------------------------------------------- #
# Host-singleton reap-before-expiry (ARCH-FIX-SPEC.md FIX 2)
#
# `Claim.acquire_or_reap()` (claim.py) only reaps an EXPIRED claim. A
# hard-killed fleetd's own host singleton (`refs/fleet/claims/host/<host>`)
# is unrenewed but sits on the hub, unexpired, for up to a full LEASE_TTL
# (600s in production, compressed in tests) -- and every successor's
# startup path refuses for that entire window, because the guard it hits
# is correct by the letter of the lease (not yet expired) and wrong by the
# fact of the matter (the holder is dead). See main()'s singleton block.
#
# The fix idiom already exists in this file for GATE and AGENT claims:
# `adopt_workers` treats "claim.holder_host == us AND claim.pgid is gone
# from the `ps` listing" as proof of death, never waiting for expiry.
# `reap_dead_same_host_singleton` is exactly that evidence, applied to the
# daemon's OWN claim instead of the work it supervises -- with one twist
# `adopt_workers` does not need, documented on `fleetd_marker_in_group`.
# --------------------------------------------------------------------- #

FLEETD_MARKER = "fleetd.py"
# The runner's own marker is the PATH SUFFIX the supervisors launch
# ("$FLEET_DIR/keel/runner.py" -- units/fleetd-wrapper.sh, fleetd.service,
# com.oxidex.fleetd.plist all use the absolute path), not the bare
# basename: "runner.py" alone would also match unrelated argv like a
# hand-run script named runner.py in some other tree. A hand-started
# `python3 runner.py` from inside keel/ is NOT matched -- the cost is a
# successor waiting out the singleton TTL instead of fast-reaping, the
# safe direction, and the supervised launch (the only one the reap fast
# path exists for) always carries the full path.
RUNNER_MARKER = "keel/runner.py"
# Both entry points hold the SAME host-singleton ref
# (`refs/fleet/claims/host/<host>`) during the fleetd->keel-runner
# migration, so a successor of EITHER kind must recognize a live
# predecessor of either kind. With fleetd.py's old single-marker default,
# a keel-runner successor probing its hard-killed... no: probing a LIVE
# fleetd predecessor still matched, but a fleetd/runner successor probing
# a LIVE runner predecessor's group found no "fleetd.py" member,
# concluded "provably dead", and CAS-deleted a claim whose holder was
# alive and between renewals -- the exact false-reap this probe exists to
# prevent. `tests/test_runner_core.py::TestDaemonMarkers` pins both
# directions.
DAEMON_MARKERS = (FLEETD_MARKER, RUNNER_MARKER)


def fleetd_marker_in_group(pgid: int, exclude_pid: Optional[int] = None,
                           marker: "Union[str, Sequence[str]]" = DAEMON_MARKERS,
                           ) -> Optional[str]:
    """Command line of a live, same-uid process in group `pgid` whose
    command contains `marker` (a string, or any of a sequence of strings
    -- the default is `DAEMON_MARKERS`, both host-scheduler entry
    points, see above) -- or None if the CURRENT `ps` listing has
    no such process, INCLUDING if the listing could not be taken at all
    (a `ps` failure returns a non-None sentinel, not None, so the caller
    fails closed onto "still there" rather than concluding death from a
    missing instrument).

    Every MEMBER of the group is checked here, not just its leader
    (contrast `fleet_worker_pgids`'s `pid == pgid` filter). That filter is
    right for gates and agents, which own their session
    (`start_new_session=True`) and so are always their own leader. fleetd
    itself is not: run under `fleetd-wrapper.sh` (R8), it is a plain
    background job (`fleetd.py "$@" &`) of a non-interactive bash script,
    which never calls `setpgid` for it -- so it SHARES the wrapper's
    process group, and the group's actual leader is the wrapper, whose
    command line is `bash .../fleetd-wrapper.sh ...` and never contains
    "fleetd.py". A leader-only scan would see only the wrapper and report
    "no fleetd here" even while a live fleetd genuinely is one member
    over -- silently defeating this whole check under the one supervisor
    the seam-4 restart test (and the supervised hosts) actually run.

    THE NASTY EDGE, and why `exclude_pid` exists: because fleetd shares
    its wrapper's process group, a freshly started SUCCESSOR is ALREADY a
    member of this exact group -- the same recorded pgid -- by the time
    it runs this check, before it has acquired anything. Its own command
    line trivially contains "fleetd.py". An unguarded scan matches that
    row and concludes the dead predecessor is alive forever: a permanent
    lockout disguised as safety. `exclude_pid` must be the CALLER's own
    `os.getpid()` so its own row is skipped -- the same self-match hazard
    `adopt_workers` guards against via `own_pgid`, applied here at pid
    grain because pgid alone cannot distinguish "me" from "the wrapper";
    both share it. (`SupervisedFleetd.fleetd_pid` in test_seams.py notes
    the identical hazard for `pgrep -f fleetd` matching the TEST's own
    command line -- same class of bug, different tool.)

    A pgid that is present but recycled to some unrelated, non-fleetd
    process reads exactly like "predecessor dead" here (no member matches
    the marker) -- there is no richer identity than a command string in a
    `ps` listing, so this is as far as "matching identity" can go. That
    residual is why the caller treats the result as "safe to attempt a
    reap", never "safe to assume", and the reap itself is a CAS delete
    against the exact sha just read (see `reap_dead_same_host_singleton`):
    if the recorded holder turns out to be alive and renewing after all,
    the CAS loses to that renewal and nothing is removed.
    """
    markers = (marker,) if isinstance(marker, str) else tuple(marker)
    try:
        out = subprocess.run(
            ["ps", "-eo", "pgid=,pid=,uid=,command="],
            capture_output=True, text=True, errors="replace", timeout=10,
        ).stdout
    except (OSError, subprocess.TimeoutExpired):
        return "<ps listing unavailable -- refusing to declare pgid %d dead>" % pgid
    try:
        uid = os.getuid()
    except AttributeError:
        uid = None
    for line in out.splitlines():
        parts = line.split(None, 3)
        if len(parts) < 4:
            continue
        spgid, spid, suid, command = parts
        if not (spgid.isdigit() and spid.isdigit() and suid.isdigit()):
            continue
        rpgid, rpid, ruid = int(spgid), int(spid), int(suid)
        if rpgid != pgid:
            continue
        if uid is not None and ruid != uid:
            continue
        if exclude_pid is not None and rpid == exclude_pid:
            continue
        if any(m in command for m in markers):
            return command
    return None


def reap_dead_same_host_singleton(
    hub: Hub,
    host: str,
    ref: str,
    own_pid: Optional[int] = None,
    marker_probe: Callable[..., Optional[str]] = fleetd_marker_in_group,
) -> bool:
    """Force a stale host-singleton claim off the hub when its holder is
    THIS host and its process group is provably dead, even though the
    claim has not yet expired. Returns True iff this call deleted it.

    Every condition below fails CLOSED (returns False, changes nothing):
    the ref is gone already (nothing to reap -- let the caller's plain
    `acquire()` retry see that), held by a DIFFERENT host (never ours to
    reason about -- `Claim.adopt`'s own rule, repeated here), carries no
    usable pgid, or the pgid still has a live fleetd.py-looking member.
    The only way this returns True is a same-host claim whose recorded
    process group has no fleetd.py process in it at all.

    Even then, the deletion itself is a CAS (`Hub.delete(expect_sha=...)`)
    against the exact sha just read: if the claim is being renewed
    concurrently by a holder that was merely slow to answer `ps` (or the
    `ps` evidence above was simply wrong), the renewal already moved the
    ref and this CAS fails harmlessly. Reaping a live lease requires BOTH
    the process-liveness check and the hub's own CAS to be wrong in the
    same instant; this only needs one of them to be right.

    Nothing here sends a signal to any process. It only ever deletes THIS
    host's own stale ref -- never another host's, never a worker's.
    """
    sha = hub.sha(ref)
    if sha is None:
        return False
    payload = hub.read(ref)
    if payload is None or payload.get("holder_host") != host:
        return False

    pgid = payload.get("pgid")
    pgid = pgid if isinstance(pgid, int) else None
    if pgid is None or pgid <= 1:
        return False  # no usable evidence -- refuse rather than guess

    if marker_probe(pgid, own_pid) is not None:
        return False  # a live fleetd.py-looking process still holds the group

    try:
        return hub.delete(ref, expect_sha=sha)
    except HubError:
        return False


def _pgid_alive(pgid: int) -> bool:
    """Does any process remain in group `pgid`? `killpg(pgid, 0)` is the
    cheap probe; `live_pgids()` (the `ps` listing) is the instrument used
    to VERIFY a kill, because a signal probe cannot distinguish a group
    that is gone from one we merely lost the right to signal."""
    try:
        os.killpg(pgid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True  # exists, not ours -- treat as alive, never as gone
    except OSError:
        return False


def kill_process_group(
    pgid: int,
    grace: Optional[float] = None,
    alive_probe: Optional[Callable[[int], bool]] = None,
    poll: float = 0.25,
) -> str:
    """SIGTERM the whole group, then SIGKILL whatever is left after
    `grace`. Returns a short human description for the log line.

    Signalling the GROUP, not the pid, is the point: gates spawn cargo,
    which spawns rustc, and a pid-only kill leaves those orphaned (M8).
    `start_gate` puts every worker in its own session precisely so this
    call can never reach anything else -- and the own-pgid guard below is
    the belt to that braces, because a fleetd that SIGKILLs its own
    process group takes out every gate on the host.
    """
    grace = KILL_GRACE_S if grace is None else grace
    alive = alive_probe or _pgid_alive
    if pgid <= 1:
        return f"refused: implausible pgid {pgid}"
    try:
        own = os.getpgrp()
    except (AttributeError, OSError):
        own = None
    if own is not None and pgid == own:
        return f"refused: pgid {pgid} is fleetd's own process group"

    try:
        os.killpg(pgid, signal.SIGTERM)
    except ProcessLookupError:
        return "already gone before SIGTERM"
    except OSError as e:
        return f"SIGTERM failed: {e}"

    deadline = time.time() + grace
    while time.time() < deadline:
        if not alive(pgid):
            return "exited on SIGTERM"
        time.sleep(poll)

    try:
        os.killpg(pgid, signal.SIGKILL)
    except ProcessLookupError:
        return "exited on SIGTERM (during grace)"
    except OSError as e:
        return f"SIGKILL failed: {e}"
    return f"SIGKILLed after {grace:g}s grace"


def kill_worker(w: "Worker", grace: Optional[float] = None) -> str:
    """Tear a worker down by process group and drop its claim.

    THE KILL IS NOT HOSTAGE TO THE HUB. The signal work above is purely
    local -- `killpg` and a `ps` listing -- and the claim delete below is
    cleanup. The lost-lease kill in `reconcile_once` runs precisely when
    the hub is misbehaving (that is what makes a lease go lost), so a
    `HubError` out of `release` must not propagate: the process is already
    dead by then, and letting the exception escape would strand a killed
    worker in the caller's `workers` list and abort the rest of the step.
    An undeleted claim is self-correcting -- it expires on its TTL and any
    host reaps it (`claim.reap_expired`); a stranded worker entry is not.
    """
    outcome = kill_process_group(w.pgid, grace=grace, alive_probe=lambda _p: w.alive())
    if w.popen is not None:
        try:
            w.popen.wait(timeout=5)  # reap, so it cannot linger as a zombie
        except subprocess.TimeoutExpired:
            outcome += " (child not reaped within 5s)"
    # Safe either way: `release` is a CAS delete, so if the lease really
    # is somebody else's now, this cannot touch their claim.
    try:
        w.claim.release()
    except HubError as e:
        outcome += f" (claim release failed, expires on TTL: {e})"
    return outcome


def _spawn_env(hub: Hub) -> dict:
    """The environment for a spawned gate/agent subprocess (B4, Stage 1
    integration review).

    `subprocess.Popen` with no `env=` fully inherits fleetd's own process
    environment -- fine as long as fleetd's config always arrived via
    FLEET_HUB_URL/FLEET_CODE_URL in that environment. It stopped being
    fine the moment fleetd could also be started `--hub <state> --code
    <code>` on argv alone (`main`'s `args.hub`/`args.code`): those land on
    `hub.url`/`hub.code_url` but are never written back into
    `os.environ`, so a spawned `gate.sh` -- which reads the two vars
    directly, never argv -- saw neither and hit its "ABORT config: … not
    set" path, every loop, with the daemon reporting nothing wrong.

    So the child's hub config is made an explicit function of the `Hub`
    object fleetd is actually running against, never of how fleetd itself
    happened to be invoked. Everything else in fleetd's own environment is
    inherited unchanged -- in particular FLEET_GIT_TOKEN_FILE (the
    credential-helper token file `fleetlib._raw_run` reads),
    EXIFTOOL_CACHE_DIR (the pinned-oracle cache dir `gate.sh` reads) and
    FLEET_TRAIN_DEPLOY_KEY (the train's ssh deploy key path) pass straight
    through when the operator set them on fleetd's own environment, so a
    gate or train subprocess sees the identical value fleetd saw rather
    than a default re-derived independently downstream.
    """
    env = dict(os.environ)
    env["FLEET_HUB_URL"] = hub.url
    env["FLEET_CODE_URL"] = hub.code_url
    return env


def start_gate(
    hub: Hub,
    branch: str,
    tag: str,
    gate_command: list,
    host: str,
    log_dir: Path,
) -> Optional[Worker]:
    """Claim the branch, then launch the gate in its OWN process group
    (os.setsid via start_new_session -- portable to macOS, which has no
    setsid binary). Kill = kill the group; orphaned cargo/rustc children
    become impossible (M8). Returns None if someone else holds the claim."""
    # holder_host=host, NOT Claim's `socket.gethostname()` default. These
    # are different strings wherever `FLEET_HOST` is set -- e.g. a pod
    # host, whose hostname is a generator-assigned `<name>-<hash>` while
    # its fleet identity is the name. `start_gate` has always taken `host`
    # and, until R6 needed it, never used it: nothing read `holder_host`,
    # so the skew cost nothing and stayed invisible. It stops being free
    # the moment adoption asks "is this claim mine?", and it would have
    # answered "no" for every claim on the pod while reporting a perfectly
    # healthy "nothing to adopt". R4's `fleet status` WORK column joins
    # claims to heartbeat rows on this same field and would likewise have
    # matched nothing.
    c = Claim(hub, kind="gate", key=branch.replace("/", "-"), work_kind="gate",
              work_key=branch, holder_host=host)
    try:
        # acquire_or_reap: an EXPIRED claim (crashed holder, TTL passed)
        # must not block the branch forever -- reap it CAS'd and proceed.
        # A live claim still refuses, which is the double-gate guard.
        c.acquire_or_reap()
    except claim_mod.ClaimHeldError:
        return None
    log_dir.mkdir(parents=True, exist_ok=True)
    log = open(log_dir / f"fleetd-gate-{tag}.launch.log", "ab")
    try:
        # The trailing scope token is inert to gate.sh (it reads $1/$2 only)
        # but visible in `ps -eo command=`, which is what entitles this
        # daemon's orphan sweep to kill the group later. See
        # `fleet_scope_token`.
        popen = subprocess.Popen(
            gate_command + [branch, tag, fleet_scope_token(hub.url)],
            stdout=log,
            stderr=subprocess.STDOUT,
            stdin=subprocess.DEVNULL,
            start_new_session=True,  # its own pgid == its pid
            env=_spawn_env(hub),  # B4: explicit FLEET_HUB_URL/FLEET_CODE_URL
        )
    except OSError:
        c.release()
        raise
    finally:
        log.close()
    worker = Worker(branch=branch, tag=tag, pgid=popen.pid, claim=c, popen=popen)
    # Persist the real pgid into the claim payload: renew() rewrites the
    # payload from the object's fields, so setting the attribute and
    # renewing once records it durably (claim-before-launch means the
    # first write couldn't know it yet).
    c.pid = popen.pid
    c.pgid = popen.pid
    c.renew()
    return worker


def start_agent(
    hub: Hub, branch: str, tag: str, host: str, log_dir: Path, repo_root: Path
) -> Optional[Worker]:
    """Claim the branch for an agent and launch agentworker.py in its own
    process group. Same discipline as gates: claim-before-launch, pgid
    persisted via renew."""
    intent_slug = branch.removeprefix("intent:") if branch.startswith("intent:") else None
    # holder_host=host: see the identical comment in start_gate above.
    c = Claim(hub, kind="agent", key=branch.replace("/", "-").replace(":", "-"),
              work_kind="agent", work_key=branch, holder_host=host)  # see start_gate
    try:
        c.acquire_or_reap()
    except claim_mod.ClaimHeldError:
        return None
    log_dir.mkdir(parents=True, exist_ok=True)
    log = open(log_dir / f"fleetd-agent-{tag}.log", "ab")
    mode_args = (["--intent", intent_slug] if intent_slug else ["--branch", branch])
    try:
        popen = subprocess.Popen(
            # `_FLEET_DIR`, not `Path(__file__).parent`: this file moved
            # one directory down (tools/fleet/keel/) while agentworker.py
            # stayed at tools/fleet/ -- the one path the verbatim move had
            # to touch.
            [sys.executable, str(_FLEET_DIR / "agentworker.py"),
             *mode_args, "--hub", hub.url,
             # S2 (Stage 1 integration review): the CODE repo is what the
             # worker clones and probes `refs/heads/*` on; `--hub` is the
             # STATE repo and answers `refs/fleet/intents/*` only.
             "--code", hub.code_url, "--host", host,
             # Inert positional (agentworker accepts and ignores it); its only
             # consumer is `ps` via the orphan sweep. See `fleet_scope_token`.
             fleet_scope_token(hub.url)],
            stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
            start_new_session=True,
            env=_spawn_env(hub),  # B4: explicit FLEET_HUB_URL/FLEET_CODE_URL
        )
    except OSError:
        c.release()
        raise
    finally:
        log.close()
    w = Worker(branch=branch, tag=tag, pgid=popen.pid, claim=c, popen=popen, kind="agent")
    c.pid = popen.pid
    c.pgid = popen.pid
    c.renew()
    return w


def _verdict_store_failed_marker(log_dir: Path, tag: str) -> Path:
    """The sibling marker `gate.sh`'s `store_verdict()` writes beside
    `gate-<tag>.verdict` when it could not push that verdict to the hub
    cache (R4).

    T2: the filename is COMPOSED IN `config.py` now, not spelled here.
    This function and `gate.sh`'s `SV=` line were two hand-kept spellings
    of one filename with nothing comparing them, so renaming either left
    the whole suite green while fleetd stopped seeing a marker gate.sh was
    still writing. `config.VERDICT_STORE_FAILED_SUFFIX` and
    `units/fleet-env.sh`'s `FLEET_VERDICT_STORE_FAILED_SUFFIX` are the two
    canonical spellings, pinned against each other by
    `tests/test_verdict_marker_seam.py::TestTheSuffixIsSpelledInExactlyTwoPlaces`
    and against gate.sh's own evaluated `SV=` line by that file's
    `TestGateShAndFleetdAgreeOnTheMarkerPath`."""
    return config.verdict_store_failed_marker(log_dir, tag)


class HostWarnings:
    """Durable, cross-reconcile host conditions -- T3.

    `ReconcileResult.refused` is a PER-LOOP field: the reap step reports
    `verdict-store-failed` on the one pass that reaps the gate, the
    heartbeat carries it for that one 15-second window, and the next
    reconcile overwrites the heartbeat with a `refused[]` that no longer
    mentions it. An operator who ran `fleet status --why` sixteen seconds
    later saw nothing. Worse, only gates *fleetd itself spawned and
    reaped* were ever checked: a marker left by `train.real_gate` or by a
    hand-run `gate.sh` was never read by anything, on any host, ever.

    This closes both. The sweep is over the LOG DIRECTORY, not over
    `workers`, so provenance stops mattering -- any `gate-*<suffix>`
    marker in `~/gatelogs` becomes a warning. Warnings persist across
    reconciles for exactly as long as their marker file exists: a gate
    that re-runs and stores successfully deletes its own marker
    (`store_verdict`'s `rm -f "$SV"`), and the next sweep drops the entry.
    Nothing here expires on a timer, because the condition it reports does
    not.

    Deliberately NOT merged into `refused[]`: "refused" answers "why did
    this loop start nothing", and a verdict-store failure does not stop
    anything from starting. Answering the wrong question loudly is how the
    two facts would end up being read as one.
    """

    def __init__(self):
        # marker path (str) -> (reason, detail). Keyed by PATH so the same
        # gate's marker cannot accumulate duplicate entries across loops,
        # and so removal is a pure set difference against what the sweep
        # sees.
        self.entries: dict = {}

    def scan(self, log_dir) -> list:
        """Re-derive the warning list from `log_dir` and return it as a
        sorted `[(reason, detail)]`.

        Both directions in one pass: a marker that has appeared becomes an
        entry, a marker that has been removed loses its entry. An
        unreadable/absent log directory yields the entries unchanged --
        "we could not look" must not read as "the condition cleared".
        """
        log_dir = Path(log_dir)
        # `Path.glob` on a directory that does not exist yields NOTHING and
        # raises NOTHING, so without this guard an absent `~/gatelogs`
        # (a fresh host, a moved `--log-dir`) read as "every marker is
        # gone" and cleared every warning on the next loop. Same rule as
        # the OSError branch: not being able to look is not the same as
        # having looked and found nothing.
        if not log_dir.is_dir():
            return self.current()
        try:
            names = sorted(p.name for p in log_dir.glob(config.verdict_store_failed_glob()))
        except OSError:
            return self.current()
        seen = {}
        for name in names:
            tag = config.verdict_store_failed_tag(name) or name
            seen[str(log_dir / name)] = (
                "verdict-store-failed",
                f"{tag}: gate.sh could not push its verdict to the hub cache "
                f"(see gate-{tag}.log); this gate's own PASS/FAIL is unaffected. "
                f"Clears when {name} is removed or a later gate on this tag stores "
                f"successfully",
            )
        # Only THIS reason is owned by the sweep; anything else another
        # caller noted stays put.
        self.entries = {
            path: entry for path, entry in self.entries.items()
            if entry[0] != "verdict-store-failed"
        }
        self.entries.update(seen)
        return self.current()

    def current(self) -> list:
        return [self.entries[k] for k in sorted(self.entries)]


def _limits_ok(limits: dict, disk_gb: float, mem_gb: float) -> Optional[str]:
    min_disk = limits.get("min_free_gb")
    if min_disk is not None and disk_gb < min_disk:
        return f"low-disk {disk_gb:.0f}G < floor {min_disk}G"
    min_mem = limits.get("min_free_mem_gb")
    # mem_gb < 0 means the probe could not answer; refusing to start work
    # because a probe is unavailable would silently idle a healthy host.
    if min_mem is not None and 0 <= mem_gb < min_mem:
        return f"low-mem {mem_gb:.0f}G < floor {min_mem}G"
    return None


def write_heartbeat(hub: Hub, host: str, payload: dict) -> bool:
    """CAS write with the transient-push retry ladder. Only this host
    writes its own heartbeat ref, so a CAS conflict means our own previous
    write raced a retry of itself -- re-read and try again."""
    ref = HOSTS_PREFIX + host
    for attempt in range(PUSH_RETRIES):
        try:
            cur = hub.sha(ref)
            ok = hub.create(ref, payload) if cur is None else hub.update(ref, payload, cur)
            if ok:
                return True
        except HubError:
            pass
        time.sleep(PUSH_BACKOFF_S * (attempt + 1))
    return False


@dataclass
class AdoptionResult:
    adopted: list = field(default_factory=list)  # (kind, key, pgid)
    released: list = field(default_factory=list)  # (ref, reason)
    orphans_killed: list = field(default_factory=list)  # (pgid, outcome)
    # Marker-matched, claim-less groups that do NOT carry this daemon's
    # scope token: a fixture daemon's view of the real fleet, or a human's
    # hand-launched gate. Reported, never killed -- absence of provenance
    # reads "not ours", not "orphan". See `fleet_scope_token`.
    unscoped: list = field(default_factory=list)  # (pgid, command)
    # Marker-and-token-matched groups whose SESSION belongs to a claimed or
    # adopted worker: gate.sh's own stage subshells (set -m gives them their
    # own pgid, fork gives them the gate's argv, token included). Part of a
    # live worker, not orphans. See `group_sessions`.
    kin: list = field(default_factory=list)  # (pgid, sid)
    skipped: list = field(default_factory=list)  # (ref, reason)
    # Claims listed on the hub whose payload could not be READ this pass.
    # Non-empty means the orphan sweep's exclusion list is INCOMPLETE and
    # the sweep was therefore not run -- see `adopt_workers`. Recorded
    # separately from `skipped` (which also holds other hosts' claims and
    # refused adoptions) because this is the one entry that suppresses a
    # kill, and a caller must be able to see that it did.
    unreadable: list = field(default_factory=list)  # (ref, error)
    sweep_skipped: Optional[str] = None  # why the orphan sweep did not run

    def summary(self) -> str:
        return (
            f"adopted={[f'{k}/{key}#{p}' for k, key, p in self.adopted]} "
            f"released={[r for r, _ in self.released]} "
            f"orphans_killed={[p for p, _ in self.orphans_killed]} "
            f"skipped={len(self.skipped)}"
            + (f" unscoped={[p for p, _ in self.unscoped]}" if self.unscoped else "")
            + (f" kin={[p for p, _ in self.kin]}" if self.kin else "")
            + (f" SWEEP-SKIPPED({self.sweep_skipped})" if self.sweep_skipped else "")
        )


def adopt_workers(
    hub: Hub,
    host: str,
    workers: list,
    pgid_probe: Callable[[], set] = live_pgids,
    worker_probe: Callable[..., dict] = fleet_worker_pgids,
    killer: Callable[..., str] = kill_process_group,
    markers: Optional[Sequence[str]] = None,
    scope_token: Optional[str] = None,
    session_probe: Callable[[int], Optional[int]] = session_of,
) -> AdoptionResult:
    """Rebuild `workers` from this host's live claims + process groups
    (ARCH-FIX-SPEC.md R6). Appends adopted workers to `workers` in place.

    Call this ONCE, at daemon start, after the host singleton is held --
    holding the singleton is what makes it safe, because it guarantees no
    other fleetd on this host is adopting the same claims concurrently.

    The three dispositions, and the evidence each requires:

      ADOPT   claim.holder_host == us AND claim.pgid is in the `ps` listing.
              `Claim.adopt` continues the lease rather than re-taking it
              (see its docstring); the ref is never absent for an instant,
              so no other host ever observes this branch as free.
      RELEASE claim.holder_host == us AND the pgid is gone. The work died
              with the daemon. Releasing now returns the branch to the
              queue immediately instead of leaving it blocked for the rest
              of the lease's TTL.
      KILL    a fleet-worker process group named by NO live claim on the
              hub AND carrying this daemon's own scope token in its command
              line (`fleet_scope_token(hub.url)`, stamped at spawn by
              `start_gate`/`start_agent`). Unleased running work is the
              exact hazard leases exist to prevent -- nothing is stopping
              another host from starting the same branch beside it -- so it
              goes, by group, per M8. But a marker match alone only proves
              a process is gate-SHAPED; the token is what proves THIS
              daemon's hub spawned it. A marker-matched group without our
              token -- a human's hand-launched gate, or (the incident that
              forced this) the entire real fleet as seen by a test's
              fixture daemon on an empty fixture hub -- lands in
              `res.unscoped`, logged and left alone.

    Claims held by other hosts are counted in `skipped` and otherwise
    untouched: not adopted, not released, and (because the kill set
    excludes every claimed pgid, whoever holds the claim) their processes
    are not swept either.

    UNREADABLE IS NOT UNOWNED. The KILL disposition rests entirely on the
    claim listing being COMPLETE: a pgid is an orphan only because no
    claim named it, and "no claim named it" is a conclusion drawn from
    the payloads that were read. A claim whose payload fails to read is
    not evidence of anything -- least of all that its process is
    unleased -- and the pgid it would have contributed to the exclusion
    list cannot be recovered by any other means, because the payload is
    the only place a claim records its pgid.

    So a failed read disarms the sweep for the whole pass, rather than
    quietly shrinking the exclusion list underneath it. Compare the two
    directions of being wrong, as the KILL rule itself does:

      * Sweep anyway. A live, correctly-leased gate belonging to this
        host is SIGKILLed by group because a transient ls-remote blip
        made its claim unreadable. An hour of CPU, and the branch's
        lease is still held by a claim whose worker no longer exists.
      * Skip the sweep. Genuinely unleased work keeps running until the
        next daemon start. That is the state the host was already in one
        second earlier, and the lost-lease kill in `reconcile_once` plus
        the next start's sweep both still reach it.

    The second is recoverable and the first is not, so this fails closed.
    `res.unreadable` and `res.sweep_skipped` record that it happened, and
    it is logged at the same volume as a kill would have been -- a
    suppressed sweep must never be silent.

    (`payload is None` is a different answer and keeps its own path: that
    is `fleetlib.Hub.read` reporting the ref genuinely ABSENT -- deleted
    between the list and the read -- which is a real "no claim" and
    carries no pgid to protect. It is trustworthy only because `read`
    raises rather than returning None on transport failure; see
    `fleetlib`'s `_ABSENT_REF_HINT` and `test_fleetlib.py`'s
    `TestFetchFailureClassification`.)
    """
    res = AdoptionResult()
    live = pgid_probe()
    if scope_token is None:
        scope_token = fleet_scope_token(hub.url)
    try:
        own_pgid = os.getpgrp()
    except (AttributeError, OSError):
        own_pgid = None

    claimed_pgids: set = set()
    adopted_pgids: set = set()
    # A claim we could not READ is a claim whose pgid we cannot add to
    # `claimed_pgids` -- and the docstring's promise ("a claim we decline
    # to adopt still protects its process from being swept") cannot be kept
    # for a pgid we never learned. Rather than guess, the whole orphan
    # sweep below is skipped for this cycle whenever this happens: a live,
    # properly-claimed gate must never be killed because its claim payload
    # merely failed to read (proof: adv-review/proof_adopt_kills.py). The
    # next reconcile loop tries the read again; the sweep only needs to run
    # once conditions allow every claim to be read.

    for kind in ("gate", "agent"):
        for ref, sha in sorted(claim_mod.list_claims(hub, kind=kind).items()):
            try:
                payload = hub.read(ref)
            except HubError as e:
                # We do not know this claim's pgid and never will on this
                # pass. Record it so the sweep below disarms itself --
                # dropping through to the sweep with a short exclusion
                # list is how a live claimed gate gets killed for a
                # network blip. See the docstring.
                res.skipped.append((ref, f"unreadable: {e}"))
                res.unreadable.append((ref, str(e)))
                continue
            if payload is None:
                continue  # deleted between list and read (a real absence)

            pgid = payload.get("pgid")
            pgid = pgid if isinstance(pgid, int) else None
            if pgid is not None and pgid > 1:
                # Recorded on EVERY claim we could read, including other
                # hosts', before the holder check below -- this set is the
                # orphan sweep's exclusion list, and a claim we decline to
                # adopt still protects its process from being swept.
                claimed_pgids.add(pgid)

            if payload.get("holder_host") != host:
                res.skipped.append((ref, f"held by {payload.get('holder_host')!r}"))
                continue

            if pgid is None or pgid <= 1 or (own_pgid is not None and pgid == own_pgid):
                # No usable process group: either the claim was taken and
                # the daemon died before `start_gate` could write the real
                # pgid (so this is the dead daemon's own group), or the
                # payload is malformed. Either way there is no work to
                # adopt.
                reason = f"no adoptable process group (pgid={payload.get('pgid')!r})"
                res.released.append((ref, reason))
                _release_claim_ref(hub, ref, sha, host, reason, res)
                continue

            if pgid not in live:
                reason = f"process group {pgid} is gone"
                res.released.append((ref, reason))
                _release_claim_ref(hub, ref, sha, host, reason, res)
                continue

            # IDENTITY, not just liveness. A pgid is a name that gets
            # recycled; between this claim's write and this daemon's start
            # (a reboot, a long outage) the number can come to mean an
            # unrelated same-uid process. Adopting it would hand that
            # process to the lost-lease kill with no further checks --
            # `fleetd_marker_in_group` refuses exactly this trust for the
            # singleton's pgid, and adoption gets the same rule: some live,
            # same-uid member of the group must carry a worker marker AND
            # this daemon's scope token. Anything else is released (the
            # work goes back to the queue) and NEVER killed -- if it is a
            # recycled bystander it was never ours; if it is a pre-scope
            # worker across the upgrade boundary it finishes unsupervised,
            # which the drained-fleet deployment makes moot.
            member = _scoped_worker_in_group(pgid, markers, scope_token)
            if member is None:
                reason = (f"recorded pgid {pgid} is not a scoped fleet "
                          f"worker (recycled, or pre-scope)")
                res.released.append((ref, reason))
                _release_claim_ref(hub, ref, sha, host, reason, res)
                claimed_pgids.discard(pgid)
                continue

            c = claim_mod.Claim.adopt(hub, ref, expected_host=host)
            if c is None:
                # The lease is no longer ours (reaped and re-taken between
                # our read and our renewal). Deliberately NOT released --
                # it belongs to whoever holds it now. Its pgid is also
                # deliberately NOT in `adopted_pgids`, so the sweep below
                # kills that process: someone else may already be running
                # this work, which is precisely T1's kill-on-lost rule
                # arriving one moment earlier.
                res.skipped.append((ref, "adopt refused: lease no longer ours"))
                claimed_pgids.discard(pgid)
                continue

            workers.append(
                Worker(
                    branch=payload.get("work_key") or c.key,
                    tag=f"adopted-{kind}-{c.key}",
                    pgid=pgid,
                    claim=c,
                    popen=None,  # not our child: `alive()` falls back to pgids
                    kind=payload.get("work_kind") or kind,
                )
            )
            adopted_pgids.add(pgid)
            res.adopted.append((kind, c.key, pgid))

    if res.unreadable:
        # FAIL CLOSED. The exclusion list is knowably incomplete, so the
        # sweep has no basis for calling anything an orphan. Loudly:
        # a sweep that silently does not run looks exactly like a host
        # with no orphans on it.
        refs = ", ".join(ref for ref, _ in res.unreadable)
        res.sweep_skipped = f"{len(res.unreadable)} claim(s) unreadable: {refs}"
        print(
            f"fleetd[{host}] ORPHAN SWEEP SKIPPED -- {res.sweep_skipped}. "
            f"An unreadable claim is not an unowned one: its pgid cannot be "
            f"excluded, so nothing is killed this pass.",
            file=sys.stderr,
            flush=True,
        )
        return res

    for pgid, command in sorted(worker_probe(markers).items()):
        if pgid in adopted_pgids or pgid in claimed_pgids:
            continue
        if own_pgid is not None and pgid == own_pgid:
            continue
        # KIN: a group whose SESSION is a claimed/adopted worker's pgid is
        # part of that worker -- gate.sh's `set -m` stage subshells are
        # their own pg leaders wearing the gate's argv (marker AND token,
        # since fork keeps argv), but no claim ever names their transient
        # pgid. The main gate's pid is both the claimed pgid and, via
        # start_new_session, the session id of everything under it. Killing
        # these is how a restarted daemon would murder its own adopted
        # gate's fleet-tests stage mid-run.
        sid = session_probe(pgid)
        if sid is not None and (sid in adopted_pgids or sid in claimed_pgids):
            res.kin.append((pgid, sid))
            continue
        if scope_token not in command:
            # Gate-shaped, claim-less, but not provably OURS. Killing here
            # is how a fixture daemon murders the real fleet (or a
            # hand-launched gate). Report it -- an operator can still see
            # and judge it -- and leave it alone.
            res.unscoped.append((pgid, command[:120]))
            print(
                f"fleetd[{host}] worker-shaped group {pgid} ({command[:80]}) has no "
                f"live claim but does not carry this daemon's scope token -- "
                f"not ours to signal, left alone",
                file=sys.stderr,
                flush=True,
            )
            continue
        # Verify the kill by LISTING, not by `killpg(pgid, 0)`. An orphan's
        # leader becomes a zombie the instant it dies and its reparenting
        # has not happened yet; a signal probe reports that zombie's group
        # as alive, so the sweep would burn the whole SIGTERM grace and then
        # SIGKILL a corpse (EPERM). `live_pgids()` filters `Z` state, which
        # is exactly why `kill_process_group`'s own docstring names the ps
        # listing as the instrument for verifying a kill.
        outcome = killer(pgid, alive_probe=lambda p: p in pgid_probe())
        res.orphans_killed.append((pgid, outcome))
        print(
            f"fleetd[{host}] ORPHAN process group {pgid} ({command[:80]}) has no "
            f"live claim -- killed: {outcome}",
            file=sys.stderr,
            flush=True,
        )

    return res


def _release_claim_ref(hub: Hub, ref: str, sha: str, host: str, reason: str,
                       res: AdoptionResult) -> None:
    """CAS-delete a claim of ours whose work is gone. A failed CAS means
    somebody moved the ref under us, which makes it not ours to delete --
    downgrade the record from `released` to `skipped` rather than retry."""
    try:
        ok = hub.delete(ref, expect_sha=sha)
    except HubError as e:
        ok = False
        reason = f"{reason}; delete failed: {e}"
    if not ok:
        res.released = [entry for entry in res.released if entry[0] != ref]
        res.skipped.append((ref, f"stale on release: {reason}"))
        return
    print(f"fleetd[{host}] RELEASED orphaned claim {ref}: {reason}",
          file=sys.stderr, flush=True)


def _exiftool_cache_dir() -> Path:
    """PLAN Stage 1 task 4: EXIFTOOL_CACHE_DIR overrides the pinned-oracle
    cache directory; unset keeps today's exact default, which is spelled
    in exactly one place (`config.DEFAULT_EXIFTOOL_CACHE_DIR`, mirrored
    by `units/fleet-env.sh` for gate.sh) -- R6: this function used to
    reassemble the literal from two pieces to dodge
    tools/fleet/tests/test_no_hardcoded_hosts.py, which now forbids that
    idiom too."""
    return Path(os.environ.get("EXIFTOOL_CACHE_DIR", config.DEFAULT_EXIFTOOL_CACHE_DIR))


def _oracle_ok() -> Optional[bool]:
    """The full capability probe (-ver AND DOCX), not -ver alone -- a
    matching -ver with a Perl that lost Archive::Zip reports FileType: ZIP
    for a .docx and silently degrades every container format."""
    cache_dir = _exiftool_cache_dir()
    oracle = cache_dir / "exiftool-pinned.sh"
    docx = cache_dir / "exiftool/t/images/OOXML.docx"
    if not oracle.is_file():
        return None
    try:
        ver = subprocess.run(
            [str(oracle), "-ver"], capture_output=True, text=True, errors="replace", timeout=30
        ).stdout.strip()
        ftype = subprocess.run(
            [str(oracle), "-s3", "-FileType", str(docx)],
            capture_output=True,
            text=True,
            errors="replace",
            timeout=30,
        ).stdout.strip()
        return ver == "13.59" and ftype == "DOCX"
    except (OSError, subprocess.TimeoutExpired):
        return None


def _gate_version(repo_root: Path) -> Optional[str]:
    p = repo_root / "tools" / "fleet" / "gate_version.txt"
    try:
        return p.read_text().strip()
    except OSError:
        return None


# --------------------------------------------------------------------- #
# The reconcile step -- SHARED, not moved (see module docstring)
# --------------------------------------------------------------------- #


def reconcile_once(*args, **kwargs):
    """One reconcile step -- the runner's entry point to the step
    function that still lives in `fleetd.py` (PLAN Stage 4 moves the
    selection half into `keel/scheduler.py`; until then `fleetd.py` is
    its single home and this delegates, so there is exactly one copy).

    The ORDER contract is the runner's own invariant regardless of where
    the body lives (SPEC I5, §4.3's reason for r1): the local reap and
    the lost-lease kill -- an in-memory `lost` flag the renewer thread
    set, plus a `ps` listing -- run BEFORE any hub read, so a hub outage
    (the very condition that loses leases) can never postpone stop-work.
    `tests/test_runner_core.py::TestReconcileOrder` asserts it through
    THIS entry point, with a negative control proving the instrument
    would catch the inverted order.

    Lazy import on purpose: `fleetd.py` imports this module at its own
    import time (the moved half re-exported), so a top-level
    `import fleetd` here would be a cycle.
    """
    import fleetd  # noqa: PLC0415 -- see docstring

    return fleetd.reconcile_once(*args, **kwargs)


# --------------------------------------------------------------------- #
# Hub wiring: FallbackHub(ServerHub, Hub) when a server is configured,
# the plain state-repo Hub when not (the offline default)
# --------------------------------------------------------------------- #


def _read_server_token(path: "str | Path") -> str:
    """The bearer token for `ServerHub`, from a file. Naming a path is a
    statement that it is there (the same rule SPEC §8 states for
    `FLEET_GIT_TOKEN_FILE`), so a missing or unreadable file raises
    `OSError` for `main()` to surface loudly rather than silently
    running unauthenticated against a server that will 401 every
    write."""
    text = Path(path).expanduser().read_text(encoding="utf-8").strip()
    if not text:
        raise OSError(f"server token file {path} is empty")
    return text


def build_hub(
    hub_url: str,
    *,
    code_url: Optional[str] = None,
    server_url: Optional[str] = None,
    server_token_file: "str | Path | None" = None,
    workdir: "str | Path | None" = None,
):
    """The hub a runner reconciles against (SPEC §2 C5, §4.3).

    No `server_url` -- the offline default this stage preserves --
    returns the plain `fleetlib.Hub` on the state repo, byte-identical
    to what `fleetd.main` builds today: a runner with no server
    configured IS fleetd. With a `server_url` it returns
    `FallbackHub(ServerHub(server_url), Hub(state))`: same eight-method
    surface, server-first, direct-to-spine when the server is away, and
    `.url`/`.workdir`/`.code_url` are the GitHub half's so everything
    downstream (scope tokens, `_spawn_env`, GIT-CODE borrowers) works
    unchanged.
    """
    github = Hub(
        hub_url,
        workdir=str(workdir) if workdir is not None else str(Path.home() / ".fleetd" / "hubcache"),
        code_url=code_url,
    )
    if not server_url:
        return github
    token = _read_server_token(server_token_file) if server_token_file else None
    return FallbackHub(ServerHub(server_url, token=token), github)


# --------------------------------------------------------------------- #
# Daemon shell: singleton + adoption + the bounded-failure loop
# (verbatim from fleetd.main's body; fleetd.main now delegates here)
# --------------------------------------------------------------------- #


def run_daemon(
    hub,
    host: str,
    *,
    gate_command: list,
    log_dir: Path,
    repo_root: Path,
    interval: float = LOOP_SECONDS,
    once: bool = False,
    reconcile: Optional[Callable] = None,
    label: str = "keel-runner",
) -> int:
    """Everything `fleetd.main` did after parsing argv and building its
    `Hub`, unchanged: acquire the host singleton (reaping a provably-dead
    same-host predecessor's), rebuild `workers` by adoption, then loop
    `reconcile(hub, host, workers, gate_command, log_dir, repo_root,
    warnings=...)` at `interval`, tolerating up to
    `RECONCILE_HUB_FAILURE_LIMIT` consecutive `HubError` steps, exiting
    on SIGTERM/SIGINT/`once` (rc 0), a lost host lease (rc 4, draining
    -- never killing -- live workers), a startup adoption failure
    (rc 5), or a persistently unusable hub (rc 6).

    `reconcile` defaults to this module's `reconcile_once` (the shared
    step in fleetd.py); `fleetd.main` passes its own module global so
    `mock.patch.object(fleetd, "reconcile_once", ...)` keeps working.
    `label` only prefixes log lines -- fleetd passes "fleetd" so its
    output stays byte-identical.
    """
    if reconcile is None:
        reconcile = reconcile_once
    workers: list = []
    log_dir = Path(log_dir)
    repo_root = Path(repo_root)

    # Singleton guard: one runner per host. Held for the daemon's life;
    # expires on crash so a restarted daemon can reap it and proceed.
    #
    # `acquire_or_reap` also starts this claim's renewer (claim.py owns
    # renewal). Before that, the host singleton was written once at
    # startup and never renewed, so it expired LEASE_TTL after every
    # daemon start and a second daemon could reap it and run alongside
    # the first -- one host, two schedulers, both starting gates.
    singleton = Claim(hub, kind="host", key=host, work_kind="fleetd", work_key=host,
                      holder_host=host,  # fleet identity, not hostname -- see start_gate
                      ttl=singleton_ttl_s())  # short TTL for the scheduler lease itself
    try:
        # acquire_or_reap: a hard-killed predecessor (launchctl kickstart -k,
        # OOM, crash) never runs its graceful release, and a plain acquire
        # then locks the host out until the claim is manually reaped -- m5
        # spent 20 minutes in a KeepAlive spawn/refuse/exit loop this way.
        # A LIVE predecessor still refuses (the singleton guard stands).
        singleton.acquire_or_reap()
    except claim_mod.ClaimHeldError:
        # ARCH-FIX-SPEC.md FIX 2 (seam 4's red half): `acquire_or_reap`
        # above only reaps an EXPIRED claim, so a hard-killed predecessor
        # whose lease has not yet timed out still locks every successor
        # out for a full LEASE_TTL -- ten production minutes of a host
        # running no scheduler at all. If the claim is OURS (this host)
        # and its recorded process group is provably dead by `ps` listing
        # (see `reap_dead_same_host_singleton`), reap it regardless of
        # expiry and proceed. A live pgid or another host's claim still
        # refuses below, exactly as before this fix.
        if reap_dead_same_host_singleton(hub, host, singleton.ref, own_pid=os.getpid()):
            try:
                singleton.acquire()
            except claim_mod.ClaimHeldError:
                # Lost a race for the ref we just deleted (another reaper,
                # or the "dead" predecessor renewing after all). Refuse,
                # same as the plain case below.
                print(f"{label}: another instance holds refs/fleet/claims/host/{host}; exiting")
                return 3
        else:
            print(f"{label}: another instance holds refs/fleet/claims/host/{host}; exiting")
            return 3

    # R6: rebuild `workers` from the hub BEFORE the first reconcile. It has
    # to be after the singleton (only one daemon per host may adopt) and
    # before reconcile_once (which would otherwise see zero workers, think
    # every slot free, and start a duplicate of everything still running).
    try:
        adoption = adopt_workers(hub, host, workers)
        print(f"{label}[{host}] adoption: {adoption.summary()}", flush=True)
    except HubError as e:
        # An unreachable hub at startup is not a reason to run with an
        # empty worker list -- that is the state that starts duplicate
        # gates. Refuse to start; the supervisor will retry.
        print(f"{label}[{host}]: cannot rebuild worker state from the hub ({e}); "
              f"refusing to start rather than risk duplicate work", file=sys.stderr)
        singleton.release()
        return 5

    stop = {"flag": False}

    def _sigterm(_sig, _frm):
        stop["flag"] = True

    signal.signal(signal.SIGTERM, _sigterm)
    signal.signal(signal.SIGINT, _sigterm)

    rc = 0
    hub_failures = 0
    # T3: one store for the daemon's whole lifetime, so a warning survives
    # every reconcile until its marker file is gone. Owned here rather than
    # module-global so two daemons in one process (the test suite) cannot
    # bleed into each other.
    host_warnings = HostWarnings()
    try:
        while True:
            # A hub failure degrades THIS ITERATION, never the daemon --
            # bounded by RECONCILE_HUB_FAILURE_LIMIT, see its comment. Only
            # `HubError` is caught: a bug in this file, a KeyboardInterrupt
            # or a MemoryError must still take the process down loudly
            # rather than be retried fifteen seconds later forever.
            degraded: Optional[HubError] = None
            try:
                res = reconcile(hub, host, workers, gate_command, log_dir,
                                repo_root, warnings=host_warnings)
            except HubError as exc:
                degraded = exc
                hub_failures += 1
            else:
                hub_failures = 0
                line = (
                    f"{label}[{host}] gates={len(workers)} started={res.started} "
                    f"finished={res.finished} killed={res.killed} refused={res.refused} "
                    + (f"warnings={res.warnings} " if res.warnings else "")
                    + f"hb={res.heartbeat_written}"
                )
                print(line, flush=True)

            if degraded is not None:
                # Loud, and counted. A skipped step means: nothing started,
                # nothing reaped, no heartbeat written this cycle. Live
                # workers are untouched -- each renews its own lease from
                # its own thread, which is exactly why losing a scheduling
                # step is survivable and losing the daemon is not.
                print(
                    f"{label}[{host}] RECONCILE DEGRADED "
                    f"({hub_failures}/{RECONCILE_HUB_FAILURE_LIMIT} consecutive): "
                    f"{type(degraded).__name__}: {degraded} -- skipping this step, "
                    f"{len(workers)} worker(s) left running and still renewing",
                    file=sys.stderr,
                    flush=True,
                )

            if singleton.lost:
                # Our host lease is gone, so another daemon may already be
                # reconciling this host. Exit rather than run a second
                # scheduler against the same machine.
                #
                # Note the asymmetry with a worker's lost lease above: we
                # do NOT kill the gates on the way out. Each gate holds
                # its own, separately renewed lease; those are still valid
                # and still exclude other hosts. Killing them here would
                # destroy work that is correctly protected, to punish a
                # different ref's expiry. Drain is right for the daemon,
                # kill is right for a worker whose OWN lease lapsed.
                print(
                    f"{label}[{host}] HOST LEASE LOST {singleton.ref}: "
                    f"{singleton.lost_reason} -- another daemon may now own this "
                    f"host; exiting without killing {len(workers)} live worker(s)",
                    file=sys.stderr,
                    flush=True,
                )
                rc = 4
                break
            if degraded is not None and hub_failures >= RECONCILE_HUB_FAILURE_LIMIT:
                # Not a blip any more. Exit nonzero so the supervisor sees a
                # failure instead of a daemon that is "up" and doing
                # nothing; live workers are again left alone on the way out
                # (drain, don't kill -- their leases are separately valid).
                print(
                    f"{label}[{host}] HUB UNUSABLE: {hub_failures} consecutive reconcile "
                    f"steps failed against {hub.url} (last: {degraded}) -- exiting for the "
                    f"supervisor to restart, leaving {len(workers)} live worker(s) alone",
                    file=sys.stderr,
                    flush=True,
                )
                rc = 6
                break
            if once or stop["flag"]:
                # `--once` is a single step, so a degraded step IS a failed
                # run: report it rather than exiting 0 on a reconcile that
                # never happened.
                if degraded is not None:
                    rc = 6
                break
            time.sleep(interval)
    finally:
        # Drain, don't kill: leave live gates running; their claims expire
        # and any host's reaper collects them if they die unowned.
        # `release` also stops the singleton's renewer.
        singleton.release()
    return rc


# --------------------------------------------------------------------- #
# Entry point
# --------------------------------------------------------------------- #


def main(argv: Optional[list] = None) -> int:
    """fleetd's CLI contract plus the server knobs, resolved
    flag > env > `~/.keel/runner.toml` (`runner_toml.resolve` already
    applies env over file, so a flag default of None falls through to
    exactly that order)."""
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--hub", default=None,
                    help="state-repo git URL (FLEET_HUB_URL / [hub].url)")
    ap.add_argument("--code", default=None,
                    help="code repo git URL (FLEET_CODE_URL / [code].url; default: same as --hub)")
    ap.add_argument("--server", default=None,
                    help="keel-server base URL (KEEL_SERVER_URL / [server].url); "
                         "absent = no server, direct to the state repo (the offline default)")
    ap.add_argument("--server-token-file", default=None,
                    help="bearer token file for --server (KEEL_TOKEN_FILE / [token].server_file)")
    ap.add_argument("--repo-root", default=str(_FLEET_DIR.parents[1]))
    ap.add_argument("--log-dir", default=str(Path.home() / "gatelogs"))
    ap.add_argument("--once", action="store_true", help="single reconcile step, then exit")
    ap.add_argument("--interval", type=int, default=LOOP_SECONDS)
    ap.add_argument("--runner-toml", default=None,
                    help="config file path (default ~/.keel/runner.toml); tests point "
                         "this into a tempdir so the operator's real file cannot leak in")
    args = ap.parse_args(argv)

    try:
        cfg = runner_toml.resolve(args.runner_toml)
    except runner_toml.RunnerTomlError as e:
        print(f"keel-runner: {e}", file=sys.stderr)
        return 2

    hub_url = args.hub or cfg.hub_url
    if not hub_url:
        print("keel-runner: no hub URL (--hub, FLEET_HUB_URL, or [hub].url in runner.toml)",
              file=sys.stderr)
        return 2
    code_url = args.code or cfg.code_url or hub_url
    server_url = args.server or cfg.server_url
    token_file = args.server_token_file or cfg.server_token_file

    try:
        hub = build_hub(hub_url, code_url=code_url, server_url=server_url,
                        server_token_file=token_file)
    except OSError as e:
        # A named-but-unreadable token file is a configuration mistake,
        # not a degraded mode -- see `_read_server_token`.
        print(f"keel-runner: cannot read server token: {e}", file=sys.stderr)
        return 2

    repo_root = Path(args.repo_root)
    return run_daemon(
        hub,
        host_identity(),
        gate_command=default_gate_command(repo_root),
        log_dir=Path(args.log_dir),
        repo_root=repo_root,
        interval=args.interval,
        once=args.once,
    )


if __name__ == "__main__":
    raise SystemExit(main())
