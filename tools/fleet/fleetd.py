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
import json
import os
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Optional, Sequence

sys.path.insert(0, str(Path(__file__).resolve().parent))

import agentworker as agentworker_mod
import dispatch as dispatch_mod
import verdict
import workqueue
from claim import compute_platform_id, compute_rustc_id
from fleetlib import Hub, HubError, HubUnreachableError

# PLAN Stage 3 task 1 (SPEC §9 "fleetd.py" row): the LOCAL half of this
# daemon -- process ownership, spawns, kills, adoption, the host
# singleton, host warnings, heartbeat writing, and the daemon shell --
# now lives in `keel/runner.py` (keel-runner, the process the `units/*`
# supervisors launch). fleetd.py remains the deployed entry point for
# now and RE-EXPORTS every moved name unchanged, so every existing
# consumer and test keeps addressing them as `fleetd.<name>`; `main`
# below delegates its post-argparse body to `keel.runner.run_daemon`.
# The SELECTION half (`reconcile_once`, `classify_branch`,
# `dispatch_agents`, the verdict-aware machinery) stays HERE until PLAN
# Stage 4 moves it into `keel/scheduler.py` -- keel/runner.py's own
# module docstring carries the full moved-vs-shared inventory.
from keel.runner import (  # noqa: E402,F401 -- re-exported, see above
    ALLOW_TOOLCHAIN_MISMATCH_ENV,
    DAEMON_MARKERS,
    FLEETD_MARKER,
    FLEET_SCOPE_PREFIX,
    HOSTS_PREFIX,
    KILL_GRACE_S,
    LOOP_SECONDS,
    PUSH_BACKOFF_S,
    PUSH_RETRIES,
    RECONCILE_HUB_FAILURE_LIMIT,
    RUNNER_MARKER,
    TOOLCHAIN_MISMATCH_RC,
    TOOLCHAIN_MISMATCH_WARNING,
    TOOLCHAIN_UNVERIFIED_WARNING,
    WORKER_MARKERS,
    AdoptionResult,
    HostWarnings,
    Worker,
    _exiftool_cache_dir,
    _gate_version,
    _limits_ok,
    _oracle_ok,
    _pgid_alive,
    _ps_env,
    _release_claim_ref,
    _scoped_worker_in_group,
    _spawn_env,
    _verdict_store_failed_marker,
    adopt_workers,
    check_toolchain_agreement,
    default_gate_command,
    fleet_scope_token,
    fleet_worker_pgids,
    fleetd_marker_in_group,
    free_disk_gb,
    free_mem_gb,
    gate_toolchain_ids,
    host_identity,
    kill_process_group,
    kill_worker,
    live_pgids,
    live_workers_payload,
    owning_user,
    reap_dead_same_host_singleton,
    session_of,
    singleton_ttl_s,
    start_agent,
    start_gate,
    worker_markers,
    write_heartbeat,
)
import keel.runner as keel_runner  # noqa: E402
from keel import journal as journal_mod  # noqa: E402 -- 3R-2 step 6, the reap's `exit` record
from keel.fallbackhub import FallbackHub  # noqa: E402 -- heartbeat `fallback` block

# --------------------------------------------------------------------- #
# Constants (FLEET_PLAN.md "Shared contracts" is the authority)
# --------------------------------------------------------------------- #

HEARTBEAT_STALE = 180  # seconds; older than this renders DOWN in `fleet status`
DESIRED_REF = "refs/fleet/desired"
TIP_SIGNAL_REF = "refs/fleet/signals/tip"


# An agent invocation is a PAID claude/codex run, and the record of what
# has already been bought is a HUB REF, not a dict in this process
# (ARCH-FIX-SPEC.md R5). The `_agent_attempts` dict that used to live here
# was reset by every restart -- and this daemon restarted roughly hourly --
# so its 30-minute cooldown was, in practice, no cooldown at all. See
# `dispatch.py` for the ledger, the cap and the cooldown, all derived from
# `refs/fleet/attempts/<key>`.


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
    # T3: DURABLE host conditions, unlike `refused` above which is this
    # loop's scheduling answer. Re-derived every reconcile by
    # `HostWarnings.scan` from the marker files themselves, so an entry
    # survives every loop until the file is gone. Carried into the
    # heartbeat as `warnings` and rendered by `fleet status --why`.
    warnings: list = field(default_factory=list)  # (reason, detail)


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


def dispatch_agents(
    hub: Hub,
    host: str,
    workers: list,
    slots: int,
    log_dir: Path,
    repo_root: Path,
    res: "ReconcileResult",
    journal: Optional["journal_mod.Journal"] = None,
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
    tip_sha = hub.code_sha(workqueue.TIP_REF)  # CODE ref -- SPEC 4.4
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
    queue, queue_refusal = workqueue.Queue(hub).compute_or_refusal()
    if queue_refusal is not None:
        res.refused.append(queue_refusal)
    for slug, entry in queue.items():
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
        if hub.code_sha(f"refs/heads/staging/{slug}") is not None:  # CODE ref
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
            w = start_agent(hub, branch, tag, host, log_dir, repo_root,
                            journal=journal)
        except (OSError, journal_mod.JournalError) as e:  # see start_gate's note
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
    # T5: the agent DID the work and the push of it failed to
    # authenticate. Its own outcome rather than "no-progress" (7), because
    # it is a host credential condition with a named fix and no amount of
    # re-buying the branch can clear it -- the ledger reads the two very
    # differently.
    agentworker_mod.RC_PUSH_AUTH_FAILED: "push-auth-failed",
}


def _journal_close(jn, w, *, rc: Optional[int], outcome: Optional[str],
                   host: str) -> None:
    """Close `w`'s journal job at reap (Keel 3R-2 step 6).

    WHY THIS IS NOT OPTIONAL. Without the `exit` record the job stays in
    `JournalScan.open_jobs` forever, so every subsequent startup pass
    re-reads a finished gate as live work: its pgid is gone, which makes
    it an OWED RELEASE on every offline start until the file is removed,
    and `Journal.prune` -- which collects CLOSED jobs after seven days --
    never collects it, so the directory grows without bound. The record
    is what turns "this job existed" into "this job is over".

    WHY A FAILURE HERE IS NOT FATAL, unlike at spawn. `JournalWriteError`
    at spawn time means a process would exist that nothing wrote down, so
    the caller must not spawn. Here the process is already GONE and its
    claim has already been released against the store; an unwritable
    journal cannot un-reap it, and raising would take the rest of this
    loop -- including other workers' lost-lease kills -- down with it.
    Loud and continue is the only defensible direction.

    A worker with no `job_key` is simply not journaled (one built before
    this stage, or by a test that does not care) and is skipped.
    """
    if jn is None or getattr(w, "job_key", None) is None:
        return
    try:
        jn.exit(job_key=w.job_key, rc=rc, outcome=outcome)
    except journal_mod.JournalError as exc:
        print(f"fleetd[{host}] journal exit record failed for {w.tag} "
              f"({type(exc).__name__}: {exc}); the reap itself stands, but this "
              f"job stays open in the journal and will be re-read as owed work",
              file=sys.stderr, flush=True)


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
    warnings: Optional["HostWarnings"] = None,
    spawn_allowed: bool = True,
    agents_allowed: bool = True,
    journal: Optional["journal_mod.Journal"] = None,
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

    `spawn_allowed=False` (Keel 3R-2 step 4) short-circuits step 3 ONLY.
    Steps 1 and 2 run in full, and that is the whole point: the caller
    passes False when the store never answered at startup and this
    runner's workers were adopted from the local journal, which is
    precisely the state in which the lost-lease kill matters MOST -- a
    journal-rebuilt claim whose renewer cannot reach the store will mark
    itself lost, and the kill that follows is the only thing standing
    between this host and a duplicate gate. Disarming step 1 alongside
    step 3 would turn a conservative "start nothing" into "start nothing
    and stop nothing", which is strictly worse than the rc-5 refusal this
    replaced. `agents_allowed=False` suppresses only the agent half
    (SPEC SS12's autonomous host gates but never dispatches); gates still
    start, because an autonomous host that cannot gate is less capable
    than today's hubless Stage 1, which SPEC forbids.

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
    # One journal for the whole step, so `start_gate`/`start_agent`/the
    # reap all address the same root. `Journal()` resolves `$KEEL_HOME`
    # at construction (see `journal.default_root`), which is what lets a
    # hermetic fixture redirect it.
    jn = journal if journal is not None else journal_mod.Journal()

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
            # The rc this runner can honestly report. An ADOPTED worker
            # (hub- or journal-adopted) is not our child, so there is no
            # `Popen` and no exit status: `None` is recorded rather than a
            # guess, exactly as the agent-outcome block below already does
            # with "unknown-adopted".
            reaped_rc = w.popen.returncode if w.popen is not None else None
            if w.kind == "gate":
                # R4: gate.sh's `store_verdict()` swallows a hub-push
                # failure so its own PASS/FAIL is never wrong because the
                # CACHE was unreachable -- but "non-fatal to this run" was
                # being read as "invisible", and a tokenless-host failure
                # would repeat silently forever. The marker it leaves
                # beside the verdict is the loud half: surface it here, at
                # reap, into THIS loop's `refused[]`, which the heartbeat
                # below carries verbatim (fleet status --why reads it).
                # One-shot by construction -- `w` leaves `workers` on this
                # same pass, so the marker is reported exactly once per
                # gate, not on every loop it happens to still exist.
                marker = _verdict_store_failed_marker(log_dir, w.tag)
                if marker.exists():
                    res.refused.append((
                        "verdict-store-failed",
                        f"{w.tag}: gate.sh could not push its verdict to the hub cache "
                        f"(see gate-{w.tag}.log); the gate's own PASS/FAIL is unaffected",
                    ))
                # NO `journal.verdict` RECORD IS WRITTEN FOR A GATE, and the
                # omission is deliberate. `journal.verdict` records what the
                # GATE stored -- `outcome`/`tree`/`rc` -- and at this point
                # in the step this runner does not know any of the three.
                # `gate.sh` writes its own verdict to the shared cache; the
                # only thing fleetd ever reads it back through is
                # `classify_branch`, keyed by BRANCH and tip sha in the
                # selection phase, not by the worker being reaped here.
                # Deriving a PASS/FAIL from `rc` alone would be an
                # approximation under a real field name -- and a
                # plausible-but-wrong `outcome` in an evidence file is worse
                # than no field at all, because nothing downstream could
                # tell. The `exit` record below carries the rc, which IS
                # known, and stops there.
            reaped_outcome: Optional[str] = None
            if w.kind == "agent":
                # Close the ledger entry this run opened. An ADOPTED worker
                # has no Popen and therefore no exit status -- its outcome
                # is honestly recorded as unknown rather than guessed at,
                # which leaves the count where the dispatch put it (the
                # conservative direction: an unknown run is not evidence of
                # progress).
                rc = reaped_rc
                if rc is None:
                    outcome = "unknown-adopted"
                elif rc == 0:
                    outcome = ("authored" if dispatch_mod.is_intent_key(w.branch)
                               else "converged")
                else:
                    outcome = _AGENT_RC_OUTCOMES.get(rc, f"exit-{rc}")
                # An agent IS a case where the outcome becomes known right
                # here -- it is computed on the line above and written to
                # the durable ledger on the line below -- so the journal
                # gets a `verdict` record as well as the `exit` that closes
                # the job. `_journal_close` explains why neither can raise
                # out of the reap.
                if jn is not None and getattr(w, "job_key", None) is not None:
                    try:
                        jn.verdict(job_key=w.job_key, outcome=outcome, rc=rc)
                    except journal_mod.JournalError as exc:
                        print(f"fleetd[{host}] journal verdict record failed for "
                              f"{w.tag} ({type(exc).__name__}: {exc})",
                              file=sys.stderr, flush=True)
                reaped_outcome = outcome
                _record_outcome(hub, w.branch, host, outcome)
            # EXACTLY ONE `exit` record per reaped worker, outside both
            # kind branches. Inside them it would be written twice for a
            # worker that is somehow both, and NOT AT ALL for one whose
            # `kind` is neither -- and a job with no exit record is one
            # that stays open in the journal forever.
            _journal_close(jn, w, rc=reaped_rc, outcome=reaped_outcome, host=host)
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
            # A killed job is over as surely as a reaped one, and needs the
            # same `exit` record for the same reason: without it the job
            # stays in `open_jobs`, and the next startup pass reads a
            # process group we deliberately SIGKILLed as work owed a
            # release. `rc` is None -- the group was signalled, so there is
            # no exit status this runner observed -- and the outcome names
            # the kill rather than guessing at what the gate would have
            # decided.
            _journal_close(jn, w, rc=None, outcome="killed-lost-lease", host=host)
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
    desired_hosts = desired_doc.get("hosts") or {}
    my_desired = desired_hosts.get(host) or {}
    limits = desired_doc.get("limits") or {}
    want_gates = int(my_desired.get("gates") or 0)
    enabled = bool(my_desired.get("enabled", False))
    # L2 (Keel Stage 1 LIVE, 2026-08-27/28): "this host is not in the
    # desired state at all" is a DIFFERENT fact from "an operator turned
    # this host off", and collapsing them is how the m5 spent the live run
    # reporting `refused: disabled ()` -- the reason `disabled` with an
    # EMPTY detail, because `my_desired` was `{}` and `{}.get("reason")`
    # is None. `fleet status --why` then showed a row named `Allens-Air`
    # (this machine's `hostname -s`) and no `m5` row at all, while
    # `rollout/seed_desired.py` had seeded the host as `m5` and no unit
    # file set `FLEET_HOST`. The operator's read of that screen is "the
    # laptop is deliberately down", which is the opposite of the truth:
    # it was running, enabled, and answering to the wrong name.
    #
    # Same doctrine as `desired_readable` above -- an unread target and a
    # deliberate stand-down must never render as the same line.
    unknown_host = desired_readable and host not in desired_hosts

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

    if not spawn_allowed:
        # Keel 3R-2 step 4. Everything above this line has already run:
        # the reap, the lost-lease kill, and both guarded hub reads. Only
        # the STARTS are refused, and named so `fleet status --why` says
        # which of the refusal reasons this is instead of leaving silence.
        res.refused.append((
            "offline-no-spawn",
            "the store answered neither route at startup, so this runner's workers "
            "were adopted from the local journal; a start whose claim cannot be "
            "CAS-arbitrated is the duplicate-gate hazard leases exist to prevent. "
            "Starts resume on the first reconcile step that completes.",
        ))
    elif not desired_readable:
        pass  # already recorded as hub-unreadable; nothing to start
    elif unknown_host:
        # L2: names the actual defect and the actual fix, because the
        # operator cannot see either from a `disabled` line. The detail
        # carries the name this daemon is using, which is the one thing
        # that has to change.
        known = ", ".join(sorted(desired_hosts)) or "<none>"
        res.refused.append((
            "unknown-host",
            f"{host} not in {DESIRED_REF} (known: {known}); set FLEET_HOST to this "
            f"machine's fleet name, or add {host} to the desired state",
        ))
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
                # B1 (Stage 1 integration review), both halves: the queue
                # asks the CODE repo for the tip (`workqueue.compute` via
                # `hub.code_sha`), and a missing tip comes back as a
                # refusal REASON rather than a `QueueError` -- which is not
                # a `HubError`, so before this it propagated straight out
                # of `reconcile_once`, ahead of every hub-read guard above,
                # and crashed the daemon before its first heartbeat (e.g. a
                # fleetd pointed via `--hub <state-repo>` at a state repo
                # that carries no `refs/heads/*` at all, only
                # `refs/fleet/*`). Degraded exactly like the HUB READS
                # block: nothing starts this loop, the reason is on the
                # record (`queue-unavailable`), the daemon lives to retry
                # next loop.
                q, queue_refusal = workqueue.Queue(hub).compute_or_refusal()
                if queue_refusal is not None:
                    res.refused.append(queue_refusal)
                    q = None

                if q is not None:
                    def _branch(entry):
                        return entry.ref.removeprefix("refs/heads/")

                    # Verdict-aware selection (ARCH-FIX R4): don't offer a
                    # branch the SHARED verdict cache -- or, failing that, this
                    # host's own gatelogs recall -- already knows the answer
                    # for. PASS is waiting on the train, FAIL needs its author.
                    # `tip_sha` gates the lookup on `hub.code_sha` rather than
                    # `res.tip_generation` because both keys are the tip's SHA:
                    # the merge tree the hub cache is keyed on, and the
                    # `base_tip` gate.sh actually recorded in its JSON.
                    # `_TreeResolver` and `platform_id` are built ONCE for the
                    # whole loop -- see the module's "THE COST, BOUNDED" note.
                    tip_sha = hub.code_sha(workqueue.TIP_REF)  # CODE ref -- SPEC 4.4
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

                    if not candidates:
                        # S1 (Stage 1 integration review): targets > 0 but
                        # nothing in the queue is claimable right now --
                        # every branch is either already running here,
                        # awaiting the train, or waiting on its author.
                        # Distinct from "limits" (host-side refusal) and
                        # "queue-unavailable" (the queue itself couldn't compute)
                        # so `fleet status --why` names which of the three
                        # it is instead of leaving silence.
                        detail = f"{len(q)} in queue"
                        if res.awaiting_train:
                            detail += f", {len(res.awaiting_train)} awaiting-train"
                        if res.needs_author:
                            detail += f", {len(res.needs_author)} needs-author"
                        res.refused.append(("queue-empty", detail))

                    for slug in candidates[:deficit]:
                        branch = _branch(q[slug])
                        tag = f"{host}-{slug}-{int(time.time()) % 100000}"
                        try:
                            w = start_gate(hub, branch, tag, gate_command, host, log_dir,
                                           journal=jn)
                        # `JournalError` joins `OSError` because
                        # `JournalWriteError`'s contract is "the caller must
                        # not spawn", and the caller's way of not spawning is
                        # a named refusal for THIS branch -- not an exception
                        # out of `reconcile_once`, which `run_daemon` does not
                        # catch (it catches `HubError` only) and which would
                        # therefore take the whole daemon down over one
                        # unwritable file.
                        except (OSError, journal_mod.JournalError) as e:
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
    if not spawn_allowed:
        pass  # already refused above, for gates and agents alike
    elif not agents_allowed:
        # SPEC SS12: an autonomous host runs GATES ONLY. Named separately
        # from `offline-no-spawn` because the two are different facts with
        # different fixes -- this host is gating fine, it is declining to
        # spend an agent run while nothing is coordinating the fleet.
        res.refused.append((
            "autonomous-no-agents",
            "no live server lease; this host is scheduling autonomously and "
            "dispatches gates only",
        ))
    elif enabled and want_gates <= 0 and want_agents <= 0:
        # S1 (Stage 1 integration review): the exact silent case flagged --
        # an enabled host with both targets at zero fell through the gates
        # block (deficit <= 0, nothing recorded) and the agents block below
        # (slots <= 0, nothing recorded) without a single refused entry, so
        # `fleet status --why` printed "(no refused reasons on file)" for a
        # host that is idle entirely by desired-state design. `target-zero`
        # names that design choice so it reads as intentional, not broken.
        res.refused.append(("target-zero", f"gates {want_gates} / agents {want_agents}"))
    if enabled and slots > 0 and spawn_allowed and agents_allowed:
        try:
            import agentworker as _aw
            has_cli = bool(_aw.available_clis())
        except Exception:
            has_cli = False
        if not has_cli:
            res.refused.append(("no-agent-cli", "neither claude nor codex on this host"))
        else:
            dispatch_agents(hub, host, workers, slots, log_dir, repo_root, res,
                            journal=jn)

    # T3: durable warnings, swept from the log directory rather than from
    # `workers` -- see `HostWarnings`. A caller that passes no store gets a
    # fresh one, which still produces a correct list for THIS call (the
    # sweep is stateless with respect to what is on disk); `main` owns one
    # across the whole daemon lifetime so a future non-file-backed warning
    # can persist too.
    res.warnings = (warnings or HostWarnings()).scan(log_dir)

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
        # PLAN Stage 1 task 5 / SPEC L121, L278: this loop's
        # `ReconcileResult.refused` -- (reason, detail) pairs -- carried
        # into the durable heartbeat verbatim so `fleet status --why` (and
        # later `/v1/why`) can answer "why is nothing starting" from a
        # ref read alone, with no ssh fan-out. JSON round-trips each tuple
        # as a 2-element array; `cmd_status`'s `--why` rendering treats
        # both shapes as equivalent.
        "refused": res.refused,
        # T3: DURABLE conditions, re-derived from marker files every loop
        # and therefore present in EVERY heartbeat for as long as the
        # condition lasts -- as opposed to `refused` above, which is this
        # loop's scheduling answer and stops mentioning a reaped gate's
        # verdict-store failure on the very next pass (15 s later). The
        # sweep is over the log directory, so markers left by gates fleetd
        # never spawned (`train.real_gate`, a hand-run `gate.sh`) surface
        # here too -- they surfaced NOWHERE before.
        "warnings": res.warnings,
        # Keel 3R-1 step 8: the live-work join and the transport's own
        # health, carried in the ref heartbeat that already exists.
        #
        # `live_workers` is built from the IN-MEMORY `workers` list, never
        # from a hub claim listing -- see `keel.runner.live_workers_payload`
        # for why an index-served listing is not admissible as the input to
        # a liveness verdict.
        "live_workers": live_workers_payload(workers),
        "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }
    if isinstance(hub, FallbackHub):
        # `FallbackHub.status()`'s own docstring calls it "heartbeat-shaped";
        # nothing had ever put it in a heartbeat. route, degraded_since (ISO
        # or None), primary_failures, fallback_reads, fallback_writes,
        # ambiguous_writes, last_primary_error. Absent entirely on a hubless
        # runner rather than present-and-null: there is no fallback to report
        # on, and a null would read as "healthy primary".
        hb["fallback"] = hub.status()
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


# --------------------------------------------------------------------- #
# Daemon loop
# --------------------------------------------------------------------- #


def main(argv: Optional[list] = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--hub", default=os.environ.get("FLEET_HUB_URL"), help="hub git URL")
    # PLAN Stage 1 task 4: the code repo (staging/* + refactor/tag-machinery)
    # is now a distinct remote from the hub (verdict/state) once the two
    # live in separate repos; default = --hub keeps today's single-repo
    # behaviour working unchanged when only FLEET_HUB_URL/--hub is set.
    ap.add_argument(
        "--code",
        default=os.environ.get("FLEET_CODE_URL"),
        help="code repo git URL (default: same as --hub)",
    )
    ap.add_argument("--repo-root", default=str(Path(__file__).resolve().parents[2]))
    ap.add_argument("--log-dir", default=str(Path.home() / "gatelogs"))
    ap.add_argument("--once", action="store_true", help="single reconcile step, then exit")
    ap.add_argument("--interval", type=int, default=LOOP_SECONDS)
    args = ap.parse_args(argv)
    if not args.hub:
        print("fleetd: no hub URL (--hub or FLEET_HUB_URL)", file=sys.stderr)
        return 2
    if not args.code:
        args.code = args.hub

    host = host_identity()
    repo_root = Path(args.repo_root)
    # `code_url` is fleetlib.Hub's own constructor argument (PLAN Stage 1
    # task 2, default `url`, resolved once at construction -- the single
    # definition) for exactly this split: coordination refs on `--hub`,
    # branch/tree history on `--code`.
    hub = Hub(args.hub, workdir=Path.home() / ".fleetd" / "hubcache", code_url=args.code)
    # PLAN Stage 3 task 1: everything below argparse + Hub construction --
    # the host singleton (with the provably-dead same-host reap), the
    # adoption rebuild, the signal handlers, the bounded-failure loop and
    # every exit code (0/3/4/5/6, plus L1's own rc 7 for a toolchain-id
    # mismatch -- deliberately NOT 6, which is the hub-unusable code a
    # supervisor should retry; see keel.runner.TOOLCHAIN_MISMATCH_RC)
    # -- MOVED VERBATIM to
    # `keel.runner.run_daemon` (SPEC §2 C7). fleetd.py is still the
    # deployed entry point, so it delegates rather than duplicates.
    #
    # `reconcile` is a late-binding wrapper on purpose: it resolves this
    # module's global `reconcile_once` on EVERY call, exactly as the old
    # inline loop did, so `mock.patch.object(fleetd, "reconcile_once", ...)`
    # (test_fleetd's main-loop tests) still intercepts the step.
    return keel_runner.run_daemon(
        hub,
        host,
        gate_command=default_gate_command(repo_root),
        log_dir=Path(args.log_dir),
        repo_root=repo_root,
        interval=args.interval,
        once=args.once,
        reconcile=lambda *a, **kw: reconcile_once(*a, **kw),
        label="fleetd",  # keeps every log line byte-identical
    )


if __name__ == "__main__":
    raise SystemExit(main())
