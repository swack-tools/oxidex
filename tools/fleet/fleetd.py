"""fleetd -- the per-host desired-state reconciler (FLEET.md M2 + M8).

One instance runs per host, as the user that OWNS that host's fleet work
(on the ryzen that is `swackhamer`, not the ssh-login user -- see
docs/FLEET.md addenda "Work identity is per-host"). Every ~15s it:

  1. reads `refs/fleet/desired` and `refs/fleet/signals/tip` from the hub,
  2. counts its OWN live gates from the claims it holds, cross-checked
     against live process groups (never `pgrep -c` -- that matches the
     invoking command line and over-reported all day on 2026-08-14),
  3. starts or drains gates to converge on the desired count, subject to
     `limits` (disk/mem floors) -- draining means "start nothing new",
     never killing live work,
  4. writes its heartbeat to `refs/fleet/hosts/<host>`.

There is exactly ONE exception to "never kill live work", and it is the
reason this daemon can be trusted at all: a worker whose CLAIM IS LOST
(`Claim.lost`, i.e. the hub no longer records us as the holder) is killed
by process group immediately, because some other host may already be
running that same branch. See the long comment in `reconcile_once`.

The daemon holds NO state a restart cannot rebuild: claims, desired counts
and heartbeats all live in hub refs. That is deliberate -- two schedulers
(a cron daemon and a launchd agent) died silently on this fleet in one
day, and the orchestrating session itself crashed three times while this
file was being written. Anything only a process remembered was lost each
time; everything a ref remembered survived.

That paragraph was a promise this file did not keep until ARCH-FIX R6.
Everything needed to rebuild `workers` was indeed on the hub -- claims
carry `holder_host` and `pgid` -- and nothing read it. A restarted fleetd
started with `workers = []`, so a gate its predecessor launched became
invisible: the daemon believed it had a free slot and started a SECOND
gate on the same branch as soon as the first's claim expired, while the
first ran on to completion unsupervised, unrenewed and unkillable. The
state was rebuildable and simply never rebuilt. `adopt_workers()` is that
rebuild, and it runs before the first reconcile:

  * a claim held by THIS host whose recorded process group is still alive
    is ADOPTED -- `Claim.adopt` continues the existing lease (same
    ownership token, no delete-and-recreate) and resumes renewing it;
  * a claim held by this host whose process group is gone is RELEASED,
    freeing the branch immediately instead of after a full TTL;
  * a process group that looks like a fleet worker but is named by no
    claim at all is KILLED by group -- it is running unleased, which means
    nothing anywhere is stopping another host from doing the same work.

Claims held by OTHER hosts are read and then left entirely alone, in all
three cases.

Adoption runs AFTER the host singleton is held -- deliberately, so that
only one fleetd on a host is ever rebuilding state at once -- but the
singleton itself had the identical bug one level up until ARCH-FIX FIX 2:
`Claim.acquire_or_reap()` only reaps an EXPIRED claim, so a hard-killed
predecessor's OWN `refs/fleet/claims/host/<host>` locked every successor
out for a full LEASE_TTL regardless of whether the predecessor's process
was actually gone -- ten production minutes of a host running no
scheduler, no heartbeat, watching nothing. `main()`'s singleton block now
applies the SAME evidence `adopt_workers` uses for gate/agent claims (a
process group provably dead by `ps` listing, not by clock) to its own
claim before falling back to waiting out the TTL; see
`reap_dead_same_host_singleton` and `fleetd_marker_in_group` for the one
complication that idiom picks up at this level: fleetd shares its
supervisor's process group (R8's wrapper never gives it its own session),
so the identity check has to look for a live "fleetd.py" among the WHOLE
group, not just its leader, and has to exclude the successor's own pid --
otherwise a fresh fleetd checking its dead predecessor's pgid finds
itself and concludes, wrongly and permanently, that the predecessor is
still alive.

`reconcile_once()` is a pure step function so tests can drive it against
a fixture hub with a stub gate command; `main()` is just the loop -- plus
one guard it lacked: a `HubError` out of a reconcile step now costs that
STEP, not the daemon, up to `RECONCILE_HUB_FAILURE_LIMIT` consecutive
failures, after which the daemon exits nonzero so a genuinely unreachable
hub still reaches a human instead of hiding behind a process that is up
and doing nothing. See that constant for the argument in both directions.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Optional, Sequence

sys.path.insert(0, str(Path(__file__).resolve().parent))

import claim as claim_mod
import dispatch as dispatch_mod
import verdict
import workqueue
from claim import Claim, compute_platform_id, compute_rustc_id
from fleetlib import Hub, HubError, HubUnreachableError

# --------------------------------------------------------------------- #
# Constants (FLEET_PLAN.md "Shared contracts" is the authority)
# --------------------------------------------------------------------- #

LOOP_SECONDS = 15
HEARTBEAT_STALE = 180  # seconds; older than this renders DOWN in `fleet status`
DESIRED_REF = "refs/fleet/desired"
TIP_SIGNAL_REF = "refs/fleet/signals/tip"
HOSTS_PREFIX = "refs/fleet/hosts/"

# Transient-push retry: rapid consecutive pushes to the real hub fail and
# then succeed on a spaced retry (observed repeatedly on 2026-08-14; the
# 1Password ssh agent drops signature requests under bursts, and the hub's
# post-receive hook holds a lock per push). Single pushes almost always
# succeed, so a short backoff ladder is enough.
PUSH_RETRIES = 3
PUSH_BACKOFF_S = 4

# An agent invocation is a PAID claude/codex run, and the record of what
# has already been bought is a HUB REF, not a dict in this process
# (ARCH-FIX-SPEC.md R5). The `_agent_attempts` dict that used to live here
# was reset by every restart -- and this daemon restarted roughly hourly --
# so its 30-minute cooldown was, in practice, no cooldown at all. See
# `dispatch.py` for the ledger, the cap and the cooldown, all derived from
# `refs/fleet/attempts/<key>`.

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
    hostname is not their fleet name (the work2 pod's hostname is a
    k8s-generated `work2box-<hash>`)."""
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


def fleetd_marker_in_group(pgid: int, exclude_pid: Optional[int] = None,
                           marker: str = FLEETD_MARKER) -> Optional[str]:
    """Command line of a live, same-uid process in group `pgid` whose
    command contains `marker` -- or None if the CURRENT `ps` listing has
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
    the seam-4 restart test (and the work2 pod) actually run.

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
        if marker in command:
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
    # are different strings wherever `FLEET_HOST` is set -- which is the
    # work2 pod, whose hostname is a k8s-generated `work2box-<hash>` while
    # its fleet identity is `work2`. `start_gate` has always taken `host`
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
            [sys.executable, str(Path(__file__).resolve().parent / "agentworker.py"),
             *mode_args, "--hub", hub.url, "--host", host,
             # Inert positional (agentworker accepts and ignores it); its only
             # consumer is `ps` via the orphan sweep. See `fleet_scope_token`.
             fleet_scope_token(hub.url)],
            stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
            start_new_session=True,
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


# --------------------------------------------------------------------- #
# The reconcile step
# --------------------------------------------------------------------- #


@dataclass
class ReconcileResult:
    started: list = field(default_factory=list)
    finished: list = field(default_factory=list)
    refused: list = field(default_factory=list)  # (reason, detail)
    killed: list = field(default_factory=list)  # (tag, reason) -- lost leases
    # `_decision` records -- {"branch", "sha", "source"} -- not bare names:
    # a verdict is about the sha it was measured at, so the sha travels with
    # it (ARCH-FIX R4's shared-cache clause). `branch_names()` recovers just
    # the names, and tolerates the bare-string shape older fleetds wrote.
    awaiting_train: list = field(default_factory=list)  # PASS -- ARCH-FIX R4
    needs_author: list = field(default_factory=list)  # FAIL -- ARCH-FIX R4
    heartbeat_written: bool = False
    tip_generation: Optional[int] = None


# --------------------------------------------------------------------- #
# Verdict-aware selection (ARCH-FIX R4 point 2)
# --------------------------------------------------------------------- #
#
# Before offering a branch as gate work: a branch whose merge-onto-tip
# already gated PASS is waiting on the train, not on another gate run; one
# that already gated FAIL needs its author, not a retry loop paying the
# same gate cost for the same answer.
#
# HUB FIRST (R4's shared-cache clause). The authority is verdict.py's
# hub-backed cache, keyed on the TRUE branch-onto-tip merge tree, and this
# path asks it DIRECTLY -- it does not wait to be let in by a local recall.
# An earlier draft only reached the hub through a `~/gatelogs` hit, which
# meant a host that had never itself gated a branch had no memo entry, so
# it never consulted the hub at all and re-bought a 20-45 minute gate whose
# answer a peer had already paid for and published. The primitives to do
# better already existed and were already composed for AGENT dispatch --
# `dispatch.merge_tree_sha` + `verdict.lookup` is exactly R5's
# `dispatch.cached_pass` -- gate selection simply never called them.
#
# The key is EXACT: `(tree, gate_version, platform_id)` with THIS host's
# platform, which is bit-for-bit the key `gate.sh` itself derives after its
# merge (see gate.sh's "T1.2: verdict cache" block). So a hit here means
# the gate we were about to dispatch would exit as a pure cache hit and buy
# nothing. A verdict from a DIFFERENT platform is deliberately not honoured
# for selection, however useful it is elsewhere: it would not short-circuit
# this host's gate.sh, so gating here still buys a real, missing verdict --
# and collapsing the two identities is the exact cross-platform skew
# verdict.py's module docstring says cost a day.
#
# THE COST, BOUNDED, on both axes.
#
#   * LOCAL. `_TreeResolver` computes at most one `merge-tree` per
#     candidate per loop and memoizes by `(branch_sha, tip_sha)` for the
#     life of the process -- both inputs are content-addressed, so the
#     answer cannot go stale, and losing the memo on restart is free
#     because the hub cache, not this dict, is the durable truth. At most
#     one hub fetch per loop backs it up, and only if an object is actually
#     missing (`workqueue.Queue`'s own ancestry fetch has normally just put
#     them there).
#   * NETWORK. `Hub.read` is a FETCH -- one ssh round trip per call -- so
#     asking the cache per candidate would put a handshake on every queued
#     branch every fifteen seconds. `_VerdictIndex` spends one `ls-remote`
#     of the verdicts namespace per loop, lazily, and only reads the keys
#     that the listing says exist. A branch with nothing cached -- which is
#     exactly the branch we are about to gate -- costs no round trip at all.
#
# LOCAL RECALL, SECOND. `_scan_gatelogs_memo` rebuilds this host's own
# recent `gate.sh` runs from `~/gatelogs/gate-*.json` -- local disk, no
# network -- and answers when the hub could not. It is keyed by
# `(branch, base_tip)`, because that is all gate.sh's JSON records; the
# branch's own sha at gate time is not in the file.
#
# WHICH IS WHY A FAIL MUST PROVE ITSELF. Keying a FAIL by NAME condemns a
# branch by name, and the one action the classification is asking the
# author for -- fix it and force-push -- changes neither the name nor the
# tip. Before this fix that was a permanent lockout, not a delay: the memo
# refused to offer the branch, and a gate of the branch was the only event
# that could have replaced the memo. So a local FAIL is honoured only when
# the tree the CURRENT branch sha merges to still equals the `tree_sha` the
# failing run recorded -- the tree is the sha's fingerprint, and it is what
# gate.sh actually gated. A new sha yields a new tree, the recall stops
# applying, and the branch is fresh work again. The same test is what stops
# `_confirm_via_shared_cache` from resurrecting the old verdict for the new
# sha: that lookup is against the OLD tree, so its answer is discarded with
# the entry that named it. A PASS needs no such proof to be safe -- an
# unconfirmable PASS only parks a branch for one more loop, and the next
# hub-first lookup at the new sha decides it properly.
#
# Everything here is a pre-claim FILTER. `gate.sh`'s own cache, consulted
# against a real merge inside every run, remains the authority on what
# lands; the worst this layer can do is buy a gate it did not have to.

GATELOGS_GLOB = "gate-*.json"
AWAITING_TRAIN = "awaiting_train"
NEEDS_AUTHOR = "needs_author"

# Where a classification came from, carried into the heartbeat so an
# operator can tell a cross-host answer from this host's own recall.
SOURCE_HUB = "hub"
SOURCE_LOCAL = "local"

# (hub_url, branch_sha, tip_sha) -> merge tree sha. Process-lifetime,
# because the sha inputs are content-addressed: the same pair always merges
# to the same tree, so a cached answer cannot become wrong. Only successful
# lookups are memoized -- a None may mean "objects not fetched yet", which a
# later loop can fix, and caching that would make a transient miss
# permanent.
#
# The hub URL is in the key even though the merge result does not depend on
# it, because the OBJECT STORE does: the tree sha this yields is only
# resolvable in that hub's own cache. Two fixture hubs built from identical
# content mint identical commit shas, so without the url a tree computed
# against one would be served for the other -- a cross-fixture bleed that
# makes test outcomes depend on execution order.
_MERGE_TREE_MEMO: dict = {}
_MERGE_TREE_MEMO_MAX = 4096


class _TreeResolver:
    """The branch-onto-tip merge tree for a candidate, at most one
    `merge-tree` per (branch_sha, tip_sha) for the life of the process and
    at most one hub fetch per instance (i.e. per reconcile loop).

    Construct one per loop with the refs of every candidate; the fetch, if
    it is needed at all, brings them all in one round trip. `None` from
    `tree()` means "cannot answer" -- a conflicting merge, a missing
    object, a git that failed -- and every caller treats that as no
    opinion, which offers the branch. Refusing work because a probe could
    not answer would idle the fleet for infrastructure reasons, the same
    fail-open `dispatch.economic_refusal` argues for.
    """

    def __init__(self, hub: Hub, tip_sha: Optional[str], candidate_refs: Sequence[str] = (),
                 tip_ref: str = workqueue.TIP_REF):
        self.hub = hub
        self.tip_sha = tip_sha
        self.tip_ref = tip_ref
        self.candidate_refs = list(candidate_refs)
        self.fetched = False
        self.computed = 0

    def tree(self, branch_sha: Optional[str]) -> Optional[str]:
        if not (self.tip_sha and branch_sha):
            return None
        key = (self.hub.url, branch_sha, self.tip_sha)
        hit = _MERGE_TREE_MEMO.get(key)
        if hit is not None:
            return hit
        self.computed += 1
        tree = dispatch_mod.merge_tree_sha(self.hub, self.tip_sha, branch_sha)
        if tree is None and not self.fetched:
            # One fetch per loop, and only once something was actually
            # missing: `workqueue.Queue.compute()` has normally just pulled
            # these very objects into the same cache for its ancestry test.
            self.fetched = True
            if dispatch_mod._have_objects(self.hub, [self.tip_ref, *self.candidate_refs]):
                tree = dispatch_mod.merge_tree_sha(self.hub, self.tip_sha, branch_sha)
        if tree is not None:
            if len(_MERGE_TREE_MEMO) >= _MERGE_TREE_MEMO_MAX:
                _MERGE_TREE_MEMO.clear()
            _MERGE_TREE_MEMO[key] = tree
        return tree


def _scan_gatelogs_memo(log_dir: Path) -> dict:
    """{(branch, base_tip): {"result", "tree_sha", "gate_version",
    "platform_id"}}, most-recent-file-wins per key (by file mtime), from
    every `gate-*.json` gate.sh has written to `log_dir`. A result that
    isn't PASS or FAIL (ABORT, or a file that fails to parse) leaves no
    entry -- an aborted or unreadable run must never block a branch from
    being offered again."""
    memo: dict = {}
    try:
        paths = sorted(Path(log_dir).glob(GATELOGS_GLOB), key=lambda p: p.stat().st_mtime)
    except OSError:
        return memo
    for p in paths:
        try:
            payload = json.loads(p.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            continue
        branch = payload.get("branch")
        base_tip = payload.get("base_tip")
        result = payload.get("result")
        if not branch or not base_tip or result not in (verdict.RESULT_PASS, verdict.RESULT_FAIL):
            continue
        memo[(branch, base_tip)] = {
            "result": result,
            "tree_sha": payload.get("tree_sha"),
            "gate_version": payload.get("gate_version"),
            "platform_id": payload.get("platform_id"),
        }
    return memo


def _shared_cache_result(
    hub: Hub,
    tree_sha: Optional[str],
    gate_version: Optional[str],
    platform_id: Optional[str],
) -> Optional[str]:
    """`"PASS"`/`"FAIL"` from verdict.py's shared hub cache for exactly
    `(tree_sha, gate_version, platform_id)`, or None.

    None covers every way of not getting an answer -- an identity field we
    do not have, nothing cached at that key, an ABORT (which `lookup`
    refuses to serve as a settled answer), or a hub that could not be
    reached. All of them mean the caller falls through to whatever it knows
    locally: a hub hiccup must never block gate dispatch, only fail to
    prevent one.
    """
    if not (tree_sha and gate_version and platform_id):
        return None
    try:
        payload = verdict.lookup(hub, tree_sha, gate_version, platform_id)
    except HubError:
        return None
    if payload is None:
        return None
    return payload.get("result")


class _VerdictIndex:
    """Which verdict refs exist on the hub, enumerated ONCE per reconcile
    loop and lazily.

    `Hub.read` is a fetch -- one ssh round trip per call -- so asking the
    cache per candidate would put a handshake on every queued branch every
    fifteen seconds. A single `ls-remote` of the verdicts namespace answers
    "is there anything at this key at all" for every candidate at once, and
    only the keys that actually exist are then read for their result. The
    common case is a branch with no cached verdict -- which is exactly the
    branch we are about to gate -- and it now costs no round trip at all.

    A hub that cannot be enumerated yields an EMPTY index, not an
    exception: no opinion, offer the work. This class never raises, so it
    adds no new failure path to `reconcile_once`'s loop.
    """

    def __init__(self, hub: Hub, prefix: str = dispatch_mod.VERDICTS_PREFIX):
        self.hub = hub
        self.prefix = prefix
        self._refs: Optional[set] = None
        self.reads = 0

    def _existing(self) -> set:
        if self._refs is None:
            try:
                self._refs = set(self.hub.list(self.prefix))
            except HubError:
                self._refs = set()
        return self._refs

    def result(self, tree_sha, gate_version, platform_id) -> Optional[str]:
        """The cached PASS/FAIL at exactly this key, or None."""
        if not (tree_sha and gate_version and platform_id):
            return None
        try:
            ref = verdict.verdict_ref(tree_sha, gate_version, platform_id)
        except ValueError:
            return None  # a malformed identity is not a cache key
        if ref not in self._existing():
            return None
        self.reads += 1
        return _shared_cache_result(self.hub, tree_sha, gate_version, platform_id)


def _confirm_via_shared_cache(hub: Hub, entry: dict,
                              index: Optional[_VerdictIndex] = None) -> Optional[str]:
    """Best-effort confirmation of a locally-recalled verdict against the
    shared hub cache, reusing the TREE_SHA the local JSON already recorded
    rather than recomputing a merge.

    NOTE the tree here is the one the RECALLED run gated, which is not
    necessarily the tree the branch's current sha would produce -- see
    `classify_branch`, which discards a FAIL whose recalled tree no longer
    matches, precisely so this confirmation cannot re-condemn a
    force-pushed branch on the strength of its predecessor's verdict."""
    tree_sha, gv, plat = (
        entry.get("tree_sha"), entry.get("gate_version"), entry.get("platform_id")
    )
    if index is not None:
        return index.result(tree_sha, gv, plat)
    return _shared_cache_result(hub, tree_sha, gv, plat)


def _decision(branch: str, branch_sha: Optional[str], source: str) -> dict:
    """The heartbeat record for one classified branch. `sha` is the branch
    sha the verdict was DECIDED AT (None only when the caller could not
    resolve one), which is what makes staleness visible in `fleet status`
    instead of a bare name with no age; `source` distinguishes a cross-host
    answer from this host's own recall."""
    return {"branch": branch, "sha": branch_sha, "source": source}


def branch_names(entries: Sequence) -> list:
    """Branch names out of a heartbeat's `awaiting_train`/`needs_author`
    list, tolerating both shapes: the `_decision` dicts written since R4's
    shared-cache clause, and the bare strings older fleetd versions wrote
    (a mixed-version fleet has both sitting on the hub at once)."""
    out = []
    for e in entries or ():
        if isinstance(e, dict):
            name = e.get("branch")
            if name:
                out.append(name)
        elif e:
            out.append(str(e))
    return out


def classify_branch(
    memo: dict,
    hub: Hub,
    branch: str,
    tip_sha: str,
    gate_version: Optional[str],
    confirm_with_hub: bool = True,
    branch_sha: Optional[str] = None,
    platform_id: Optional[str] = None,
    trees: Optional[_TreeResolver] = None,
    index: Optional[_VerdictIndex] = None,
) -> Optional[tuple]:
    """`(status, entry)` -- status being AWAITING_TRAIN or NEEDS_AUTHOR --
    for `branch` gated onto `tip_sha`, or None for "no opinion, offer it as
    ordinary gate work". `entry` is `_decision`'s record, carrying the
    branch sha the classification was decided at.

    Two sources, in this order:

      1. the shared verdict cache, keyed on the true `branch_sha`-onto-
         `tip_sha` merge tree at this host's own `platform_id` -- the same
         key `gate.sh` derives after its own merge, so a hit here proves
         the gate we were about to dispatch would exit as a cache hit and
         buy nothing;
      2. this host's `~/gatelogs` recall, for when the hub had nothing to
         say (or could not be reached at all).

    A memo entry recorded under a different `gate_version` than the one
    this host currently runs is ignored, not honoured: GATE_VERSION only
    bumps when gate BEHAVIOUR changes, so an old-version verdict is not
    evidence about what the current gate would say.
    """
    tree = trees.tree(branch_sha) if (trees is not None and branch_sha) else None

    # ---- 1. hub first ------------------------------------------------ #
    if confirm_with_hub:
        shared = (index.result(tree, gate_version, platform_id) if index is not None
                  else _shared_cache_result(hub, tree, gate_version, platform_id))
        if shared == verdict.RESULT_PASS:
            return (AWAITING_TRAIN, _decision(branch, branch_sha, SOURCE_HUB))
        if shared == verdict.RESULT_FAIL:
            return (NEEDS_AUTHOR, _decision(branch, branch_sha, SOURCE_HUB))

    # ---- 2. this host's own recall ------------------------------------ #
    entry = memo.get((branch, tip_sha))
    if entry is None:
        return None
    if gate_version is not None and entry.get("gate_version") != gate_version:
        return None
    result = entry.get("result")
    if confirm_with_hub:
        confirmed = _confirm_via_shared_cache(hub, entry, index)
        if confirmed is not None:
            result = confirmed
    if result == verdict.RESULT_PASS:
        return (AWAITING_TRAIN, _decision(branch, branch_sha, SOURCE_LOCAL))
    if result == verdict.RESULT_FAIL:
        # A FAIL condemns a branch until its author acts, so it is honoured
        # only where it can be shown to be about the branch AS IT IS NOW:
        # the tree the current sha merges to must still be the tree the
        # failing run gated. Anything else -- a force-push, or simply not
        # being able to compute the tree -- is not evidence about this sha,
        # and the fail-open direction costs one gate rather than locking a
        # fixed branch out of the fleet forever.
        if tree is None or entry.get("tree_sha") != tree:
            return None
        return (NEEDS_AUTHOR, _decision(branch, branch_sha, SOURCE_LOCAL))
    return None


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


def dispatch_agents(
    hub: Hub,
    host: str,
    workers: list,
    slots: int,
    log_dir: Path,
    repo_root: Path,
    res: "ReconcileResult",
) -> None:
    """Fill up to `slots` agent slots, buying nothing that cannot pay off
    (ARCH-FIX-SPEC.md R5). Appends started workers to `workers` in place.

    Three gates stand between a candidate and a spawn, in increasing order
    of cost to evaluate:

      1. BUSY -- somebody (here or elsewhere) is already on this key.
      2. BUDGET -- `dispatch.budget_refusal` over the DURABLE attempt
         record: the hard cap on consecutive failures, and the cooldown
         derived from `last_at`. Both survive a restart, which the dict
         this replaced did not.
      3. ECONOMICS -- `dispatch.economic_refusal`: is there drift to
         converge, and has this (branch, tip) pair already PASSed? These
         cost git fetches, so they run last and only on keys that got
         through 1 and 2.

    Then `dispatch.order_candidates` decides who actually gets the slots,
    which is where the reserved authoring slot lives.

    This is the layer where a refusal is CHEAPEST: nothing has been forked,
    no repository has been cloned, no CLI token has been spent, and the
    claim that a spawn would have taken is never taken -- so the branch
    stays visible to the rest of the fleet instead of looking busy for the
    lifetime of a doomed worker. `agentworker` re-checks the economics for
    itself (see its module docstring); that second check exists to catch
    the tip moving between here and there, not to make this one optional.
    """
    records = dispatch_mod.load_all(hub)
    busy = {w.branch for w in workers}
    tip_sha = hub.sha(workqueue.TIP_REF)
    cooled: list = []

    def _budget_ok(key: str) -> bool:
        refusal = dispatch_mod.budget_refusal(dispatch_mod.get(records, key))
        if refusal is None:
            return True
        code, detail = refusal
        if code == "cooldown":
            # Aggregated below: the common, boring, working-as-intended
            # case, and one line per backlog branch every 15s drowns the
            # log lines that matter.
            cooled.append(key)
        else:
            res.refused.append((f"agent-{code}", f"{key}: {detail}"))
        return False

    # Gates 1 and 2 only -- both are pure over data already in hand, so the
    # whole queue can be filtered for free. Gate 3 (economics) costs a git
    # fetch per branch and is deferred to the dispatch loop below, which
    # evaluates it lazily and stops as soon as the slots are full: on a
    # 30-branch backlog with one free slot that is one fetch per reconcile
    # instead of thirty, every fifteen seconds, against the real hub.
    convergence: list = []
    branch_shas: dict = {}
    for slug, entry in workqueue.Queue(hub).compute().items():
        branch = entry.ref.removeprefix("refs/heads/")
        if branch in busy or not _budget_ok(branch):
            continue
        branch_shas[branch] = entry.sha
        convergence.append(branch)

    # AUTHORING candidates: open intents with no staging branch yet. Note
    # this scan is no longer guarded by "only if the convergence queue is
    # empty" -- that guard is exactly what starved the intent backlog, and
    # `order_candidates` replaces it with an alternation that cannot.
    authoring: list = []
    for iref in hub.list("refs/fleet/intents/"):
        slug = iref.rsplit("/", 1)[-1]
        doc = hub.read(iref) or {}
        if doc.get("status") != "open":
            continue
        if hub.sha(f"refs/heads/staging/{slug}") is not None:
            continue  # branch exists; the convergence path owns it now
        key = f"{dispatch_mod.INTENT_PREFIX}{slug}"
        if key in busy or not _budget_ok(key):
            continue
        authoring.append(key)

    if cooled:
        res.refused.append(("agent-cooldown", f"{len(cooled)} key(s): {', '.join(sorted(cooled)[:5])}"))

    # A local counter, NOT `len(res.started)`: `res.started` already holds
    # this step's GATE tags from the block above, so counting it here would
    # let one started gate silently consume every agent slot.
    filled = 0
    for branch in dispatch_mod.order_candidates(convergence, authoring, records):
        if filled >= slots:
            break
        refusal = dispatch_mod.economic_refusal(
            hub, branch, tip_sha, branch_sha=branch_shas.get(branch))
        if refusal is not None:
            code, detail = refusal
            res.refused.append((f"agent-{code}", f"{branch}: {detail}"))
            continue

        tag = f"{host}-a-{branch.split('/')[-1].removeprefix(dispatch_mod.INTENT_PREFIX)}-{int(time.time()) % 100000}"
        # Count the purchase BEFORE making it. A crash between here and the
        # spawn costs this key one retry; the other order costs unbounded
        # money (see dispatch.py's "counting direction").
        try:
            dispatch_mod.record_dispatch(hub, branch, host)
        except HubError as e:
            # An unwritable ledger means the NEXT loop cannot know this run
            # happened. Refuse rather than spend unaccounted money.
            res.refused.append(("agent-ledger-unwritable", f"{branch}: {e}"))
            continue
        w = None
        spawn_failed = False
        try:
            w = start_agent(hub, branch, tag, host, log_dir, repo_root)
        except OSError as e:
            spawn_failed = True
            res.refused.append(("agent-spawn-failed", f"{branch}: {e}"))
        if w is None:
            if not spawn_failed:
                res.refused.append(("agent-claimed-elsewhere", branch))
            # Nothing was bought: hand the counted attempt back.
            _record_outcome(hub, branch, host, dispatch_mod.NOT_PAID)
            continue
        workers.append(w)
        res.started.append(tag)
        filled += 1


def _record_outcome(hub: Hub, branch: str, host: str, outcome: str) -> None:
    """Best-effort ledger update. A failure to record an OUTCOME is not
    worth taking the daemon down or aborting a reconcile: the dispatch
    itself is already counted, so the worst case is that a key looks more
    expensive than it was -- the conservative direction."""
    try:
        dispatch_mod.record_outcome(hub, branch, host, outcome)
    except HubError as e:
        print(f"fleetd[{host}] attempt-ledger write failed for {branch} "
              f"({outcome}): {e}", file=sys.stderr, flush=True)


# Worker exit codes -> attempt-ledger outcomes. Only `0` resets the
# consecutive-failure count; `agentworker.RC_PREFLIGHT_REFUSED` is the one
# code that hands the attempt back, because it proves no CLI was invoked.
_AGENT_RC_OUTCOMES = {
    0: "converged",
    4: "no-agent-cli",
    5: "missing-refs",
    6: "timeout",
    7: "no-progress",
    8: dispatch_mod.NOT_PAID,
    9: "blocked",
}


def reconcile_once(
    hub: Hub,
    host: str,
    workers: list,
    gate_command: list,
    log_dir: Path,
    repo_root: Path,
    disk_probe: Callable[[], float] = free_disk_gb,
    mem_probe: Callable[[], float] = free_mem_gb,
    pgid_probe: Callable[[], set] = live_pgids,
) -> ReconcileResult:
    """One reconcile step. Mutates `workers` in place (removing finished
    and killed ones) and returns what changed. Over-target and disabled
    both DRAIN (stop starting) and never kill, per FLEET.md M2 -- the one
    worker this step does kill is one whose lease was lost, for the
    reasons argued inline below.

    ORDER OF WORK, and why it is this order:

      1. REAP + LOST-LEASE KILL. Purely local -- an in-memory `lost` flag
         set by the renewer thread, and a `ps` listing. No hub read stands
         in front of it.
      2. HUB READS, each guarded on its own.
      3. STARTS, which need (2) to have succeeded to mean anything.

    Steps 1 and 2 used to be the other way round, and the inversion was
    the bug. `hub.read(DESIRED_REF)` and `hub.read(TIP_SIGNAL_REF)` sat
    unguarded at the top of this function, so a hub that could not be read
    raised out of the step before the kill loop was ever reached. That is
    not an unlucky ordering, it is exactly backwards: a lease goes LOST
    because its renewal push failed, and renewals fail for the same reason
    the reads do. The one condition under which stop-work matters most was
    the one condition under which stop-work did not run.

    The cost was bounded but real. `main` tolerates
    `RECONCILE_HUB_FAILURE_LIMIT` consecutive failed steps before exiting
    nonzero, so an unleased gate kept running for ~5 loops -- over a
    minute at the 15s interval -- while another host, seeing an expired
    claim, was free to reap it and start the same branch. Two gates on one
    branch is the duplicate-merge hazard argued at the KILL comment below,
    and it is not retryable after the fact.

    Nothing here swallows a hub failure. Each read degrades ITS OWN
    concern (an unreadable `desired` means "start nothing", not "the host
    is disabled"), the step still RAISES at the end, and `main`'s bounded
    counter still trips -- a daemon that cannot reach its hub must
    surface. What changed is only that the local safety work is complete
    before that happens, and `workers` is mutated in place, so the kill
    survives the raise.
    """
    res = ReconcileResult()

    # ---- (1) LOCAL FIRST. No hub call precedes this loop. ------------ #
    # Reap finished/dead workers, and kill any worker whose lease is lost.
    # A worker whose process group is gone releases its claim here; a
    # crashed fleetd's claims expire on their own (LEASE_TTL) and are
    # reaped by any host via claim.reap_expired.
    pgids = pgid_probe()
    for w in list(workers):
        if not w.alive(pgids):
            # Best-effort: an undeleted claim expires on its TTL, but a
            # worker left in `workers` because the release raised would
            # hold a slot until restart -- and would take the rest of this
            # loop, including other workers' lost-lease kills, with it.
            try:
                w.claim.release()
            except HubError as e:
                print(f"fleetd[{host}] claim release failed for finished worker "
                      f"{w.tag} (expires on TTL): {e}", file=sys.stderr, flush=True)
            workers.remove(w)
            res.finished.append(w.tag)
            if w.kind == "agent":
                # Close the ledger entry this run opened. An ADOPTED worker
                # has no Popen and therefore no exit status -- its outcome
                # is honestly recorded as unknown rather than guessed at,
                # which leaves the count where the dispatch put it (the
                # conservative direction: an unknown run is not evidence of
                # progress).
                rc = w.popen.returncode if w.popen is not None else None
                if rc is None:
                    outcome = "unknown-adopted"
                elif rc == 0:
                    outcome = ("authored" if dispatch_mod.is_intent_key(w.branch)
                               else "converged")
                else:
                    outcome = _AGENT_RC_OUTCOMES.get(rc, f"exit-{rc}")
                _record_outcome(hub, w.branch, host, outcome)
            continue

        if w.claim.lost:
            # ---- KILL. Do not drain. ------------------------------- #
            # Everywhere else in this daemon the rule is drain, never
            # kill: over-target drains, disabled drains, shutdown drains,
            # because a half-finished gate wastes an hour of CPU and a
            # running gate hurts nobody. A LOST LEASE inverts that rule,
            # and the inversion is deliberate.
            #
            # `lost` means the hub no longer records this host as the
            # holder of this work. Some other host may already have
            # reaped the claim and started the same branch -- that is
            # precisely the event leases exist to prevent. Two gates on
            # one branch corrupt the shared verdict cache (two verdicts
            # for one (tree, gate_version, platform) pair), race for the
            # same target directory, and can drive two merges of the same
            # tree onto the tip.
            #
            # So compare the two directions of being wrong. Killing a
            # worker that still legitimately held its lease costs one
            # retryable gate run. Letting an unleased worker run to
            # completion risks a duplicate merge race, which is not
            # retryable and not detectable after the fact. The safe
            # direction is the kill, and it is safe only because it is
            # the group (M8): cargo and rustc children go with it.
            #
            # Every input to this decision is LOCAL: `w.claim.lost` is an
            # in-memory flag the renewer thread set, and `pgids` came from
            # `ps`. Nothing here needs the hub, which is the whole reason
            # this loop now runs before the reads -- see the docstring.
            reason = w.claim.lost_reason or "renewal failed (no reason recorded)"
            outcome = kill_worker(w)
            workers.remove(w)
            res.killed.append((w.tag, reason))
            print(
                f"fleetd[{host}] LOST LEASE {w.claim.ref} kind={w.kind} "
                f"branch={w.branch} tag={w.tag} pgid={w.pgid}: {reason} "
                f"-- killed process group: {outcome}",
                file=sys.stderr,
                flush=True,
            )

    # ---- (2) HUB READS, each guarded on its own. --------------------- #
    # Independently, so that one unreadable ref degrades one concern. The
    # failures are collected and re-raised at the end of the step rather
    # than swallowed: `main` counts consecutive `HubError`s and exits
    # nonzero at RECONCILE_HUB_FAILURE_LIMIT, and a daemon that quietly
    # reported success while reaching nothing would be the worse bug (see
    # `test_fleetd.py`'s TestMainLoopSurvivesHubErrors, which argues both
    # directions). `workers` is mutated in place, so everything step (1)
    # decided survives that raise.
    hub_failures: list = []
    desired_readable = True

    try:
        desired_doc = hub.read(DESIRED_REF) or {}
    except HubError as e:
        # NOT the same as `enabled: false`. "The operator turned this host
        # off" and "we could not ask" are different facts, and recording
        # the second as the first would have `fleet status` report a
        # deliberate stand-down during a network outage. Either way
        # nothing starts -- an unread target is never a licence to spawn.
        hub_failures.append((DESIRED_REF, e))
        desired_readable = False
        desired_doc = {}
        res.refused.append(("hub-unreadable", f"{DESIRED_REF}: {e}"))
    my_desired = (desired_doc.get("hosts") or {}).get(host) or {}
    limits = desired_doc.get("limits") or {}
    want_gates = int(my_desired.get("gates") or 0)
    enabled = bool(my_desired.get("enabled", False))

    try:
        tip_sig = hub.read(TIP_SIGNAL_REF) or {}
    except HubError as e:
        # Degrades to "generation unknown" and nothing else. This ref
        # feeds a heartbeat field, so its failure must not cost the
        # starts below -- that is what "each read degrades its own
        # concern" buys.
        hub_failures.append((TIP_SIGNAL_REF, e))
        tip_sig = {}
        res.refused.append(("hub-unreadable", f"{TIP_SIGNAL_REF}: {e}"))
    res.tip_generation = tip_sig.get("generation")

    running = len([w for w in workers if w.kind == "gate"])

    if not desired_readable:
        pass  # already recorded as hub-unreadable; nothing to start
    elif not enabled:
        if running == 0:
            pass  # fully drained
        res.refused.append(("disabled", my_desired.get("reason") or ""))
    else:
        deficit = want_gates - running
        if deficit > 0:
            reason = _limits_ok(limits, disk_probe(), mem_probe())
            if reason is not None:
                res.refused.append(("limits", reason))
            else:
                q = workqueue.Queue(hub).compute()

                def _branch(entry):
                    return entry.ref.removeprefix("refs/heads/")

                # Verdict-aware selection (ARCH-FIX R4): don't offer a
                # branch the SHARED verdict cache -- or, failing that, this
                # host's own gatelogs recall -- already knows the answer
                # for. PASS is waiting on the train, FAIL needs its author.
                # `tip_sha` gates the lookup on `hub.sha` rather than
                # `res.tip_generation` because both keys are the tip's SHA:
                # the merge tree the hub cache is keyed on, and the
                # `base_tip` gate.sh actually recorded in its JSON.
                # `_TreeResolver` and `platform_id` are built ONCE for the
                # whole loop -- see the module's "THE COST, BOUNDED" note.
                tip_sha = hub.sha(workqueue.TIP_REF)
                gate_version = _gate_version(repo_root)
                memo = _scan_gatelogs_memo(log_dir) if tip_sha else {}
                trees = _TreeResolver(hub, tip_sha, [q[s].ref for s in q])
                index = _VerdictIndex(hub)
                platform_id = compute_platform_id() if tip_sha else None

                candidates = []
                for s in q:
                    b = _branch(q[s])
                    if any(w.branch == b for w in workers):
                        continue
                    decided = classify_branch(
                        memo, hub, b, tip_sha, gate_version,
                        branch_sha=q[s].sha, platform_id=platform_id, trees=trees,
                        index=index,
                    ) if tip_sha else None
                    if decided is not None:
                        status, entry = decided
                        if status == AWAITING_TRAIN:
                            res.awaiting_train.append(entry)
                            continue
                        if status == NEEDS_AUTHOR:
                            res.needs_author.append(entry)
                            continue
                    candidates.append(s)
                for slug in candidates[:deficit]:
                    branch = _branch(q[slug])
                    tag = f"{host}-{slug}-{int(time.time()) % 100000}"
                    try:
                        w = start_gate(hub, branch, tag, gate_command, host, log_dir)
                    except OSError as e:
                        res.refused.append(("spawn-failed", f"{branch}: {e}"))
                        continue
                    if w is None:
                        res.refused.append(("claimed-elsewhere", branch))
                        continue
                    workers.append(w)
                    res.started.append(tag)

    want_agents = int(my_desired.get("agents") or 0)
    agent_workers = [w for w in workers if w.kind == "agent"]
    slots = want_agents - len(agent_workers)
    if enabled and slots > 0:
        try:
            import agentworker as _aw
            has_cli = bool(_aw.available_clis())
        except Exception:
            has_cli = False
        if not has_cli:
            res.refused.append(("no-agent-cli", "neither claude nor codex on this host"))
        else:
            dispatch_agents(hub, host, workers, slots, log_dir, repo_root, res)

    hb = {
        "gates_running": len([w for w in workers if w.kind == "gate"]),
        "agents_running": len([w for w in workers if w.kind == "agent"]),
        "free_gb": round(disk_probe(), 1),
        "free_mem_gb": round(mem_probe(), 1),
        "rustc_id": compute_rustc_id(),
        "platform_id": compute_platform_id(),
        "owning_user": owning_user(),
        "oracle_ok": _oracle_ok(),
        "gate_version": _gate_version(repo_root),
        "tip_generation_seen": res.tip_generation,
        # ARCH-FIX R4: branch states this loop's selection surfaced, so
        # `fleet status` can render them without re-deriving anything.
        # Each entry carries the branch sha its verdict was decided at and
        # where that verdict came from ("hub" or "local"), so an operator
        # can see a parked branch that has since been force-pushed rather
        # than a bare name with no age on it.
        "awaiting_train": res.awaiting_train,
        "needs_author": res.needs_author,
        # ARCH-FIX R4 point 3 / R2: this loop's lost-lease kills (T1's
        # `res.killed`, module docstring). Per-loop, not a lifetime total --
        # `reconcile_once` keeps no state across calls beyond `workers`, and
        # a cumulative counter would need to survive a restart to mean
        # anything, which nothing here currently persists.
        "killed_this_loop": len(res.killed),
        "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }
    if any(isinstance(err, HubUnreachableError) for _, err in hub_failures):
        # The transport is already known to be down. `write_heartbeat`'s
        # ladder would spend PUSH_RETRIES * PUSH_BACKOFF_S (~24s) finding
        # that out again -- on a loop whose most urgent job right now is
        # to come back in 15s and kill any further lost leases. Before the
        # reorder above this was moot: the step raised at the first read
        # and never got here. Skipping keeps that latency, and
        # `heartbeat_written` stays False, which is the truth.
        res.heartbeat_written = False
    else:
        res.heartbeat_written = write_heartbeat(hub, host, hb)

    if hub_failures:
        # The step did every local thing it could -- reaped, killed lost
        # leases, wrote what heartbeat it could -- and now says so. The
        # raise is the point: `main` counts these, and five consecutive
        # ones exit the daemon nonzero for a supervisor to notice. What
        # the reorder changed is that stop-work no longer waits on it.
        detail = "; ".join(f"{ref}: {err}" for ref, err in hub_failures)
        # Keep the narrower type when every failure was one: `main` only
        # needs `HubError`, but a caller (or a log reader) that can tell
        # "the hub is unreachable" from "a payload is malformed" should
        # not lose that because the two reads were bundled.
        cls = (HubUnreachableError
               if all(isinstance(err, HubUnreachableError) for _, err in hub_failures)
               else HubError)
        raise cls(f"reconcile step could not read {len(hub_failures)} hub ref(s): {detail}")

    return res


def _oracle_ok() -> Optional[bool]:
    """The full capability probe (-ver AND DOCX), not -ver alone -- a
    matching -ver with a Perl that lost Archive::Zip reports FileType: ZIP
    for a .docx and silently degrades every container format."""
    oracle = Path("/tmp/oxidex-exiftool-cache/exiftool-pinned.sh")
    docx = Path("/tmp/oxidex-exiftool-cache/exiftool/t/images/OOXML.docx")
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
# Daemon loop
# --------------------------------------------------------------------- #


def main(argv: Optional[list] = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--hub", default=os.environ.get("FLEET_HUB_URL"), help="hub git URL")
    ap.add_argument("--repo-root", default=str(Path(__file__).resolve().parents[2]))
    ap.add_argument("--log-dir", default=str(Path.home() / "gatelogs"))
    ap.add_argument("--once", action="store_true", help="single reconcile step, then exit")
    ap.add_argument("--interval", type=int, default=LOOP_SECONDS)
    args = ap.parse_args(argv)
    if not args.hub:
        print("fleetd: no hub URL (--hub or FLEET_HUB_URL)", file=sys.stderr)
        return 2

    host = host_identity()
    repo_root = Path(args.repo_root)
    hub = Hub(args.hub, workdir=Path.home() / ".fleetd" / "hubcache")
    workers: list = []
    gate_command = default_gate_command(repo_root)

    # Singleton guard: one fleetd per host. Held for the daemon's life;
    # expires on crash so a restarted daemon can reap it and proceed.
    #
    # `acquire_or_reap` also starts this claim's renewer (claim.py owns
    # renewal). Before that, the host singleton was written once at
    # startup and never renewed, so it expired LEASE_TTL after every
    # daemon start and a second fleetd could reap it and run alongside
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
                print(f"fleetd: another instance holds refs/fleet/claims/host/{host}; exiting")
                return 3
        else:
            print(f"fleetd: another instance holds refs/fleet/claims/host/{host}; exiting")
            return 3

    # R6: rebuild `workers` from the hub BEFORE the first reconcile. It has
    # to be after the singleton (only one fleetd per host may adopt) and
    # before reconcile_once (which would otherwise see zero workers, think
    # every slot free, and start a duplicate of everything still running).
    try:
        adoption = adopt_workers(hub, host, workers)
        print(f"fleetd[{host}] adoption: {adoption.summary()}", flush=True)
    except HubError as e:
        # An unreachable hub at startup is not a reason to run with an
        # empty worker list -- that is the state that starts duplicate
        # gates. Refuse to start; the supervisor will retry.
        print(f"fleetd[{host}]: cannot rebuild worker state from the hub ({e}); "
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
    try:
        while True:
            # A hub failure degrades THIS ITERATION, never the daemon --
            # bounded by RECONCILE_HUB_FAILURE_LIMIT, see its comment. Only
            # `HubError` is caught: a bug in this file, a KeyboardInterrupt
            # or a MemoryError must still take the process down loudly
            # rather than be retried fifteen seconds later forever.
            degraded: Optional[HubError] = None
            try:
                res = reconcile_once(hub, host, workers, gate_command, Path(args.log_dir), repo_root)
            except HubError as exc:
                degraded = exc
                hub_failures += 1
            else:
                hub_failures = 0
                line = (
                    f"fleetd[{host}] gates={len(workers)} started={res.started} "
                    f"finished={res.finished} killed={res.killed} refused={res.refused} "
                    f"hb={res.heartbeat_written}"
                )
                print(line, flush=True)

            if degraded is not None:
                # Loud, and counted. A skipped step means: nothing started,
                # nothing reaped, no heartbeat written this cycle. Live
                # workers are untouched -- each renews its own lease from
                # its own thread, which is exactly why losing a scheduling
                # step is survivable and losing the daemon is not.
                print(
                    f"fleetd[{host}] RECONCILE DEGRADED "
                    f"({hub_failures}/{RECONCILE_HUB_FAILURE_LIMIT} consecutive): "
                    f"{type(degraded).__name__}: {degraded} -- skipping this step, "
                    f"{len(workers)} worker(s) left running and still renewing",
                    file=sys.stderr,
                    flush=True,
                )

            if singleton.lost:
                # Our host lease is gone, so another fleetd may already be
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
                    f"fleetd[{host}] HOST LEASE LOST {singleton.ref}: "
                    f"{singleton.lost_reason} -- another fleetd may now own this "
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
                    f"fleetd[{host}] HUB UNUSABLE: {hub_failures} consecutive reconcile "
                    f"steps failed against {hub.url} (last: {degraded}) -- exiting for the "
                    f"supervisor to restart, leaving {len(workers)} live worker(s) alone",
                    file=sys.stderr,
                    flush=True,
                )
                rc = 6
                break
            if args.once or stop["flag"]:
                # `--once` is a single step, so a degraded step IS a failed
                # run: report it rather than exiting 0 on a reconcile that
                # never happened.
                if degraded is not None:
                    rc = 6
                break
            time.sleep(args.interval)
    finally:
        # Drain, don't kill: leave live gates running; their claims expire
        # and any host's reaper collects them if they die unowned.
        # `release` also stops the singleton's renewer.
        singleton.release()
    return rc


if __name__ == "__main__":
    raise SystemExit(main())
