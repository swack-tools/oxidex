#!/usr/bin/env python3
"""keel-runner -- the per-host runner: fleetd's LOCAL half, re-homed
(SPEC §2 C7, §9 "fleetd.py"; PLAN Stage 3 task 1).

This is the process the `units/*` supervisors launch (they were
re-pointed at this file by Stage 3 task 7). It keeps fleetd's CLI, env
and exit-code contract byte-for-byte -- rc 0 deliberate stop, rc 3
singleton refused, rc 4 host lease lost, rc 5 cannot rebuild worker
state, rc 6 hub unusable -- and its behaviour with NO server configured
is exactly fleetd's today: singleton, adoption, reconcile, drain-never-
kill, lost-lease kill by process group.

ONE code is ADDED to that contract rather than changed: rc 7, the L1
toolchain check below, when this runner's `platform_id` differs from
the one its own gate command computes. It is deliberately not 6 --
`TOOLCHAIN_MISMATCH_RC` carries the argument -- because a supervisor
should retry a hub outage and must not hot-loop a mis-installed
toolchain, and rc 6 already meant the first. That offline default is the
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
import shlex
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
import toolchain  # noqa: E402  -- L1: the ONE rustc resolver + id formula
from claim import Claim  # noqa: E402
from fleetlib import Hub, HubError, HubUnreachableError  # noqa: E402

# Qualified `keel.<name>` imports, not bare ones -- see serverhub.py's
# module docstring: a bare `import fallbackhub` here would risk a second,
# distinct module object (and a second, distinct exception class)
# alongside what every other file imports qualified.
from keel.fallbackhub import FallbackHub  # noqa: E402
from keel.serverhub import ServerHub  # noqa: E402
# `keel.journal` is safe to import at THIS module's top level and `fleetd`
# is not: `fleetd` imports `keel.runner` at its own import time, so a
# top-level `import fleetd` here is a cycle (see `reconcile_once`).
# `journal` imports only `claim` and `fleetlib` at module level and defers
# its own `fleetd` import into function bodies for exactly that reason.
from keel import journal as journal_mod  # noqa: E402
from keel import runner_toml  # noqa: E402
# The server lease ref, imported rather than re-spelled. `keel.election`
# owns `refs/fleet/claims/server/singleton`; a second spelling of it here
# is the "named in more than one place" failure `config.py`'s docstring
# exists to prevent, and this one would be silent -- a runner watching the
# wrong ref sees a lease that is absent forever and goes autonomous on a
# perfectly healthy fleet. `keel.election` imports only `claim` and
# `fleetlib` at module level, so this costs nothing and cannot cycle.
from keel.election import SERVER_LEASE_REF  # noqa: E402

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


class ProcessListingUnavailable(RuntimeError):
    """`ps` could not be made to answer, so this host's process listing is
    UNKNOWN this pass.

    Emphatically not the same fact as "no process groups are alive", and
    the difference is the whole reason this exception exists. `live_pgids`
    used to answer both with `set()`:

        except (OSError, subprocess.TimeoutExpired):
            return set()

    Every consumer of that listing reads absence as death. One failed
    `ps` therefore made `reconcile_once`'s reap declare EVERY worker
    finished, release each one's CAS'd lease, and -- with the branches now
    unclaimed -- start a second gate on a branch whose first gate was
    still running. Measured at 2c7716a7 by injecting a single empty
    listing into `reconcile_once`: the adopted worker for `staging/one`
    was reaped while its process group was demonstrably alive, and the
    same step started `journalhost-one-...` beside it. That is the
    duplicate-merge hazard argued at `reconcile_once`'s KILL comment,
    reached without a single lost lease -- two gates on one branch, two
    verdicts for one (tree, gate_version, platform) pair, and not
    detectable after the fact.

    The failure is silent by construction: `ps` failing is a fork away
    from an ordinary run (EAGAIN under process pressure, EMFILE, a
    timeout on a loaded host), and nothing downstream can tell a phantom
    empty listing from a genuinely idle machine. So the probe refuses to
    hand anyone a listing it cannot vouch for, and each caller decides
    what "unknown" means for ITS decision -- which is never "kill it".
    """


def live_pgids() -> set:
    """Process groups alive on this host, by listing -- the instrument is
    `ps -eo pgid=`, never pgrep.

    Raises `ProcessListingUnavailable` rather than returning a listing
    this function cannot vouch for. Three ways it cannot:

      * `ps` could not be spawned or did not finish (`OSError`,
        `TimeoutExpired`) -- the case that used to return `set()`.
      * `ps` exited non-zero. `subprocess.run` without `check=True`
        reports that only in `returncode`, so a `ps` killed by a signal
        previously parsed as an empty -- i.e. universally fatal -- listing.
      * the listing does not name THIS process's own group. That is the
        floor assertion (AGENTS.md: "a degraded run does not crash, it
        reports a confident, precisely-formatted, completely wrong
        number"): the caller is itself a live, non-zombie process, so a
        truthful listing always contains its pgid. An answer that omits
        it is malfunctioning however plausible it looks.
    """
    try:
        proc = subprocess.run(
            ["ps", "-eo", "pgid=,stat="], capture_output=True, text=True, errors="replace", timeout=10
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise ProcessListingUnavailable(f"ps could not be run: {exc!r}") from exc
    if proc.returncode != 0:
        raise ProcessListingUnavailable(
            f"ps exited {proc.returncode}: {(proc.stderr or '').strip()[:200]}")
    pgids = set()
    for line in proc.stdout.splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[0].isdigit() and not parts[1].startswith("Z"):
            pgids.add(int(parts[0]))
    try:
        own = os.getpgrp()
    except (AttributeError, OSError):
        own = None
    if own is not None and own not in pgids:
        raise ProcessListingUnavailable(
            f"ps listed {len(pgids)} process group(s) and not this runner's own "
            f"({own}); the listing is not trustworthy")
    if own is None and not pgids:
        raise ProcessListingUnavailable("ps listed no process groups at all")
    return pgids

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
    #: This job's `keel.journal` file identity (`journal_job_key`), so the
    #: reap step can close the job without re-deriving it. `None` on a
    #: worker built before the journal existed, or by a test that does not
    #: care -- `fleetd`'s reap treats `None` as "nothing to close", never
    #: as an error, because a missing journal record must not cost a
    #: correct reap.
    job_key: Optional[str] = None

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
        proc = subprocess.run(
            ["ps", "-wweo", "pgid=,uid=,command="],
            capture_output=True, text=True, errors="replace", timeout=10,
            env=_ps_env(),
        )
        out = proc.stdout if proc.returncode == 0 else None
    except (OSError, subprocess.TimeoutExpired):
        out = None
    if out is None:
        # NOT None-the-answer. `None` here means "no live member of this
        # group is a scoped fleet worker", and both adoption paths spend
        # that answer on a RELEASE: `journal.adopt_from_journal` records
        # the claim as owed a CAS-delete, `adopt_workers` treats the group
        # as an orphan. Handing them that verdict because `ps` would not
        # run frees a branch whose gate is still running, which is the
        # duplicate-gate hazard leases exist to prevent.
        #
        # So a failed listing fails CLOSED, exactly as
        # `fleetd_marker_in_group` below already does and for the reason
        # its docstring gives: a truthy sentinel reads as "a member
        # matched", the group is adopted rather than released, and the
        # next pass with a working `ps` reaps it if it really is dead.
        # That direction costs at most one branch held to its TTL; the
        # other is not retryable.
        return f"<ps listing unavailable -- refusing to declare pgid {pgid} unscoped>"
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


def _assume_alive_if_unlisted(probe: Callable[[], bool]) -> bool:
    """`probe()`, with `ProcessListingUnavailable` answered as ALIVE.

    ONLY for verifying a kill that has already been decided on. There,
    "assume alive" escalates SIGTERM to SIGKILL -- the direction that
    finishes the job -- whereas "assume gone" would end the grace loop
    early and leave the group running. It is the opposite of the answer
    the same exception must get in `reconcile_once`'s reap, where the
    decision is whether to kill at all, and that asymmetry is the point:
    an unknown listing is not a verdict, so each site spends it on
    whichever direction is retryable.
    """
    try:
        return probe()
    except ProcessListingUnavailable:
        return True


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
    outcome = kill_process_group(
        w.pgid, grace=grace,
        alive_probe=lambda _p: _assume_alive_if_unlisted(w.alive))
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


def journal_job_key(kind: str, claim_key: str) -> str:
    """The journal's identity for one job: `<kind>-<claim key>`.

    NOT the bare claim key, which is what `journal.file_stem`'s docstring
    assumes ("claim keys are already filesystem-shaped"). The claim key
    alone is NOT injective across kinds: `start_gate` builds it as
    `branch.replace("/", "-")` and `start_agent` as
    `branch.replace("/", "-").replace(":", "-")`, so a gate and an agent
    on `staging/one` produce the same key `staging-one` under two
    DIFFERENT claim refs (`refs/fleet/claims/gate/staging-one` and
    `.../agent/staging-one`). One journal file per job is the module's
    whole file layout, and folding two live jobs' records into one file
    would hand adoption a single JobState carrying the second job's pgid
    against the first job's claim ref -- an adoption that renews the
    wrong lease.

    The prefix keeps the two namespaces disjoint by construction: no
    `gate-...` string is ever an `agent-...` string, whatever the branch
    is called.

    IT IDENTIFIES A BRANCH, NOT A RUN, and it stays that way on purpose:
    `fleetd.adopt_workers` DERIVES this same key for a hub-adopted worker
    from (kind, claim key) alone -- it has neither the tag nor, at
    `offer` time, a `started_at` -- so folding either into the key would
    make the two derivations disagree and a hub-adopted worker would
    close a file its predecessor never opened. One file therefore
    accumulates every run this host makes on a branch, and it is
    `journal._runs`/`_fold` that separate them: the fold takes the
    trailing run only, so a completed run's `exit` neither closes the
    file for ever nor lends its `rc`/`outcome`/`started_at` to the live
    run that follows it.
    """
    return f"{kind}-{claim_key}"


def _close_failed_offer(jn, job_key: str, outcome: str) -> None:
    """Close an `offer` whose acquire did not complete, without raising.

    `start_gate`/`start_agent` write the `offer` BEFORE the CAS, and used
    to catch only `ClaimHeldError`. Any other failure out of
    `acquire_or_reap` -- `HubUnreachableError`, an ambiguous write --
    propagates through `reconcile_once` (which catches `OSError` and
    `JournalError` only) and left the job OPEN with a `claim_ref` and no
    `claim` record: an owed release forever, re-read by every later
    startup pass. `release_pending` now REFUSES such an entry (it has no
    ownership token to prove with), so this is hygiene rather than
    safety -- but a job that can never be acted on must not stay in
    `open_jobs`, or `prune` never collects the file.

    Swallows `JournalError` deliberately: the caller is already on its
    way out with a real failure, and replacing that exception with a
    journal one would hide the reason the start failed.
    """
    try:
        jn.exit(job_key=job_key, outcome=outcome)
    except journal_mod.JournalError:
        pass


def journal_claim_record(jn, job_key: str, c: Claim, *, kind: str,
                         work_key: str) -> None:
    """The `claim` record for a lease just acquired, written from the
    Claim object's OWN state -- never re-derived.

    `c.handle()` is the public accessor for the ownership token, and it
    formats `started_at`/`expires_at` through `claim._iso`, which is the
    exact spelling `claim._owns` compares as literal text and the exact
    one `journal.rebuild_claim` re-checks for round-tripping. Re-deriving
    the timestamp here with a second formatter -- `keel/cli.py`'s
    `strftime("%Y-%m-%dT%H:%M:%SZ")`, say -- would produce a claim record
    a rebuilt claim cannot recognize as its own.

    `_resolved_rustc_id()`/`_resolved_platform_id()` are MEMO HITS at this
    point, not measurements: `acquire_or_reap` has already built the
    payload once, which resolves both. They are recorded because
    `rebuild_claim` restores them into the rebuilt claim rather than
    re-measuring them under the runner's PATH instead of the gate's
    (invariant I15).
    """
    h = c.handle()
    if h is None:  # pragma: no cover -- acquire_or_reap succeeded above
        raise journal_mod.JournalWriteError(
            f"{job_key}: the claim reports no ownership token after acquire")
    jn.claim(job_key=job_key, claim_ref=h.ref, claim_sha=h.sha,
             holder_host=c.holder_host, started_at=h.started_at,
             expires_at=h.expires_at, kind=kind, work_key=work_key,
             gate_version=c.gate_version,
             rustc_id=c._resolved_rustc_id(),
             platform_id=c._resolved_platform_id())


def start_gate(
    hub: Hub,
    branch: str,
    tag: str,
    gate_command: list,
    host: str,
    log_dir: Path,
    journal: Optional["journal_mod.Journal"] = None,
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
    # Keel 3R-2 step 5 -- WRITE BEFORE SPAWN (SPEC SS5.3). The order is
    # offer -> acquire_or_reap -> claim -> Popen -> spawn, and every one
    # of those arrows is load-bearing:
    #
    #   * `offer` precedes the CAS because the runner can die between
    #     taking the lease and recording it, and a lease with no local
    #     record is a claim nothing will ever release offline.
    #   * `spawn` follows `Popen` immediately and precedes the post-spawn
    #     `renew` that persists the real pgid into the payload, so the
    #     window in which a process exists that nothing has written down
    #     is one `write()+fsync` wide.
    #
    # A `JournalWriteError` is therefore FATAL to this start and is not
    # caught here: `JournalWriteError`'s own docstring states the rule --
    # "THE CALLER MUST NOT SPAWN" -- because a process this runner cannot
    # journal is a process it can never adopt, which after a restart is a
    # live group with no local record and no pgid in the payload yet, the
    # exact shape the orphan sweep kills. `reconcile_once` turns the raise
    # into a `spawn-failed` refusal for this branch and keeps gating.
    jn = journal if journal is not None else journal_mod.Journal()
    job_key = journal_job_key("gate", c.key)
    jn.offer(job_key=job_key, kind="gate", work_key=branch, tag=tag,
             claim_ref=c.ref)
    try:
        # acquire_or_reap: an EXPIRED claim (crashed holder, TTL passed)
        # must not block the branch forever -- reap it CAS'd and proceed.
        # A live claim still refuses, which is the double-gate guard.
        c.acquire_or_reap()
    except claim_mod.ClaimHeldError:
        # CLOSE the job. An `offer` left open with no `claim` and no
        # `spawn` is read by `adopt_from_journal` as "never spawned" and,
        # because the offer carries a `claim_ref`, is recorded as an OWED
        # RELEASE -- a deferred CAS-delete of a ref we never held. Against
        # a claim held by another host `release_pending` re-verifies and
        # leaves it alone, but against one held by THIS host (an adopted
        # worker, an `autonomous_when_serverless` gate) the journaled
        # `started_at` is None, so the started_at re-check is skipped and
        # the delete would drop a live gate's lease. The exit record makes
        # the job closed: never adopted, never released, never swept.
        jn.exit(job_key=job_key, outcome="claimed-elsewhere")
        return None
    except Exception:
        # Not "someone else holds it" -- the CAS itself failed (the store
        # went away, the write was ambiguous). No lease is recorded here
        # either way, so close the offer before the exception leaves.
        _close_failed_offer(jn, job_key, "claim-failed")
        raise
    try:
        journal_claim_record(jn, job_key, c, kind="gate", work_key=branch)
    except journal_mod.JournalError:
        # The lease is held and the record that would make it recoverable
        # cannot be written. Give the lease back rather than leave the
        # branch blocked for a full TTL by a job that will never start,
        # then re-raise for `reconcile_once` to record as `spawn-failed`.
        c.release()
        raise
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
        jn.exit(job_key=job_key, outcome="spawn-failed")
        raise
    finally:
        log.close()
    try:
        jn.spawn(job_key=job_key, pid=popen.pid, pgid=popen.pid,
                 scope_token=fleet_scope_token(hub.url), argv0=str(gate_command[0]))
    except journal_mod.JournalError:
        # A process exists that this runner cannot write down. Compare the
        # two directions of being wrong, the way the lost-lease kill does:
        # killing it costs one retryable gate run, while letting it live
        # leaves a group holding a claim that no future startup pass can
        # adopt -- it has no journal record, and until the post-spawn
        # `renew` below lands it has no pgid in the payload either, which
        # is exactly the shape the orphan sweep kills, later, with no
        # verdict and no trace. Kill the group and give the lease back.
        kill_process_group(popen.pid)
        c.release()
        raise
    worker = Worker(branch=branch, tag=tag, pgid=popen.pid, claim=c, popen=popen,
                    job_key=job_key)
    # Persist the real pgid into the claim payload: renew() rewrites the
    # payload from the object's fields, so setting the attribute and
    # renewing once records it durably (claim-before-launch means the
    # first write couldn't know it yet).
    c.pid = popen.pid
    c.pgid = popen.pid
    c.renew()
    return worker


def start_agent(
    hub: Hub, branch: str, tag: str, host: str, log_dir: Path, repo_root: Path,
    journal: Optional["journal_mod.Journal"] = None,
) -> Optional[Worker]:
    """Claim the branch for an agent and launch agentworker.py in its own
    process group. Same discipline as gates: claim-before-launch, pgid
    persisted via renew."""
    intent_slug = branch.removeprefix("intent:") if branch.startswith("intent:") else None
    # holder_host=host: see the identical comment in start_gate above.
    c = Claim(hub, kind="agent", key=branch.replace("/", "-").replace(":", "-"),
              work_kind="agent", work_key=branch, holder_host=host)  # see start_gate
    # Write-before-spawn, identical discipline to `start_gate` -- see the
    # long comment there for why each arrow in offer -> acquire -> claim
    # -> Popen -> spawn points the way it does.
    jn = journal if journal is not None else journal_mod.Journal()
    job_key = journal_job_key("agent", c.key)
    jn.offer(job_key=job_key, kind="agent", work_key=branch, tag=tag,
             claim_ref=c.ref)
    try:
        c.acquire_or_reap()
    except claim_mod.ClaimHeldError:
        jn.exit(job_key=job_key, outcome="claimed-elsewhere")
        return None
    except Exception:
        _close_failed_offer(jn, job_key, "claim-failed")  # see `start_gate`
        raise
    try:
        journal_claim_record(jn, job_key, c, kind="agent", work_key=branch)
    except journal_mod.JournalError:
        c.release()  # see the identical comment in `start_gate`
        raise
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
        jn.exit(job_key=job_key, outcome="spawn-failed")
        raise
    finally:
        log.close()
    try:
        jn.spawn(job_key=job_key, pid=popen.pid, pgid=popen.pid,
                 scope_token=fleet_scope_token(hub.url), argv0=sys.executable)
    except journal_mod.JournalError:
        kill_process_group(popen.pid)  # see the identical comment in `start_gate`
        c.release()
        raise
    w = Worker(branch=branch, tag=tag, pgid=popen.pid, claim=c, popen=popen, kind="agent",
               job_key=job_key)
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

    def note(self, key: str, reason: str, detail: str) -> None:
        """Record a warning that is NOT backed by a marker file.

        `scan` only owns the `verdict-store-failed` reason and leaves
        every other entry alone (see its final block), so an entry noted
        here persists for the daemon's life -- which is right for a
        condition like L1's toolchain disagreement: it is a property of
        how this host is installed, and it does not clear because fifteen
        seconds passed. `key` namespaces the entry the way a marker path
        does, so re-noting the same condition cannot accumulate copies.
        """
        self.entries[key] = (reason, detail)

    def current(self) -> list:
        return [self.entries[k] for k in sorted(self.entries)]


# --------------------------------------------------------------------- #
# L1 -- the scheduler and the gate it spawns must derive the same key
# --------------------------------------------------------------------- #
#
# THE INCIDENT (Keel Stage 1 LIVE, 2026-08-27/28). On the i7, fleetd's
# host claim recorded `platform_id b2bdf493...` while the gate fleetd had
# itself just spawned wrote its verdict under `b6613b19...` -- same host,
# same minute, same compiler. `platform_id` is one third of the verdict
# cache key, so `verdict.lookup` under fleetd's key returned None for a
# tree whose PASS was already published under the gate's key.
# `classify_branch` therefore never returned AWAITING_TRAIN and this host
# re-gated the identical merge tree forever, ~21 minutes a pass, while
# the answer it was paying for sat unread on the state repo.
#
# WHAT MADE IT INVISIBLE, and what this check fixes. Nothing anywhere
# compared the two sides. Stage 1's acceptance bullet asserted only that
# `git ls-remote` listed A verdict with A platform_id -- true in the
# broken state, because the GATE's key was perfectly well formed. An
# assertion that a value exists cannot catch two components disagreeing
# about the value; only an assertion that the two AGREE can.
#
# So the runner now derives the id BOTH ways at startup -- its own
# (`toolchain.compute_platform_id`) and its gate command's (by running
# the gate's own `units/fleet-toolchain.sh` in a shell, not by
# re-deriving the formula here, which would only add a fourth
# implementation) -- and refuses to start when they differ.
#
# The check itself lives HERE rather than in fleetd.py because it is a
# property of the process this host runs and of the gate command that
# process spawns -- both runner concerns under SPEC S9's split. fleetd.py
# re-exports the three names below, so `fleetd.check_toolchain_agreement`
# and friends resolve exactly as the original fix spelled them.

TOOLCHAIN_MISMATCH_WARNING = "toolchain-id-mismatch"
TOOLCHAIN_UNVERIFIED_WARNING = "toolchain-id-unverified"

# Escape hatch. A mismatch means every gate this host runs writes to a
# cache slot this host cannot read, so refusing is right -- but an
# operator who is mid-migration and knows it should be able to run
# degraded rather than have a supervisor respawn a refusing daemon every
# ten seconds. Downgrades the refusal to a durable warning; it never
# suppresses the warning itself.
ALLOW_TOOLCHAIN_MISMATCH_ENV = "FLEET_ALLOW_TOOLCHAIN_MISMATCH"

# BLOCKER 2 (this port, not the original fix). The original wrote this
# refusal as `return 6` -- the code `run_daemon` ALREADY used for "a
# persistently unusable hub", which `tests/test_fleetd.py` asserts in two
# places and `units/fleetd-wrapper.sh` restarts on. Two conditions under
# one code is not cosmetic: a wedged hub may come back, so restarting it
# is correct, while a toolchain-id mismatch is a property of how this
# host is installed and cannot resolve without a human, so restarting it
# is a hot loop. Nothing pinned the refusal to 6 (the seam test asserts
# no rc at all), which is exactly how the collision could be written
# silently -- so it gets its own code, and
# `test_toolchain_seam.TestTheRefusalCodeIsItsOwn` asserts the two
# conditions never share one again.
TOOLCHAIN_MISMATCH_RC = 7


def gate_toolchain_ids(gate_command: list, timeout: int = 30,
                       env: Optional[dict] = None) -> "tuple[dict, Optional[str]]":
    """`{"rustc_id", "platform_id"}` as THE GATE COMMAND would compute
    them, plus an error string when they could not be obtained.

    Runs the gate's own `units/fleet-toolchain.sh` -- the file `gate.sh`
    sources -- resolved relative to the gate script the runner would
    actually spawn, so a runner pointed at some other checkout measures
    THAT checkout's resolver rather than this process's sibling
    directory. Deliberately not a re-implementation: a fourth spelling of
    the formula is the disease, not the cure.

    `env` is the environment the shell side is measured under, and it
    MUST be the same dict the Python side is measured under -- comparing
    two ids derived under two environments would answer a question
    nobody asked.
    """
    if not gate_command:
        return {}, "no gate command configured"
    gate_path = Path(gate_command[0])
    helper = gate_path.parent / "units" / "fleet-toolchain.sh"
    if not helper.is_file():
        return {}, f"gate toolchain resolver not found at {helper}"
    script = "\n".join((
        f"SELF_DIR={shlex.quote(str(gate_path.parent))}",
        f". {shlex.quote(str(helper))}",
        'if ! fleet_toolchain_ids; then',
        '  printf "ERROR=%s\\n" "$FLEET_TOOLCHAIN_ERROR"',
        '  exit 1',
        'fi',
        'printf "RUSTC_ID=%s\\nPLATFORM_ID=%s\\n" "$RUSTC_ID" "$PLATFORM_ID"',
        "",
    ))
    try:
        out = subprocess.run(  # nosec B603 -- list argv, our own script text
            ["bash", "-c", script], capture_output=True, text=True, timeout=timeout,
            env=dict(os.environ if env is None else env),
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        return {}, f"could not run {helper}: {exc}"
    parsed = {}
    for line in out.stdout.splitlines():
        key, _, value = line.partition("=")
        if key in ("RUSTC_ID", "PLATFORM_ID"):
            parsed[key.lower()] = value
    if out.returncode != 0 or not parsed.get("platform_id"):
        detail = (out.stdout + out.stderr).strip().replace("\n", "; ")[:400]
        return {}, f"{helper} exited {out.returncode}: {detail or 'no ids produced'}"
    return parsed, None


def check_toolchain_agreement(gate_command: list, host: str,
                              warnings: "HostWarnings",
                              env: Optional[dict] = None) -> "tuple[bool, str]":
    """`(may_start, message)` -- L1's loud mismatch check.

    THREE OUTCOMES, kept distinct on purpose (the same doctrine as
    `desired_readable` vs `enabled` in `reconcile_once`: "we could not
    ask" is not "the answer is no"):

      * AGREE     -- `(True, "")`, no warning.
      * DISAGREE  -- `(False, ...)` and a durable warning. Every verdict
                     this host's gates write lands in a slot this host
                     cannot read; there is no useful work it can do.
                     `FLEET_ALLOW_TOOLCHAIN_MISMATCH=1` turns this into
                     `(True, ...)` with the warning intact.
      * UNVERIFIED -- `(True, ...)` and a durable warning. The gate's
                     resolver could not be run at all (no bash, no
                     checkout, a fixture whose "gate" is a stub script).
                     Not being able to compare is not evidence of a
                     mismatch, and taking a host down for it would turn
                     one silent bug into a different one.
    """
    src = os.environ if env is None else env
    mine = toolchain.compute_platform_id(env=src)
    theirs, err = gate_toolchain_ids(gate_command, env=src)
    if err is not None:
        detail = (
            f"could not compute the gate's platform_id to compare against this "
            f"scheduler's {mine[:12]}...: {err}. Until this is resolved, nothing "
            f"proves the runner and gate.sh address the same verdict-cache slot"
        )
        warnings.note(f"toolchain:{host}", TOOLCHAIN_UNVERIFIED_WARNING, detail)
        return True, detail
    if theirs.get("platform_id") == mine:
        return True, ""
    detail = (
        f"fleetd computes platform_id {mine} but its own gate command "
        f"({gate_command[0]}) computes {theirs.get('platform_id')}. The verdict "
        f"cache is keyed by (tree, gate_version, platform_id), so every PASS "
        f"this host's gates publish would be unreadable by the scheduler that "
        f"paid for it, and this host would re-gate the same merge tree forever. "
        f"Set {ALLOW_TOOLCHAIN_MISMATCH_ENV}=1 to run degraded anyway"
    )
    warnings.note(f"toolchain:{host}", TOOLCHAIN_MISMATCH_WARNING, detail)
    allowed = (src.get(ALLOW_TOOLCHAIN_MISMATCH_ENV) or "").strip() not in ("", "0", "false")
    return allowed, detail


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
                    # DERIVED, not read: `journal_job_key` is a pure
                    # function of (kind, claim key), and `start_gate`/
                    # `start_agent` built this job's key from the same two
                    # values, so a hub-adopted worker closes the very file
                    # its own predecessor opened. `kind` here is the CLAIM
                    # kind (the ref namespace), never `payload["work_kind"]`
                    # -- the ref is what the key was built from.
                    job_key=journal_job_key(kind, c.key),
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
        outcome = killer(
            pgid,
            alive_probe=lambda p: _assume_alive_if_unlisted(lambda: p in pgid_probe()))
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
# Runner registration (SPEC SS5.3 step 1) -- OUTBOUND ONLY
#
# Three small functions and one rule: registration is best-effort, never
# load-bearing. A runner whose registration never lands must still gate,
# because gating is what a runner is for; the registration exists so the
# server can stop believing itself unreachable (`election.note_registration`
# is the only inbound proof that averts `demote_unreachable`) and, later,
# so it can see this host's capabilities and live work.
#
# It travels on `ServerHub` ONLY, reached via `FallbackHub.primary`. It is
# not a ref write and must never become one -- see `ServerHub.register`'s
# docstring for why (SPEC SS4.3 r2), and `write_heartbeat` above for the one
# write loop in this tree that is allowed to survive an ambiguous outcome,
# which this must not be conflated with or attached to.
# --------------------------------------------------------------------- #


# Registration's OWN latency budget, and why it is not the hub's.
#
# `ServerHub`'s default 5 s connect / 20 s read (SPEC §4.2) is a CAS
# write's budget: a write cut off mid-flight is an AMBIGUOUS outcome that
# `FallbackHub` refuses to paper over, so the wait is bought deliberately.
# An announcement has no such property -- its failure mode is "we stay
# unregistered until the next cycle" -- and `register_cycle` runs on the
# reconcile loop's own thread, between the step and `time.sleep(interval)`.
# A server that ACCEPTS the connection and then answers nothing (a
# half-open tailnet path, a wedged accept loop, a SIGSTOPped process --
# NOT a closed port, which fails in ~1 ms) therefore charged the whole
# 20 s read to the loop's cadence, every cycle, forever: at
# LOOP_SECONDS=15 the loop's real period became 35 s. Measured with a
# black-hole listener: 20.15 s and 20.16 s between consecutive reconcile
# steps at `interval=0`.
#
# So registration gets a budget sized to itself. Kept above a loopback/
# tailnet round trip by a wide margin -- this is a stall detector, not a
# latency SLO -- and far below LOOP_SECONDS so a worst-case cycle still
# costs a fraction of the cadence rather than doubling it.
REGISTER_CONNECT_TIMEOUT_S = 2.0
REGISTER_READ_TIMEOUT_S = 3.0

# ...and a backoff, because a bounded per-cycle cost is still a per-cycle
# cost. A server that is down stays down for minutes, not milliseconds,
# and paying 5 s of the loop's 15 s to re-learn that on every pass is a
# third of the scheduler spent on an announcement.
#
# The ceiling is 60 s from two directions that agree. SPEC §5.3 already
# names it -- "Reconnect: long-poll 30 s, connect 5 s, backoff 1->60 s
# with jitter" -- and `election.DEMOTION_S` (180 s) independently forbids
# anything much larger: a server that comes back must be registered with
# before it demotes itself unreachable, and `election.note_registration`
# is the only inbound signal that averts `demote_unreachable`. MAX + one
# cycle is the worst-case detection delay, so 60 s leaves that window a
# factor of ~2.4 of headroom.
#
# NO JITTER, deliberately and with the omission stated rather than
# approximated. §5.3's jitter belongs to the long-poll reconnect loop
# that sentence is about, which is not built yet; this ladder rides the
# reconcile loop, whose phase is already unsynchronised across hosts, and
# adding a random source here would only make the ladder untestable
# without an injected RNG. When the reconnect loop lands, the jitter
# belongs there.
#
# The first failure waits nothing at all -- one blip must still be retried
# on the very next cycle, which is the retry policy `register_once`'s
# docstring names -- and only a SECOND consecutive failure starts the
# ladder.
REGISTER_BACKOFF_BASE_S = 15.0
REGISTER_BACKOFF_MAX_S = 60.0


def server_client(hub) -> Optional[ServerHub]:
    """The `ServerHub` inside `hub`, or `None` when no server is
    configured.

    THE ONE place that knows the hub chain's shape. `run_daemon`'s `hub`
    parameter is duck-typed on purpose -- everything downstream of it
    works against the eight-method coordination contract and must keep
    working against a plain `fleetlib.Hub` -- so the knowledge that a
    server-configured runner is holding a `FallbackHub` whose `.primary`
    is a `ServerHub` lives here and nowhere else. `None` is the normal
    answer on a hubless Stage-1 runner, not an error.
    """
    if isinstance(hub, FallbackHub) and isinstance(hub.primary, ServerHub):
        # A CLONE with registration's own budget, not `hub.primary`
        # itself. Handing back the primary hands back the CAS write
        # path's 20 s read timeout, and this client is used from the
        # reconcile loop's own thread -- see `REGISTER_READ_TIMEOUT_S`
        # for the stall that buys. It is also emphatically NOT the
        # object `FallbackHub` damps: `_primary_worth_trying`'s 30 s
        # sticky window (`fallbackhub.STICKY_S`) applies to calls made
        # THROUGH the FallbackHub, and `register` is deliberately not one
        # of them (SPEC §4.3 r2), so registration has to carry its own
        # damping. That is `register_cycle`'s `backoff`.
        return hub.primary.with_timeouts(
            connect_timeout_s=REGISTER_CONNECT_TIMEOUT_S,
            read_timeout_s=REGISTER_READ_TIMEOUT_S,
        )
    return None


def live_workers_payload(workers: Sequence["Worker"]) -> list:
    """`live_workers[]` (SPEC SS5.3, SS9's liveness join) built from the
    IN-MEMORY `workers` list -- the join of claims x live pgids that
    `adopt_workers` already computed and that `reconcile_once` keeps
    current, and which costs nothing to read.

    Deliberately NOT built from a hub claim listing. `CachedHub.list()`
    and `fetch_namespace()` over `refs/fleet/claims/` are index-served
    with no freshness test at all (cachedhub.py `list`/`fetch_namespace`);
    the fresh-claims invariant covers `sha`/`read`/`read_with_sha` only.
    That is safe for a consumer that then CASes against a live-read sha --
    the CAS catches the staleness -- and it is NOT safe here, where the
    listing would be the input to a liveness verdict with no CAS behind it
    to catch the error.

    `claim_sha`/`started_at` come off the `Claim` object's own recorded
    state, the same two privates `election.status_fields` reads for the
    same reason: they are the lease as this process holds it, and half of
    them (`started_at`) is half the ownership token, compared downstream
    as literal text.
    """
    out = []
    for w in workers:
        c = getattr(w, "claim", None)
        started = getattr(c, "_started_at", None) if c is not None else None
        out.append({
            "claim_ref": getattr(c, "ref", None),
            "claim_sha": getattr(c, "_sha", None),
            "pgid": w.pgid,
            "tag": w.tag,
            "kind": w.kind,
            "started_at": claim_mod._iso(started) if started is not None else None,
        })
    return out


def registration_payload(host: str, workers: Sequence["Worker"], repo_root: Path,
                         scope_token: str) -> dict:
    """The body of `POST /v1/runners/{id}/register`: what this host is
    (`capabilities`) and what it is running (`live_workers[]`, top level,
    per SPEC SS5.3's shape).

    NAME COLLISION, on purpose and worth knowing: `doctor.py` already has
    a `registration_payload`, whose docstring says it exists so "a runner's
    `register` call has both the summary numbers and the reasoning behind
    each one". This is NOT that function and does not call it. `doctor`'s
    takes a list of already-run `Check`s, one of which is an NTP round
    trip, and this one is called from inside the reconcile loop, where a
    network probe is a latency source the loop's whole reason for
    existing (reap + lost-lease kill on a 15 s cadence) cannot afford. If
    the two ever need to be the same numbers, the fix is for `doctor` to
    call this, not the reverse.

    WHAT IS DELIBERATELY ABSENT: the GATE's `platform_id`/`rustc_id` as
    `gate_toolchain_ids` computes them. `check_toolchain_agreement` has
    already compared them against this process's own ids at startup and
    refused to start on a mismatch (`TOOLCHAIN_MISMATCH_RC`) unless
    `FLEET_ALLOW_TOOLCHAIN_MISMATCH=1`, in which case the disagreement is
    already durable in `HostWarnings` and therefore already in every
    heartbeat. So on any runner that reaches this call the gate's ids are
    either equal to the ones below or already reported elsewhere, and
    re-deriving them here would spend a `bash` + `rustc -vV` (30 s
    timeout) per registration inside that same loop to learn nothing new.
    Omitted and said so rather than approximated.
    """
    mem = free_mem_gb()
    return {
        "id": host,
        "capabilities": {
            "owning_user": owning_user(),
            # `claim`'s copies, not `toolchain`'s directly and not
            # `verdict.compute_ids` -- the same two functions `fleetd`'s
            # heartbeat payload calls, so this host's registered number and
            # its heartbeat number are computed by literally one code path.
            "platform_id": claim_mod.compute_platform_id(),
            "rustc_id": claim_mod.compute_rustc_id(),
            "cores": os.cpu_count(),
            "free_disk_gb": round(free_disk_gb(), 1),
            # -1.0 is `free_mem_gb`'s "unknowable" answer (macOS without
            # psutil); it must not be reported as a real measurement.
            "free_mem_gb": round(mem, 1) if mem >= 0 else None,
            "oracle_ok": _oracle_ok(),
            "gate_version": _gate_version(repo_root),
            "scope_token": scope_token,
        },
        "live_workers": live_workers_payload(workers),
    }


def register_once(client, runner_id: str, payload: dict,
                  log: Callable[[str], None]) -> Optional[dict]:
    """ONE registration attempt. Returns the server's reply dict, or
    `None` on any failure -- and NEVER raises into the caller.

    Non-fatal is the entire contract. The retry policy is the reconcile
    loop's own 15 s cadence and nothing else: no second retry ladder, no
    second thread. The runner already has one bounded-failure counter with
    a supervisor-visible exit (`RECONCILE_HUB_FAILURE_LIMIT`), and a
    second, independent ladder around a call that is not load-bearing is a
    second thing that can wedge.

    The `except` is broader than this file's usual policy (the loop below
    catches `HubError` only, so a bug in this tree takes the process down
    loudly rather than being retried forever). That asymmetry is
    deliberate and bounded to here: a defect in an ANNOUNCEMENT must not
    stop a healthy host from gating. It is loud in the log, with the
    exception type named, so it cannot pass for a quiet server outage.
    """
    try:
        reply = client.register(runner_id, payload)
    except Exception as exc:  # noqa: BLE001 -- see docstring
        log(f"REGISTER failed ({type(exc).__name__}: {exc}) -- "
            f"continuing unregistered; gating is unaffected")
        return None
    if not isinstance(reply, dict):
        log(f"REGISTER answered a {type(reply).__name__}, not an object -- ignoring")
        return None
    return reply


def _boot_id_changed(health: object, session: dict) -> bool:
    """True when the server's advertised `boot_id` differs from the one
    our last registration was answered with -- i.e. the process we are
    talking to is not the process that holds our registration, so the
    registration is gone and must be re-sent.

    Its own named function for one reason: `tests/test_runner_register.py`
    disables exactly this comparison as a negative control and requires
    the reconnect test to go RED. A reconnect test that cannot fail proves
    nothing.

    A non-dict `health` answers False -- `ServerHub.health` already raises
    on a 200 that is not an object, so reaching here with one would mean a
    different client entirely, and guessing "the server rebooted" from an
    unreadable answer would produce a re-register storm rather than a
    reconnection.
    """
    if not isinstance(health, dict):
        return False
    return health.get("boot_id") != session.get("boot_id")


def _register_backoff_wait_s(fails: int) -> float:
    """Seconds to wait before the next attempt after `fails` consecutive
    failed registration attempts. `fails <= 1` waits nothing: a single
    blip is retried on the very next cycle, unchanged.

    Its own named function so the ladder can be asserted directly, and so
    the DEMOTION_S headroom argument above is checked against one
    expression rather than re-derived from an inline shift.
    """
    if fails <= 1:
        return 0.0
    return min(REGISTER_BACKOFF_BASE_S * (2 ** (fails - 2)), REGISTER_BACKOFF_MAX_S)


def _register_backoff_skip(backoff: Optional[dict], now: float) -> bool:
    """True when this cycle is inside the backoff window and must not
    make the call. `None` disables backoff entirely -- which is what a
    direct caller (and every pre-existing test) gets."""
    if backoff is None:
        return False
    return now < backoff.get("not_before", 0.0)


def _register_backoff_note(backoff: Optional[dict], now: float, ok: bool,
                           log: Callable[[str], None]) -> None:
    """Record one attempt's outcome. Success clears the ladder; failure
    advances it and logs the wait, because a silent gap between
    registrations is indistinguishable from a runner that stopped trying.
    """
    if backoff is None:
        return
    if ok:
        backoff["fails"] = 0
        backoff["not_before"] = 0.0
        return
    fails = int(backoff.get("fails", 0)) + 1
    wait = _register_backoff_wait_s(fails)
    backoff["fails"] = fails
    backoff["not_before"] = now + wait
    if wait:
        log(f"REGISTER backing off {wait:.0f}s after {fails} consecutive "
            f"failed attempts -- gating is unaffected")


def register_cycle(client, runner_id: str, session: dict,
                   build_payload: Callable[[], dict],
                   log: Callable[[str], None],
                   backoff: Optional[dict] = None,
                   clock: Callable[[], float] = time.monotonic) -> Optional[str]:
    """One loop iteration's worth of registration. Mutates `session` in
    place on success and returns why it registered (`"first"` or
    `"reconnect"`), or `None` when it did not register or could not.

    NEVER RAISES, and never touches the reconcile result. It runs AFTER
    the reconcile step so that a slow server delays the NEXT cycle rather
    than sitting between adoption and the first reap, where `run_daemon`
    already argues (see `check_toolchain_agreement`'s call site) that a
    second of avoidable latency is a live gate reported as an orphan and
    killed.

    BUT "delays the next cycle" is only tolerable because the delay is
    BOUNDED, and it was not. This call ran on the loop's own thread with
    the CAS write path's 5 s + 20 s budget and no damping of any kind, so
    a server that accepted TCP and answered nothing cost 20 s on EVERY
    cycle, in either steady state (`health()` when registered,
    `register_once` when not) and forever. Two things bound it now, and
    both are needed:

      * `server_client` hands this function a client with
        `REGISTER_CONNECT_TIMEOUT_S` / `REGISTER_READ_TIMEOUT_S`, so ONE
        cycle's worst case is ~5 s rather than ~25 s; and
      * `backoff` -- a caller-owned dict, mutated in place -- makes the
        SECOND and later consecutive failures skip cycles entirely, so a
        server that is down costs one attempt per `REGISTER_BACKOFF_MAX_S`
        instead of one per cycle.

    `backoff=None` (the default, and what every direct caller in the tests
    passes) disables the ladder and preserves the retry-on-the-very-next-
    cycle behaviour exactly. A payload-build failure is NOT counted into
    it: that is a local defect, it costs no wall-clock on the loop, and
    backing off from it would only delay noticing that the server is fine.

    `build_payload` is a callable, not a payload, so nothing is measured
    on the cycles where no registration is sent -- which is nearly all of
    them.
    """
    if client is None:
        return None
    now = clock()
    if _register_backoff_skip(backoff, now):
        return None
    reason = "first"
    if session:
        try:
            health = client.health()
        except Exception as exc:  # noqa: BLE001 -- best-effort, see docstring
            log(f"REGISTER health probe failed ({type(exc).__name__}: {exc}) -- "
                f"keeping the existing registration")
            _register_backoff_note(backoff, now, False, log)
            return None
        if not _boot_id_changed(health, session):
            # The steady state, and the ONLY place a successful health
            # probe clears the ladder. Clearing it on the probe alone
            # would defeat the backoff in the one case that still costs
            # wall-clock: a server that answers `/v1/health` quickly but
            # black-holes `register` would reset `fails` to 0 on every
            # cycle and pay a full read timeout on every cycle forever.
            _register_backoff_note(backoff, now, True, log)
            return None
        reason = "reconnect"
    try:
        payload = build_payload()
    except Exception as exc:  # noqa: BLE001 -- an announcement must not stop gating
        log(f"REGISTER payload build failed ({type(exc).__name__}: {exc})")
        return None
    reply = register_once(client, runner_id, payload, log)
    if reply is None:
        _register_backoff_note(backoff, now, False, log)
        return None
    _register_backoff_note(backoff, now, True, log)
    session.clear()
    session.update(reply)
    log(f"REGISTERED ({reason}) boot_id={reply.get('boot_id')} "
        f"settle_until={reply.get('settle_until')} "
        f"lease_expires_at={reply.get('lease_expires_at')} "
        f"live_workers={len(payload.get('live_workers') or [])}")
    return reason


# --------------------------------------------------------------------- #
# autonomous_when_serverless (Keel 3R-2 steps 9-10; SPEC SS12 "never less
# capable than today's hubless Stage 1")
# --------------------------------------------------------------------- #

#: SPEC SS12's entry gate: the server lease absent or expired for longer
#: than this, CONTINUOUSLY, is what makes a host autonomous.
AUTONOMY_ENTER_AFTER_S = 60.0

#: The exit hysteresis, in OBSERVATIONS rather than seconds. Two: the
#: observation that ends the absence, and the one a full loop later. SPEC
#: gives no exit condition at all, and "exit on the first live lease" is
#: the wrong one -- a lease that flaps (a server settling, an election
#: handing over, a 5 s network blip) would then toggle this host between
#: two scheduling regimes every cycle, and each toggle changes both the
#: dispatch set and the loop interval.
AUTONOMY_EXIT_LIVE_OBSERVATIONS = 2

#: The loop interval while autonomous. SPEC SS12 asks for one minute
#: rather than `LOOP_SECONDS`: there is no server to answer to, the work
#: is gates only, and a 15 s cadence spends four times the git traffic on
#: a store that is by hypothesis the only route left.
AUTONOMOUS_INTERVAL_S = 60.0


class AutonomyGate:
    """Whether this host should schedule for itself, with no server.

    THE TRIGGER IS THE LEASE REF, NOT `FallbackHub.degraded_since`. That
    field (`fallbackhub.FallbackHub.degraded_since`) is the convenient
    signal and the wrong one, for three independent reasons: it is
    in-memory, so it resets to `None` on every runner restart because
    `build_hub` constructs a fresh `FallbackHub`; it goes non-`None` for
    a five-second blip, which is four orders of magnitude short of
    SPEC's minute; and it describes THIS PROCESS's luck with a transport
    rather than the fleet's actual state. `refs/fleet/claims/server/
    singleton` is durable, shared, and is the thing SPEC names. Use
    `degraded_since` for the heartbeat and nothing else.

    THREE observations, not two. A lease that is present and unexpired is
    LIVE. A lease that is absent, or present and expired, is DOWN and
    starts (or continues) the clock. A lease that could not be READ is
    NEITHER: the state repo is unreachable, which is not evidence about
    the server and must not be counted as absence. An unreadable lease
    therefore freezes this gate exactly where it was -- it neither starts
    the clock nor resets it -- because both alternatives are wrong in a
    way that matters. Counting it as absence makes a host go autonomous
    because ITS OWN git route broke, which is the one condition under
    which it can least afford to schedule unilaterally; counting it as
    liveness would drag a genuinely serverless host back out of autonomy
    on a read failure.

    `now` is a MONOTONIC clock (`time.monotonic` by default), never wall
    time: the entry gate measures a duration this process observed, and a
    host whose clock steps (ntp correction, a laptop waking) must not
    thereby enter or leave autonomy.
    """

    def __init__(self, hub, *, enabled: bool,
                 enter_after_s: Optional[float] = None,
                 exit_after_live: Optional[int] = None,
                 clock: Callable[[], float] = time.monotonic,
                 log: Callable[[str], None] = lambda msg: None):
        # LATE-BOUND from the module constants rather than defaulted to
        # them in the signature, and the difference is not stylistic:
        # Python evaluates a default once, at `def` time, so
        # `exit_after_live=AUTONOMY_EXIT_LIVE_OBSERVATIONS` would freeze
        # the value at import and `mock.patch` on the constant would
        # change nothing. `tests/test_runner_autonomy.py` disables the
        # hysteresis by patching exactly that constant, as its mandatory
        # negative control, and a control that cannot take effect proves
        # nothing.
        self.hub = hub
        self.enabled = bool(enabled)
        self.enter_after_s = float(
            AUTONOMY_ENTER_AFTER_S if enter_after_s is None else enter_after_s)
        self.exit_after_live = int(
            AUTONOMY_EXIT_LIVE_OBSERVATIONS if exit_after_live is None else exit_after_live)
        self._clock = clock
        self._log = log
        self.autonomous = False
        #: Monotonic instant the current run of DOWN observations began.
        self.down_since: Optional[float] = None
        #: Consecutive LIVE observations, for the exit hysteresis.
        self.live_streak = 0
        #: The last observation's verdict: "live", "down", "unreadable"
        #: or None before the first one. Reported, never re-derived.
        self.last_observation: Optional[str] = None

    def read_lease(self) -> str:
        """One observation of the server lease: `"live"`, `"down"` or
        `"unreadable"`. Separated from `observe` so a test can drive the
        state machine without a store.

        `expires_at` IS PARSED HERE rather than delegated to
        `claim.is_expired`, and the difference is the third verdict.
        `is_expired` fails OPEN by contract -- "a payload with no (or
        unparseable) `expires_at` is treated as *not* expired", and its
        docstring says in as many words that "callers that require a
        well-formed claim payload should validate that separately". This
        is such a caller: through `is_expired` a lease whose deadline
        cannot be read came back LIVE, permanently, which is the one
        answer that can never be right about a payload this gate could
        not evaluate. It is not evidence of a live server; it is a
        payload we do not understand, so it takes the SAME disposition as
        a lease we could not read at all -- freeze, per the class
        docstring's "THREE observations, not two".

        The concrete way this bit: `datetime.fromisoformat` only accepts
        a trailing `Z` from Python 3.11, and the SPEC floor is py >=3.10
        (docs/AGENT-SERVER-SPEC.md). `keel server rehost` wrote this very
        ref with `strftime('%Y-%m-%dT%H:%M:%SZ')` until the same commit
        as this comment, so on a 3.10 host an expired rehost-written
        lease read LIVE for ever and `autonomous_when_serverless` could
        never engage -- silently, on exactly the host SPEC SS12 built the
        feature for. Both halves are fixed: `cli.cmd_server_rehost` now
        writes `claim._iso` like every other writer of a lease payload,
        and this reader no longer treats "cannot evaluate" as "live".
        """
        try:
            payload = self.hub.read(SERVER_LEASE_REF)
        except HubError:
            return "unreadable"
        if not payload:
            # ABSENT is evidence, and it is evidence of absence: SPEC's
            # "no server lease" is the DOWN case, not the unknown one.
            return "down"
        raw = payload.get("expires_at")
        if not raw:
            return "unreadable"
        try:
            # VALIDATE ONLY. The live/expired comparison stays in
            # `is_expired`, which owns it -- re-deriving `now` here would
            # be a second clock for one decision. `claim._parse_iso` is
            # the exact parser `is_expired` uses, so "this parses" here
            # and "this parsed" there cannot disagree.
            claim_mod._parse_iso(raw)
        except (ValueError, TypeError):
            return "unreadable"
        return "down" if claim_mod.is_expired(payload) else "live"

    def observe(self, verdict: Optional[str] = None) -> bool:
        """Fold one observation in and return whether this host is
        autonomous. A no-op returning False while `enabled` is False --
        SPEC SS12 makes this config, default false, enabled on the i7
        only, so a runner that has not opted in never even reads the
        lease ref."""
        if not self.enabled:
            return False
        verdict = self.read_lease() if verdict is None else verdict
        self.last_observation = verdict
        now = self._clock()
        if verdict == "unreadable":
            return self.autonomous  # frozen -- see the class docstring
        if verdict == "live":
            self.down_since = None
            self.live_streak += 1
            if self.autonomous and self.live_streak >= self.exit_after_live:
                self.autonomous = False
                self._log(
                    f"AUTONOMY OFF: {SERVER_LEASE_REF} live for "
                    f"{self.live_streak} consecutive observations -- resuming "
                    f"agent dispatch and the {LOOP_SECONDS}s loop")
            return self.autonomous
        # DOWN.
        self.live_streak = 0
        if self.down_since is None:
            self.down_since = now
        elif not self.autonomous and (now - self.down_since) > self.enter_after_s:
            self.autonomous = True
            self._log(
                f"AUTONOMY ON: {SERVER_LEASE_REF} absent or expired for "
                f"{now - self.down_since:.0f}s (> {self.enter_after_s:.0f}s) -- "
                f"gates only, no agent dispatch, {AUTONOMOUS_INTERVAL_S:.0f}s loop")
        return self.autonomous


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
    autonomous_when_serverless: bool = False,
    autonomous_interval: float = AUTONOMOUS_INTERVAL_S,
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

    KEEL 3R-2 NARROWED rc 5 and did not remove it. Adoption is now
    `journal.adopt_at_startup`, which falls back to the local job journal
    when the store answers neither route instead of refusing to start, so
    the ordinary "both routes down" case no longer produces rc 5 at all --
    it produces an offline runner that adopts what it journaled, sweeps
    nothing, and starts nothing until a reconcile step completes. rc 5
    remains for the case where that call ITSELF fails.

    NOT FIXED HERE, and worth knowing before reading the above as more
    than it is: `run_daemon` takes the host singleton BEFORE adoption, and
    that is a CAS against the store. A store that answers NOTHING raises
    `HubUnreachableError` out of `Claim.acquire_or_reap` -- an exception,
    not one of the codes above -- before any of the adoption path runs. So
    SPEC SS5.3's "both routes unreachable at start" is only partly served
    by this stage. Making the singleton survive an unreachable store is a
    separate change with its own argument to settle, because holding it is
    what entitles adoption to touch this host's claims at all and an
    offline runner cannot establish that by CAS.
    `tests/test_journal.py::TestOfflineStartThroughRunDaemon` pins both
    halves: the narrowed rc 5, and the singleton raise that still stands.

    One addition to fleetd's original body, between adoption and the
    loop: L1's `check_toolchain_agreement`. It exits **rc 7** when this
    runner's `platform_id` differs from the one its own gate command
    computes -- a code of its own, never 6, because the two conditions
    need opposite supervisor behaviour (a hub comes back; a mismatched
    toolchain install does not until a human fixes it). See
    `TOOLCHAIN_MISMATCH_RC`.

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
    #
    # Keel 3R-2 step 2. This used to be `adopt_workers(hub, host, workers)`
    # under `except HubError: singleton.release(); return 5`, and rc 5 was
    # "both routes are down, so refuse to start rather than run with an
    # empty worker list". The reasoning was right and the dichotomy was
    # false: "run with an empty worker list" was only ever the alternative
    # because the store was the only place this runner had written down
    # what it launched. `journal.adopt_at_startup` is the same call with
    # the local journal as the second route -- the store is still truth
    # whenever it answers (`mode == "store"`, the hub claim wins and the
    # journal contributes nothing), and only a `HubError` from the hub
    # pass -- which through a `FallbackHub` means BOTH routes failed --
    # makes the journal decisive.
    #
    # rc 5 SURVIVES, narrowed to its honest case: `adopt_at_startup`
    # itself raising. It handles `HubError` internally, so anything that
    # still escapes is a different failure from "the store is away" and
    # the old refusal is the right answer to it.
    #
    # `markers`/`scope_token`/`ttl`/`renew_interval` are deliberately NOT
    # passed: `adopt_at_startup` forwards them only when given, and both
    # halves derive the same production defaults themselves
    # (`worker_markers()`, `fleet_scope_token(hub.url)`). Passing them
    # would add a second place for the derivation to drift.
    #
    # ONE `Journal` object for this daemon, built here and handed to both
    # halves. `adopt_at_startup` and `release_pending` address the same
    # root either way (`Journal()` resolves `$KEEL_HOME` at construction),
    # but `release_pending` only writes the `exit` that CLOSES a released
    # job when it is given a journal at all -- its `journal=` parameter
    # defaults to None, and the whole `_close` path sits behind
    # `if journal is not None`. Calling it without one released the ref on
    # the store and left the job in `open_jobs` forever: re-read as owed
    # work by every later startup pass, and never collected by `prune`,
    # which is the exact failure `fleetd._journal_close`'s docstring
    # argues must not happen.
    jn = journal_mod.Journal()
    try:
        adoption = journal_mod.adopt_at_startup(hub, host, workers, journal=jn)
        print(f"{label}[{host}] adoption: {adoption.summary()}", flush=True)
    # `ProcessListingUnavailable` joins the two store/journal failures for
    # the reason the message below already gives. Adoption's whole job is
    # to decide which of this host's process groups are still running
    # work; with `ps` refusing to answer there is no evidence for that
    # decision at all, and the alternatives are to adopt nothing (a
    # duplicate of everything still running, once the first reconcile sees
    # empty slots) or to sweep on a guess. rc 5 -- refuse to start, drop
    # the host lease, let the supervisor retry -- is exactly the answer
    # this clause was written for, and `ps` failures are transient.
    except (HubError, journal_mod.JournalError, ProcessListingUnavailable) as e:
        print(f"{label}[{host}]: cannot rebuild worker state from the hub or the "
              f"journal ({type(e).__name__}: {e}); refusing to start rather than "
              f"risk duplicate work", file=sys.stderr)
        singleton.release()
        return 5

    # Keel 3R-2 step 3. `spawn_allowed` is False for exactly as long as
    # this runner has never had an answer from the store -- see
    # `StartupAdoption.spawn_allowed`, which is False whenever
    # `mode == "journal"`. Starting work whose claim cannot be
    # CAS-arbitrated is the duplicate-gate hazard leases exist to prevent:
    # an offline runner cannot ask whether another host already holds the
    # branch, so every start it makes is a coin flip it is not entitled to
    # toss. It is a LOOP variable, not a constant, because the store
    # coming back is exactly what re-entitles it -- see the reconnect
    # block below.
    spawn_allowed = adoption.spawn_allowed
    # The owed CAS-deletes `adopt_from_journal` recorded but had no route
    # to perform. Carried so the reconnect block can hand it to
    # `journal.release_pending`, which re-verifies each one against the
    # store before deleting. None on the store path: there is nothing owed
    # when the store answered.
    owed_releases = adoption.journal_result
    if not spawn_allowed:
        print(f"{label}[{host}] OFFLINE START: the store answered neither route; "
              f"adopted {len(workers)} worker(s) from the local journal and will "
              f"start nothing until it answers again", file=sys.stderr, flush=True)

    # T3: one warning store for the daemon's whole lifetime, so a warning
    # survives every reconcile until its marker file is gone. Owned by
    # `run_daemon` rather than module-global so two daemons in one process
    # (the test suite) cannot bleed into each other. Built HERE, ahead of
    # the L1 check, so a toolchain condition noted before the first
    # reconcile is carried into every heartbeat afterwards.
    host_warnings = HostWarnings()

    # L1 (Keel Stage 1 LIVE, 2026-08-27/28): refuse to schedule work whose
    # results this scheduler could not read. See `check_toolchain_agreement`
    # for the incident and for why "could not verify" is a warning rather
    # than a refusal.
    #
    # AFTER adoption, not before, and the ordering is load-bearing in the
    # other direction from the singleton's. This check spawns `rustc -vV`
    # and a shell; adoption is racing a predecessor's gate claims against
    # their TTL, and a second of avoidable startup latency there is a live
    # gate reported as an orphan and killed. Nothing this check protects
    # against is urgent enough to pay for that: a mismatched host has been
    # writing unreadable verdicts for however long it took to notice, so
    # one more reconcile's worth is free. Refusing here still costs the
    # host lease, exactly like the adoption failure above -- a host that
    # will not run must not hold the lease that stops a fixed peer from
    # taking over.
    may_start, toolchain_msg = check_toolchain_agreement(gate_command, host, host_warnings)
    if toolchain_msg:
        print(f"{label}[{host}] TOOLCHAIN: {toolchain_msg}", file=sys.stderr, flush=True)
    if not may_start:
        # NOT rc 6 -- see TOOLCHAIN_MISMATCH_RC. rc 6 is "the hub is
        # unusable", which a supervisor should retry; this one cannot
        # clear without a human touching the install.
        print(f"{label}[{host}]: refusing to start on a toolchain-id mismatch",
              file=sys.stderr, flush=True)
        singleton.release()
        return TOOLCHAIN_MISMATCH_RC

    stop = {"flag": False}

    # SPEC SS5.3 step 1. `server_client` is None on a hubless Stage-1
    # runner, which is the normal case and not an error; everything below
    # is then a no-op and this runner behaves exactly as it did before.
    #
    # DEFERRED BY CHOICE: the first attempt is made inside the loop, after
    # the first reconcile, not here between the toolchain gate and the
    # loop. `ServerHub`'s per-call budget is a 5 s connect plus a 20 s
    # read, and this point in `run_daemon` is the one the comment above
    # `check_toolchain_agreement` argues hardest about -- adoption has
    # just raced a predecessor's claims against their TTL and the first
    # reap has not happened yet, so up to 25 s spent here is a live gate
    # reported as an orphan and killed. Registration is idempotent by
    # `{id}` and its retry policy is this loop's own cadence, so one
    # cycle's delay costs nothing that a failed first attempt would not
    # have cost anyway. Building the state here (not in the loop) keeps
    # the loop body free of setup.
    reg_client = server_client(hub)
    reg_session: dict = {}
    # The registration ladder's state, owned by this loop and by nothing
    # else. A dict rather than a nonlocal so `register_cycle` stays a
    # plain function that a test can drive one cycle at a time.
    reg_backoff: dict = {}
    reg_scope_token = fleet_scope_token(hub.url)

    def _reg_log(msg: str) -> None:
        print(f"{label}[{host}] {msg}", file=sys.stderr, flush=True)

    # Keel 3R-2 steps 9-10. Constructed unconditionally so the loop body
    # has no `if` around it, but INERT unless the operator opted in:
    # `AutonomyGate.observe` returns False without reading anything while
    # `enabled` is False, so a runner that has not set
    # `autonomous_when_serverless` behaves exactly as it did before this
    # stage -- no extra ref read, no interval change, no dispatch change.
    autonomy = AutonomyGate(hub, enabled=autonomous_when_serverless, log=_reg_log)

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
            # THE VERDICT THIS CYCLE ACTS ON IS THE ONE ALREADY OBSERVED,
            # and the observation for the next cycle is taken AFTER the
            # reconcile step, below. `autonomy.observe()` reads
            # `SERVER_LEASE_REF` off the store, and `reconcile_once`'s step
            # (1) -- the reap and the lost-lease kill -- is documented as
            # running with "no hub call before it" for a reason its own
            # docstring spells out: a lease goes LOST because its renewal
            # push failed, and renewals fail for the same reason reads do.
            # Taking the observation first put up to a full `run_git`
            # timeout (30 s, and 5 s connect + 20 s read more through a
            # `FallbackHub`'s primary) in front of the kill, EVERY cycle,
            # under exactly the store outage that arms the feature.
            #
            # The cost is one cycle of lag in the dispatch verdict, and it
            # is immaterial against the gate's own hysteresis:
            # `AUTONOMY_ENTER_AFTER_S` requires 60 s of consecutive DOWN
            # observations before autonomy turns on and
            # `AUTONOMY_EXIT_LIVE_OBSERVATIONS` several LIVE ones before it
            # turns off, so a 15 s cycle's lag cannot change which side of
            # either threshold this host is on. `register_cycle` is
            # deferred one cycle for the same reason and says so.
            autonomous = autonomy.autonomous
            try:
                res = reconcile(hub, host, workers, gate_command, log_dir,
                                repo_root, warnings=host_warnings,
                                # Keel 3R-2 steps 4 and 10. Two SEPARATE
                                # permissions, deliberately not collapsed:
                                # `spawn_allowed` is "can any start of mine
                                # be CAS-arbitrated at all", and
                                # `agents_allowed` is "may this host spend
                                # money on an agent while nothing is
                                # coordinating the fleet". An autonomous
                                # host still gates (SPEC SS12: never less
                                # capable than hubless Stage 1); it just
                                # never dispatches.
                                spawn_allowed=spawn_allowed,
                                agents_allowed=not autonomous)
            except HubError as exc:
                degraded = exc
                hub_failures += 1
            else:
                hub_failures = 0
                # Keel 3R-2 step 7. A step that completed without a
                # `HubError` is the only proof this runner gets that a route
                # to the store is back, so it is where the owed CAS-deletes
                # are attempted -- EVERY such step while anything is still
                # owed, not just the first. `release_pending` turns a store
                # that is still away into an outcome string and leaves the
                # entry owed "for the next attempt"; a caller that attempts
                # exactly once makes that promise vacuous, and the entry
                # would then sit until its TTL with nothing ever retrying.
                #
                # Do NOT shortcut the re-verification inside it. It re-reads
                # each ref and requires BOTH halves of `claim._owns`'
                # ownership token -- `holder_host` is us AND `started_at`
                # matches the journaled text -- before deleting, precisely
                # because this host may have legitimately re-acquired the
                # branch in the interval (its own next runner reaping an
                # expired claim, an autonomous gate). Half the token is not
                # enough and the cost of getting it wrong is deleting a live
                # gate's lease.
                if owed_releases is not None and owed_releases.to_release:
                    for ref, outcome in journal_mod.release_pending(
                            hub, host, owed_releases, journal=jn):
                        print(f"{label}[{host}] OWED RELEASE {ref}: {outcome}",
                              file=sys.stderr, flush=True)
                if not spawn_allowed:
                    # THE STORE JUST ANSWERED, so the reason `spawn_allowed`
                    # was False no longer holds: every start from here is
                    # CAS-arbitrated again.
                    #
                    # Re-arming does not WAIT for the releases above to have
                    # all landed. An entry that stays owed blocks exactly one
                    # branch until its TTL expires; it does not make a start
                    # on any OTHER branch unsafe, because with the store up
                    # every start goes through `acquire_or_reap`. Conflating
                    # the two would let one undeletable ref stop this host
                    # gating forever, which is a strictly worse failure than
                    # one branch waiting out a lease.
                    spawn_allowed = True
                    print(f"{label}[{host}] STORE BACK: a reconcile step completed "
                          f"against {hub.url}; starts are CAS-arbitrated again",
                          file=sys.stderr, flush=True)
                line = (
                    f"{label}[{host}] gates={len(workers)} started={res.started} "
                    f"finished={res.finished} killed={res.killed} refused={res.refused} "
                    + (f"warnings={res.warnings} " if res.warnings else "")
                    + (f"autonomous=1 " if autonomous else "")
                    + f"hb={res.heartbeat_written}"
                )
                print(line, flush=True)

            # The observation for the NEXT cycle, taken here rather than at
            # the top of the loop -- see the note above `autonomous =
            # autonomy.autonomous`. It never raises: `read_lease` turns a
            # `HubError` into the "unreadable" verdict, which freezes the
            # gate rather than moving it. Inert (no ref read at all) unless
            # the operator set `autonomous_when_serverless`.
            autonomy.observe()

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

            # Registration rides this loop and nothing else -- after both
            # log lines above, so a DEGRADED cycle re-registers too (a
            # server that just came back is exactly the cycle we most want
            # to announce ourselves on). `register_cycle` never raises and
            # never touches `res`, `degraded` or `hub_failures`: the
            # reconcile verdict for this step is already decided and
            # printed above, and an announcement must not be able to
            # change it.
            register_cycle(
                reg_client, host, reg_session,
                lambda: registration_payload(host, workers, repo_root, reg_scope_token),
                _reg_log,
                reg_backoff,
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
            # Keel 3R-2 step 10: the autonomous cadence is a MINUTE, not
            # `LOOP_SECONDS`. It is a PARAMETER rather than a computation
            # over `interval` -- `max(interval, 60)` would make the suite's
            # `interval=0` sleep a full minute, and `min` would leave the
            # production default at 15 s and change nothing at all. There
            # is no arithmetic that means both; the two cadences are two
            # numbers, so they are two arguments.
            time.sleep(autonomous_interval if autonomous else interval)
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
        # SPEC SS12: config, DEFAULT FALSE. `cfg.autonomous_when_serverless`
        # is `None` when nothing configured it, and `bool(None)` is the
        # production default rather than a coincidence -- the loader keeps
        # "unset" and "false" distinct precisely so this line can be the
        # one place that collapses them, and say so.
        autonomous_when_serverless=bool(cfg.autonomous_when_serverless),
    )


if __name__ == "__main__":
    raise SystemExit(main())
