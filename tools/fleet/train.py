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

import workqueue
from claim import Claim, ClaimHeldError
from fleetlib import Hub, HubError

TIP_REF = "refs/heads/refactor/tag-machinery"
BATCH_MAX = 8
PUSH_RETRIES = 3
PUSH_BACKOFF_S = 4


class TrainError(Exception):
    pass


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
    outcome: str  # "advanced" | "empty" | "restarted" | "all-ejected"
    landed: list = field(default_factory=list)
    ejected: list = field(default_factory=list)  # (branch, reason)
    gate_invocations: int = 0
    missing_domain_proposals: list = field(default_factory=list)
    new_tip: Optional[str] = None


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
) -> RunResult:
    """One train run. `gate_fn(clone, label) -> "PASS"|"FAIL"|"ABORT"`
    gates the CURRENT HEAD of `clone` (injected so tests mock it)."""
    res = RunResult(outcome="empty")
    hub = Hub(hub_url, workdir=Path.home() / ".fleetd" / "traincache")

    tmp = Path(tempfile.mkdtemp(prefix="train-"))
    try:
        clone_src = _clone_src or hub_url
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
            survivors = _gate_and_bisect(clone, tip_sha, merged, gate_fn, res)
            if survivors:
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
    clone: Path, tip_sha: str, members: list, gate_fn, res: RunResult, _passed: Optional[set] = None
) -> list:
    """Gate the merged HEAD; on FAIL bisect by re-merging halves. Returns
    the members that belong in the tree (whose combined merge passed).
    `_passed` memoizes member-sets that already gated PASS, so a bisect
    never re-gates a set it just proved (keeps the 2*log2(N)+1 bound)."""
    if _passed is None:
        _passed = set()
    key = tuple(sorted(c.slug for c in members))
    if key in _passed:
        return members
    label = "+".join(c.slug for c in members)
    verdict = gate_fn(clone, label)
    res.gate_invocations += 1
    if verdict == "ABORT":
        # environmental -- retry once, never condemn the branch
        verdict = gate_fn(clone, label + "+retry")
        res.gate_invocations += 1
    if verdict == "PASS":
        _passed.add(key)
        return members
    if len(members) == 1:
        c = members[0]
        res.ejected.append((c.branch, f"gate {verdict}"))
        # file-disjoint culprit in a wider context = missing domain evidence
        if not c.solo:
            res.missing_domain_proposals.extend(c.write_set[:5])
        # rebuild tree without it
        _rebuild(clone, tip_sha, [])
        return []
    mid = len(members) // 2
    halves = [members[:mid], members[mid:]]
    survivors: list = []
    for half in halves:
        merged, ejected = ( _rebuild(clone, tip_sha, half), [] )
        keep = _gate_and_bisect(clone, tip_sha, merged, gate_fn, res, _passed)
        survivors.extend(keep)
    # Re-merge all survivors together for the final tree; their union
    # passed only in halves, so gate the combination once more.
    if survivors and len(survivors) != len(members):
        merged = _rebuild(clone, tip_sha, survivors)
        keep = _gate_and_bisect(clone, tip_sha, merged, gate_fn, res, _passed)
        return keep
    _rebuild(clone, tip_sha, survivors)
    return survivors


def _rebuild(clone: Path, tip_sha: str, members: list) -> list:
    merged, _ej = merge_members(clone, tip_sha, members)
    return merged


def _push_tip_and_retire(hub: Hub, clone: Path, members: list, new_tip: str, res: RunResult):
    for attempt in range(PUSH_RETRIES):
        r = _git(["push", "origin", f"HEAD:{TIP_REF}"], cwd=clone, check=False)
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
                _git(["push", "origin", "--delete", f"refs/heads/staging/{c.slug}"], cwd=clone, check=False)
                res.landed.append(c.branch)
                break
            time.sleep(PUSH_BACKOFF_S * (attempt + 1))
        else:
            # verified-rescue failed: KEEP the staging ref; losing the last
            # copy is the one unrecoverable outcome.
            res.ejected.append((c.branch, "landed but rescue unverified -- staging ref kept"))


# --------------------------------------------------------------------- #
# Real gate adapter + CLI
# --------------------------------------------------------------------- #


def real_gate(clone: Path, label: str) -> str:
    """Run tools/fleet/gate.sh from the merged tree against its own HEAD
    by pushing HEAD to a temp staging ref first (gate.sh takes a branch)."""
    tag = f"train-{label.replace('/', '-')[:40]}-{int(time.time()) % 100000}"
    branch = f"staging/train-tmp-{tag}"
    _git(["push", "origin", f"HEAD:refs/heads/{branch}"], cwd=clone)
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
        _git(["push", "origin", "--delete", f"refs/heads/{branch}"], cwd=clone, check=False)


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

    hub = Hub(args.hub, workdir=Path.home() / ".fleetd" / "traincache")
    try:
        with Claim(hub, kind="train", key=epoch, work_kind="train", work_key=epoch):
            res = run_train(args.hub, repo_root, gate_fn=real_gate, epoch=epoch,
                            batch_max=args.batch_max)
    except ClaimHeldError:
        print("train: another train run holds the claim; not starting a second")
        return 3
    print(json.dumps({
        "outcome": res.outcome, "landed": res.landed, "ejected": res.ejected,
        "gate_invocations": res.gate_invocations, "new_tip": res.new_tip,
        "missing_domain_proposals": res.missing_domain_proposals,
    }, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
