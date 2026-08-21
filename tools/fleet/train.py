"""train -- the merge train: batch admissible branches, gate the merged
result ONCE, bisect on failure (FLEET.md M6 / P4).

Why batching: the gate costs 20-45 minutes and 2026-08-14 paid it per
branch. N disjoint branches merged onto the tip and gated together cost
one gate when green and 2*log2(N)+1 when one is bad -- the difference
between clearing a queue in an hour and in a day.

Safety properties, each traced to a real incident:
- Gates the MERGE RESULT, never branches alone (green-alone-red-together
  broke the tree three times in one day).
- A branch touching a conflict domain (fleet/domains.toml) rides SOLO --
  file-disjointness does not prove independence for cross-cutting
  invariants like the census or the generated-enum pair.
- Merge conflicts EJECT the branch rather than resolving it: generated
  files need regen.sh, which runs only on the i7 (oracle-ledger Perl
  digest), and semantic resolutions need judgment a train must not fake.
- On PASS, merged branch tips are rescued to refs/heads/rescued/* and
  VERIFIED by sha before their staging refs are deleted -- squash merges
  make branch tips unreachable from the new tip (assumed otherwise once;
  the verify caught it).
- If the tip moves mid-run, the run restarts from the new tip; the train
  never force-pushes over someone else's advance.
- A bisect culprit that was file-disjoint from every other member is
  evidence of a MISSING conflict domain: the train emits a proposed
  domains.toml addition rather than silently absorbing the signal.

Three more, each a day-one defect an adversarial review found before this
module had ever run in production (2026-08-15):

- ONLY a tree whose exact member set a gate returned PASS for is ever
  pushed. The bisect memo (`_Memo.passed`) is the sole authority on that,
  and `run_train` re-checks it against the survivor set immediately before
  the push. The prior guard was `len(survivors) != len(members)`, a
  heuristic that skipped the re-gate precisely when every member survived
  its half -- i.e. when the *union* is the poison -- and pushed the tree
  that had just FAILed.
- Every staging-ref retirement is a CAS: `Hub.delete(expect_sha=...)`,
  never `git push --delete`. The gate window is 20-45 minutes; a raw delete
  discards whatever the author pushed during it, with no trace.
- The train is a singleton, so its claim key is the CONSTANT "singleton".
  It was the caller's own epoch string, which gave every invocation its own
  ref and made two trains provably unable to contend -- the one thing the
  claim existed to do. The epoch identifies the run, in the payload.

The tip push carries `--push-option=train-token=<secret>` when the
hub-local token file exists, which is what the hub's `update` hook checks
(docs/FLEET.md R1). No file = hook not installed yet = push without it.

`--dry-run` prints the batch it would run and why, touching nothing.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))

import intent
import workqueue
from claim import Claim, ClaimHeldError
from fleetlib import Hub, HubError

TIP_REF = "refs/heads/refactor/tag-machinery"
BATCH_MAX = 8
PUSH_RETRIES = 3
PUSH_BACKOFF_S = 4

# One train, fleet-wide. A constant key is what makes the claim a mutual
# exclusion rather than a decoration -- see the module docstring.
TRAIN_CLAIM_KIND = "train"
TRAIN_CLAIM_KEY = "singleton"

# The hub-side `update` hook denies every write to the tip that does not
# carry this push option. The secret lives in a hub-local file (mode 0600);
# its ABSENCE means the hook is not installed yet, and the train must keep
# working in that state, so the option is simply omitted.
TRAIN_TOKEN_ENV = "FLEET_TRAIN_TOKEN_FILE"
TRAIN_TOKEN_DEFAULT = "~/git/oxidex.git/train.token"
_NO_PUSH_OPTIONS = "does not support push options"


class TrainError(Exception):
    pass


def _warn(msg: str) -> None:
    print(f"train: WARNING {msg}", file=sys.stderr)


# --------------------------------------------------------------------- #
# Tip push token (R1)
# --------------------------------------------------------------------- #


def train_token_path() -> Path:
    return Path(os.environ.get(TRAIN_TOKEN_ENV) or TRAIN_TOKEN_DEFAULT).expanduser()


def tip_push_options() -> list:
    """`["--push-option=train-token=<secret>"]`, or `[]` if the hub-local
    token file is absent/unreadable/empty.

    Absent is the pre-rollout state and must not be an error. Empty or
    unreadable is reported loudly and treated as absent: sending
    `train-token=` can only ever be rejected, and failing the same way as
    "no token at all" is easier to diagnose than a mysterious hook denial.
    """
    p = train_token_path()
    try:
        token = p.read_text().strip()
    except FileNotFoundError:
        return []
    except OSError as exc:
        _warn(f"cannot read train token {p}: {exc} -- pushing without it")
        return []
    if not token:
        _warn(f"train token {p} is empty -- pushing without it")
        return []
    try:
        mode = p.stat().st_mode & 0o777
        if mode & 0o077:
            _warn(f"train token {p} is group/world accessible (mode {mode:04o}); expected 0600")
    except OSError:
        pass
    return [f"--push-option=train-token={token}"]


# --------------------------------------------------------------------- #
# Conflict domains
# --------------------------------------------------------------------- #


def load_domains(repo_root: Path) -> list:
    """fleet/domains.toml, parsed without tomllib dependency drama (the
    file is a single `domains = [...]` list of quoted strings)."""
    p = repo_root / "fleet" / "domains.toml"
    if not p.is_file():
        return []
    out = []
    for line in p.read_text().splitlines():
        line = line.split("#", 1)[0].strip().strip(",")
        if line.startswith('"') and line.endswith('"'):
            out.append(line.strip('"'))
    return out


# --------------------------------------------------------------------- #
# Batch assembly
# --------------------------------------------------------------------- #


@dataclass
class Candidate:
    slug: str
    branch: str
    sha: str
    write_set: list = field(default_factory=list)
    solo: bool = False
    excluded: Optional[str] = None  # reason, if not batchable


def _git(args: list, cwd: Optional[Path] = None, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["git"] + args, cwd=cwd, capture_output=True, text=True, errors="replace", check=check
    )


def write_set_for(clone: Path, sha: str, tip_sha: str) -> list:
    base = _git(["merge-base", tip_sha, sha], cwd=clone).stdout.strip()
    out = _git(["diff", "--name-only", f"{base}..{sha}"], cwd=clone).stdout
    return [l for l in out.splitlines() if l.strip()]


def assemble_batch(
    clone: Path, tip_sha: str, queue: dict, domains: list, batch_max: int = BATCH_MAX
) -> list:
    """Classify every queue entry; returns all Candidates with .excluded/
    .solo set. Batch = the unexcluded, non-solo prefix with pairwise
    disjoint write sets, oldest base first."""
    cands = []
    for slug, entry in queue.items():
        c = Candidate(slug=slug, branch=entry.ref.removeprefix("refs/heads/"), sha=entry.sha)
        try:
            c.write_set = write_set_for(clone, c.sha, tip_sha)
        except subprocess.CalledProcessError:
            c.excluded = "write-set-unresolvable (sha missing locally?)"
            cands.append(c)
            continue
        if any(f in domains for f in c.write_set):
            c.solo = True
        cands.append(c)

    taken: set = set()
    picked = 0
    for c in cands:
        if c.excluded or c.solo:
            continue
        if picked >= batch_max:
            c.excluded = f"batch full ({batch_max})"
            continue
        overlap = taken.intersection(c.write_set)
        if overlap:
            c.excluded = f"write-set overlap: {sorted(overlap)[:3]}"
            continue
        taken.update(c.write_set)
        picked += 1
    return cands


# --------------------------------------------------------------------- #
# The run
# --------------------------------------------------------------------- #


@dataclass
class RunResult:
    # "advanced" | "empty" | "restarted" | "all-ejected" | "claim-held"
    outcome: str
    landed: list = field(default_factory=list)
    ejected: list = field(default_factory=list)  # (branch, reason)
    gate_invocations: int = 0
    missing_domain_proposals: list = field(default_factory=list)
    new_tip: Optional[str] = None
    # Branches that LANDED but whose staging ref could not be retired --
    # the CAS refused because the ref moved while we gated. Loud, never
    # silent: somebody's commit is sitting on a branch the train just
    # partially consumed.
    retire_failures: list = field(default_factory=list)  # (branch, why)


@dataclass
class _Memo:
    """Gate verdicts by EXACT member set, the only authority on whether a
    tree may be pushed.

    `passed` keeps the bisect's 2*log2(N)+1 bound by never re-gating a set
    already proven. `failed` is what makes the reassembly path terminate:
    a survivor union that reassembles into a set which already failed
    cannot be improved by gating it again, and gating it again is an
    infinite loop (halves are memoized, so the bisect returns the same
    union forever).
    """

    passed: set = field(default_factory=set)
    failed: set = field(default_factory=set)
    ejected: set = field(default_factory=set)


def _memo_key(members: list) -> tuple:
    return tuple(sorted(c.slug for c in members))


def merge_members(clone: Path, tip_sha: str, members: list) -> tuple:
    """Merge members onto tip in a scratch checkout; conflicting members
    are ejected, not resolved. Returns (merged_members, ejected)."""
    _git(["checkout", "-q", "-B", "train-run", tip_sha], cwd=clone)
    merged, ejected = [], []
    for c in members:
        r = _git(["merge", "--squash", "-q", c.sha], cwd=clone, check=False)
        conflicted = _git(["ls-files", "-u"], cwd=clone).stdout.strip()
        if r.returncode != 0 or conflicted:
            _git(["merge", "--abort"], cwd=clone, check=False)
            _git(["reset", "-q", "--hard", "HEAD"], cwd=clone, check=False)
            _git(["clean", "-qfd"], cwd=clone, check=False)
            ejected.append((c, "merge-conflict"))
            continue
        staged = _git(["diff", "--cached", "--name-only"], cwd=clone).stdout.strip()
        if not staged:
            ejected.append((c, "empty (already in tip)"))
            continue
        _git(["commit", "-qm", f"train: {c.branch} (squash onto {tip_sha[:8]})"], cwd=clone)
        merged.append(c)
    return merged, ejected


def run_train(
    hub_url: str,
    repo_root: Path,
    gate_fn: Callable[[Path, str], str],
    epoch: str,
    batch_max: int = BATCH_MAX,
    dry_run: bool = False,
    _clone_src: Optional[str] = None,
    hub_workdir: Optional[Path] = None,
) -> RunResult:
    """One train run, under the fleet-wide singleton claim.

    `gate_fn(clone, label) -> "PASS"|"FAIL"|"ABORT"` gates the CURRENT HEAD
    of `clone` (injected so tests mock it). Returns
    `RunResult(outcome="claim-held")` -- it does not raise -- if another
    train already holds the singleton, so a caller can report "one train is
    already running" without treating it as a failure.

    The claim is skipped for `dry_run`, which writes nothing and must not
    be able to block a real run.
    """
    hub = Hub(
        hub_url,
        workdir=Path(hub_workdir) if hub_workdir else Path.home() / ".fleetd" / "traincache",
    )
    if dry_run:
        return _run_train_locked(hub, hub_url, gate_fn, batch_max, True, _clone_src)
    try:
        # Constant key, epoch in the payload: two trains contend here or
        # nowhere. `Claim.__enter__` reaps an EXPIRED holder, so a train
        # killed mid-run cannot wedge the fleet past its lease.
        with Claim(
            hub,
            kind=TRAIN_CLAIM_KIND,
            key=TRAIN_CLAIM_KEY,
            work_kind="train",
            work_key=epoch,
        ):
            return _run_train_locked(hub, hub_url, gate_fn, batch_max, False, _clone_src)
    except ClaimHeldError:
        return RunResult(outcome="claim-held")


def _run_train_locked(
    hub: Hub,
    hub_url: str,
    gate_fn: Callable[[Path, str], str],
    batch_max: int,
    dry_run: bool,
    _clone_src: Optional[str],
) -> RunResult:
    res = RunResult(outcome="empty")

    tmp = Path(tempfile.mkdtemp(prefix="train-"))
    try:
        # `hub_url` is whatever Hub(hub_url, ...) above was constructed
        # with -- the STATE hub once code and state are split across two
        # repos. This clone needs the CODE tree (tip, staging/*,
        # domains.toml), so it prefers `hub.code_url`, which defaults to
        # `url` (`fleetlib.Hub`) and therefore equals `hub_url` on any hub
        # that doesn't carry the attribute yet.
        clone_src = _clone_src or getattr(hub, "code_url", hub_url)
        _git(["clone", "-q", clone_src, str(tmp / "w")])
        clone = tmp / "w"
        tip_sha = hub.sha(TIP_REF)
        if tip_sha is None:
            raise TrainError(f"no tip at {TIP_REF}")
        _git(["fetch", "-q", "origin", f"+refs/heads/staging/*:refs/remotes/origin/staging/*"], cwd=clone, check=False)
        # Check out the TIP explicitly before reading domains.toml: a bare
        # hub whose HEAD names a missing/old default branch gives a clone
        # with an empty or stale working tree, and reading domains from it
        # silently disables solo routing (found by the fixture, would have
        # served main's stale domains in production).
        _git(["checkout", "-q", "-B", "train-base", tip_sha], cwd=clone)

        domains = load_domains(clone)
        queue = workqueue.Queue(hub).compute()
        cands = assemble_batch(clone, tip_sha, queue, domains, batch_max)

        if dry_run:
            for c in cands:
                tag = "SOLO" if c.solo else (f"EXCLUDED: {c.excluded}" if c.excluded else "BATCH")
                print(f"  {c.branch:40s} {tag}  ({len(c.write_set)} files)")
            return res

        members = [c for c in cands if not c.excluded and not c.solo]
        solos = [c for c in cands if c.solo and not c.excluded]
        # Solo branches run one at a time, before any batch, oldest first;
        # a solo run is just a batch of one.
        for group in ([ [s] for s in solos ] + ([members] if members else [])):
            if not group:
                continue
            merged, ejected = merge_members(clone, tip_sha, group)
            res.ejected.extend((c.branch, why) for c, why in ejected)
            if not merged:
                continue
            memo = _Memo()
            survivors = _gate_and_bisect(clone, tip_sha, merged, gate_fn, res, memo)
            if survivors:
                # The one invariant worth restating at the push site: a
                # tree is pushable only if a gate said PASS for EXACTLY
                # this member set. Not "for its halves", not "for a
                # superset that failed". If this ever trips, the bisect
                # returned an unproven tree and the correct move is to
                # push nothing at all.
                key = _memo_key(survivors)
                if key not in memo.passed:
                    raise TrainError(
                        "refusing to advance the tip: survivor set "
                        f"{list(key)} was never gated as exactly that set "
                        f"(gated PASS sets: {sorted(memo.passed)})"
                    )
                # Tip may have moved while we gated.
                now_tip = hub.sha(TIP_REF)
                if now_tip != tip_sha:
                    res.outcome = "restarted"
                    return res
                new_tip = _git(["rev-parse", "HEAD"], cwd=clone).stdout.strip()
                _push_tip_and_retire(hub, clone, survivors, new_tip, res)
                res.outcome = "advanced"
                res.new_tip = new_tip
                tip_sha = new_tip
        if res.outcome == "empty" and res.ejected:
            res.outcome = "all-ejected"
        return res
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def _gate_and_bisect(
    clone: Path, tip_sha: str, members: list, gate_fn, res: RunResult, memo: Optional[_Memo] = None
) -> list:
    """Gate the merged HEAD; on FAIL bisect by re-merging halves.

    Returns the members that belong in the tree -- and it returns them ONLY
    when a gate said PASS for exactly that set, which the caller re-checks
    against `memo.passed` before pushing. On return, the clone's HEAD is
    the tree containing exactly the returned members, on every path.

    `memo` carries both verdicts keyed by exact member set: `passed` keeps
    the 2*log2(N)+1 bound, `failed` stops the reassembly path from re-gating
    (forever) a set already known bad.
    """
    if memo is None:
        memo = _Memo()
    if not members:
        return []
    key = _memo_key(members)
    if key in memo.passed:
        return members
    if key in memo.failed:
        # Already gated as this exact set, and it did not pass. Gating it
        # again cannot change the answer and does not terminate (the halves
        # are memoized, so the bisect hands back this same union forever).
        _eject_union(clone, tip_sha, members, res, memo)
        return []
    label = "+".join(c.slug for c in members)
    verdict = gate_fn(clone, label)
    res.gate_invocations += 1
    if verdict == "ABORT":
        # environmental -- retry once, never condemn the branch
        verdict = gate_fn(clone, label + "+retry")
        res.gate_invocations += 1
    if verdict == "PASS":
        memo.passed.add(key)
        return members
    memo.failed.add(key)
    if len(members) == 1:
        c = members[0]
        res.ejected.append((c.branch, f"gate {verdict}"))
        # file-disjoint culprit in a wider context = missing domain evidence
        if not c.solo:
            res.missing_domain_proposals.extend(c.write_set[:5])
        # rebuild tree without it
        _rebuild(clone, tip_sha, [], res)
        return []
    mid = len(members) // 2
    survivors: list = []
    for half in (members[:mid], members[mid:]):
        # Use what actually re-merged: a member ejected here (conflict, or
        # empty because a sibling already carried its change) is NOT in the
        # tree and must never be reported as landed.
        merged = _rebuild(clone, tip_sha, half, res)
        survivors.extend(_gate_and_bisect(clone, tip_sha, merged, gate_fn, res, memo))
    if not survivors:
        _rebuild(clone, tip_sha, [], res)
        return []
    # Reassemble the survivors and gate THAT set. Their union has passed
    # only in halves; whether this exact combination has ever been gated is
    # a question only the memo can answer, and the memo is asked -- by the
    # recursive call, whose first act is that lookup. The guard here used
    # to be `len(survivors) != len(members)`, which skipped the re-gate in
    # exactly the case where the union is the poison and shipped the tree
    # that had just FAILed (2026-08-15 adversarial review).
    merged = _rebuild(clone, tip_sha, survivors, res)
    return _gate_and_bisect(clone, tip_sha, merged, gate_fn, res, memo)


def _eject_union(clone: Path, tip_sha: str, members: list, res: RunResult, memo: _Memo) -> None:
    """Nothing lands from a set that fails only in combination.

    Every member gated clean on its own, so there is no culprit to condemn
    and no subset the train can prove good; the branches stay on the hub
    (an `ejected` entry deletes nothing) and go back in the queue. A
    file-disjoint set that fails only together is the same evidence a lone
    bisect culprit is: a conflict domain nobody declared.
    """
    key = _memo_key(members)
    if key not in memo.ejected:
        memo.ejected.add(key)
        for c in members:
            res.ejected.append(
                (c.branch, "union gate FAIL (no single culprit; every member passes apart)")
            )
            if not c.solo:
                res.missing_domain_proposals.extend(c.write_set[:2])
    _rebuild(clone, tip_sha, [], res)


def _rebuild(clone: Path, tip_sha: str, members: list, res: Optional[RunResult] = None) -> list:
    """Re-merge `members` onto the tip and return the ones that ACTUALLY
    merged. The return value is load-bearing: the final reassembly used to
    discard it, so a member ejected during that re-merge stayed in the
    survivor list and was reported as landed while absent from the tree."""
    merged, ejected = merge_members(clone, tip_sha, members)
    if res is not None:
        res.ejected.extend((c.branch, f"{why} (during bisect re-merge)") for c, why in ejected)
    return merged


def _fetch_into_hub_cache(hub: Hub, clone: Path, sha: str, check: bool = True):
    """`fleetlib.Hub.push_ref` pushes FROM the hub's own local object cache
    (`hub.workdir`), never from `clone` -- so a commit only known to
    `clone`'s object store must be fetched into that cache first. Mirrors
    `tests/test_update_hook.py`'s `_fetch_into_hub_cache` fixture helper,
    which documents the identical `Hub.push_ref` contract ("this mirrors
    real usage; it is not a workaround for a bug in push_ref"). `fetch` is
    not a hub *write*, so it is outside test_no_raw_hub_push.py's scope."""
    return _git(["--git-dir", str(hub.workdir), "fetch", "--quiet", str(clone), sha], check=check)


def _push_tip(hub: Hub, clone: Path, options: list):
    """The one write to the protected tip. Carries the train token when the
    hub-local file exists (R1). Routed through `fleetlib.Hub.push_ref`
    rather than a raw `git push` (ARCH-FIX R9 -- no fleet tool writes the
    hub outside `fleetlib.Hub`; see tools/fleet/tests/test_no_raw_hub_push.py).
    `options` is `tip_push_options()`'s `--push-option=key=value` CLI-flag
    spelling; `Hub.push_ref` (like `git push -o`) wants the bare
    `key=value`, so the flag prefix is stripped before it crosses into
    fleetlib."""
    head = _git(["rev-parse", "HEAD"], cwd=clone).stdout.strip()
    fetch = _fetch_into_hub_cache(hub, clone, head, check=False)
    if fetch.returncode != 0:
        return fetch
    raw_options = [o.removeprefix("--push-option=") for o in options]
    r = hub.push_ref(f"{head}:{TIP_REF}", push_options=raw_options)
    if r.returncode != 0 and options and _NO_PUSH_OPTIONS in (r.stderr or "").lower():
        # The hub cannot receive push options at all (its
        # receive.advertisePushOptions is unset), so no update hook can be
        # reading one either. Retrying without the token keeps the train
        # working through a half-finished rollout instead of wedging on
        # every run -- loudly, because it means the hub is misconfigured.
        _warn(
            "hub does not advertise push options (receive.advertisePushOptions); "
            "retrying the tip push WITHOUT the train token -- the update hook "
            "cannot be enforcing one in this state"
        )
        r = hub.push_ref(f"{head}:{TIP_REF}")
    return r


def _delete_ref_cas(hub: Hub, ref: str, expect_sha: str) -> bool:
    """Delete `ref` ONLY if it still points at `expect_sha`, via
    `fleetlib.Hub.delete`'s own `--force-with-lease` CAS (ARCH-FIX R9).

    This used to fall back to a hand-rolled `git push --force-with-lease`
    when called with `hub=None` -- an exact duplicate of `Hub.delete`'s own
    semantics, kept alive for a case no real caller (both call sites always
    pass a real `Hub`) ever exercised. See
    tools/fleet/tests/test_no_raw_hub_push.py."""
    try:
        return hub.delete(ref, expect_sha=expect_sha)
    except HubError as exc:
        _warn(f"CAS delete of {ref} failed: {exc}")
        return False


def _mark_intent_done(hub: Hub, slug: str, landed_sha: str) -> None:
    """Best-effort: close the intent (if any) the now-landed branch `slug`
    was registered against -- CAS `refs/fleet/intents/<slug>` from status
    "open" to "done", recording `landed_sha` + `landed_at`.

    Before this, nothing in tools/fleet ever wrote intent status "done"
    (`intent.py` only ever wrote "open"/`register` and "withdrawn"/
    `withdraw`), so a completed intent's ref stayed "open" forever and
    `fleetd`'s authoring path kept offering it as work to author once its
    staging branch was gone (docs/FLEET.md M5). A failed update here must
    NOT fail the train run -- the branch has already landed -- so every
    outcome (done, no-op, failure) is logged instead of raised."""
    try:
        if intent.mark_done(hub, slug, landed_sha):
            print(f"train: intent {slug!r} marked done (landed {landed_sha[:12]})")
        else:
            print(
                f"train: intent {slug!r} not marked done -- no open intent at "
                f"{intent.intent_ref(slug)!r} (never registered, already closed, or a lost race)"
            )
    except HubError as exc:
        _warn(f"best-effort intent-done update for {slug!r} failed (train run continues): {exc}")


def _retire_staging_ref(hub: Hub, clone: Path, c: Candidate, res: RunResult) -> None:
    """Retire `staging/<slug>` -- but only the exact commit the train
    gated. The gate window is 20-45 minutes; a raw delete here throws away
    whatever the author pushed during it, and nothing downstream can tell
    that it happened. A refused delete leaves the branch queued (its new
    head is not an ancestor of the tip, so the next run picks it up)."""
    ref = f"refs/heads/staging/{c.slug}"
    if _delete_ref_cas(hub, ref, c.sha):
        _mark_intent_done(hub, c.slug, c.sha)
        return
    try:
        actual = hub.sha(ref)
    except HubError:
        actual = None
    why = (
        f"{ref} NOT retired: expected {c.sha[:12]}, hub has "
        f"{(actual or 'absent')[:12]} -- the branch moved while the train gated. "
        f"{c.branch} IS in the new tip; its staging ref is kept and requeued."
    )
    _warn(why)
    res.retire_failures.append((c.branch, why))


def _push_tip_and_retire(hub: Hub, clone: Path, members: list, new_tip: str, res: RunResult):
    options = tip_push_options()
    for attempt in range(PUSH_RETRIES):
        r = _push_tip(hub, clone, options)
        if r.returncode == 0:
            break
        time.sleep(PUSH_BACKOFF_S * (attempt + 1))
    else:
        raise TrainError(f"could not advance tip after {PUSH_RETRIES} attempts: {r.stderr[-200:]}")
    for c in members:
        rescued_ref = f"refs/heads/rescued/{c.slug}"
        for attempt in range(PUSH_RETRIES):
            _git(["push", "origin", f"{c.sha}:{rescued_ref}"], cwd=clone, check=False)
            got = _git(["ls-remote", "origin", rescued_ref], cwd=clone).stdout.split("\t")[0].strip()
            if got == c.sha:
                res.landed.append(c.branch)
                _retire_staging_ref(hub, clone, c, res)
                break
            time.sleep(PUSH_BACKOFF_S * (attempt + 1))
        else:
            # verified-rescue failed: KEEP the staging ref; losing the last
            # copy is the one unrecoverable outcome.
            res.ejected.append((c.branch, "landed but rescue unverified -- staging ref kept"))


# --------------------------------------------------------------------- #
# Real gate adapter + CLI
# --------------------------------------------------------------------- #


def real_gate(clone: Path, label: str, hub: Hub) -> str:
    """Run tools/fleet/gate.sh from the merged tree against its own HEAD
    by pushing HEAD to a temp staging ref first (gate.sh takes a branch).
    Routed through `fleetlib.Hub.push_ref` (ARCH-FIX R9); see `_push_tip`'s
    docstring for why the commit must be fetched into the hub's own local
    cache before `push_ref` can push it from there."""
    tag = f"train-{label.replace('/', '-')[:40]}-{int(time.time()) % 100000}"
    branch = f"staging/train-tmp-{tag}"
    head = _git(["rev-parse", "HEAD"], cwd=clone).stdout.strip()
    _fetch_into_hub_cache(hub, clone, head)
    r = hub.push_ref(f"{head}:refs/heads/{branch}")
    if r.returncode != 0:
        raise TrainError(f"could not push temp gate ref refs/heads/{branch}: {r.stderr}")
    try:
        script = clone / "tools" / "fleet" / "gate.sh"
        subprocess.run(["bash", str(script), branch, tag], check=False)
        v = Path.home() / "gatelogs" / f"gate-{tag}.verdict"
        text = v.read_text().strip() if v.is_file() else "ABORT missing-verdict"
        if text.startswith("PASS"):
            return "PASS"
        if text.startswith("ABORT"):
            return "ABORT"
        return "FAIL"
    finally:
        # CAS even here: the ref is ours and short-lived, but "delete a ref
        # without checking what it points at" is the shape of the bug, not
        # the identity of the ref.
        if not _delete_ref_cas(hub, f"refs/heads/{branch}", head):
            _warn(f"temp gate ref refs/heads/{branch} not deleted (moved or unreachable)")


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--hub", default=os.environ.get("FLEET_HUB_URL"))
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--batch-max", type=int, default=BATCH_MAX)
    args = ap.parse_args(argv)
    if not args.hub:
        print("train: no hub URL (--hub or FLEET_HUB_URL)", file=sys.stderr)
        return 2
    epoch = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
    repo_root = Path(__file__).resolve().parents[2]

    if args.dry_run:
        run_train(args.hub, repo_root, gate_fn=lambda c, l: "PASS", epoch=epoch, dry_run=True,
                  batch_max=args.batch_max)
        return 0

    # The singleton claim is taken inside run_train (constant key, epoch in
    # the payload) so that every caller contends, not just this one.
    hub = Hub(args.hub, workdir=Path.home() / ".fleetd" / "traincache")
    res = run_train(args.hub, repo_root, epoch=epoch, batch_max=args.batch_max,
                    gate_fn=lambda clone, label: real_gate(clone, label, hub=hub))
    if res.outcome == "claim-held":
        print("train: another train run holds the singleton claim; not starting a second")
        return 3
    print(json.dumps({
        "outcome": res.outcome, "landed": res.landed, "ejected": res.ejected,
        "gate_invocations": res.gate_invocations, "new_tip": res.new_tip,
        "missing_domain_proposals": res.missing_domain_proposals,
        "retire_failures": res.retire_failures,
    }, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
