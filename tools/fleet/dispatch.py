#!/usr/bin/env python3
"""Dispatch economics for agent runs (ARCH-FIX-SPEC.md R5).

**Every agent run is paid.** `claude -p` / `codex exec` cost real money and
real wall-clock, and until this module existed `fleetd` would happily spend
both on a run that could not possibly accomplish anything:

  * a convergence run against a branch that ALREADY contains the tip --
    there is no drift to converge, so the agent's entire task is a no-op it
    will spend twenty minutes discovering;
  * a run against a (branch, tip) pair whose merge tree has already PASSed
    the gate -- the branch is not stale, it is *awaiting the train*;
  * the same failing branch, retried forever, because the only thing
    stopping a re-dispatch was a dict in the daemon's memory
    (`fleetd._agent_attempts`) that every restart reset to empty. Two
    fleetd restarts an hour -- the observed rate on 2026-08-14/15 -- turned
    a 30-minute cooldown into no cooldown at all, and the fleet re-bought
    the same failure indefinitely with nothing anywhere recording that it
    had.

So the attempt ledger is a hub ref, not a dict: `refs/fleet/attempts/<key>`
carrying `{count, last_at, last_outcome, last_host}`, written through
`fleetlib.Hub`'s CAS. A restart re-reads it; a second host reads the same
one. The cooldown is derived from `last_at` in that payload rather than
kept alongside it in memory, so there is no second copy that a restart can
disagree with -- the same "queue derived from refs, never copied" rule
`workqueue` follows.

## The counting direction, and why it is deliberately pessimistic

`record_dispatch()` is called BEFORE the agent process is spawned, never
after. If the daemon dies between the two, we have counted a run that never
happened: the branch gets one fewer retry than it deserved. The other order
-- spawn, then count -- loses the record of a run that DID happen, and the
next loop buys it again. One of those errors costs a retry; the other costs
unbounded money. `record_outcome(..., "not-paid")` exists to give the
retry back on the one path where we can prove nothing was bought (the
worker's own preflight refused before invoking a CLI).

A run that made real progress resets `count` to zero: the cap counts
CONSECUTIVE failures, not lifetime attempts. A branch that converged, was
gated, and went stale again a week later is not a branch that has used up
its three chances.

## The reserved authoring slot

Before this module, `fleetd` only looked at the intent backlog when the
convergence queue was completely empty (`if not todo:`). A fleet with a
standing backlog of stale branches -- i.e. every fleet, always -- never
authored anything at all: intents starved by construction, and the
`agents` column could run at 100% utilisation for days while the intent
backlog it existed to serve never moved.

`order_candidates()` implements the fix as strict alternation (see its
docstring for the exact policy). The alternation state is DERIVED from the
attempt records rather than stored, because a stored "whose turn is it"
counter is precisely the kind of daemon-memory state a restart resets --
the defect this whole module is a response to.

Standard library only.
"""

from __future__ import annotations

import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional, Sequence

sys.path.insert(0, str(Path(__file__).resolve().parent))

import fleetlib  # noqa: E402
from fleetlib import Hub, HubError  # noqa: E402

ATTEMPTS_PREFIX = "refs/fleet/attempts"
VERDICTS_PREFIX = "refs/fleet/verdicts"

# A dispatch key naming an INTENT to author, rather than a branch to
# converge. Mirrors the marker `fleetd.start_agent` already parses off the
# front of its `branch` argument, kept here as the single definition so the
# two cannot drift apart.
INTENT_PREFIX = "intent:"

# Hard cap on CONSECUTIVE failed attempts per key (R5: "a hard cap (3)").
# Env-overridable so the seam suite can drive the cap without three real
# dispatches, and so an operator can raise it for one shift without a
# deploy.
MAX_ATTEMPTS = int(os.environ.get("FLEET_AGENT_MAX_ATTEMPTS", "3"))

# Seconds after a dispatch before the same key may be dispatched again.
# Same env var name the in-memory cooldown used, so existing host
# configuration keeps working; the value now applies to a DURABLE record.
COOLDOWN_S = float(os.environ.get("FLEET_AGENT_COOLDOWN_S", "1800"))

# Outcomes that mean the run accomplished its task, and therefore reset the
# consecutive-failure count. Everything else (no-progress, blocked, killed,
# unknown) leaves the count where `record_dispatch` put it.
PROGRESS_OUTCOMES = frozenset({"converged", "authored"})

# The outcome that means "we counted a dispatch, but nothing was actually
# bought" -- the only path that gives an attempt back.
NOT_PAID = "not-paid"


def _utcnow() -> datetime:
    return datetime.now(timezone.utc)


def _iso(dt: datetime) -> str:
    return dt.astimezone(timezone.utc).isoformat()


def _parse_iso(value: str) -> Optional[datetime]:
    try:
        dt = datetime.fromisoformat(value)
    except (TypeError, ValueError):
        return None
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return dt


# --------------------------------------------------------------------- #
# Keys and refs
# --------------------------------------------------------------------- #


def is_intent_key(key: str) -> bool:
    """Does this dispatch key name an intent to AUTHOR (as opposed to a
    branch to converge)?"""
    return key.startswith(INTENT_PREFIX)


def attempt_key(branch: str) -> str:
    """The ref-safe slug for a dispatch key.

    Deliberately the SAME transform `fleetd.start_agent` applies when it
    builds the agent claim's key, so `refs/fleet/claims/agent/<k>` and
    `refs/fleet/attempts/<k>` name the same work with the same `<k>`. One
    key, two namespaces: an operator reading either ref can find the other
    without knowing a second slug convention, and a divergence between the
    two is not expressible.
    """
    return branch.replace("/", "-").replace(":", "-")


def attempt_ref(branch: str) -> str:
    return f"{ATTEMPTS_PREFIX}/{attempt_key(branch)}"


def empty_record(branch: str) -> dict:
    """The record a never-before-dispatched key behaves as if it had.

    Returned rather than None so every caller takes the same code path for
    "no attempts yet" as for "some attempts" -- an `if record is None`
    branch in each consumer is exactly where the in-memory version's
    restart bug lived.
    """
    return {
        "key": attempt_key(branch),
        "branch": branch,
        "count": 0,
        "last_at": None,
        "last_outcome": None,
        "last_host": None,
    }


# --------------------------------------------------------------------- #
# The durable ledger
# --------------------------------------------------------------------- #


def load(hub: Hub, branch: str) -> dict:
    """The attempt record for one key, or `empty_record` if there is none."""
    payload = hub.read(attempt_ref(branch))
    if payload is None:
        return empty_record(branch)
    return _normalize(payload, branch)


def load_all(hub: Hub) -> dict:
    """`{attempt_key: record}` for every key with a record on the hub.

    One `ls-remote` for the whole namespace, then a read per EXISTING
    record -- a key that has never been dispatched costs nothing at all.
    That matters: `reconcile_once` runs every 15 seconds against a queue
    that is mostly keys with no history, and a per-candidate `load()` would
    be one ssh round trip per branch per loop.
    """
    out: dict = {}
    for ref in hub.list(ATTEMPTS_PREFIX):
        key = ref.rsplit("/", 1)[-1]
        payload = hub.read(ref)
        if payload is None:
            continue  # deleted between list and read
        out[key] = _normalize(payload, payload.get("branch") or key)
    return out


def get(records: dict, branch: str) -> dict:
    """`records[attempt_key(branch)]`, or an empty record. The lookup helper
    for a `load_all()` result, so callers never index it by raw branch name
    (which is not the key) by accident."""
    return records.get(attempt_key(branch)) or empty_record(branch)


def _normalize(payload: dict, branch: str) -> dict:
    """A payload from the hub coerced into the record shape.

    Tolerant on purpose: a record written by an older/newer fleetd, or one
    an operator hand-edited, must degrade to "no useful history" rather
    than crash the reconcile loop that read it.
    """
    try:
        count = int(payload.get("count") or 0)
    except (TypeError, ValueError):
        count = 0
    return {
        "key": payload.get("key") or attempt_key(branch),
        "branch": payload.get("branch") or branch,
        "count": max(0, count),
        "last_at": payload.get("last_at"),
        "last_outcome": payload.get("last_outcome"),
        "last_host": payload.get("last_host"),
    }


def _write(hub: Hub, branch: str, mutate, max_attempts: int = 5) -> dict:
    """CAS read-modify-write of one attempt ref. `mutate(record) -> record`.

    Retries on a lost race rather than clobbering: two hosts may legitimately
    dispatch different keys in the same instant, and (more to the point) a
    host racing ITSELF across a restart must not lose a count.
    """
    ref = attempt_ref(branch)
    for _ in range(max_attempts):
        existing_sha = hub.sha(ref)
        if existing_sha is None:
            record = mutate(empty_record(branch))
            if hub.create(ref, record):
                return record
            continue
        current = hub.read(ref)
        if current is None:
            continue  # deleted under us; the create path will take it
        record = mutate(_normalize(current, branch))
        if hub.update(ref, record, existing_sha):
            return record
    raise HubError(f"attempt ledger did not converge on {ref} after {max_attempts} tries")


def record_dispatch(hub: Hub, branch: str, host: str, now: Optional[datetime] = None) -> dict:
    """Count one paid dispatch. Call this BEFORE spawning the worker --
    see the module docstring on why this order is the safe one."""
    stamp = _iso(now or _utcnow())

    def _bump(record: dict) -> dict:
        record["count"] = int(record["count"]) + 1
        record["last_at"] = stamp
        record["last_outcome"] = "dispatched"
        record["last_host"] = host
        return record

    return _write(hub, branch, _bump)


def record_outcome(
    hub: Hub, branch: str, host: str, outcome: str, now: Optional[datetime] = None
) -> dict:
    """Record how a dispatched run ended.

    `PROGRESS_OUTCOMES` reset the consecutive-failure count; `NOT_PAID`
    gives back the attempt `record_dispatch` optimistically counted;
    anything else just labels the existing count.
    """
    stamp = _iso(now or _utcnow())

    def _finish(record: dict) -> dict:
        if outcome in PROGRESS_OUTCOMES:
            record["count"] = 0
        elif outcome == NOT_PAID:
            record["count"] = max(0, int(record["count"]) - 1)
        record["last_at"] = stamp
        record["last_outcome"] = outcome
        record["last_host"] = host
        return record

    return _write(hub, branch, _finish)


def clear(hub: Hub, branch: str) -> bool:
    """Delete a key's record (operator escape hatch: "give this branch its
    three chances back"). CAS'd, so it cannot race a live dispatch."""
    ref = attempt_ref(branch)
    sha = hub.sha(ref)
    if sha is None:
        return True
    return hub.delete(ref, expect_sha=sha)


# --------------------------------------------------------------------- #
# Budget: cap and cooldown, both derived from the durable record
# --------------------------------------------------------------------- #


def budget_refusal(
    record: dict,
    now: Optional[datetime] = None,
    max_attempts: Optional[int] = None,
    cooldown_s: Optional[float] = None,
) -> Optional[tuple]:
    """`(code, detail)` for why this key may not be dispatched right now, or
    None if it may. `code` is one of `"attempt-cap"` / `"cooldown"`.

    Both answers come from the ref payload alone -- no process memory is
    consulted, so a fleetd that started ten seconds ago gives exactly the
    same answer as one that has been up for a week. That equality is the
    property R5 asks for ("a restart must not reset counts"), and it is a
    property of this function being pure over `record`.

    The code is returned separately from the prose because the two
    refusals want opposite treatment in a log: a cooldown is the system
    working and should be aggregated to one line, while hitting the cap is
    a branch that has given up and needs naming individually.
    """
    cap = MAX_ATTEMPTS if max_attempts is None else max_attempts
    cool = COOLDOWN_S if cooldown_s is None else cooldown_s
    count = int(record.get("count") or 0)
    if cap >= 0 and count >= cap:
        return (
            "attempt-cap",
            f"{count}/{cap} consecutive paid runs made no progress (last "
            f"{record.get('last_outcome')!r} on {record.get('last_host')!r}); "
            f"needs a human, not another purchase",
        )
    last_at = record.get("last_at")
    if last_at and cool > 0:
        when = _parse_iso(last_at)
        if when is not None:
            elapsed = ((now or _utcnow()) - when).total_seconds()
            if elapsed <= cool:
                return (
                    "cooldown",
                    f"{elapsed:.0f}s since last dispatch, floor {cool:.0f}s",
                )
    return None


# --------------------------------------------------------------------- #
# The reserved authoring slot
# --------------------------------------------------------------------- #


def last_dispatch_was_authoring(records: dict) -> bool:
    """Was the most recent dispatch on record an AUTHORING one?

    Derived from the ledger, never stored. Records with no parseable
    `last_at` are ignored rather than treated as epoch-zero: an unparseable
    timestamp must not silently become "the oldest record" and skew the
    alternation.
    """
    newest_at: Optional[datetime] = None
    newest_is_author = False
    for record in records.values():
        when = _parse_iso(record.get("last_at") or "")
        if when is None:
            continue
        if newest_at is None or when > newest_at:
            newest_at = when
            newest_is_author = is_intent_key(record.get("branch") or "")
    return newest_is_author


def order_candidates(
    convergence: Sequence[str], authoring: Sequence[str], records: dict
) -> list:
    """Dispatch order for this round: the reserved-authoring-slot policy.

    THE POLICY, stated once so it can be argued with:

      Whenever at least one open intent is dispatchable and at least one
      agent slot is free, authoring and convergence STRICTLY ALTERNATE for
      the first slot -- if the most recent dispatch anywhere on the ledger
      was an authoring run, convergence takes this slot; otherwise
      authoring does. Remaining slots are filled from the other list first,
      then from whatever is left.

    Consequences, deliberately chosen:

      * At `agents=1` with a convergence backlog of any depth, an open
        intent gets a slot at least every other dispatch. It cannot starve,
        which is what happened under the previous rule (`if not todo:` --
        authoring was reachable ONLY when the convergence queue was
        entirely empty, i.e. essentially never).
      * At `agents>=2` both kinds get a slot in the same round, so the
        alternation is invisible and costs nothing.
      * Convergence is never starved either: alternation is symmetric. A
        fleet with a huge intent backlog and one stale branch still gates
        that branch's convergence every other dispatch.

    The alternation state is read out of the attempt ledger
    (`last_dispatch_was_authoring`), so it survives a restart -- a
    process-local "whose turn" flag would reset to the same value on every
    start and could pin the fleet to one kind forever.
    """
    convergence = list(convergence)
    authoring = list(authoring)
    if not authoring:
        return convergence
    if not convergence:
        return authoring
    if last_dispatch_was_authoring(records):
        first, second = convergence, authoring
    else:
        first, second = authoring, convergence
    # Interleave rather than concatenate: with N free slots the round
    # should spend them on both kinds, not exhaust one list first.
    out: list = []
    for i in range(max(len(first), len(second))):
        if i < len(first):
            out.append(first[i])
        if i < len(second):
            out.append(second[i])
    return out


# --------------------------------------------------------------------- #
# Economic preflight: is there anything for this run to DO?
# --------------------------------------------------------------------- #


def _git(hub: Hub, args: list, timeout: int = 60):
    """Run git plumbing against `Hub`'s own disposable object cache,
    through `fleetlib.run_git`.

    Same borrow `workqueue._fetch_for_ancestry` makes and for the same
    reason: the cache already holds (or can cheaply fetch) the real commit
    history, and git's object store is content-addressed so sharing it with
    Hub's orphan payload commits is harmless. Nothing here writes a ref --
    the fetches below land in FETCH_HEAD only, so there is no scratch
    namespace to leak and no cleanup to forget.

    ROUTED THROUGH `fleetlib.run_git` (T5, extending R5's fix for
    `workqueue.Queue._git` -- found by
    `tests/test_agent_delivery.py`'s "every git the worker spawns"
    fence, which reaches here through `economic_refusal`). This was a bare
    `subprocess.run(["git", ...])`, and one of its three call sites --
    `_have_objects` -- fetches from `hub.code_url`, a real remote: on a
    private HTTPS spine it ran with no credential helper, with whatever
    `GIT_SSH_COMMAND` the ambient environment carried instead of the pinned
    `BatchMode=yes`/`ConnectTimeout=10`, and with `GIT_TERMINAL_PROMPT`
    unset. Its failure was silent by construction -- `_have_objects`
    returns False on any non-zero exit, `economic_refusal` reads that as
    "objects not available yet" and simply declines to answer -- so an
    unauthenticated fetch here looks exactly like a cold cache, forever.

    `run_git` raises `HubUnreachableError` on timeout where
    `subprocess.run` raised `TimeoutExpired`; both call sites below catch
    `HubError` alongside `OSError` for that reason.
    """
    cmd = ["git", "--git-dir", str(hub.workdir)] + args
    return fleetlib.run_git(cmd, timeout=timeout)


def _have_objects(hub: Hub, hub_refs: Sequence[str]) -> bool:
    """Fetch `hub_refs` into the cache's object store. No destination
    refspec, so nothing but FETCH_HEAD is written.

    `hub_refs` name CODE commits (staging branches, the tip), so this
    fetches from `hub.code_url` rather than `hub.url` -- the state hub,
    once code and state live in separate repos. `code_url` defaults to
    `url` (`fleetlib.Hub`), so a combined-repo fixture behaves exactly as
    before.
    """
    if not hub_refs:
        return True
    try:
        result = _git(hub, ["fetch", "--no-tags", "--quiet", hub.code_url, *hub_refs])
    except (OSError, HubError, subprocess.TimeoutExpired):
        return False
    return result.returncode == 0


def _is_ancestor(hub: Hub, maybe_ancestor: str, descendant: str) -> Optional[bool]:
    """True/False, or None if git could not answer (missing object, etc.)."""
    try:
        result = _git(hub, ["merge-base", "--is-ancestor", maybe_ancestor, descendant])
    except (OSError, HubError, subprocess.TimeoutExpired):
        return None
    if result.returncode == 0:
        return True
    if result.returncode == 1:
        return False
    return None


def merge_tree_sha(hub: Hub, tip_sha: str, branch_sha: str) -> Optional[str]:
    """The tree `branch_sha` merged onto `tip_sha` would produce, or None.

    None covers "this merge conflicts", "git could not tell us", AND "the
    commits are not in the local cache" -- none of the three is a tree that
    could have been gated, so all three mean the verdict cache has nothing
    to say about this pair.

    PRECONDITION: both commits must already be in `hub.workdir`'s object
    store. `economic_refusal` guarantees that with its `_have_objects` call
    before it gets here; a direct caller must make the same call first or
    it will get a None that means "not fetched", not "no such tree".
    """
    try:
        result = _git(hub, ["merge-tree", "--write-tree", tip_sha, branch_sha])
    except (OSError, HubError, subprocess.TimeoutExpired):
        return None
    if result.returncode != 0:
        return None
    first = result.stdout.strip().splitlines()
    if not first:
        return None
    candidate = first[0].strip()
    if len(candidate) < 40 or any(c not in "0123456789abcdef" for c in candidate.lower()):
        return None
    return candidate


def cached_pass(hub: Hub, tip_sha: str, branch_sha: str) -> Optional[dict]:
    """A cached PASS verdict for the tree this (branch, tip) pair produces,
    or None.

    The verdict cache is keyed by `(tree_sha, gate_version, platform_id)`
    (`verdict.verdict_ref`), so the honest way to ask "has this pair already
    passed?" is to compute the merge tree and look for ANY passing verdict
    beneath it -- any gate version, any platform. A PASS from one platform
    is enough to make an agent run pointless even if this host would need
    its own verdict to merge: the branch is not stale, so there is nothing
    for a convergence agent to converge.

    Reads only. Writing verdicts is `gate.sh`'s job and consuming them for
    GATE selection is R4's (T5, `workqueue`/selection path); this function
    deliberately touches neither.

    Shares `merge_tree_sha`'s precondition: the commits must already be in
    the local object cache.
    """
    tree = merge_tree_sha(hub, tip_sha, branch_sha)
    if tree is None:
        return None
    for ref in hub.list(f"{VERDICTS_PREFIX}/{tree}"):
        payload = hub.read(ref)
        if payload is not None and payload.get("result") == "PASS":
            return payload
    return None


def economic_refusal(
    hub: Hub,
    branch: str,
    tip_sha: str,
    branch_sha: Optional[str] = None,
    tip_ref: str = "refs/heads/refactor/tag-machinery",
) -> Optional[tuple]:
    """`(code, detail)` for why dispatching an agent for `branch` would be
    structurally wasted, or None if the run has real work to do. `code` is
    one of `"no-such-branch"` / `"already-merged"` / `"no-drift"` /
    `"cached-pass"`.

    ONE implementation, called from two layers (see `fleetd`'s agent block
    and `agentworker.preflight`). The layers answer different questions
    with the same predicate: fleetd asks "should I buy this?", the worker
    asks "is what I was bought for still true?" -- and the worker is also
    the only guard when someone runs `agentworker.py` by hand.

    AUTHORING keys (`intent:<slug>`) are exempt from the drift test: there
    is no branch yet, so there is no merge-base to be equal to the tip.
    `fleetd`'s intent scan already refuses a slug whose staging branch
    exists, which is the authoring equivalent of "no drift".

    Fails OPEN on an unanswerable question (a fetch that failed, a merge
    that conflicts): refusing work because a probe could not answer would
    idle the fleet for infrastructure reasons, which is the mistake
    `_limits_ok` already names about its memory probe.
    """
    if is_intent_key(branch):
        return None
    if not tip_sha:
        return None

    ref = f"refs/heads/{branch}"
    if branch_sha is None:
        # `code_sha`: `refs/heads/*` is a CODE ref. Against a split spine
        # `hub.sha` asks the state repo and answers None, and None here is
        # the "no-such-branch" refusal below -- so every convergence
        # dispatch would be refused for a branch that plainly exists, with
        # a message naming the wrong repo.
        branch_sha = hub.code_sha(ref)
    if branch_sha is None:
        return ("no-such-branch", f"{ref} is not on the code repo {hub.code_url}")

    if not _have_objects(hub, [tip_ref, ref]):
        return None  # cannot answer -> do not block on it

    if _is_ancestor(hub, branch_sha, tip_sha) is True:
        return (
            "already-merged",
            f"{branch}@{branch_sha[:8]} is an ancestor of the tip "
            f"{tip_sha[:8]}; nothing left to converge",
        )

    if _is_ancestor(hub, tip_sha, branch_sha) is True:
        return (
            "no-drift",
            f"{branch}@{branch_sha[:8]} already contains tip {tip_sha[:8]} "
            f"(merge-base == tip); a convergence run has nothing to merge",
        )

    hit = cached_pass(hub, tip_sha, branch_sha)
    if hit is not None:
        return (
            "cached-pass",
            f"the merge of {branch}@{branch_sha[:8]} onto tip {tip_sha[:8]} "
            f"already PASSed (gate_version {hit.get('gate_version')!r} on "
            f"{hit.get('host')!r}); this branch is awaiting the train, not stale",
        )
    return None
