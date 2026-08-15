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

`reconcile_once()` is a pure step function so tests can drive it against
a fixture hub with a stub gate command; `main()` is just the loop.
"""

from __future__ import annotations

import argparse
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
from typing import Callable, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))

import claim as claim_mod
import verdict
import workqueue
from claim import Claim, compute_platform_id, compute_rustc_id
from fleetlib import Hub, HubError

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

# An agent invocation is a PAID claude/codex run. A branch whose agent
# made no progress must not be retried every loop: 30min cooldown,
# in-memory (a fleetd restart forgetting it is acceptable).
AGENT_RETRY_COOLDOWN_S = int(os.environ.get("FLEET_AGENT_COOLDOWN_S", "1800"))
_agent_attempts: dict = {}

# Seconds between SIGTERM and SIGKILL when tearing down a worker whose
# lease was lost. Short by design: the window we are closing is "two hosts
# running the same gate", and every second of grace is a second of that.
KILL_GRACE_S = float(os.environ.get("FLEET_KILL_GRACE_S", "10"))


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
    """Tear a worker down by process group and drop its claim."""
    outcome = kill_process_group(w.pgid, grace=grace, alive_probe=lambda _p: w.alive())
    if w.popen is not None:
        try:
            w.popen.wait(timeout=5)  # reap, so it cannot linger as a zombie
        except subprocess.TimeoutExpired:
            outcome += " (child not reaped within 5s)"
    # Safe either way: `release` is a CAS delete, so if the lease really
    # is somebody else's now, this cannot touch their claim.
    w.claim.release()
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
    # holder_host=host, not Claim's own socket.gethostname() default: `host`
    # is this daemon's FLEET identity (host_identity(), FLEET_HOST-
    # overridable -- the work2 pod's real hostname is a k8s-generated
    # `work2box-<hash>`) and is what every other hub write this daemon
    # makes (the heartbeat ref, HOSTS_PREFIX + host) is keyed by. Leaving
    # it to default would record a claim under the machine's raw hostname
    # while everything else about this host is filed under `host` -- e.g.
    # `fleet status`'s WORK column (ARCH-FIX R4), which joins claims to
    # heartbeat rows on this exact field, would never find a match.
    c = Claim(
        hub, kind="gate", key=branch.replace("/", "-"), work_kind="gate", work_key=branch,
        holder_host=host,
    )
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
        popen = subprocess.Popen(
            gate_command + [branch, tag],
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
              work_kind="agent", work_key=branch, holder_host=host)
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
             *mode_args, "--hub", hub.url, "--host", host],
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
    awaiting_train: list = field(default_factory=list)  # branches -- PASS memo, ARCH-FIX R4
    needs_author: list = field(default_factory=list)  # branches -- FAIL memo, ARCH-FIX R4
    heartbeat_written: bool = False
    tip_generation: Optional[int] = None


# --------------------------------------------------------------------- #
# Verdict-aware selection (ARCH-FIX R4 point 2)
# --------------------------------------------------------------------- #
#
# Before offering a branch as gate work: a branch whose merge-onto-tip
# already gated PASS is waiting on the train, not on another gate run; one
# that already gated FAIL needs its author, not a retry loop paying the
# same gate cost for the same answer. The spec's ideal is a lookup against
# the shared verdict cache (verdict.py) keyed on the TRUE branch-onto-tip
# merge tree -- but computing that merge just to ask the question is
# exactly the per-loop cost this exists to avoid. The compromise:
#
#   1. `_scan_gatelogs_memo` rebuilds an in-memory recall of THIS host's
#      own recent gate.sh runs from `~/gatelogs/gate-*.json` -- files
#      gate.sh already writes for every run (its `write_json`, unchanged
#      here). Local disk only, no network. Rescanned every reconcile loop,
#      so "recall after a restart" and "stay current within one run" are
#      the same code path, not two.
#   2. When a memo hit names a `tree_sha` (the merge gate.sh actually
#      built and gated), `_confirm_via_shared_cache` spends one lookup
#      against verdict.py's real hub-backed cache for that exact tree --
#      the genuine cross-host answer, reusing a tree this host already
#      paid to compute rather than recomputing one pre-claim. A hub hiccup
#      here falls back to the local recall; it must never block dispatch.
#
# Keyed by (branch, base_tip), NOT (branch_sha, tip_sha): gate.sh's JSON
# verdict records the branch's NAME and the tip it gated against, but never
# the branch's own commit sha at gate time (see gate.sh's write_json), so a
# sha-precise key cannot be reconstructed from what gate.sh writes today.
# Consequence: if a branch is force-pushed to a new commit while the tip
# has not moved, a stale verdict for its previous commit could shadow one
# loop's worth of selection for the new one, until this host's next gate of
# that branch overwrites the memo entry. This is a pre-claim FILTER only --
# gate.sh's own verdict cache, keyed on the true merged TREE_SHA, remains
# the real authority and is re-consulted (against a real merge) inside
# every gate.sh run regardless, so the exposure is "one extra loop of not
# re-offering it," never a wrong tree landing.

GATELOGS_GLOB = "gate-*.json"
AWAITING_TRAIN = "awaiting_train"
NEEDS_AUTHOR = "needs_author"


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


def _confirm_via_shared_cache(hub: Hub, entry: dict) -> Optional[str]:
    """Best-effort confirmation of a locally-recalled verdict against
    verdict.py's shared hub cache, reusing the TREE_SHA the local JSON
    already recorded rather than recomputing a merge. None if the hub
    can't answer (unreachable, nothing cached for that exact tree, or the
    entry is missing an identity field) -- the caller falls back to the
    local result; a miss or hiccup here must never block gate dispatch."""
    tree_sha = entry.get("tree_sha")
    gate_version = entry.get("gate_version")
    platform_id = entry.get("platform_id")
    if not (tree_sha and gate_version and platform_id):
        return None
    try:
        payload = verdict.lookup(hub, tree_sha, gate_version, platform_id)
    except HubError:
        return None
    if payload is None:
        return None
    return payload.get("result")


def classify_branch(
    memo: dict,
    hub: Hub,
    branch: str,
    tip_sha: str,
    gate_version: Optional[str],
    confirm_with_hub: bool = True,
) -> Optional[str]:
    """AWAITING_TRAIN | NEEDS_AUTHOR | None for `branch` gated onto
    `tip_sha`, per the memo `_scan_gatelogs_memo` built. None means no
    opinion -- offer it as ordinary gate work.

    A memo entry recorded under a different `gate_version` than the one
    this host currently runs is ignored, not honoured: GATE_VERSION only
    bumps when gate BEHAVIOUR changes, so an old-version verdict is not
    evidence about what the current gate would say.
    """
    entry = memo.get((branch, tip_sha))
    if entry is None:
        return None
    if gate_version is not None and entry.get("gate_version") != gate_version:
        return None
    result = entry.get("result")
    if confirm_with_hub:
        confirmed = _confirm_via_shared_cache(hub, entry)
        if confirmed is not None:
            result = confirmed
    if result == verdict.RESULT_PASS:
        return AWAITING_TRAIN
    if result == verdict.RESULT_FAIL:
        return NEEDS_AUTHOR
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
    reasons argued inline below."""
    res = ReconcileResult()

    desired_doc = hub.read(DESIRED_REF) or {}
    my_desired = (desired_doc.get("hosts") or {}).get(host) or {}
    limits = desired_doc.get("limits") or {}
    want_gates = int(my_desired.get("gates") or 0)
    enabled = bool(my_desired.get("enabled", False))

    tip_sig = hub.read(TIP_SIGNAL_REF) or {}
    res.tip_generation = tip_sig.get("generation")

    # Reap finished/dead workers. A worker whose process group is gone
    # releases its claim here; a crashed fleetd's claims expire on their
    # own (LEASE_TTL) and are reaped by any host via claim.reap_expired.
    pgids = pgid_probe()
    for w in list(workers):
        if not w.alive(pgids):
            w.claim.release()
            workers.remove(w)
            res.finished.append(w.tag)
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

    running = len([w for w in workers if w.kind == "gate"])

    if not enabled:
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
                # branch the local gatelogs recall (confirmed, best-effort,
                # against the shared hub cache) already knows the answer
                # for -- PASS is waiting on the train, FAIL needs its
                # author. `tip_sha` gates the memo lookup on `hub.sha`
                # rather than `res.tip_generation` because the memo's key
                # is the tip's SHA (what gate.sh actually recorded as
                # `base_tip`), not its generation counter.
                tip_sha = hub.sha(workqueue.TIP_REF)
                gate_version = _gate_version(repo_root)
                memo = _scan_gatelogs_memo(log_dir) if tip_sha else {}

                candidates = []
                for s in q:
                    b = _branch(q[s])
                    if any(w.branch == b for w in workers):
                        continue
                    status = classify_branch(memo, hub, b, tip_sha, gate_version) if tip_sha else None
                    if status == AWAITING_TRAIN:
                        res.awaiting_train.append(b)
                        continue
                    if status == NEEDS_AUTHOR:
                        res.needs_author.append(b)
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
    gate_workers = [w for w in workers if w.kind == "gate"]
    if enabled and want_agents > len(agent_workers):
        try:
            import agentworker as _aw
            has_cli = bool(_aw.available_clis())
        except Exception:
            has_cli = False
        if not has_cli:
            res.refused.append(("no-agent-cli", "neither claude nor codex on this host"))
        else:
            q = workqueue.Queue(hub).compute()
            busy = {w.branch for w in workers}
            now = time.time()
            todo = [b for b in (q[s2].ref.removeprefix("refs/heads/") for s2 in q)
                    if b not in busy
                    and now - _agent_attempts.get(b, 0) > AGENT_RETRY_COOLDOWN_S]
            # No stale branch to converge -> AUTHOR from the intent backlog:
            # open intents with no staging branch yet, no live agent claim,
            # and not in cooldown. Prefixed "intent:" so start_agent knows.
            if not todo:
                for iref, _sha in hub.list("refs/fleet/intents/").items():
                    slug = iref.rsplit("/", 1)[-1]
                    doc = hub.read(iref) or {}
                    if doc.get("status") != "open":
                        continue
                    if hub.sha(f"refs/heads/staging/{slug}") is not None:
                        continue  # branch exists; convergence path owns it
                    key = f"intent:{slug}"
                    if key in busy or now - _agent_attempts.get(key, 0) <= AGENT_RETRY_COOLDOWN_S:
                        continue
                    todo.append(key)
            for branch in todo[: want_agents - len(agent_workers)]:
                tag = f"{host}-a-{branch.split('/')[-1].removeprefix('intent:')}-{int(time.time()) % 100000}"
                try:
                    w = start_agent(hub, branch, tag, host, log_dir, repo_root)
                except OSError as e:
                    res.refused.append(("agent-spawn-failed", f"{branch}: {e}"))
                    continue
                if w is None:
                    res.refused.append(("agent-claimed-elsewhere", branch))
                    continue
                workers.append(w)
                _agent_attempts[branch] = time.time()
                res.started.append(tag)

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
    res.heartbeat_written = write_heartbeat(hub, host, hb)
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
    # holder_host=host: same reasoning as start_gate/start_agent above --
    # `host` is the FLEET_HOST-overridable fleet identity, not necessarily
    # this machine's raw socket.gethostname().
    singleton = Claim(hub, kind="host", key=host, work_kind="fleetd", work_key=host, holder_host=host)
    try:
        # acquire_or_reap: a hard-killed predecessor (launchctl kickstart -k,
        # OOM, crash) never runs its graceful release, and a plain acquire
        # then locks the host out until the claim is manually reaped -- m5
        # spent 20 minutes in a KeepAlive spawn/refuse/exit loop this way.
        # A LIVE predecessor still refuses (the singleton guard stands).
        singleton.acquire_or_reap()
    except claim_mod.ClaimHeldError:
        print(f"fleetd: another instance holds refs/fleet/claims/host/{host}; exiting")
        return 3

    stop = {"flag": False}

    def _sigterm(_sig, _frm):
        stop["flag"] = True

    signal.signal(signal.SIGTERM, _sigterm)
    signal.signal(signal.SIGINT, _sigterm)

    rc = 0
    try:
        while True:
            res = reconcile_once(hub, host, workers, gate_command, Path(args.log_dir), repo_root)
            line = (
                f"fleetd[{host}] gates={len(workers)} started={res.started} "
                f"finished={res.finished} killed={res.killed} refused={res.refused} "
                f"hb={res.heartbeat_written}"
            )
            print(line, flush=True)
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
            if args.once or stop["flag"]:
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
