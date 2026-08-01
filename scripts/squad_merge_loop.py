#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Per-squad merger daemon: consumes worker branches into squad/<squad>.

Spec: docs/plans/specs/2026-07-24-fleet-knowledge-and-scaling-design.md,
section M2 (squad staging branches + merger daemon) plus the merger-owned
half of M5 (squad branch re-cut rule). One process per squad.

Lifecycle per poll (``poll_once``):

1. Ensure this squad's staging worktree exists, checked out on
   ``squad/<squad>`` (cut from origin/main the first time it is needed --
   see ``ensure_staging_worktree``).
2. Find candidate worker branches: squads.toml's ``formats`` list for
   this squad, run through ``parallel_model_fix_loop.branch_name``,
   keeping the ones that currently exist in the repo
   (``candidate_worker_branches``) -- UNIONED with any squad-mode slot
   branches (spec S2: ``model-fix-parallel-<squad>-<n>``) currently
   present (``squad_slot_branches``), whose per-commit format comes from
   the commit's own ``Format:`` trailer rather than a static per-branch
   mapping (``commit_format_trailer``).
3. Per candidate branch, find commits ahead of ``squad/<squad>`` (oldest
   first) not already recorded ``consumed``/``quarantined`` in this
   squad's status file (``candidate_commits``).
4. Per candidate commit, in order (``process_commit``):
   a. patch-id novelty vs origin/main UNION squad/<squad> (``git
      cherry``, via ``parallel_model_fix_loop.novel_commits``) -- not
      novel -> recorded ``consumed`` with ``work_done=False`` (the
      change already landed elsewhere by patch-id; nothing to do).
   b. quarantine-ledger lookup by patch-id -- already quarantined ->
      skipped without retry, no re-validation, no second ledger entry.
   c. ``validate_fix_commit.validate_commit`` -- any flag -> quarantined
      (never silently dropped: a rejection entry is always appended).
   d. clean validate: cherry-pick onto a DETACHED HEAD in the staging
      worktree (never the branch directly), run the targeted test gate
      and a pre/post duplicate-emission / new-oxidex-only-key recheck.
      Any failure here quarantines too, and the staging worktree is left
      re-attached to squad/<squad>'s UNMOVED tip -- squad/<squad> is
      never pointed at a tree that was not fully validated.
   e. full success: fast-forward squad/<squad> to the validated detached
      head (a plain ref update, never a merge commit -- append-only
      publication), record the ORIGINAL worker-branch head ``consumed``
      in squad-status, and auto-log a ``machine_accepted``/
      ``machine_rejected`` sweep-review entry (spec M6, called as a
      library import) for the outcome.
5. Batch full-corpus check every ``batch_commits`` commits or
   ``batch_seconds`` seconds since the last one, whichever first
   (``run_batch_check``): compares the staging tip's current per-format
   reports against the last recorded baseline; on failure this logs
   loudly and blocks further publication (next poll's candidates are
   still validated/cherry-picked, but the branch is not fast-forwarded)
   until the next successful batch check -- it never crashes the daemon.

Squad branch re-cut (``recut_squad_branch``, spec M5): re-cuts
squad/<squad> from origin/main and re-cherry-picks only the still-open
(patch-id-novel-vs-origin/main) consumed commits recorded in
squad-status. Invokable standalone (``--recut``) and checked
automatically at the start of every poll once the merge-base with
origin/main is older than ``--recut-staleness-seconds``.

Singleton discipline mirrors distill_lessons.py's lock takeover exactly
(the lock-file helpers are generically named there, so this module
imports and reuses them rather than duplicating the logic): lock file
``<home>/logs/knowledge/merger-<squad>.lock`` carries
``{pid, script_git_sha, heartbeat_ts}``; a stale heartbeat (>10 min) or a
script-sha mismatch SIGTERMs the holder and takes over.

Everything side-effectful that would need network, a real cargo build,
or the real ~/.oxidex is injectable: ``validate_fn``,
``cargo_test_targeted_fn``, ``comparison_fn``, ``kill_fn``, ``now_fn``,
``sleep_fn``. Hermetic tests exercise the git mechanics (staging
worktree, detached-HEAD cherry-pick, fast-forward-only publish) against
real tempdir git repos, matching test_validate_fix_commit.py's style.

Usage:
    uv run scripts/squad_merge_loop.py --squad nikon --once
    uv run scripts/squad_merge_loop.py --squad canon --infinite
    uv run scripts/squad_merge_loop.py --squad canon --recut
"""
import argparse
import errno
import json
import os
import shutil
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
import tempfile
import time
import tomllib
import traceback
from pathlib import Path

from find_tag_gaps import (
    OXIDEX_HOME,
    REPO_ROOT,
    group_gaps_by_format,
    load_comparison_report,
    run_format_comparison,
)
from parallel_model_fix_loop import branch_name as worker_branch_name
from parallel_model_fix_loop import novel_commits
from find_tag_gaps import DEFAULT_BUILD_SEMAPHORE_MAX_HOLDERS, DEFAULT_BUILD_SEMAPHORE_PATH
from model_fix_loop import cargo_test_targeted as _real_cargo_test_targeted
from model_fix_loop import new_oxidex_only_keys, newly_duplicated_emissions
import validate_fix_commit
from validate_fix_commit import validate_commit as _real_validate_commit
from distill_lessons import (
    STALE_HEARTBEAT_SECONDS,
    acquire_lock,
    compute_script_sha,
    release_lock,
    write_lock,
)
from log_sweep_review import append_from_commits, make_git_runner

SCRIPTS_DIR = Path(__file__).resolve().parent
DEFAULT_SQUADS_TOML = SCRIPTS_DIR / "squads.toml"

DEFAULT_STAGING_BASE = OXIDEX_HOME / "worktrees" / "squad-staging"
DEFAULT_WORKTREE_DIR = OXIDEX_HOME / "worktrees" / "parallel-fix"

# Spec M2 default knobs.
DEFAULT_POLL_SECONDS = 120
DEFAULT_BATCH_COMMITS = 10
DEFAULT_BATCH_SECONDS = 900

# Spec M5 janitor uses the same ">3 days" figure for worktree staleness;
# reused here for the squad branch re-cut trigger absent a more specific
# number in the spec.
DEFAULT_RECUT_STALENESS_SECONDS = 3 * 24 * 3600

#: Re-cut once origin/main has moved this far past the squad branch's base,
#: regardless of how RECENT that base is.
#:
#: The time threshold above assumes main moves slowly. It does not: with the
#: fleet publishing, main advanced 13 commits in about four hours on
#: 2026-07-27, and every squad branch sat on a base that was hours old and
#: therefore "fresh" by the time rule -- while being far enough behind that
#: ALL THIRTEEN failed to merge. Simulated against current main that day:
#: merged cleanly 0, conflicted 13.
#:
#: The conflict is not incidental. The blocking commit was b9d0245d, the
#: sweep's own PR #154: main now holds the merged-and-formatted form of work
#: the squad branches still carry in pre-merge form. A branch is stale the
#: moment its content lands upstream by another route, which is a DISTANCE
#: question, not an age one.
DEFAULT_RECUT_BEHIND_COMMITS = 8

ORIGIN_MAIN = "origin/main"

# Operational code is different from ordinary parser drift: workers execute
# these files directly from their squad worktree.  Waiting for the generic
# eight-commit/three-day re-cut cadence after one of them changes leaves a
# running fleet on the old scheduler, governor, comparison harness, or build
# policy even though the supervisor itself is already on the new main.  A
# single upstream change here therefore forces the same loss-checked re-cut
# used by the normal staleness policy.  Test modules do not affect a running
# worker and are deliberately excluded below.
IMMEDIATE_RECUT_PATHS = ("Cargo.toml", "Cargo.lock", "justfile", "Justfile")
IMMEDIATE_RECUT_PREFIXES = ("scripts/", "src/bin/tag-comparison/")


# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------

def merger_lock_path(home, squad):
    return Path(home) / "logs" / "knowledge" / f"merger-{squad}.lock"


def squad_status_file(home, squad):
    return Path(home) / "logs" / "squad-status" / f"{squad}.json"


def quarantine_ledger_path(home):
    return Path(home) / "logs" / "quarantine.jsonl"


def batch_state_path(home, squad):
    return Path(home) / "logs" / "squad-status" / f"{squad}-batch.json"


def staging_branch(squad):
    return f"squad/{squad}"


def default_staging_dir(home, squad):
    return Path(home) / "worktrees" / "squad-staging" / squad


# ---------------------------------------------------------------------------
# squads.toml
# ---------------------------------------------------------------------------

def all_squad_names(squads_toml_path):
    """Every squad name in squads.toml, or [] if it cannot be read.

    Used to find which OTHER squad is already staging a file -- see
    squad_holding_any_file. Unreadable config must never BLOCK a merger, so
    a failure here degrades to "no other squads known", i.e. today's
    behaviour.
    """
    try:
        with open(squads_toml_path, "rb") as f:
            data = tomllib.load(f)
    except (OSError, ValueError):
        return []
    return sorted((data.get("squads") or {}).keys())


def squad_formats(squads_toml_path, squad):
    """squads.toml's advisory ``formats`` list for `squad` (spec S2) --
    the candidate-branch source (item 3a: read literally, not narrowed to
    a "wholly owned" subset -- that curation is an operational rollout
    decision, made by whichever squads get a merger launched during the
    Phase 2 pilot, not by this function)."""
    with open(squads_toml_path, "rb") as f:
        data = tomllib.load(f)
    squads = data.get("squads") or {}
    cfg = squads.get(squad)
    if cfg is None:
        raise ValueError(f"squad {squad!r} not found in {squads_toml_path}")
    return list(cfg.get("formats") or [])


def format_owner_map(squads_toml_path):
    """format (lowercased) -> the ONE squad that consumes that format's
    legacy per-format worker branch.

    squads.toml's ``formats`` is many-to-one on purpose -- 13 of the 14
    squads list "JPEG" -- and it is correct AS the advisory "which
    formats must this squad re-verify" list its own comment calls it.
    It is NOT a work partition, and candidate_worker_branches was reading
    it as one: there is exactly ONE model-fix-parallel-jpeg branch, so
    all 13 mergers resolved to the same branch and each cherry-picked the
    same commits onto its own staging branch. Measured 2026-07-30: a
    single 28-line change to app12_olympus.rs existed as 13 commits
    sharing one patch-id (5703eaa44c114f4c), 8 of them created inside a
    37-second window; squad/tail, the only squad not listing JPEG, was
    the only one without it. Across all 14 branches, 22 commits carried
    just 7 distinct patch-ids.

    The existing guards (squad_that_already_staged, squad_holding_any_file)
    cannot close this: both read state another merger only writes AFTER a
    full cherry-pick + cargo test + corpus compare, while all 13 mergers
    poll on the same 60s cadence -- so they all look, all see nothing
    staged, and all proceed. squad_that_already_staged's own docstring
    concedes it ("a race between two mergers checking at once degrades to
    exactly today's behaviour"). Those guards stay: they remain the right
    second line of defence for genuinely different code touching one file.
    This function removes the race instead of narrowing it.

    Ownership rule, in order:
      1. the squad whose ``modules`` names the format itself -- JPEG.pm
         belongs to standards-appn, XMP.pm to xmp, PDF.pm to ps-docs.
         This is squads.toml's own documented routing key ("attribute_gaps
         routes every attributed gap module -> squad through this table").
      2. otherwise the MOST SPECIALISED claimant: fewest formats listed,
         so RW2 goes to panasonic-leica (2) rather than exif-core (9).
      3. ties broken by squad name, so the map is stable.

    Pure and total -- config in, mapping out. No clock, no disk state, no
    lock, so two mergers evaluating in the same instant cannot both claim.
    """
    with open(squads_toml_path, "rb") as f:
        squads = tomllib.load(f).get("squads") or {}
    sizes = {name: len(cfg.get("formats") or []) for name, cfg in squads.items()}
    module_owner = {}
    for name in sorted(squads):
        for module in squads[name].get("modules") or []:
            module_owner.setdefault(module.lower(), name)
    owners = {}
    for name in sorted(squads):
        for fmt in squads[name].get("formats") or []:
            key = fmt.lower()
            if module_owner.get(key) == name:
                owners[key] = name          # rule 1 -- names the module outright
                continue
            if key in owners and module_owner.get(key) == owners[key]:
                continue                    # rule 1 already settled it
            if key not in owners or sizes[name] < sizes[owners[key]]:
                owners[key] = name          # rules 2 and 3
    return owners


# ---------------------------------------------------------------------------
# Git mechanics (real subprocess; list-argv only, no shell=True)
# ---------------------------------------------------------------------------

def _git(args, cwd, check=True, input_text=None):
    return subprocess.run(  # nosec B603
        ["git", *args], cwd=cwd, capture_output=True, text=True,
        check=check, input=input_text,
    )


def branch_exists(repo_root, branch):
    result = _git(["rev-parse", "--verify", "--quiet", f"refs/heads/{branch}"], repo_root, check=False)
    return result.returncode == 0


def branch_head_sha(repo_root, branch):
    result = _git(["rev-parse", "--verify", "--quiet", branch], repo_root, check=False)
    return result.stdout.strip() if result.returncode == 0 else None


def new_commits_since(repo_root, since_ref, branch):
    """Commit shas on `branch` not reachable from `since_ref`, oldest first."""
    result = _git(["log", f"{since_ref}..{branch}", "--format=%H", "--reverse"], repo_root)
    return [line for line in result.stdout.splitlines() if line]


def checkout_detached(staging_path, ref):
    _git(["checkout", "--detach", ref], staging_path)


def checkout_branch(staging_path, branch):
    _git(["checkout", branch], staging_path)


def cherry_pick(staging_path, sha):
    """Cherry-pick `sha` onto whatever is currently checked out (a
    detached HEAD, by the caller's convention). On failure the
    cherry-pick is aborted so the worktree is left clean. Returns
    (ok, message)."""
    result = _git(["cherry-pick", sha], staging_path, check=False)
    if result.returncode != 0:
        _git(["cherry-pick", "--abort"], staging_path, check=False)
        return False, (result.stdout + result.stderr).strip()
    return True, "cherry-picked"


def head_sha(path):
    return _git(["rev-parse", "HEAD"], path).stdout.strip()


def is_ancestor(repo_root, ancestor_ref, descendant_ref):
    result = _git(["merge-base", "--is-ancestor", ancestor_ref, descendant_ref], repo_root, check=False)
    return result.returncode == 0


def fast_forward_branch(repo_root, branch, new_sha):
    """Advance `branch` to `new_sha` via a plain ref update (`git
    update-ref`, not `branch -f`) so this works even while the branch is
    checked out in another worktree (git refuses `branch -f`/`checkout
    -B` on a branch checked out elsewhere; `update-ref` does not have
    that restriction). Asserts the move is a genuine fast-forward first
    -- spec M2's append-only publication rule: squad/<squad> is never
    pointed anywhere except a state reachable forward from where it was.
    Returns (ok, message)."""
    if not is_ancestor(repo_root, branch, new_sha):
        return False, f"{new_sha} is not a fast-forward of {branch} -- refusing to move the ref"
    result = _git(["update-ref", f"refs/heads/{branch}", new_sha], repo_root, check=False)
    if result.returncode != 0:
        return False, (result.stdout + result.stderr).strip()
    return True, "fast-forwarded"


def compute_patch_id_for_sha(repo_root, sha):
    """`git patch-id --stable` over sha's own diff -- same identity
    validate_fix_commit.compute_patch_id computes, recomputed here so the
    quarantine-ledger lookup (spec M2 step 1/2) can run BEFORE paying for
    a full validate_commit call."""
    show = _git(["show", "--format=", sha], repo_root)
    result = _git(["patch-id", "--stable"], repo_root, input_text=show.stdout)
    parts = result.stdout.split()
    return parts[0] if parts else ""


def ensure_squad_branch(repo_root, squad, origin_ref=ORIGIN_MAIN, log_fn=print):
    """Create squad/<squad> from origin_ref if it doesn't already exist
    -- just the branch-existence half of ensure_staging_worktree's
    bootstrap, factored out so a caller that only ever needs the REF to
    exist (never the staging WORKTREE's checked-out state) doesn't have
    to touch the merger's own staging worktree as a side effect.
    parallel_model_fix_loop.ensure_squad_staging_branch is exactly that
    caller: the dispatcher only needs squad/<squad> to exist so a squad
    worker's create_worktree can check it out as a base ref -- it must
    never reset/clean the staging worktree the squad's OWN merger daemon
    may be concurrently mid-cherry-pick/mid-test inside of, on its own
    ~120s poll cadence, with no lock coordination between the two
    processes. Returns the branch name."""
    branch = staging_branch(squad)
    if not branch_exists(repo_root, branch):
        _git(["branch", branch, origin_ref], repo_root)
        log_fn(f"created {branch!r} from {origin_ref}")
    return branch


def ensure_staging_worktree(repo_root, staging_path, squad, origin_ref=ORIGIN_MAIN, log_fn=print):
    """Make sure squad/<squad> exists (cut from origin_ref if not) and
    the staging worktree at `staging_path` exists, checked out on it.

    Mirrors parallel_model_fix_loop.create_worktree's reuse-in-place
    pattern: an existing directory is cleaned and checked out AS-IS
    (plain `checkout`, never `-B`/`branch -f`) -- squad/<squad> only ever
    advances via fast_forward_branch, never via a reset here. Returns the
    branch name.

    Recovery uses `git reset --hard HEAD`, not `checkout -- .`: a plain
    `checkout -- .` only restores tracked files to their index content --
    it does nothing about an UNMERGED index (a conflicted cherry-pick
    whose own `--abort` never ran, e.g. this process was SIGTERM'd
    mid-`subprocess.run` by the lock-takeover path or by
    stop_parallel_fix.py's merger-pgid reaping). Left as `checkout -- .`,
    every following poll's plain `checkout branch` call fails outright
    ("needs merge") and the whole daemon dies uncaught, permanently,
    until a human runs `git reset --hard`/`cherry-pick --abort` by hand.
    `reset --hard HEAD` clears both the dirty worktree AND any in-progress
    cherry-pick/merge/revert state in one shot, so this recovers on the
    very next poll with no human involved.

    ONLY this merger's own poll cycle (and its --recut bootstrap) may
    ever call this: it resets/cleans the staging worktree unconditionally,
    which would corrupt a concurrent in-flight cherry-pick/test if any
    other process called it against the same staging_path. A caller that
    only needs squad/<squad> to EXIST (never the worktree) must use
    ensure_squad_branch instead."""
    staging_path = Path(staging_path)
    branch = ensure_squad_branch(repo_root, squad, origin_ref=origin_ref, log_fn=log_fn)
    if staging_path.is_dir():
        _git(["reset", "--hard", "HEAD"], staging_path, check=False)
        _git(["clean", "-fd"], staging_path, check=False)
        _git(["checkout", branch], staging_path)
    else:
        staging_path.parent.mkdir(parents=True, exist_ok=True)
        _git(["worktree", "add", str(staging_path), branch], repo_root)
    return branch


# ---------------------------------------------------------------------------
# Candidate discovery
# ---------------------------------------------------------------------------

def candidate_worker_branches(repo_root, squads_toml_path, squad):
    """(format, branch) pairs for every format squads.toml lists under
    `squad` whose LEGACY per-format worker branch currently exists in
    the repo (spec M2 step (a); parallel_model_fix_loop.branch_name's
    one-branch-per-format naming, from run_round/process_format).

    Squad-mode dispatch (spec S2, run_squad_round/process_squad_worker)
    creates DIFFERENT branches -- model-fix-parallel-<squad>-<n>, one per
    allocated slot -- which this function deliberately does not look for
    (a slot's format cycles round to round, so there is no static
    per-branch format to pair it with the way there is here); see
    squad_slot_branches / commit_format_trailer for that discovery path,
    consulted alongside this one in poll_once.

    Only the format's OWNING squad consumes its worker branch (see
    format_owner_map): a per-format branch is a single shared resource,
    and handing it to all 13 squads that merely LIST the format is what
    produced 13 copies of one patch-id. Filtering here, rather than in
    squad_formats, is deliberate and load-bearing -- poll_once unions
    squad_formats into the batch full-corpus check, and a squad must
    still VERIFY JPEG even once it no longer CONSUMES the JPEG branch."""
    owners = format_owner_map(squads_toml_path)
    out = []
    for fmt in squad_formats(squads_toml_path, squad):
        if owners.get(fmt.lower(), squad) != squad:
            continue
        branch = worker_branch_name(fmt)
        if branch_exists(repo_root, branch):
            out.append((fmt, branch))
    return out


def squad_slot_branches(repo_root, squad):
    """Squad-mode worker branches (spec S2 worker identity:
    model-fix-parallel-<squad>-<n>, one per allocated slot) currently
    present in the repo -- discovered by git ref pattern rather than a
    static squads.toml list, since a squad's slot COUNT varies round to
    round with allocate_squad_slots and a single slot's FORMAT can also
    change round to round (squad_worker_formats round-robins a slot
    through the squad's formats). These branches carry no single fixed
    format the way a legacy candidate_worker_branches branch does, so a
    candidate commit's format comes from its own `Format:` trailer
    instead (see commit_format_trailer) -- not from this function.

    Together with candidate_worker_branches, this is the full set of
    branches a squad's merger must consume (poll_once processes both);
    without it, every squad-mode worker's commits would sit unconsumed
    forever, since candidate_worker_branches only ever matches the
    legacy per-format naming."""
    result = _git(
        ["for-each-ref", "--format=%(refname:short)", f"refs/heads/model-fix-parallel-{squad}-*"],
        repo_root, check=False,
    )
    return [line for line in result.stdout.splitlines() if line]


def commit_format_trailer(repo_root, sha):
    """The commit's own `Format:` trailer (spec M1) -- the format ground
    truth for a squad-slot worker-branch commit, whose slot may cycle
    through several of the squad's formats round to round (unlike a
    legacy per-format branch, which is exactly one format for its whole
    life, known statically). Reuses validate_fix_commit's own
    commit_message/parse_trailers (`git interpret-trailers --parse`) --
    the exact same trailer parser validate_fix_commit.py and
    overlord_sweep.py already use -- rather than a second hand-rolled
    reader. Returns None when the commit carries no such trailer (an
    absent Format: trailer is itself flagged by
    validate_fix_commit.check_trailers as missing-trailer:Format, so
    this only ever surfaces as a quarantine, never a crash from a
    downstream fmt.lower() on None)."""
    def run(args, repo, input_text=None):
        result = _git(args, repo, check=False, input_text=input_text)
        return result.returncode, result.stdout, result.stderr

    message = validate_fix_commit.commit_message(sha, repo_root, run)
    trailers = validate_fix_commit.parse_trailers(message, repo_root, run)
    values = [v for v in trailers.get("Format", []) if v]
    return values[0] if values else None


def stale_policy(entry, policy_version=None):
    """True when `entry` is a quarantine recorded under a DIFFERENT
    validator policy than the one running now, so the rejection was
    reached by rules we no longer apply and deserves one fresh attempt.

    Entries written before policy stamping existed have no
    "policy_version" key at all; those are treated as stale too, which is
    exactly right -- they predate every fix that made them stale.

    A "consumed" entry is never stale: it is already published.
    """
    if not isinstance(entry, dict) or entry.get("status") != "quarantined":
        return False
    current = (
        validate_fix_commit.POLICY_VERSION if policy_version is None
        else policy_version
    )
    return entry.get("policy_version") != current


def candidate_commits(repo_root, worker_branch, squad_branch, status,
                       policy_version=None):
    """Oldest-first commits on `worker_branch` not yet reachable from
    `squad_branch` and not already recorded consumed/quarantined in
    `status` (spec M2 step (b)).

    A head quarantined under a superseded validator policy IS a candidate
    again -- see stale_policy. Otherwise a validator bugfix can never
    take effect retroactively, which is how 20 valid fixes stayed
    rejected across the whole of 2026-07-25 even after the rules that
    rejected them were repaired."""
    shas = new_commits_since(repo_root, squad_branch, worker_branch)
    heads = (status or {}).get("heads") or {}
    return [
        s for s in shas
        if s not in heads or stale_policy(heads[s], policy_version)
    ]


def union_novel_shas(repo_root, worker_branch, origin_ref, squad_branch):
    """Shas on `worker_branch` whose patch-id is novel against BOTH
    origin_ref and squad_branch (spec M2 step 1: novelty vs the UNION --
    already present in either means not novel)."""
    novel_vs_main = set(novel_commits(repo_root, origin_ref, worker_branch))
    novel_vs_squad = set(novel_commits(repo_root, squad_branch, worker_branch))
    return novel_vs_main & novel_vs_squad


def is_patch_novel_against(repo_root, base_ref, sha):
    """True when `sha`'s own patch-id is not already present in
    `base_ref` -- `git cherry base_ref sha` walks every ancestor of sha
    not in base_ref (not just sha itself), oldest first, so only the
    LAST line (sha's own verdict, since sha is the head passed in)
    answers the question this function asks. Empty output means sha's
    patch-id is fully contained in base_ref already."""
    result = _git(["cherry", base_ref, sha], repo_root, check=False)
    lines = [line for line in result.stdout.splitlines() if line]
    if not lines:
        return False
    return lines[-1].startswith("+ ")


# ---------------------------------------------------------------------------
# Quarantine ledger (spec M2: JSONL, patch-id keyed, one entry per rejection)
# ---------------------------------------------------------------------------

def load_quarantine(path):
    """{patch_id: latest entry} folded from the append-only ledger.
    Malformed lines are skipped (never raised on) -- same discipline as
    every other JSONL ledger in this fleet (lessons.jsonl, sweep-review).
    A missing file just means nothing is quarantined yet."""
    entries = {}
    path = Path(path)
    if not path.exists():
        return entries
    try:
        lines = path.read_text().splitlines()
    except OSError:
        return entries
    for line in lines:
        line = line.strip()
        if not line:
            continue
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(entry, dict):
            continue
        patch_id = entry.get("patch_id")
        if not patch_id:
            continue
        prior = entries.get(patch_id)
        if prior is None or entry.get("attempt", 0) >= prior.get("attempt", 0):
            entries[patch_id] = entry
    return entries


def append_quarantine(path, *, patch_id, sha, format_name, squad, reason, flags,
                       quarantine_entries=None, now_fn=time.time,
                       policy_version=None):
    """Append one rejection entry (K1-style: O_APPEND|O_CREAT|O_WRONLY,
    exactly one os.write of one line). A quarantined patch-id is skipped
    without retry by every later poll (spec M2) -- the attempt/backoff
    fields exist for operator visibility, not because this daemon ever
    automatically retries a quarantined patch-id.

    The ONE exception is a change to the acceptance rules themselves:
    `policy_version` stamps which validator policy did the rejecting, so
    a later poll running a DIFFERENT policy can give the head one fresh
    attempt (see quarantine_blocks and candidate_commits). A rejection is
    only permanent with respect to the rules that produced it."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    prior_attempt = 0
    if quarantine_entries and patch_id in quarantine_entries:
        prior_attempt = quarantine_entries[patch_id].get("attempt", 0)
    attempt = prior_attempt + 1
    backoff_seconds = min(3600, 60 * (2 ** (attempt - 1)))
    stamped_at = now_fn()
    entry = {
        # Same pair, same reasoning as record_head's: a human-readable
        # local `ts` plus an unambiguous instant, from ONE clock read.
        # Nothing orders the quarantine ledger by time today, which is
        # exactly why the offset-free string is worth pairing before
        # something does.
        "ts": time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime(stamped_at)),
        "ts_epoch": float(stamped_at),
        "patch_id": patch_id,
        "sha": sha,
        "format": format_name,
        "squad": squad,
        "reason": reason,
        "flags": list(flags or []),
        "attempt": attempt,
        "backoff_seconds": backoff_seconds,
        "policy_version": (
            validate_fix_commit.POLICY_VERSION if policy_version is None
            else policy_version
        ),
    }
    line = (json.dumps(entry, separators=(",", ":")) + "\n").encode("utf-8")
    fd = os.open(str(path), os.O_APPEND | os.O_CREAT | os.O_WRONLY, 0o644)
    try:
        os.write(fd, line)
    finally:
        os.close(fd)
    return entry


# ---------------------------------------------------------------------------
# Squad-status ledger (spec M2/M5: consumed/quarantined heads, tempfile+replace)
# ---------------------------------------------------------------------------

def load_squad_status(path):
    """{"heads": {sha: entry}} -- a missing file or a parse failure both
    read as "no news" (never raise, never wipe): a corrupt reader must
    not accidentally unblock the consume handshake."""
    path = Path(path)
    if not path.exists():
        return {"heads": {}}
    try:
        data = json.loads(path.read_text())
    except (OSError, ValueError):
        return {"heads": {}}
    if not isinstance(data, dict) or not isinstance(data.get("heads"), dict):
        return {"heads": {}}
    return data


def stamp_order_key(sha_entry):
    """Sort key for a (sha, squad-status entry) pair, newest last.

    Orders by the entry's `ts_epoch` INSTANT when it has one and falls
    back to the legacy naive-local-time `ts` string when it does not, so
    a ledger written before 2026-07-26 keeps exactly the order it always
    had. Legacy entries sort ahead of every epoch-stamped one (the
    leading `is not None`), which is the truth of the matter: they can
    only have been written earlier than the fix that added the field.

    Shared by overlord_sweep.collect_green_stamps (which head is "the
    newest news") and recut_squad_branch (which order to replay
    cherry-picks in) so the two can never disagree about "newer".
    """
    entry = sha_entry[1]
    epoch = entry.get("ts_epoch")
    return (epoch is not None, float(epoch) if epoch is not None else 0.0, entry.get("ts") or "")


def _write_squad_status(path, data):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=str(path.parent), prefix=f".{path.name}.", suffix=".tmp")
    try:
        with os.fdopen(fd, "w") as f:
            json.dump(data, f, indent=2)
        os.replace(tmp, str(path))
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def record_head(path, sha, *, status, patch_id, format_name, work_done=True,
                 squad_sha=None, reason=None, now_fn=time.time,
                 policy_version=None):
    """Record one worker-branch head's outcome (spec M2 step 5 / M5
    consume handshake): status is "consumed" or "quarantined". Written
    via tempfile + os.replace so a concurrent reader (create_worktree's
    consume handshake) never sees a torn file."""
    if status not in ("consumed", "quarantined"):
        raise ValueError(f"status must be 'consumed' or 'quarantined', got {status!r}")
    data = load_squad_status(path)
    # ONE clock read for both spellings of the same moment: `ts` and
    # `ts_epoch` must describe the same instant, and now_fn is injectable
    # (an advancing test clock would otherwise silently disagree).
    stamped_at = now_fn()
    entry = {
        "status": status,
        "patch_id": patch_id,
        "format": format_name,
        "work_done": work_done,
        # `ts` stays offset-free LOCAL time, for humans reading the
        # ledger and for every consumer written before 2026-07-26.
        # `ts_epoch` is the unambiguous INSTANT, and it is what
        # overlord_sweep.collect_green_stamps orders by: inside the DST
        # fall-back's repeated hour two different instants produce local
        # strings that sort the WRONG WAY ROUND. Measured with
        # TZ=America/Los_Angeles: 1793521800 -> "2026-11-01T01:30:00"
        # (PDT) and 1793524500 -> "2026-11-01T01:15:00" (PST, 2700s
        # LATER), and "01:15:00" < "01:30:00", so the later head was
        # filtered out as "not news" and its already-validated commits
        # waited for that squad's next stamp.
        #
        # Added ALONGSIDE `ts` rather than replacing it, deliberately.
        # Swapping `ts` to UTC breaks the migration: cursors and stamps
        # already on disk hold naive local strings, and in a
        # positive-offset zone a fresh UTC string sorts BELOW the stored
        # local cursor -- every squad would read "no news" for up to the
        # length of the offset (9h in Asia/Tokyo), a real stall, worse
        # than the once-a-year fold.
        "ts": time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime(stamped_at)),
        "ts_epoch": float(stamped_at),
    }
    if squad_sha is not None:
        entry["squad_sha"] = squad_sha
    if reason is not None:
        entry["reason"] = reason
    if status == "quarantined":
        # Only rejections carry a policy stamp -- a "consumed" head is
        # already published and must never be reconsidered, no matter how
        # the acceptance rules move.
        entry["policy_version"] = (
            validate_fix_commit.POLICY_VERSION if policy_version is None
            else policy_version
        )
    data["heads"][sha] = entry
    _write_squad_status(path, data)
    return data


# ---------------------------------------------------------------------------
# Comparison / test wiring (production defaults; injectable for tests)
# ---------------------------------------------------------------------------

def real_format_match(repo_root, cache_dir, fmt, out_suffix,
                       semaphore_max_holders=DEFAULT_BUILD_SEMAPHORE_MAX_HOLDERS):
    """One fresh single-format comparison, scoped by out_suffix so
    concurrent processes (workers, other squads' mergers, the sweep)
    never clobber each other's /tmp/tagcmp-<FMT>-<suffix> report (spec
    S1). Mirrors model_fix_loop.py's real_fix_tag.current_match().

    Spec section 5 build semaphore: this re-runs ensure_tag_comparison_built
    (a full `cargo build --profile fixloop --bin tag-comparison`) on every
    per-commit pre/post check and every batch full-corpus recheck -- shares
    the same cross-process slot ceiling every worker's cargo build/test
    call goes through (mirrors real_cargo_test_targeted's own
    semaphore_path=DEFAULT_BUILD_SEMAPHORE_PATH wiring just below)."""
    path = run_format_comparison(
        fmt, cache_dir, repo_root=repo_root, out_suffix=out_suffix,
        semaphore_path=DEFAULT_BUILD_SEMAPHORE_PATH, semaphore_max_holders=semaphore_max_holders,
    )
    regrouped = group_gaps_by_format(load_comparison_report(path))
    return next((g for g in regrouped if g["format"] == fmt), None)


def real_cargo_test_targeted(repo_root, filter_str, semaphore_max_holders=DEFAULT_BUILD_SEMAPHORE_MAX_HOLDERS):
    """Spec section 5: the merger's own targeted-test call site shares
    the same cross-process build semaphore every worker's cargo
    build/test call goes through (model_fix_loop.cargo_test_targeted's
    own semaphore_path/semaphore_max_holders params) -- a merger
    cherry-picking and test-gating several commits per poll is itself
    a cargo-build-heavy process, and must count against the same host
    core-oversubscription ceiling as the workers it's validating behind."""
    return _real_cargo_test_targeted(
        repo_root, filter_str,
        semaphore_path=DEFAULT_BUILD_SEMAPHORE_PATH, semaphore_max_holders=semaphore_max_holders,
    )


def real_validate_commit(sha, repo, **kwargs):
    return _real_validate_commit(sha, repo, **kwargs)


# ---------------------------------------------------------------------------
# Per-commit processing (spec M2 step (c)/(d))
# ---------------------------------------------------------------------------

def squad_that_already_staged(home, squad, patch_id):
    """The OTHER squad that already green-stamped this exact patch, or None.

    squads.toml deliberately lets several squads claim one format -- JPEG
    carries every brand's makernotes, so canon/nikon/sony all care about it --
    and squad_slot_branches discovers work by ref pattern with no ownership
    filter at all. The result is that every squad consumes the same worker
    commit and each pays a full cherry-pick + cargo test + corpus comparison
    to reach the identical verdict.

    Measured 2026-07-27 with the fleet up: 35 staged commits were only 7
    distinct patches. fix(rar) had been consumed by 13 squads and fix(jpeg) by
    12 -- about 80% of all merger work was redundant. It is also how the sweep
    ended up with 12 no-op merges of one patch, which is the empty-revert case
    #149 had to handle.

    Deliberately keyed on PATCH-ID, not sha: the same fix reaches different
    squads under different shas via cherry-pick.

    This only ever SKIPS duplicate effort -- the first squad to see a patch
    still consumes it normally, so nothing can be starved. A race between two
    mergers checking at once degrades to exactly today's behaviour (both
    consume), never to neither.
    """
    directory = squad_status_file(home, squad).parent
    if not directory.is_dir():
        return None
    for path in sorted(directory.glob("*.json")):
        other = path.stem
        if other == squad:
            continue
        for entry in (load_squad_status(path).get("heads") or {}).values():
            if (entry.get("patch_id") == patch_id
                    and entry.get("status") == "consumed"
                    and entry.get("work_done")):
                return other
    return None


def commit_files(repo_root, sha, run_git=None):
    """Paths one commit changes. Empty on any git failure."""
    runner = run_git or make_git_runner(repo_root)
    try:
        out = runner(["show", "--name-only", "--format=", sha])
    except Exception:  # noqa: BLE001 -- must never block a merger
        return set()
    return {line.strip() for line in out.splitlines() if line.strip()}


def files_touched(repo_root, base, tip, run_git=None):
    """Paths changed between `base` and `tip`. Empty on any git failure.

    make_git_runner's contract is run_git(args, input_text=None) -> stdout,
    with check=True, so a failure raises rather than returning a code.
    """
    runner = run_git or make_git_runner(repo_root)
    try:
        out = runner(["diff", "--name-only", f"{base}..{tip}"])
    except Exception:  # noqa: BLE001 -- an unreadable range must never block
        return set()
    return {line.strip() for line in out.splitlines() if line.strip()}


def squad_holding_any_file(repo_root, squad, files, squads, origin_ref=ORIGIN_MAIN,
                           run_git=None):
    """The OTHER squad already staging work that touches one of `files`.

    Enforces the invariant merge_squad_into_sweep is written against and
    states in its own docstring -- "one squad per shared emitter file" --
    which nothing was actually maintaining.

    squads.toml deliberately lets several squads claim a format (JPEG carries
    every brand's makernotes), and squad_slot_branches discovers work by ref
    pattern with no ownership filter at all. So two squads routinely fix the
    SAME tags in the SAME file with DIFFERENT code. Different code means
    different patch-ids, so the cross-squad patch dedup (#150) correctly lets
    both through -- and then they conflict textually when the sweep merges
    them.

    Measured 2026-07-27, and this is now the binding constraint on publishing:

      * PR #154 shipped duplicate match arms (0xA405/0xA407/0xA411/0xA412/
        0xA413 listed twice in src/parsers/raw/metadata.rs) -- six clippy
        errors, born red.
      * The very next sweep collected four squads' stamps and every single
        merge failed:
            HARD ERROR: cross-squad conflict merging squad/exif-core
            HARD ERROR: cross-squad conflict merging squad/ps-docs
            HARD ERROR: cross-squad conflict merging squad/sigma-c2pa
            HARD ERROR: cross-squad conflict merging squad/xmp
        -> nothing_merged, in one second, with real work available.

    #154 published only because that round happened to hold two COMPATIBLE
    squads. That was luck, not robustness.

    Deferring is deliberately NOT consuming: the commit is left for a later
    poll, so it lands once the holding squad's work reaches origin/main and
    the file is free again. Nothing is dropped and nothing is starved.
    """
    if not files:
        return None, []
    runner = run_git or make_git_runner(repo_root)
    for other in sorted(squads):
        if other == squad:
            continue
        branch = f"squad/{other}"
        if not branch_exists(repo_root, branch):
            continue
        try:
            base = runner(["merge-base", branch, origin_ref]).strip()
        except Exception:  # noqa: BLE001
            continue
        if not base:
            continue
        staged = files_touched(repo_root, base, branch, run_git=runner)
        overlap = files & staged
        if overlap:
            return other, sorted(overlap)[:3]
    return None, []


def process_commit(*, repo_root, staging_path, squad, squad_branch, sha, fmt, is_novel,
                    quarantine_entries, cache_dir, home, validate_fn, cargo_test_targeted_fn,
                    all_squads=None, origin_ref=ORIGIN_MAIN,
                    comparison_fn, sweep_review_log_path=None, validate_kwargs=None,
                    now_fn=time.time, log_fn=print):
    """Process exactly one candidate commit through the full merger
    pipeline. Returns a result dict with at least {"sha", "outcome",
    "patch_id"}; outcome is one of "consumed_no_work" (patch-id already
    present upstream), "consumed_elsewhere" (another squad already
    green-stamped this exact patch-id), "skipped_quarantined" (already in the ledger,
    zero work done), "quarantined" (freshly rejected this call), or
    "consumed" (green-stamped and fast-forwarded).

    Never raises for an ordinary rejection (build/test/recheck failure,
    validate flags) -- those are quarantine outcomes, not exceptions.
    """
    validate_kwargs = validate_kwargs or {}
    status_path = squad_status_file(home, squad)
    run_git = make_git_runner(repo_root)
    patch_id = compute_patch_id_for_sha(repo_root, sha)

    if not is_novel:
        record_head(status_path, sha, status="consumed", patch_id=patch_id,
                     format_name=fmt, work_done=False, now_fn=now_fn)
        log_fn(f"[{squad}] {sha[:12]} ({fmt}): patch-id already present upstream -- "
               "marked consumed, no work done")
        return {"sha": sha, "outcome": "consumed_no_work", "patch_id": patch_id}

    # Before paying for a cherry-pick, a build and a corpus comparison, check
    # whether another squad already did exactly this. See
    # squad_that_already_staged -- 80% of merger work was this.
    # The invariant merge_squad_into_sweep assumes and nothing enforced:
    # one squad per shared emitter file. See squad_holding_any_file -- four
    # squads' merges all failed with cross-squad conflicts on 2026-07-27,
    # which is nothing_merged with real work sitting available.
    #
    # DEFER, never consume: no head is recorded, so this exact commit is
    # re-offered on a later poll once the holding squad's work reaches
    # origin/main and the file is free.
    if all_squads:
        # The commit's OWN changed files, not a diff against a ref. Using
        # ORIGIN_MAIN here silently returned nothing wherever that ref does
        # not exist -- which made the whole check inert instead of loud.
        changed = commit_files(repo_root, sha, run_git=run_git)
        holder, overlap = squad_holding_any_file(
            repo_root, squad, changed, all_squads, origin_ref=origin_ref,
            run_git=run_git)
        if holder:
            log_fn(f"[{squad}] {sha[:12]} ({fmt}): squad/{holder} is already staging "
                   f"{', '.join(overlap)} -- deferring rather than racing it into a "
                   "cross-squad conflict")
            return {"sha": sha, "outcome": "deferred_file_held", "patch_id": patch_id,
                    "held_by": holder, "files": overlap}

    staged_by = squad_that_already_staged(home, squad, patch_id)
    if staged_by:
        record_head(status_path, sha, status="consumed", patch_id=patch_id,
                    format_name=fmt, work_done=False, now_fn=now_fn)
        log_fn(f"[{squad}] {sha[:12]} ({fmt}): identical patch already staged by "
               f"squad/{staged_by} -- skipped without duplicating the work")
        return {"sha": sha, "outcome": "consumed_elsewhere", "patch_id": patch_id,
                "staged_by": staged_by}

    if patch_id in quarantine_entries and not stale_policy(
        {**quarantine_entries[patch_id], "status": "quarantined"}
    ):
        log_fn(f"[{squad}] {sha[:12]} ({fmt}): patch-id already quarantined -- skipped without retry")
        return {"sha": sha, "outcome": "skipped_quarantined", "patch_id": patch_id}
    if patch_id in quarantine_entries:
        log_fn(f"[{squad}] {sha[:12]} ({fmt}): quarantined under validator policy "
               f"{quarantine_entries[patch_id].get('policy_version')!r}, now on "
               f"{validate_fix_commit.POLICY_VERSION} -- retrying once under the new rules")

    def quarantine(reason, flags):
        entry = append_quarantine(
            quarantine_ledger_path(home), patch_id=patch_id, sha=sha, format_name=fmt,
            squad=squad, reason=reason, flags=flags,
            quarantine_entries=quarantine_entries, now_fn=now_fn,
        )
        quarantine_entries[patch_id] = entry
        record_head(status_path, sha, status="quarantined", patch_id=patch_id,
                    format_name=fmt, work_done=True, reason=reason, now_fn=now_fn)
        if sweep_review_log_path is not None:
            append_from_commits(
                sweep_review_log_path, [sha], run_git,
                verdict="machine_rejected", reason=f"quarantined: {reason}", now_fn=now_fn,
            )
        log_fn(f"[{squad}] {sha[:12]} ({fmt}) QUARANTINED: {reason}")
        return {"sha": sha, "outcome": "quarantined", "patch_id": patch_id,
                "reason": reason, "flags": flags}

    result = validate_fn(sha, repo_root, **validate_kwargs)
    if not result.get("ok", False):
        flags = result.get("flags") or []
        return quarantine(f"validate_fix_commit flags: {', '.join(flags)}", flags)

    # Detached-HEAD validate-then-ff publish (spec M2/D3-H4): squad/<squad>
    # is never checked out directly onto the candidate -- only a
    # FULLY validated state ever gets fast-forwarded onto it.
    checkout_detached(staging_path, squad_branch)
    # Each squad merger is its own process and several routinely compare the
    # same format at once. Include the squad in the suffix so their /tmp
    # reports and markdown directories cannot overwrite one another.
    comparison_suffix = f"squad-{squad}-staging"
    pre = comparison_fn(staging_path, cache_dir, fmt, comparison_suffix)

    ok, message = cherry_pick(staging_path, sha)
    if not ok:
        checkout_branch(staging_path, squad_branch)
        return quarantine(f"cherry-pick failed: {message}", ["cherry-pick-conflict"])

    test_ok, test_output = cargo_test_targeted_fn(staging_path, fmt.lower())
    if not test_ok:
        checkout_branch(staging_path, squad_branch)
        return quarantine(f"cargo test --lib {fmt.lower()} failed", ["targeted-test-failed"])

    post = comparison_fn(staging_path, cache_dir, fmt, comparison_suffix)
    # Both halves of this gate are now diffed against PRE. Reading
    # duplicate_emissions straight off POST held every format with a
    # pre-existing duplicate hostage -- see newly_duplicated_emissions.
    dup = newly_duplicated_emissions(pre, post) if pre is not None or post is not None else []
    introduced = new_oxidex_only_keys(pre, post) if pre is not None or post is not None else []
    if dup or introduced:
        checkout_branch(staging_path, squad_branch)
        reason_bits = []
        flags = []
        if dup:
            reason_bits.append(f"duplicate_emissions={dup}")
            flags.append("duplicate-emission")
        if introduced:
            reason_bits.append(f"new_oxidex_only={introduced}")
            flags.append("new-oxidex-only")
        return quarantine("; ".join(reason_bits), flags)

    published_sha = head_sha(staging_path)
    ff_ok, ff_message = fast_forward_branch(repo_root, squad_branch, published_sha)
    if not ff_ok:
        # Should not happen (one merger per squad, singleton lock) -- if
        # it ever does, never silently drop a validated commit: route it
        # to the quarantine ledger so a human sees the anomaly.
        checkout_branch(staging_path, squad_branch)
        return quarantine(f"fast-forward refused: {ff_message}", ["ff-refused"])

    checkout_branch(staging_path, squad_branch)
    record_head(status_path, sha, status="consumed", patch_id=patch_id, format_name=fmt,
                work_done=True, squad_sha=published_sha, now_fn=now_fn)
    if sweep_review_log_path is not None:
        append_from_commits(sweep_review_log_path, [published_sha], run_git,
                            verdict="machine_accepted", now_fn=now_fn)
    log_fn(f"[{squad}] {sha[:12]} ({fmt}) -> {published_sha[:12]} CONSUMED (green-stamped)")
    return {"sha": sha, "outcome": "consumed", "patch_id": patch_id, "squad_sha": published_sha}


# ---------------------------------------------------------------------------
# Batch full-corpus check (spec M2 step 3b)
# ---------------------------------------------------------------------------

def empty_batch_state():
    return {
        "blocked": False,
        "last_batch_ts": 0,
        "commits_since": 0,
        "baselines": {},
        "baseline_head": None,
        "staging_head": None,
    }


def load_batch_state(path):
    path = Path(path)
    if not path.exists():
        return empty_batch_state()
    try:
        data = json.loads(path.read_text())
    except (OSError, ValueError):
        return empty_batch_state()
    if not isinstance(data, dict):
        return empty_batch_state()
    data.setdefault("blocked", False)
    data.setdefault("last_batch_ts", 0)
    data.setdefault("commits_since", 0)
    data.setdefault("baselines", {})
    # Legacy state has neither head. Treating the absent staging_head as a
    # mismatch makes the first poll after this upgrade establish a baseline
    # from the tree it is actually running, rather than trusting a report
    # whose lineage cannot be proved.
    data.setdefault("baseline_head", None)
    data.setdefault("staging_head", None)
    return data


def save_batch_state(path, data):
    _write_squad_status(path, data)


def batch_check_due(state, batch_commits, batch_seconds, now_fn=time.time):
    """spec M2: every `batch_commits` commits OR `batch_seconds` seconds
    since the last batch check, whichever comes first."""
    if state.get("commits_since", 0) >= batch_commits:
        return True
    last = state.get("last_batch_ts") or 0
    return (now_fn() - last) >= batch_seconds


def run_batch_check(*, staging_path, squad, formats, cache_dir, comparison_fn,
                     baselines, log_fn=print):
    """FULL-corpus comparison for every format this squad owns, on the
    staging tip's CURRENT state vs the last recorded batch baseline.
    Never raises: a failure is logged loudly (ERROR-level) and reported
    via the returned `ok` flag -- callers hold publication until the
    next successful check (spec: "skip publishing further ... do not
    crash the daemon"); building a bisection system is explicitly out of
    scope (Phase 3 overlord work).

    Returns (ok, problems, new_baselines).
    """
    ok = True
    problems = []
    new_baselines = {}
    for fmt in formats:
        # This function's contract (above) is "never raises -- log loudly and
        # report via `ok`". comparison_fn bottoms out in
        # find_tag_gaps.run_format_comparison, which shells out with
        # check=True, so ANY non-zero exit (a SIGTERM from an operator's
        # pkill, an OOM kill, a diff that compiles under `--bin oxidex` but
        # breaks the tag-comparison-binary feature path) raised straight
        # through this loop and killed the whole daemon -- and nothing
        # respawns a merger. Observed live 2026-07-25: 7 of 14 mergers died
        # on this exact line within seconds of each other, stranding 68% of
        # worker slots with no publish path for over an hour.
        #
        # Deliberately does NOT fall through to whatever report may already
        # be on disk: /tmp accumulates tagcmp-*.json for days, so reusing a
        # stale one would silently hand a previous round's verdicts to the
        # publication gate -- turning a loud crash into a false "clean".
        # Treated as a check FAILURE (hold publication), which is the
        # existing, already-safe behavior for an unhealthy batch check.
        try:
            report = comparison_fn(
                staging_path, cache_dir, fmt, f"squad-{squad}-staging-batch"
            )
        except subprocess.CalledProcessError as exc:
            ok = False
            problems.append(f"{fmt}: comparison run failed ({exc})")
            new_baselines[fmt] = None
            continue
        new_baselines[fmt] = report
        if report is None:
            continue
        # Diffed against the squad's own prior baseline, exactly like the
        # new_oxidex_only check five lines below. #147 fixed this asymmetry in
        # the PER-COMMIT gate and missed this BATCH one, so publication stayed
        # blocked for every squad holding a format with pre-existing
        # duplicates -- NEF carries nine on clean main. Measured 2026-07-27:
        # the judgment daemon queued 57 of 58 entries, and the dominant reason
        # was "squad '<x>' publication is blocked by a failed batch check".
        dup = newly_duplicated_emissions(baselines.get(fmt), report)
        if dup:
            ok = False
            problems.append(f"{fmt}: duplicate_emissions {dup}")
        prior = baselines.get(fmt)
        if prior is not None:
            introduced = new_oxidex_only_keys(prior, report)
            if introduced:
                ok = False
                problems.append(f"{fmt}: unexplained new_oxidex_only {introduced}")
    if not ok:
        log_fn(
            f"ERROR: squad {squad!r} batch full-corpus check FAILED: {'; '.join(problems)} -- "
            "holding publication until the next successful batch check"
        )
    return ok, problems, new_baselines


def capture_batch_baselines(*, staging_path, squad, formats, cache_dir,
                            comparison_fn, log_fn=print):
    """Capture the current reports without judging them as newly introduced.

    This is used only when the staging tree moved outside the merger's
    observed per-commit flow (normally a squad re-cut onto a newer main).
    `run_batch_check(..., baselines={})` is not equivalent: it would blame
    the squad for duplicate emissions already present on the new main.
    """
    ok = True
    problems = []
    baselines = {}
    for fmt in formats:
        try:
            baselines[fmt] = comparison_fn(
                staging_path, cache_dir, fmt, f"squad-{squad}-staging-batch"
            )
        except subprocess.CalledProcessError as exc:
            ok = False
            problems.append(f"{fmt}: comparison run failed ({exc})")
            baselines[fmt] = None
    if not ok:
        log_fn(
            f"ERROR: squad {squad!r} could not refresh its batch baseline: "
            f"{'; '.join(problems)} -- holding publication until the next successful check"
        )
    return ok, problems, baselines


def refresh_batch_state_for_head_change(*, repo_root, staging_path, squad,
                                        squad_branch, origin_ref, formats,
                                        cache_dir, comparison_fn, state,
                                        now_fn=time.time, log_fn=print):
    """Reconcile persisted batch lineage with the staging worktree's HEAD.

    `staging_head` is updated whenever this merger intentionally consumes a
    commit. A mismatch therefore means an external ref move or a re-cut. If
    the moved tree is clean main, capture it as the new inherited baseline.
    If it still carries squad commits, capture main while detached and then
    validate the staging tip against that fresh baseline so the refresh can
    never silently bless retained, not-yet-batch-checked work.

    Returns (state, batch_result_or_none).
    """
    current_head = head_sha(staging_path)
    has_persisted_check = bool(
        state.get("baselines")
        or state.get("blocked")
        or (state.get("last_batch_ts") or 0)
    )
    if not has_persisted_check or state.get("staging_head") == current_head:
        return state, None

    previous_head = state.get("staging_head") or "unknown legacy head"
    log_fn(
        f"[{squad}] staging HEAD changed outside the recorded merger flow "
        f"({previous_head} -> {current_head}); refreshing batch baseline"
    )
    origin_head = branch_head_sha(repo_root, origin_ref)
    if not origin_head:
        problems = [f"cannot resolve {origin_ref}"]
        refreshed = {
            "blocked": True,
            "commits_since": state.get("commits_since", 0),
            "last_batch_ts": now_fn(),
            "baselines": state.get("baselines") or {},
            "baseline_head": state.get("baseline_head"),
            "staging_head": current_head,
        }
        log_fn(
            f"ERROR: squad {squad!r} cannot refresh its batch baseline: "
            f"cannot resolve {origin_ref} -- holding publication"
        )
        return refreshed, {"ran": True, "ok": False, "problems": problems,
                           "reason": "staging_head_changed"}

    if current_head == origin_head:
        ok, problems, current_baselines = capture_batch_baselines(
            staging_path=staging_path, squad=squad, formats=formats,
            cache_dir=cache_dir, comparison_fn=comparison_fn, log_fn=log_fn,
        )
        refreshed = {
            "blocked": not ok,
            "commits_since": 0,
            "last_batch_ts": now_fn(),
            "baselines": current_baselines,
            "baseline_head": current_head,
            "staging_head": current_head,
        }
        return refreshed, {"ran": True, "ok": ok, "problems": problems,
                           "reason": "staging_head_changed"}

    # The re-cut retained squad commits. Establish their comparison parent
    # from current main, then restore the squad branch before judging them.
    checkout_detached(staging_path, origin_ref)
    try:
        base_ok, base_problems, base_baselines = capture_batch_baselines(
            staging_path=staging_path, squad=squad, formats=formats,
            cache_dir=cache_dir, comparison_fn=comparison_fn, log_fn=log_fn,
        )
    finally:
        checkout_branch(staging_path, squad_branch)

    if not base_ok:
        refreshed = {
            "blocked": True,
            "commits_since": state.get("commits_since", 0),
            "last_batch_ts": now_fn(),
            "baselines": base_baselines,
            "baseline_head": origin_head,
            "staging_head": current_head,
        }
        return refreshed, {"ran": True, "ok": False, "problems": base_problems,
                           "reason": "staging_head_changed"}

    ok, problems, current_baselines = run_batch_check(
        staging_path=staging_path, squad=squad, formats=formats,
        cache_dir=cache_dir, comparison_fn=comparison_fn,
        baselines=base_baselines, log_fn=log_fn,
    )
    refreshed = {
        "blocked": not ok,
        "commits_since": 0,
        "last_batch_ts": now_fn(),
        # A failed staged-tip check must keep the clean-main baseline so the
        # same regression remains visible on every retry. Replacing it with
        # the failing report would make an unchanged second run pass.
        "baselines": current_baselines if ok else base_baselines,
        "baseline_head": current_head if ok else origin_head,
        "staging_head": current_head,
    }
    return refreshed, {"ran": True, "ok": ok, "problems": problems,
                       "reason": "staging_head_changed"}


# ---------------------------------------------------------------------------
# Squad branch re-cut (spec M5)
# ---------------------------------------------------------------------------

def should_recut(repo_root, squad_branch, origin_ref=ORIGIN_MAIN,
                  staleness_seconds=DEFAULT_RECUT_STALENESS_SECONDS, now_fn=time.time,
                  behind_commits=DEFAULT_RECUT_BEHIND_COMMITS):
    """True when squad/<squad> is stale by any re-cut measure (spec M5
    re-cut trigger): its merge-base with origin_ref is older than
    `staleness_seconds`, origin_ref has moved more than `behind_commits`
    past that base, OR origin_ref changed operational worker/harness code
    since that base.

    The distance clause exists because the age clause alone cannot see the
    failure it is meant to prevent -- see DEFAULT_RECUT_BEHIND_COMMITS. False
    when either ref is missing (nothing to recut yet) or the merge-base
    cannot be resolved.
    """
    if not branch_exists(repo_root, squad_branch):
        return False
    merge_base = _git(["merge-base", squad_branch, origin_ref], repo_root, check=False)
    if merge_base.returncode != 0:
        return False
    sha = merge_base.stdout.strip()
    operational = _git(
        ["diff", "--name-only", f"{sha}..{origin_ref}", "--",
         *IMMEDIATE_RECUT_PATHS, *IMMEDIATE_RECUT_PREFIXES],
        repo_root, check=False,
    )
    if operational.returncode == 0:
        for changed in operational.stdout.splitlines():
            # Unit-test-only changes never alter a running worker.  Everything
            # else below scripts/ is conservatively operational: the entry
            # points import helper modules from the same directory.
            if changed.startswith("scripts/") and Path(changed).name.startswith("test_"):
                continue
            return True
    if behind_commits:
        behind = _git(["rev-list", "--count", f"{sha}..{origin_ref}"], repo_root, check=False)
        if behind.returncode == 0 and behind.stdout.strip().isdigit():
            if int(behind.stdout.strip()) > behind_commits:
                return True
    committed = _git(["log", "-1", "--format=%ct", sha], repo_root, check=False)
    if committed.returncode != 0 or not committed.stdout.strip():
        return False
    try:
        commit_ts = int(committed.stdout.strip())
    except ValueError:
        return False
    return (now_fn() - commit_ts) >= staleness_seconds


def _commits_only_on(repo_root, ref, other_ref):
    """Commit shas reachable from `ref` but not `other_ref`, oldest
    first. Returns None (rather than []) when git can't answer at all --
    callers must NOT treat that the same as "nothing found"."""
    result = _git(["log", f"{other_ref}..{ref}", "--format=%H", "--reverse"], repo_root, check=False)
    if result.returncode != 0:
        return None
    return [line for line in result.stdout.splitlines() if line]


def recut_lost_commits(repo_root, old_tip, origin_ref, new_tip):
    """Commits squad/<squad>'s PRE-recut tip (`old_tip`) carried beyond
    `origin_ref` whose patch-id survives NOWHERE in the rebuilt result --
    neither `origin_ref` itself nor the freshly rebuilt `new_tip` (spec
    M5's explicit no-discard invariant: "no ref reset may discard
    commits not contained in origin/main ... or a squad staging
    branch").

    This is the ground-truth safety net for `recut_squad_branch`: it
    inspects what the OLD squad/<squad> ref actually carried by walking
    real git history, not just what squad-status happens to have a
    "consumed" entry for -- so it catches a commit that landed on
    squad/<squad> outside this merger's own pipeline (manual/bootstrap,
    never recorded in squad-status at all) exactly the same way it
    catches a recorded-consumed head whose re-cherry-pick genuinely
    conflicted.

    Returns None (never a silent "nothing lost") when git can't
    enumerate the range at all -- the caller treats that identically to
    a non-empty loss list (fail closed, never fail open on an
    irreversible ref move)."""
    if not old_tip:
        return []
    candidates = _commits_only_on(repo_root, old_tip, origin_ref)
    if candidates is None:
        return None
    lost = []
    for sha in candidates:
        if is_patch_novel_against(repo_root, origin_ref, sha) and is_patch_novel_against(repo_root, new_tip, sha):
            lost.append(sha)
    return lost


def recut_squad_branch(*, repo_root, staging_path, squad, squad_branch, home,
                        origin_ref=ORIGIN_MAIN, log_fn=print):
    """Re-cut squad/<squad> from origin_ref and re-cherry-pick only the
    still-open (patch-id-novel vs origin_ref) commits this squad's
    status file recorded as consumed-with-work (spec M5): a commit whose
    patch-id already landed on origin_ref by the time of the recut comes
    back "for free" via the fresh base and does not need re-picking.

    Deliberately a standalone function, not entangled with poll_once's
    per-commit loop -- invokable directly (--recut) or from poll_once's
    staleness check at the top of a cycle.

    Before ever moving squad/<squad>'s ref, `recut_lost_commits` checks
    the rebuilt result against what the OLD squad/<squad> tip actually
    carried (spec M5's explicit no-discard invariant). If that check
    finds -- or can't rule out -- a commit that would be permanently
    discarded (a recorded-consumed head whose re-cherry-pick hit a
    genuine conflict against the fresh base, or any commit that landed
    on squad/<squad> outside this merger's own pipeline and so was never
    recorded in squad-status at all), the WHOLE recut is aborted: the
    ref is left exactly where it was -- still carrying that commit --
    and nothing is silently dropped. A human has to look at it (the log
    line says so); a later poll retries the recut from scratch once the
    conflict is resolved by hand or origin_ref moves again.

    Returns {"kept": [...], "dropped": [...], "new_tip": sha}, plus
    {"aborted": True, "lost": [...]} when the safety net fired.
    """
    old_tip = branch_head_sha(repo_root, squad_branch)
    status = load_squad_status(squad_status_file(home, squad))
    entries = [
        (sha, e) for sha, e in (status.get("heads") or {}).items()
        if e.get("status") == "consumed" and e.get("work_done", True) and e.get("squad_sha")
    ]
    # Ordered by INSTANT where the stamp carries one (see record_head's
    # `ts_epoch`): a recut replays cherry-picks in the order the mergers
    # accepted them, and inside the DST fall-back the naive local strings
    # sort two of them backwards. Legacy stamps (no ts_epoch) keep the
    # string order they have always had, and sort ahead of epoch-stamped
    # ones -- correct, since they were necessarily recorded earlier.
    entries.sort(key=stamp_order_key)

    checkout_detached(staging_path, origin_ref)
    kept, dropped = [], []
    for sha, entry in entries:
        squad_sha = entry["squad_sha"]
        # squad_sha's patch-id already reachable from the fresh base ->
        # it comes back "for free" (the overlord already swept it to
        # origin/main); re-cherry-picking it would just create a
        # patch-id-duplicate commit, so it is dropped from the recut,
        # not re-picked.
        if not is_patch_novel_against(repo_root, origin_ref, squad_sha):
            dropped.append(sha)
            continue
        ok, message = cherry_pick(staging_path, squad_sha)
        if ok:
            kept.append(sha)
        else:
            dropped.append(sha)
            log_fn(f"recut {squad!r}: could not re-cherry-pick {squad_sha} (from {sha}) "
                   f"onto fresh {origin_ref}: {message}")

    new_tip = head_sha(staging_path)

    lost = recut_lost_commits(repo_root, old_tip, origin_ref, new_tip)
    if lost is None or lost:
        checkout_branch(staging_path, squad_branch)
        reason = ("git could not enumerate what would be lost" if lost is None
                  else f"{len(lost)} commit(s) would be permanently discarded: {lost}")
        log_fn(f"recut {squad!r}: ABORTED (no-discard invariant, spec M5) -- {reason} -- "
               f"leaving {squad_branch!r} untouched at its current tip {old_tip!r}; "
               "resolve the conflict manually and retry the recut")
        return {"kept": [], "dropped": [], "new_tip": old_tip, "aborted": True, "lost": lost or []}

    update = _git(["update-ref", f"refs/heads/{squad_branch}", new_tip], repo_root, check=False)
    if update.returncode != 0:
        log_fn(f"recut {squad!r}: could not update {squad_branch!r} to {new_tip}: {update.stderr.strip()}")
    checkout_branch(staging_path, squad_branch)
    log_fn(f"recut {squad!r}: rebuilt {squad_branch!r} from {origin_ref} -- "
           f"{len(kept)} re-cherry-picked, {len(dropped)} already landed/dropped")
    return {"kept": kept, "dropped": dropped, "new_tip": new_tip}


# ---------------------------------------------------------------------------
# Poll cycle
# ---------------------------------------------------------------------------

def poll_once(*, repo_root, squad, home, staging_dir, squads_toml_path=DEFAULT_SQUADS_TOML,
              cache_dir, batch_commits=DEFAULT_BATCH_COMMITS, batch_seconds=DEFAULT_BATCH_SECONDS,
              origin_ref=ORIGIN_MAIN, validate_fn=real_validate_commit,
              cargo_test_targeted_fn=real_cargo_test_targeted, comparison_fn=real_format_match,
              sweep_review_log_path=None, validate_kwargs=None, now_fn=time.time, log_fn=print,
              recut_staleness_seconds=DEFAULT_RECUT_STALENESS_SECONDS, check_recut=True,
              heartbeat_fn=None):
    """One full poll cycle for `squad`. Returns a summary dict:
    {"branch": squad/<squad>, "processed": [...per-commit results...],
     "batch_check": {"ran": bool, "ok": bool, "problems": [...]} or None,
     "recut": {...} or None}.

    heartbeat_fn, if given, is called after each candidate commit is
    processed and after the batch check -- a real cargo build/test round
    trip can run long, so this refreshes the merger lock's heartbeat_ts
    mid-poll rather than only at acquire time (mirrors distill_once's own
    "heartbeat refreshed after each output written" discipline).
    """
    heartbeat_fn = heartbeat_fn or (lambda: None)
    squad_branch_ref = staging_branch(squad)
    if check_recut and should_recut(repo_root, squad_branch_ref, origin_ref,
                                     recut_staleness_seconds, now_fn):
        log_fn(f"[{squad}] squad/{squad} is stale vs {origin_ref} -- re-cutting before this poll")
        recut_result = recut_squad_branch(
            repo_root=repo_root, staging_path=staging_dir, squad=squad,
            squad_branch=squad_branch_ref, home=home, origin_ref=origin_ref, log_fn=log_fn,
        )
    else:
        recut_result = None

    ensure_staging_worktree(repo_root, staging_dir, squad, origin_ref=origin_ref, log_fn=log_fn)

    status_path = squad_status_file(home, squad)
    quarantine_entries = load_quarantine(quarantine_ledger_path(home))
    # Every squad that could be racing this one for a file. Read once per
    # poll: squads.toml is small and this is the only consumer.
    known_squads = all_squad_names(squads_toml_path) or None
    branches = candidate_worker_branches(repo_root, squads_toml_path, squad)
    slot_branches = squad_slot_branches(repo_root, squad)
    # Union with squads.toml's own advisory list (not just formats seen on
    # currently-existing branches): the batch full-corpus check must cover
    # every format this squad owns even in a round where every worker
    # branch happens to be squad-mode (no legacy per-format branch exists
    # at all to derive a format from).
    formats = sorted(set(squad_formats(squads_toml_path, squad)) | {fmt for fmt, _ in branches})

    batch_path = batch_state_path(home, squad)
    batch_state = load_batch_state(batch_path)
    batch_result = None

    batch_state, head_refresh = refresh_batch_state_for_head_change(
        repo_root=repo_root, staging_path=staging_dir, squad=squad,
        squad_branch=squad_branch_ref, origin_ref=origin_ref, formats=formats,
        cache_dir=cache_dir, comparison_fn=comparison_fn, state=batch_state,
        now_fn=now_fn, log_fn=log_fn,
    )
    if head_refresh is not None:
        save_batch_state(batch_path, batch_state)
        heartbeat_fn()
        batch_result = head_refresh
        if batch_state.get("blocked"):
            return {"branch": squad_branch_ref, "processed": [],
                    "batch_check": batch_result, "recut": recut_result,
                    "blocked": True}

    if batch_state.get("blocked"):
        # A prior batch check failed -- publication stays held (spec:
        # "skip publishing further until the next successful batch
        # check") until the next scheduled batch-check cadence gives it
        # a chance to clear. The cadence check is time/commit-count-since
        # the LAST successful check, independent of being blocked, so a
        # stuck squad is guaranteed a periodic recovery attempt (purely
        # time-based once commits stop flowing while blocked).
        if not batch_check_due(batch_state, batch_commits, batch_seconds, now_fn):
            log_fn(f"[{squad}] publication held since the last batch full-corpus check failed -- "
                   "skipping candidate processing this poll")
            return {"branch": squad_branch_ref, "processed": [], "batch_check": None,
                    "recut": recut_result, "blocked": True}
        ok, problems, new_baselines = run_batch_check(
            staging_path=staging_dir, squad=squad, formats=formats, cache_dir=cache_dir,
            comparison_fn=comparison_fn, baselines=batch_state.get("baselines") or {}, log_fn=log_fn,
        )
        current_head = head_sha(staging_dir)
        batch_state = {
            "blocked": not ok,
            "commits_since": 0,
            "last_batch_ts": now_fn(),
            "baselines": new_baselines,
            "baseline_head": current_head,
            "staging_head": current_head,
        }
        save_batch_state(batch_path, batch_state)
        batch_result = {"ran": True, "ok": ok, "problems": problems}
        if not ok:
            return {"branch": squad_branch_ref, "processed": [], "batch_check": batch_result,
                    "recut": recut_result, "blocked": True}

    processed = []
    for fmt, worker_branch in branches:
        status = load_squad_status(status_path)
        shas = candidate_commits(repo_root, worker_branch, squad_branch_ref, status)
        if not shas:
            continue
        novel = union_novel_shas(repo_root, worker_branch, origin_ref, squad_branch_ref)
        for sha in shas:
            result = process_commit(
                repo_root=repo_root, staging_path=staging_dir, squad=squad,
                squad_branch=squad_branch_ref, sha=sha, fmt=fmt, is_novel=(sha in novel),
                quarantine_entries=quarantine_entries, cache_dir=cache_dir, home=home,
                validate_fn=validate_fn, cargo_test_targeted_fn=cargo_test_targeted_fn,
                comparison_fn=comparison_fn, sweep_review_log_path=sweep_review_log_path,
                validate_kwargs=validate_kwargs, now_fn=now_fn, log_fn=log_fn,
                all_squads=known_squads, origin_ref=origin_ref,
            )
            processed.append(result)
            heartbeat_fn()

    # Squad-mode worker branches (spec S2): unlike a legacy branch, a
    # slot has no single fixed format -- it cycles round to round -- so
    # each candidate commit's format comes from its own Format: trailer.
    for worker_branch in slot_branches:
        status = load_squad_status(status_path)
        shas = candidate_commits(repo_root, worker_branch, squad_branch_ref, status)
        if not shas:
            continue
        novel = union_novel_shas(repo_root, worker_branch, origin_ref, squad_branch_ref)
        for sha in shas:
            fmt = commit_format_trailer(repo_root, sha) or "UNKNOWN"
            result = process_commit(
                repo_root=repo_root, staging_path=staging_dir, squad=squad,
                squad_branch=squad_branch_ref, sha=sha, fmt=fmt, is_novel=(sha in novel),
                quarantine_entries=quarantine_entries, cache_dir=cache_dir, home=home,
                validate_fn=validate_fn, cargo_test_targeted_fn=cargo_test_targeted_fn,
                comparison_fn=comparison_fn, sweep_review_log_path=sweep_review_log_path,
                validate_kwargs=validate_kwargs, now_fn=now_fn, log_fn=log_fn,
                all_squads=known_squads, origin_ref=origin_ref,
            )
            processed.append(result)
            heartbeat_fn()

    # spec M2: "every batch_commits commits OR batch_seconds seconds ...
    # whichever first" -- batch_check_due's seconds-elapsed arm must be
    # evaluated every poll, independent of whether THIS poll consumed any
    # commits (a squad that goes quiet -- all candidates quarantined, no
    # worker branches with new commits -- must still get its periodic
    # full-corpus safety check once batch_seconds has elapsed; gating the
    # whole check behind "did this poll consume something" silently
    # starves that cadence for as long as the squad stays quiet).
    consumed_with_work = sum(1 for r in processed if r["outcome"] == "consumed")
    batch_state["commits_since"] = batch_state.get("commits_since", 0) + consumed_with_work
    state_changed = consumed_with_work > 0
    if state_changed:
        batch_state["staging_head"] = head_sha(staging_dir)
    if batch_check_due(batch_state, batch_commits, batch_seconds, now_fn):
        ok, problems, new_baselines = run_batch_check(
            staging_path=staging_dir, squad=squad, formats=formats, cache_dir=cache_dir,
            comparison_fn=comparison_fn, baselines=batch_state.get("baselines") or {},
            log_fn=log_fn,
        )
        batch_state["blocked"] = not ok
        batch_state["commits_since"] = 0
        batch_state["last_batch_ts"] = now_fn()
        batch_state["baselines"] = new_baselines
        current_head = head_sha(staging_dir)
        batch_state["baseline_head"] = current_head
        batch_state["staging_head"] = current_head
        batch_result = {"ran": True, "ok": ok, "problems": problems}
        heartbeat_fn()
        state_changed = True
    if state_changed:
        save_batch_state(batch_path, batch_state)

    return {"branch": squad_branch_ref, "processed": processed,
            "batch_check": batch_result, "recut": recut_result}


# ---------------------------------------------------------------------------
# Singleton lock (reuses distill_lessons.py's generically-named helpers)
# ---------------------------------------------------------------------------

def run_locked(home, squad, fn, *, now_fn=time.time, kill_fn=None, script_sha=None, pid=None):
    """Run `fn(heartbeat)` under this squad's merger singleton lock
    (mirrors distill_lessons.distill_once's own lock/heartbeat/release
    shape exactly, reusing its lock-file helpers directly rather than
    duplicating the takeover logic). Returns {"status": "already_running"}
    without calling fn() when a fresh same-sha holder is running;
    otherwise {"status": "ok", "result": fn(heartbeat)}, where calling
    `heartbeat()` refreshes heartbeat_ts (a real poll can run long --
    cargo builds/tests -- so this must be callable mid-run, not just at
    acquire time)."""
    kill_fn = kill_fn or os.kill
    lock_path = merger_lock_path(home, squad)
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    script_sha = script_sha or compute_script_sha()
    pid = os.getpid() if pid is None else pid
    if not acquire_lock(lock_path, pid, script_sha, now_fn, kill_fn, STALE_HEARTBEAT_SECONDS):
        return {"status": "already_running"}

    def heartbeat():
        write_lock(lock_path, pid, script_sha, now_fn())

    try:
        result = fn(heartbeat)
        return {"status": "ok", "result": result}
    finally:
        release_lock(lock_path, pid)


# ---------------------------------------------------------------------------
# Crash classification: when this daemon may retry, and when it must not
# ---------------------------------------------------------------------------
#
# WHY THIS EXISTS
# ---------------
# `main`'s `while True:` had no exception handling at all, so ANY exception
# out of a poll killed the daemon. On 2026-07-30 at 23:40:41 the disk hit
# 100% and eleven of the fourteen mergers died inside 32 seconds, none of
# them on anything to do with merging:
#
#   9x  find_tag_gaps.py:98 _semaphore_locked -> NamedTemporaryFile
#       OSError: [Errno 28] No space left on device:
#       '.../logs/build-semaphore.json._reu5673.tmp'
#   2x  distill_lessons.py:768 write_lock -> mkstemp
#       OSError: [Errno 28] ... '.../logs/knowledge/.distiller.lock.*.tmp'
#
# Both are the merger asking the filesystem for a scratch file before doing
# any work. Neither says anything is wrong with the merger, the squad branch
# or the commits -- the machine was simply out of space for ten minutes.
# Dying on that, permanently, is the wrong answer.
#
# The wrong FIX is a bare `except: continue`. A merger that retries forever
# against a genuinely broken input is the same stall wearing a different hat:
# it looks alive, publishes nothing, and nobody is paged. So the retry here
# is bounded three ways -- only for errors that describe the MACHINE, only
# MAX_TRANSIENT_FAILURES times consecutively, and with a backoff -- and any
# successful pass resets the budget. Everything else exits non-zero and lets
# the supervisor's restart budget surface it as GIVING UP.

#: errnos meaning "the machine cannot serve this right now", not "this
#: program is wrong". Retrying one of these can succeed with no code change;
#: retrying anything else cannot.
TRANSIENT_ERRNOS = frozenset({
    errno.ENOSPC,   # disk full -- the 2026-07-30 mass death
    errno.EDQUOT,   # quota exhausted; same shape as ENOSPC
    errno.EROFS,    # filesystem went read-only under us
    errno.EIO,      # transient device error
    errno.ENOMEM,   # out of memory
    errno.EMFILE,   # per-process fd limit
    errno.ENFILE,   # system-wide fd limit
    errno.EAGAIN,   # fork/thread resource exhaustion (load average ~400 here)
})

DEFAULT_MAX_TRANSIENT_FAILURES = 20
TRANSIENT_BACKOFF_BASE = 30.0
TRANSIENT_BACKOFF_MAX = 600.0


def classify_exception(exc):
    """``"transient"`` if `exc` (or anything it was raised from) describes a
    machine resource shortage, else ``"fatal"``.

    The chain walk matters: the ENOSPC that killed nine mergers surfaced
    through ``contextlib``/``tempfile`` frames, and a future refactor that
    wraps it in a domain exception must not silently reclassify a full disk
    as a bug. ``KeyboardInterrupt``/``SystemExit`` never reach here -- the
    caller re-raises them before classifying.
    """
    seen = set()
    cur = exc
    while cur is not None and id(cur) not in seen:
        seen.add(id(cur))
        if isinstance(cur, OSError) and cur.errno in TRANSIENT_ERRNOS:
            return "transient"
        cur = cur.__cause__ or cur.__context__
    return "fatal"


def transient_backoff_seconds(consecutive, base=TRANSIENT_BACKOFF_BASE,
                              cap=TRANSIENT_BACKOFF_MAX):
    """Exponential, capped. A merger that died because the disk filled must
    not spin retrying -- each attempt itself wants a temp file, so a tight
    loop is a load source on an already-wedged machine."""
    if consecutive < 1:
        return base
    delay = base * (2 ** (consecutive - 1))
    return float(min(delay, cap))


def free_space_note(path):
    """Best-effort ``free=12.3GB`` for the crash line. Never raises: this
    runs on the path where the disk is already known to be unhappy."""
    try:
        return "free=%.1fGB" % (shutil.disk_usage(str(path)).free / 1e9)
    except OSError:
        return "free=unknown"


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def main(argv=None, sleep_fn=time.sleep, now_fn=time.time, kill_fn=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--squad", required=True, help="squad name, e.g. nikon, canon")
    parser.add_argument("--repo", default=str(REPO_ROOT))
    parser.add_argument("--staging-dir", default=None,
                        help="default: <home>/worktrees/squad-staging/<squad>")
    parser.add_argument("--worktree-dir", default=str(DEFAULT_WORKTREE_DIR),
                        help="where worker branches/worktrees for this squad's formats live "
                             "(parallel_model_fix_loop.py's --worktree-dir convention)")
    parser.add_argument("--squads-toml", default=str(DEFAULT_SQUADS_TOML))
    parser.add_argument("--home", default=str(OXIDEX_HOME))
    parser.add_argument("--cache-dir", default=os.environ.get("EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache"))  # nosec B108
    parser.add_argument("--poll-seconds", type=float, default=DEFAULT_POLL_SECONDS)
    parser.add_argument("--batch-commits", type=int, default=DEFAULT_BATCH_COMMITS)
    parser.add_argument("--batch-seconds", type=float, default=DEFAULT_BATCH_SECONDS)
    parser.add_argument("--recut-staleness-seconds", type=float, default=DEFAULT_RECUT_STALENESS_SECONDS)
    parser.add_argument("--perl-lib", default=None, help="passed through to validate_fix_commit")
    parser.add_argument("--samples-cache", default=None, help="passed through to validate_fix_commit")
    parser.add_argument("--comparison-cmd", default=None, help="passed through to validate_fix_commit")
    parser.add_argument("--sweep-review-log", default=None,
                        help="default: <home>/logs/sweep-review-history.jsonl")
    parser.add_argument("--once", action="store_true", help="single pass then exit (default)")
    parser.add_argument("--infinite", action="store_true", help="poll forever until interrupted")
    parser.add_argument("--max-transient-failures", type=int,
                        default=DEFAULT_MAX_TRANSIENT_FAILURES,
                        help="consecutive machine-resource failures (disk full, fd "
                             "exhaustion, OOM) tolerated in --infinite mode before "
                             "exiting non-zero. Any successful poll resets the count. "
                             "Non-resource errors are never retried.")
    parser.add_argument("--recut", action="store_true",
                        help="run only the squad-branch re-cut (spec M5), then exit")
    args = parser.parse_args(argv)

    home = Path(args.home)
    staging_dir = Path(args.staging_dir) if args.staging_dir else default_staging_dir(home, args.squad)
    sweep_review_log_path = (
        Path(args.sweep_review_log) if args.sweep_review_log
        else home / "logs" / "sweep-review-history.jsonl"
    )

    from validate_fix_commit import build_comparison_fn
    validate_kwargs = {
        "perl_lib": args.perl_lib,
        "samples_cache": args.samples_cache,
        "squads_toml": args.squads_toml,
        "comparison_fn": build_comparison_fn(args.comparison_cmd) if args.comparison_cmd else None,
    }

    def one_pass(heartbeat):
        if args.recut:
            # poll_once ensures the staging worktree itself internally;
            # the standalone --recut path bypasses poll_once entirely, so
            # it must ensure one exists on its own.
            ensure_staging_worktree(Path(args.repo), staging_dir, args.squad, log_fn=print)
            return recut_squad_branch(
                repo_root=Path(args.repo), staging_path=staging_dir, squad=args.squad,
                squad_branch=staging_branch(args.squad), home=home, log_fn=print,
            )
        return poll_once(
            repo_root=Path(args.repo), squad=args.squad, home=home, staging_dir=staging_dir,
            squads_toml_path=args.squads_toml, cache_dir=args.cache_dir,
            batch_commits=args.batch_commits, batch_seconds=args.batch_seconds,
            recut_staleness_seconds=args.recut_staleness_seconds,
            sweep_review_log_path=sweep_review_log_path, validate_kwargs=validate_kwargs,
            now_fn=now_fn, log_fn=print, heartbeat_fn=heartbeat,
        )

    consecutive_transient = 0
    while True:
        try:
            outcome = run_locked(home, args.squad, one_pass, now_fn=now_fn, kill_fn=kill_fn)
        except (KeyboardInterrupt, SystemExit):
            # An operator or the supervisor asked us to stop. Not a crash.
            raise
        except Exception as exc:  # noqa: BLE001 -- deliberately broad; see below
            # ALWAYS say why, BEFORE deciding what to do about it. The
            # 2026-07-30 deaths were diagnosable only because Python's
            # default handler printed a traceback on the way out; a daemon
            # that decides to keep running must not become quieter than the
            # one that crashed.
            kind = classify_exception(exc)
            print(
                f"[{args.squad}] merger pass FAILED ({kind}): "
                f"{type(exc).__name__}: {exc} [{free_space_note(home)}]",
                file=sys.stderr, flush=True,
            )
            traceback.print_exc()
            sys.stderr.flush()

            if kind == "fatal" or not args.infinite:
                # A bug, bad config, or a one-shot run. Exit non-zero so the
                # supervisor counts a restart and eventually reports GIVING
                # UP, instead of this process hiding the defect behind a
                # retry loop.
                print(
                    f"[{args.squad}] EXITING on a {kind} error -- this will not "
                    "fix itself by retrying; the supervisor's restart budget "
                    "is the right place for it to surface",
                    file=sys.stderr, flush=True,
                )
                return 2

            consecutive_transient += 1
            if consecutive_transient > args.max_transient_failures:
                print(
                    f"[{args.squad}] EXITING after {consecutive_transient} consecutive "
                    f"transient failures (limit {args.max_transient_failures}). The machine "
                    "has not recovered; retrying further would just be a stall that "
                    f"looks alive. Last cause above. [{free_space_note(home)}]",
                    file=sys.stderr, flush=True,
                )
                return 3

            delay = transient_backoff_seconds(consecutive_transient)
            print(
                f"[{args.squad}] transient failure "
                f"{consecutive_transient}/{args.max_transient_failures} -- "
                f"retrying in {delay:.0f}s",
                file=sys.stderr, flush=True,
            )
            sleep_fn(delay)
            continue

        if consecutive_transient:
            print(
                f"[{args.squad}] recovered after {consecutive_transient} "
                "transient failure(s) -- budget reset",
                file=sys.stderr, flush=True,
            )
            consecutive_transient = 0

        if outcome["status"] == "already_running":
            print(f"another merger already holds the lock for squad {args.squad!r} -- exiting quietly")
        if args.recut or not args.infinite:
            return 0
        sleep_fn(args.poll_seconds)


if __name__ == "__main__":
    sys.exit(main())
