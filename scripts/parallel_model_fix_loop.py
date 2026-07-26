#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Run scripts/model_fix_loop.py in parallel across formats, each in its
own git worktree with its own target/ dir (never shared -- CARGO_TARGET_DIR
is explicitly stripped from each worker's environment), then merge
completed work back sequentially once each worker finishes.

Exactly one dispatcher runs per host: startup takes a process-lifetime
flock on ~/.oxidex/logs/dispatcher.lock (a second instance fails fast --
see acquire_dispatcher_lock) and then reaps any worker process groups a
previous, dead dispatcher left behind (persisted in dispatcher-pgids.json;
see reap_orphan_worker_pgids).

Git hygiene (spec M5): local `main` is a MIRROR of origin/main -- each
round starts by fast-forwarding it (ff-only; divergence is loudly skipped,
never reset away), and round-end merges land on a dedicated local
integration branch (model-fix-sweep-local) whenever the repo sits on main,
so nothing ever merges into main itself. Every ref reset in this file is
guarded by the no-discard invariant: no reset may discard commits not
contained in origin/main or another live branch.

Config: config.toml (see config.example.toml), same file model_fix_loop.py
reads directly. Since config.toml is gitignored, `git worktree add` won't
bring it into a freshly created worktree on its own, so each worker's
worktree gets its own copy at creation time (see create_worktree).

Auto-publish: with --infinite (and unless --no-auto-publish is passed),
every round ends by running overlord_sweep.run_sweep IN-PROCESS in its
own worktree -- sweep branch, cargo fmt, push, PR, squash-merge once all
checks are green, then a ff-only sync of every worktree to the new
origin/main. See the "Auto-publish" section below for why the dispatcher
owns that last mile. A round with nothing newly stamped green is a
complete no-op.

Usage:
    uv run scripts/parallel_model_fix_loop.py
    uv run scripts/parallel_model_fix_loop.py --max-parallel 8
    uv run scripts/parallel_model_fix_loop.py --formats JPEG,NEF,DNG
    uv run scripts/parallel_model_fix_loop.py --infinite    # publishes itself
"""
import argparse
import concurrent.futures
import fcntl
import json
import os
import re
import shutil
import signal
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
import tempfile
import threading
import time
from pathlib import Path

from find_tag_gaps import OXIDEX_HOME, REPO_ROOT, group_gaps_by_format, load_comparison_report, run_full_comparison
from model_fix_loop import DEFAULT_CONFIG_PATH, DEFAULT_TAG_STATE_PATH, _state_locked
# The fleet-wide (args, repo, input_text=None) -> (returncode, stdout,
# stderr) git runner (overlord_sweep.default_run_git and every squad
# merger use the identical shape) -- imported rather than re-implemented
# so one injected fake covers the auto-publish path AND the run_sweep it
# calls. validate_fix_commit imports nothing from this module, so this
# is not part of the overlord_sweep -> squad_merge_loop -> here import
# cycle default_sweep_fn works around.
from validate_fix_commit import run_git as default_run_git

# Each worker runs a full `cargo test --workspace` before committing --
# running more of those concurrently than there are cores just makes them
# contend for CPU, which can produce spurious regressions unrelated to the
# fix under test. Capping at the core count keeps every worker's test run
# meaningful.
DEFAULT_MAX_PARALLEL = min(20, os.cpu_count() or 4)

# Per-worker log files default here instead of /tmp: /tmp is wiped on
# reboot (and never included in Time Machine backups), which otherwise
# destroys the only record of why a run's fixes did or didn't land. Also
# not REPO_ROOT-relative -- see OXIDEX_HOME's docstring in find_tag_gaps.py.
DEFAULT_LOG_DIR = OXIDEX_HOME / "logs" / "parallel-model-fix"

# Dispatcher singleton: exactly one parallel_model_fix_loop.py per host.
# Two concurrent dispatchers double-spawn same-format workers, double-merge
# branches, and race each other's worktree resets (observed live: a
# duplicate dispatcher quietly ran for hours). The flock on this file is
# held for the whole process lifetime and vanishes with the process, so a
# crashed dispatcher never leaves a stale lock behind.
DISPATCHER_LOCK_PATH = OXIDEX_HOME / "logs" / "dispatcher.lock"

# Where the dispatcher persists its spawned workers' process-group ids
# (tempfile+os.replace, never torn). A dispatcher that dies without
# cleaning up (SIGKILL, power loss) leaves its workers orphaned with
# nothing tracking them -- the next dispatcher startup reads this file and
# reaps whatever is still alive before spawning anything of its own.
DISPATCHER_PGIDS_PATH = OXIDEX_HOME / "logs" / "dispatcher-pgids.json"

# M5 "local main is a mirror": when the dispatcher repo sits on `main`,
# round-end merges are retargeted onto this dedicated local integration
# branch instead of merging into main (see ensure_integration_branch).
SWEEP_LOCAL_BRANCH = "model-fix-sweep-local"

# Squad-mode (spec S2/S5) defaults. Deliberately separate constants from
# the legacy per-format ones above -- --squad-mode is a new, opt-in
# entrypoint (run_squad_round) alongside run_round, not a replacement.
SCRIPTS_DIR = Path(__file__).resolve().parent
DEFAULT_SQUADS_TOML = SCRIPTS_DIR / "squads.toml"
DEFAULT_GAP_ATTRIBUTION_PATH = OXIDEX_HOME / "logs" / "gap-attribution.json"

# Every in-flight worker's process group, so an interrupted wrapper
# (Ctrl-C, SIGTERM) can force-terminate all of them rather than leaving
# cargo/rustc grandchildren running unsupervised.
_active_pgids = set()
_active_pgids_lock = threading.Lock()

# Where _register_pgid/_unregister_pgid mirror _active_pgids to disk for
# the next dispatcher's orphan reaper. None (imports, unit tests) disables
# persistence entirely; main() points it at DISPATCHER_PGIDS_PATH (or its
# injected test path) once the singleton lock is held.
_pgids_persist_path = None

# Serializes the snapshot+write pair in _persist_active_pgids. Up to
# max_parallel worker threads register/unregister concurrently; if the
# snapshot were taken under _active_pgids_lock but written OUTSIDE any
# lock, two persists could complete their writes in the opposite order of
# their snapshots and a stale snapshot would win -- e.g. an unregister's
# empty snapshot overwriting a register's, erasing a live worker's pgid
# from the very record the next dispatcher's orphan reaper depends on.
# Deliberately a second lock (not _active_pgids_lock itself) so set
# mutation stays cheap: registering a pgid never waits behind another
# thread's file I/O, and _kill_all_active_workers (signal path) never
# contends with a write in flight.
_pgids_persist_lock = threading.Lock()


def _set_pgids_persist_path(path):
    global _pgids_persist_path
    _pgids_persist_path = Path(path) if path else None


def _write_pgids_file(path, pgids):
    """Atomically persist a pgid list: NamedTemporaryFile in the same
    directory, then os.replace -- a reader (the next dispatcher's orphan
    reaper) can never observe a torn file."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        "w", dir=path.parent, prefix=path.name + ".", suffix=".tmp", delete=False,
    ) as f:
        json.dump({"pgids": sorted(pgids)}, f)
        tmp_name = f.name
    os.replace(tmp_name, path)


def _persist_active_pgids():
    if _pgids_persist_path is None:
        return
    # Snapshot AND write under one persist lock: because the snapshot is
    # taken at write time (not at mutation time), whichever persist call
    # writes last always wrote the then-current set -- a delayed thread
    # can never clobber the file with an older view of _active_pgids.
    with _pgids_persist_lock:
        with _active_pgids_lock:
            pgids = sorted(_active_pgids)
        _write_pgids_file(_pgids_persist_path, pgids)


def acquire_dispatcher_lock(lock_path=DISPATCHER_LOCK_PATH):
    """Take the dispatcher singleton flock (O_CREAT via open mode "a+").

    Returns the open lock file on success -- the caller must keep the
    object alive for the whole process lifetime, since closing it (or
    the process dying, however abruptly) is what releases the flock.
    Returns None when another dispatcher already holds it: flock is
    per-open-file-description, so even a second instance inside the same
    process is correctly refused."""
    lock_path = Path(lock_path)
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    lock_f = open(lock_path, "a+")
    try:
        fcntl.flock(lock_f, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        lock_f.close()
        return None
    try:
        # Advisory only (the flock is the actual mutex): record who holds
        # it so a human staring at the file can find the process.
        lock_f.seek(0)
        lock_f.truncate()
        lock_f.write(f"{os.getpid()}\n")
        lock_f.flush()
    except OSError:
        lock_f.close()
        raise
    return lock_f


def reap_orphan_worker_pgids(pgids_path, kill_fn=os.killpg, alive_fn=None,
                             sleep_fn=time.sleep, grace_seconds=5.0, log_fn=print):
    """Startup pass, run only AFTER winning the dispatcher singleton
    flock: any pgids persisted in pgids_path were spawned by a previous
    dispatcher, and since the flock was free, no dispatcher is alive to
    own them -- whatever still runs is an orphaned worker tree
    (model_fix_loop.py plus cargo/rustc grandchildren) burning CPU and
    governor budget unsupervised. SIGTERM each still-alive group, wait
    up to grace_seconds, SIGKILL whatever ignored it, then clear the
    file. A missing or corrupt file just means nothing to reap.

    kill_fn/alive_fn/sleep_fn are injectable so tests exercise the
    logic without a real killpg. Returns the pgids that were signaled.
    """
    if alive_fn is None:
        alive_fn = _process_group_alive
    pgids_path = Path(pgids_path)
    try:
        data = json.loads(pgids_path.read_text())
        pgids = [int(p) for p in (data.get("pgids") or [])]
    except FileNotFoundError:
        return []
    except (OSError, ValueError, TypeError):
        log_fn(f"dispatcher pgid file {pgids_path} is unreadable -- clearing it, nothing to reap")
        pgids = []
    own_pgid = os.getpgrp()
    leftovers = [p for p in pgids if p != own_pgid and alive_fn(p)]
    for pgid in leftovers:
        log_fn(f"reaping orphaned worker process group {pgid} (SIGTERM)")
        kill_fn(pgid, signal.SIGTERM)
    if leftovers:
        waited = 0.0
        while waited < grace_seconds and any(alive_fn(p) for p in leftovers):
            sleep_fn(0.5)
            waited += 0.5
        for pgid in leftovers:
            if alive_fn(pgid):
                log_fn(f"orphaned worker process group {pgid} ignored SIGTERM -- escalating to SIGKILL")
                kill_fn(pgid, signal.SIGKILL)
    _write_pgids_file(pgids_path, [])
    return leftovers


def discover_formats(cache_dir):
    """Run the full comparison once, return format names with gaps,
    sorted by gap count descending (biggest first)."""
    report_path = run_full_comparison(cache_dir)
    gaps = group_gaps_by_format(load_comparison_report(report_path))
    return [g["format"] for g in gaps]


def worktree_path(base_dir, fmt):
    return base_dir / f"model-fix-{fmt.lower()}"


def branch_name(fmt):
    return f"model-fix-parallel-{fmt.lower()}"


# ---------------------------------------------------------------------------
# Squad-mode: slot allocation (spec S2)
# ---------------------------------------------------------------------------

def allocate_squad_slots(squad_open_gaps, total_slots):
    """The spec S2 slot-allocation formula, implemented exactly:

        slots_i = max(1, round(total_slots * open_gaps_i / sum(open_gaps)))

    followed by a reconciliation pass (the max(1, .) floor overshoots the
    moment there are more squads than total_slots can give two-plus slots
    to, which is every round on this host's 14 squads): while
    sum(slots) > total_slots, decrement the squad with the LOWEST
    gaps-per-slot among those currently holding more than 1 slot (it is
    the squad "wasting" the most slack per slot); while sum(slots) <
    total_slots, increment the squad with the HIGHEST gaps-per-slot (the
    most under-served squad gets the extra capacity first).

    A squad with open_gaps <= 0 is excluded entirely, BEFORE the floor
    is ever applied -- max(1, .) is a floor for squads that have work,
    never a slot handed to genuinely empty work (mirrors
    discover_formats already dropping formats with zero gaps today).

    Pure function: no I/O, no clock, no subprocess -- squad_open_gaps is
    ordinarily {squad: open_gaps} straight from gap-attribution.json's
    "squads" summary (attribute_gaps.py), but this takes a plain dict so
    it is trivially unit-testable against the spec's worked census
    example without needing a real attribution run.

    Returns {squad: slot_count} for every squad with open_gaps > 0. The
    values sum to exactly total_slots whenever total_slots >= the
    number of active squads (the case every worked example in the spec
    covers); with fewer slots than active squads the floor overshoot
    cannot be reconciled away (every squad is already pinned at its
    floor of 1) and the returned total exceeds total_slots -- a
    dispatcher that ever sees that case is asking for fewer slots than
    it has squads with work, which is its own signal to raise
    total_slots, not something this function can silently paper over by
    dropping a squad's only slot to zero.
    """
    active = {squad: gaps for squad, gaps in squad_open_gaps.items() if gaps and gaps > 0}
    if not active or total_slots <= 0:
        return {}

    total_gaps = sum(active.values())
    slots = {squad: max(1, round(total_slots * gaps / total_gaps)) for squad, gaps in active.items()}

    def gaps_per_slot(squad):
        return active[squad] / slots[squad]

    while sum(slots.values()) > total_slots:
        decrementable = [squad for squad in slots if slots[squad] > 1]
        if not decrementable:
            # Every squad is already at its floor of 1 -- cannot
            # reconcile further without taking a squad with open gaps
            # down to zero, which spec S2 forbids outright.
            break
        victim = min(decrementable, key=lambda squad: (gaps_per_slot(squad), squad))
        slots[victim] -= 1

    while sum(slots.values()) < total_slots:
        beneficiary = max(slots, key=lambda squad: (gaps_per_slot(squad), squad))
        slots[beneficiary] += 1

    return slots


def squad_worktree_path(base_dir, squad, n):
    return base_dir / f"model-fix-{squad}-{n}"


def squad_branch_name(squad, n):
    return f"model-fix-parallel-{squad}-{n}"


# List-argv only throughout this file, no shell=True -- repo_root/path are
# local filesystem locations this process already trusts, branch/base_ref
# are derived from format names (a closed set from tag-comparison's own
# output) or the caller's own current git ref, never network input.


def clean_worktree(path):
    """Discard uncommitted changes and untracked files in a worker's
    worktree -- git clean -fd never touches gitignored paths (target/, in
    particular), so this can't evict the worktree's own cargo build cache.
    """
    subprocess.run(["git", "checkout", "--", "."], cwd=path, check=True)  # nosec B603
    subprocess.run(["git", "clean", "-fd"], cwd=path, check=True)  # nosec B603


def _git(args, repo_root, check=False):
    """One captured git invocation -- list-argv, no shell, cwd-scoped."""
    return subprocess.run(  # nosec B603
        ["git", *args], cwd=repo_root, capture_output=True, text=True, check=check,
    )


def fast_forward_local_main(repo_root, log_fn=print):
    """Round-start M5 rule: local `main` is a MIRROR of origin/main.

    Fast-forward it when that is a pure fast-forward, and otherwise
    leave it completely alone: if local main carries commits origin/main
    doesn't have (someone committed to it directly, or a legacy round
    merged into it), log loudly and skip -- resetting away commits is
    never this script's call to make (the no-discard invariant: no ref
    reset may discard commits not contained in origin/main, an open
    sweep PR branch, or a squad staging branch).

    The fetch is best-effort; offline, the update runs against the
    last-known origin/main, which is still a pure fast-forward or
    nothing. Returns (updated, message).
    """
    fetch = _git(["fetch", "origin", "main"], repo_root)
    if fetch.returncode != 0:
        log_fn(
            f"git fetch origin main failed ({fetch.stderr.strip()}) -- "
            "checking local main against the last-known origin/main instead"
        )
    main_sha = _git(["rev-parse", "--verify", "--quiet", "refs/heads/main"], repo_root)
    origin_sha = _git(["rev-parse", "--verify", "--quiet", "refs/remotes/origin/main"], repo_root)
    if main_sha.returncode != 0 or origin_sha.returncode != 0:
        return False, "no local main and/or origin/main ref -- nothing to mirror"
    if main_sha.stdout.strip() == origin_sha.stdout.strip():
        return True, "local main already matches origin/main"
    ancestor = _git(
        ["merge-base", "--is-ancestor", "refs/heads/main", "refs/remotes/origin/main"], repo_root,
    )
    if ancestor.returncode != 0:
        message = (
            "local main has commits that are NOT on origin/main -- refusing to touch it "
            "(no-discard invariant). Sweep or push those commits, then rerun."
        )
        log_fn(f"WARNING: {message}")
        return False, message
    current = _git(["rev-parse", "--abbrev-ref", "HEAD"], repo_root, check=True).stdout.strip()
    if current == "main":
        result = _git(["merge", "--ff-only", "refs/remotes/origin/main"], repo_root)
    else:
        # branch -f on a ref that isn't checked out; the ancestor check
        # above guarantees this is a pure fast-forward, discarding
        # nothing.
        result = _git(["branch", "-f", "main", "refs/remotes/origin/main"], repo_root)
    if result.returncode != 0:
        log_fn(f"fast-forward of local main failed: {result.stderr.strip()}")
        return False, f"fast-forward failed: {result.stderr.strip()}"
    return True, "fast-forwarded local main to origin/main"


def ensure_integration_branch(repo_root, log_fn=print):
    """The branch this round's worker merges land on, or None when no
    integration target can be safely established this round.

    M5: local `main` is a mirror of origin/main, never an integration
    target -- merging worker branches into it makes it diverge
    permanently, which then blocks its round-start fast-forward and (in
    the old flow) invited a destructive reset to "fix" it. When the
    dispatcher repo is checked out on main, integration is retargeted to
    the dedicated SWEEP_LOCAL_BRANCH: cut from main's current tip if it
    doesn't exist yet, and checked out AS-IS (plain checkout, never -B,
    never a reset) if it does, so unswept merges from prior rounds
    survive every restart. A repo already on any other branch keeps that
    branch as the integration target, exactly as before.

    The retarget can legitimately fail: once SWEEP_LOCAL_BRANCH has
    diverged from main, a dirty dispatcher checkout (an operator's
    uncommitted edits touching files the branches differ on) makes
    `git checkout` refuse rather than clobber them -- correct, and not
    this function's place to override. That failure must NOT propagate:
    an --infinite dispatcher lives for weeks, and one un-checkoutable
    round killing the whole loop is far worse than skipping the round.
    Log loudly and return None; run_round treats None as "no safe
    integration target, skip this round entirely" (dispatching workers
    just to refuse to merge them would waste a full round of API budget).

    Rollout notes (spec M5, Phase 0 -- operator-facing, land these
    expectations with the rollout-ops pass):
      * The dispatcher repo's checkout silently moves from main to
        SWEEP_LOCAL_BRANCH at the first round after a restart; anyone
        (human or tooling) working inside that checkout should expect
        to find it on SWEEP_LOCAL_BRANCH, not main.
      * Sweep tooling that used to look for integrated-but-unswept
        commits on local main must look at SWEEP_LOCAL_BRANCH instead.
      * Phase 0 cuts SWEEP_LOCAL_BRANCH once and never re-anchors it to
        origin/main -- the integration base drifts behind origin/main
        until the Phase 2+ merger re-cut rule (or a manual sweep +
        branch delete) re-cuts it. The round-start mirror ff plus the
        patch-id dup gate keep that drift from double-landing fixes.
    """
    current = _git(["rev-parse", "--abbrev-ref", "HEAD"], repo_root, check=True).stdout.strip()
    if current != "main":
        return current
    if not _branch_exists(repo_root, SWEEP_LOCAL_BRANCH):
        created = _git(["branch", SWEEP_LOCAL_BRANCH, "main"], repo_root)
        if created.returncode != 0:
            log_fn(
                f"WARNING: could not create {SWEEP_LOCAL_BRANCH!r} from main "
                f"({created.stderr.strip()}) -- refusing to integrate on main (M5); "
                "this round will be skipped"
            )
            return None
    checkout = _git(["checkout", SWEEP_LOCAL_BRANCH], repo_root)
    if checkout.returncode != 0:
        log_fn(
            f"WARNING: could not check out {SWEEP_LOCAL_BRANCH!r} "
            f"({checkout.stderr.strip()}) -- likely uncommitted changes in the "
            "dispatcher checkout conflicting with it. Refusing to integrate on "
            "main (M5); this round will be skipped. Commit/stash those changes "
            "to unblock the next round."
        )
        return None
    log_fn(
        f"local main is a mirror (M5) -- integrating this round on {SWEEP_LOCAL_BRANCH!r} instead"
    )
    return SWEEP_LOCAL_BRANCH


def _branch_exists(repo_root, branch):
    result = subprocess.run(  # nosec B603
        ["git", "rev-parse", "--verify", "--quiet", f"refs/heads/{branch}"],
        cwd=repo_root, capture_output=True, text=True,
    )
    return result.returncode == 0


def _branch_has_undiscardable_commits(repo_root, branch, base_ref):
    """True when `branch` carries commits reachable from neither
    base_ref nor origin/main -- exactly what a `checkout -B` reset would
    discard for good (the no-discard invariant, M5). A failed round-end
    merge deliberately leaves its worker branch in place; without this
    check the next round's re-anchor silently threw those commits away.

    Ancestry alone isn't enough, though: a squash merge gives previously
    landed work a brand new SHA with no parent relationship to the
    original commit, so content that already made it upstream can still
    look "unreachable" by ancestry forever -- e.g. every config/prompt
    tuning commit this fleet squash-merges to main is, from that point
    on, permanently "not reachable from base_ref" on any worker branch
    that happened to carry it, which would refuse that branch's reset
    on every single round for good, blocking it from ever picking up a
    newer base -- for a branch that has nothing left worth protecting.
    `git cherry` compares by patch-id instead of ancestry, so a commit
    whose diff already exists upstream (under any SHA) gets filtered out
    before anything is declared undiscardable.

    Errs on the safe side: if git can't answer (bad ref, unreadable
    output), the commits are treated as present and the reset refused.
    """
    exclude = [f"^{base_ref}"]
    origin_main = _git(["rev-parse", "--verify", "--quiet", "refs/remotes/origin/main"], repo_root)
    if origin_main.returncode == 0:
        exclude.append("^refs/remotes/origin/main")
    result = _git(["rev-list", branch, *exclude], repo_root)
    if result.returncode != 0:
        return True
    candidates = {line.strip() for line in result.stdout.splitlines() if line.strip()}
    if not candidates:
        return False

    upstream_refs = [base_ref]
    if origin_main.returncode == 0:
        upstream_refs.append("refs/remotes/origin/main")
    for ref in upstream_refs:
        cherry = _git(["cherry", ref, branch], repo_root)
        if cherry.returncode != 0:
            return True
        merged_by_patch_id = {
            line[2:] for line in cherry.stdout.splitlines() if line.startswith("- ")
        }
        candidates -= merged_by_patch_id
        if not candidates:
            return False
    return bool(candidates)


def _branch_head_sha(repo_root, branch):
    result = subprocess.run(  # nosec B603
        ["git", "rev-parse", "--verify", "--quiet", branch],
        cwd=repo_root, capture_output=True, text=True,
    )
    return result.stdout.strip() if result.returncode == 0 else None


def _head_already_covered_by_base(repo_root, head_sha, base_ref):
    """True when head_sha is base_ref itself or an ancestor of it -- i.e.
    the worker branch never committed anything of its own beyond whatever
    base_ref already was, so a `checkout -B` reset discards nothing the
    squad merger hasn't seen (there is nothing NEW for it to have seen).

    Without this check, a worker that investigates a tag and never lands
    a commit sits forever at its creation-time head sha, which the merger
    (correctly) never records in squad-status -- nothing ever happened on
    that branch worth recording. The consume-handshake guard below reads
    "no entry" as "not yet resolved" and blocks the reset every round,
    forever, even though there was never anything to protect. This is the
    common case (most attempts fail before landing a commit), so without
    this escape hatch worker worktrees never refresh to a newer squad
    branch tip at all once created.

    Errs on the side of GUARDING (returns False, blocking the reset) if
    git can't answer -- the same fail-closed posture as the sibling
    checks this sits next to.
    """
    result = _git(["merge-base", "--is-ancestor", head_sha, base_ref], repo_root)
    return result.returncode == 0


def _squad_status_resolved(squad_status_path, sha):
    """True when `sha` is recorded consumed OR quarantined in a squad
    merger's status file (spec M2/M5 consume handshake).

    A missing file or a parse failure both read as "no entries yet" --
    get-with-default, never raise: a corrupt/absent status file must
    never be mistaken for a resolution, since that would defeat the
    whole point of the guard this feeds (it must fail closed, not open).
    """
    try:
        data = json.loads(Path(squad_status_path).read_text())
    except (OSError, ValueError):
        return False
    heads = data.get("heads") if isinstance(data, dict) else None
    return isinstance(heads, dict) and sha in heads


def create_worktree(repo_root, path, branch, base_ref, config_path=DEFAULT_CONFIG_PATH,
                    squad_status_path=None):
    """Create fmt's worktree, or -- if one from a prior failed attempt is
    still sitting at `path` (left in place for inspection, or surviving
    into the next --infinite round) -- reuse it in place after resetting it
    to a clean base_ref checkout. Reusing preserves the worktree's own
    target/ build cache; tearing down and recreating it would force a
    from-scratch cargo build every single round, which is exactly the
    "pollution" this is meant to avoid paying for repeatedly.

    A worktree's directory and its branch don't always disappear together
    -- e.g. /tmp getting wiped on reboot removes the directory but the
    branch ref lives in the repo's own object database and survives. Left
    alone, that orphaned branch makes `git worktree add -b` fail outright
    ("a branch named ... already exists") even though nothing is actually
    using it, so it's discarded here rather than treated as real state
    worth keeping.

    The reuse path's `checkout -B` re-anchor is guarded by the
    no-discard invariant (M5): if the branch still carries commits
    reachable from neither base_ref nor origin/main (a previous round's
    failed merge left them), the branch is kept as-is -- see
    _branch_has_undiscardable_commits.

    squad_status_path (spec M2/M5 CONSUME HANDSHAKE): when given, adds a
    SECOND, independent guard in front of the same `checkout -B` call --
    the branch's current head sha must be recorded consumed OR
    quarantined in that squad's status file (written by
    squad_merge_loop.py) or the reset is skipped this round too (leaving
    the branch as-is; the merger picks the commit up on its next poll).
    None (the default) is exactly today's unguarded behavior, preserving
    every existing caller and test unchanged -- this only gates formats a
    caller has explicitly opted into squad-status tracking for (a
    piloted format resolves its own squad-status path and passes it in;
    an un-piloted format passes None, same as before this parameter
    existed).
    """
    if path.is_dir():
        clean_worktree(path)
        if _branch_exists(repo_root, branch) and _branch_has_undiscardable_commits(repo_root, branch, base_ref):
            # No-discard invariant (M5): `checkout -B` would reset this
            # branch onto base_ref, permanently discarding commits a
            # previous round left unmerged (a failed merge or timeout
            # keeps the branch on purpose). Keep the branch exactly
            # where it is -- this round's worker continues on top of it
            # and the round-end merge gets another shot at consuming it.
            print(
                f"WARNING: {branch} still carries commits not on {base_ref!r}/origin/main -- "
                "reusing it as-is instead of resetting (no-discard invariant)"
            )
            subprocess.run(  # nosec B603
                ["git", "checkout", branch],
                cwd=path, check=True, capture_output=True, text=True,
            )
        elif (
            squad_status_path is not None
            and _branch_exists(repo_root, branch)
            and (head_sha := _branch_head_sha(repo_root, branch))
            and not _head_already_covered_by_base(repo_root, head_sha, base_ref)
            and not _squad_status_resolved(squad_status_path, head_sha)
        ):
            # Consume handshake (spec M2/M5): the squad merger hasn't
            # recorded this exact head as consumed or quarantined yet --
            # resetting now would race its next poll and silently drop a
            # commit it hasn't looked at. Skip the reset this round; the
            # branch is left exactly where it is (same as the no-discard
            # branch above), and the merger's next poll picks it up.
            print(
                f"WARNING: {branch} head {head_sha} is not yet consumed/quarantined in "
                f"{squad_status_path} -- skipping checkout -B this round (consume handshake, "
                "spec M2/M5); the squad merger will pick it up on its next poll"
            )
            subprocess.run(  # nosec B603
                ["git", "checkout", branch],
                cwd=path, check=True, capture_output=True, text=True,
            )
        else:
            subprocess.run(  # nosec B603
                ["git", "checkout", "-B", branch, base_ref],
                cwd=path, check=True, capture_output=True, text=True,
            )
    else:
        if _branch_exists(repo_root, branch):
            subprocess.run(["git", "branch", "-D", branch], cwd=repo_root, check=True)  # nosec B603
        subprocess.run(  # nosec B603
            ["git", "worktree", "add", "-b", branch, str(path), base_ref],
            cwd=repo_root, check=True, capture_output=True, text=True,
        )
    # config.toml is gitignored (holds API keys), so a fresh worktree
    # checkout never has one -- copy it explicitly so the worker's own
    # model_fix_loop.py finds it at its default path.
    if config_path.is_file():
        shutil.copy(config_path, path / config_path.name)


def remove_worktree(repo_root, path):
    subprocess.run(["git", "worktree", "remove", "--force", str(path)], cwd=repo_root, check=True)  # nosec B603


def delete_branch(repo_root, branch):
    subprocess.run(["git", "branch", "-D", branch], cwd=repo_root, check=True)  # nosec B603


def commits_on_branch(repo_root, base_ref, branch):
    """Commit subjects unique to branch vs base_ref, oldest first (empty
    if the worker made no commits)."""
    result = subprocess.run(  # nosec B603
        ["git", "log", f"{base_ref}..{branch}", "--format=%s", "--reverse"],
        cwd=repo_root, capture_output=True, text=True, check=True,
    )
    return [line for line in result.stdout.splitlines() if line]


def novel_commits(repo_root, base_ref, branch):
    """SHAs on `branch` whose *changes* are not already present in base_ref
    by patch-id -- i.e. genuinely new work, oldest first.

    commits_on_branch lists every commit not reachable from base_ref by SHA,
    which counts a "dirty dup" as new: a worker re-derives a fix that another
    format's sweep already landed in base_ref while this worker's worktree
    sat on an older base, so the diff is identical but the commit hash (new
    parent) differs. `git cherry` compares by patch-id instead -- a hash of
    the normalized diff, blind to line numbers and parent -- and prints
    "+ <sha>" for commits with no equivalent upstream, "- <sha>" for those
    whose patch already exists in base_ref. Keeping only the "+" lines yields
    exactly the commits worth merging; an all-"-" branch contributes nothing
    and must be dropped rather than merged as a redundant no-op.
    """
    result = subprocess.run(  # nosec B603
        ["git", "cherry", base_ref, branch],
        cwd=repo_root, capture_output=True, text=True, check=True,
    )
    return [line[2:] for line in result.stdout.splitlines() if line.startswith("+ ")]


def merge_branch(repo_root, branch, cargo_test_fn=None):
    """Merge branch into repo_root's current branch. On merge success, run
    the full test suite; if it regresses, roll back just this merge (never
    the commits before it). Returns (merged: bool, message: str).

    The rollback obeys the no-discard invariant (M5): the pre-merge HEAD
    is recorded up front, and the reset only runs when HEAD is exactly
    the merge commit just created (its first parent is that recorded
    sha) -- then the reset discards only that merge commit, whose entire
    content stays reachable from `branch`, which this failure path
    deliberately leaves in place. If HEAD is anything else (a hook or a
    concurrent process moved it), resetting could destroy commits this
    function never created, so it refuses and leaves the state for a
    human.

    cargo_test_fn, if provided, overrides the real `cargo test --workspace`
    call for testing -- must return True/False like the real check would.
    """
    pre_merge = subprocess.run(  # nosec B603
        ["git", "rev-parse", "HEAD"], cwd=repo_root, capture_output=True, text=True, check=True,
    ).stdout.strip()
    merge = subprocess.run(  # nosec B603
        ["git", "merge", "--no-ff", branch, "-m", f"merge: {branch}"],
        cwd=repo_root, capture_output=True, text=True,
    )
    if merge.returncode != 0:
        subprocess.run(["git", "merge", "--abort"], cwd=repo_root, capture_output=True, text=True)  # nosec B603
        return False, f"merge conflict: {merge.stderr.strip()}"

    tests_pass = cargo_test_fn() if cargo_test_fn else _real_cargo_test(repo_root)
    if not tests_pass:
        first_parent = subprocess.run(  # nosec B603
            ["git", "rev-parse", "HEAD^1"], cwd=repo_root, capture_output=True, text=True,
        )
        if first_parent.returncode == 0 and first_parent.stdout.strip() == pre_merge:
            subprocess.run(["git", "reset", "--hard", pre_merge], cwd=repo_root, check=True)  # nosec B603
            return False, "cargo test --workspace regressed after merge, rolled back"
        return False, (
            "cargo test --workspace regressed after merge, but HEAD no longer looks like the "
            "merge just created -- refusing to reset (no-discard invariant); resolve by hand"
        )

    return True, "merged"


def _real_cargo_test(repo_root):
    result = subprocess.run(  # nosec B603
        ["cargo", "test", "--workspace"], cwd=repo_root, capture_output=True, text=True,
    )
    return result.returncode == 0


def _process_group_alive(pgid):
    """True if any process in the group is still alive."""
    try:
        os.killpg(pgid, 0)
        return True
    except ProcessLookupError:
        return False


def _kill_process_group(pgid, sig=signal.SIGKILL):
    try:
        os.killpg(pgid, sig)
    except ProcessLookupError:
        pass


def _wait_for_process_group_exit(pgid, poll_interval=0.5, force_after=30, sleep_fn=time.sleep):
    """Block until every process in pgid has exited.

    Popen.wait()/subprocess.run() only wait for the direct child --
    cargo build/test spawn grandchildren (rustc etc.) that can outlive it,
    especially if the wait was ever interrupted. This is the single point
    that must return true before a worker is reported "done", so the
    wrapper's merge phase never starts while a worker's real work (rustc
    compilation, in particular) is still in flight. Force-kills the group
    if it's still alive well past when the direct child exited.
    """
    waited = 0.0
    while _process_group_alive(pgid):
        sleep_fn(poll_interval)
        waited += poll_interval
        if waited >= force_after:
            _kill_process_group(pgid)
            break


def _register_pgid(pgid):
    with _active_pgids_lock:
        _active_pgids.add(pgid)
    # Persist outside the lock -- _persist_active_pgids re-takes it, and
    # threading.Lock is not reentrant. Cross-thread write ordering is
    # _pgids_persist_lock's job (see _persist_active_pgids).
    _persist_active_pgids()


def _unregister_pgid(pgid):
    with _active_pgids_lock:
        _active_pgids.discard(pgid)
    _persist_active_pgids()


def _kill_all_active_workers():
    """Force-terminate every worker process group still registered. Called
    on SIGINT/SIGTERM so an interrupted wrapper never leaves orphaned
    cargo/rustc processes running unsupervised."""
    with _active_pgids_lock:
        pgids = list(_active_pgids)
    for pgid in pgids:
        _kill_process_group(pgid)


def _handle_shutdown_signal(signum, frame):
    _kill_all_active_workers()
    sys.exit(1)


def run_worker(fmt, worktree, cache_dir, log_path, timeout=None, worker_id=None):
    """Run model_fix_loop.py --only-format <fmt> inside worktree, logging
    combined stdout/stderr to log_path. Returns the process's exit code.

    worker_id defaults to fmt -- today's per-format legacy identity,
    unchanged. Squad-mode callers (process_squad_worker) pass an
    explicit "<squad>-<n>" SLOT identity instead (spec S2 "worker
    identity at >1 worker per squad"): the worker id flows into the
    claim record, /tmp/tagcmp-* suffix, prompt-log filename, and every
    model-fix-diffs//model-fix-requests filename, so it must name the
    SLOT that stays stable across rounds, not whichever format that
    slot happens to be round-robining through this particular
    invocation (see squad_worker_formats) -- two different formats
    sharing one worker_id would otherwise silently share (and overwrite
    each other's) every one of those artifacts.

    Launched in its own process group (POSIX) so this function can
    positively confirm -- and if needed, force-terminate -- the worker's
    entire process tree before returning, not just the immediate `uv run`
    child. See _wait_for_process_group_exit for why that distinction
    matters.
    """
    worker_label = worker_id if worker_id is not None else fmt
    env = dict(os.environ)
    env.pop("CARGO_TARGET_DIR", None)  # each worktree gets its own default target/, never shared
    env["EXIFTOOL_CACHE_DIR"] = str(cache_dir)
    # stdout redirected to a regular file (not a TTY) makes Python default
    # to full block buffering instead of line buffering -- print() output
    # (what watch_parallel_fix.py tails) can sit unflushed behind the
    # worker's true progress. Force unbuffered so the log file actually
    # reflects real-time state.
    env["PYTHONUNBUFFERED"] = "1"
    with open(log_path, "w") as log_file:
        proc = subprocess.Popen(  # nosec B603
            # --worker-id tags this format's manifest.log lines (see
            # model_fix_loop.py's make_logging_call_model) -- req_log_dir is
            # a single OXIDEX_HOME-fixed location every format's worker
            # shares, so without a distinct id per format, watch_parallel_fix.py
            # couldn't attribute a shared manifest.log line back to this fmt.
            ["uv", "run", "scripts/model_fix_loop.py", "--only-format", fmt, "--worker-id", worker_label],
            cwd=worktree, env=env, stdout=log_file, stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        pgid = os.getpgid(proc.pid)
        _register_pgid(pgid)
        try:
            returncode = proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            _kill_process_group(pgid)
            raise
        except BaseException:
            # Any interruption mid-wait (KeyboardInterrupt, etc.): never
            # leave the process group running unsupervised.
            _kill_process_group(pgid)
            raise
        finally:
            _wait_for_process_group_exit(pgid)
            _unregister_pgid(pgid)

    return returncode


def process_format(fmt, repo_root, base_ref, worktree_base, log_base, cache_dir, timeout,
                    config_path=DEFAULT_CONFIG_PATH):
    """Create fmt's worktree, run its worker, report what happened. Never
    raises -- failures are reported in the returned dict's status."""
    path = worktree_path(worktree_base, fmt)
    branch = branch_name(fmt)
    log_path = log_base / f"{fmt}.log"

    try:
        create_worktree(repo_root, path, branch, base_ref, config_path=config_path)
    except subprocess.CalledProcessError as e:
        return fmt, {"status": "worktree_failed", "error": e.stderr}

    try:
        returncode = run_worker(fmt, path, cache_dir, log_path, timeout=timeout)
    except subprocess.TimeoutExpired:
        return fmt, {"status": "timeout", "worktree": path, "branch": branch, "log": log_path}

    return fmt, {
        "status": "worker_done", "returncode": returncode,
        "worktree": path, "branch": branch, "log": log_path,
    }


# ---------------------------------------------------------------------------
# Squad-mode dispatch (spec S2/S5) -- alongside, not instead of, run_round
# ---------------------------------------------------------------------------

def _sml():
    """Lazy import of squad_merge_loop.py.

    squad_merge_loop imports `branch_name`/`novel_commits` from THIS
    module at ITS OWN top level (so it can reuse them rather than
    duplicate them) -- importing squad_merge_loop back at this module's
    top level would make the two modules circularly import each other.
    Deferring the import to call time sidesteps that entirely: by the
    time any squad-mode function actually runs, both modules are always
    fully initialized regardless of which one was imported first.
    """
    import squad_merge_loop
    return squad_merge_loop


def ensure_squad_staging_branch(repo_root, squad, home, log_fn=print, origin_ref=None):
    """Make sure squad/<squad> exists, cut from origin/main if this is
    the first time this squad has ever been dispatched (spec S5: squad
    workers branch from THEIR SQUAD's staging branch, never local main).

    Reuses squad_merge_loop.ensure_squad_branch (just the "create the
    branch from origin/main if missing" half of the merger's own
    ensure_staging_worktree bootstrap) rather than re-implementing that
    creation logic a second time here. Deliberately does NOT call
    ensure_staging_worktree: that function also resets/cleans the
    STAGING WORKTREE as a side effect, and that worktree is the squad's
    own merger daemon's private working area (squad_merge_loop.poll_once
    checks it out, cherry-picks, and runs targeted tests there on its
    own ~120s cadence, all under merger-<squad>.lock). This function
    runs from every squad-mode dispatch round with no lock coordination
    with that merger at all, so touching the worktree here would race a
    concurrent in-flight cherry-pick/test and could corrupt it. All the
    dispatcher actually needs is the branch NAME (squad/<squad>) to hand
    to a squad worker's create_worktree as its base ref -- never the
    worktree's checked-out state -- so only ensure_squad_branch's
    branch-existence check runs; `home` is accepted for call-signature
    symmetry with the previous version and every existing call site, but
    is no longer consulted (no staging worktree path is ever built here).

    origin_ref=None (the default) lets squad_merge_loop's own default
    ("origin/main") apply -- only tests, which have no real "origin"
    remote in their throwaway repos, ever pass an explicit local-branch
    stand-in.

    Returns the branch name ("squad/<squad>").
    """
    sml = _sml()
    kwargs = {"origin_ref": origin_ref} if origin_ref is not None else {}
    return sml.ensure_squad_branch(repo_root, squad, log_fn=log_fn, **kwargs)


def load_gap_attribution(path):
    """Read gap-attribution.json (attribute_gaps.py's own tempfile +
    os.replace output) -- a missing file or a parse failure both read
    as "nothing attributed yet" (None), never raise: production always
    regenerates this fresh before consulting it (see
    real_build_attribution), so an empty read here only shows up if
    that regeneration itself already failed and logged why."""
    try:
        return json.loads(Path(path).read_text())
    except (OSError, ValueError):
        return None


def real_build_attribution(cache_dir, squads_toml_path, out_path, perl_lib=None, repo_root=REPO_ROOT):
    """Production attribution regeneration (spec S1: "Regenerated by the
    dispatcher once per round after its full comparison"). Runs a fresh
    full-corpus comparison, hands the report to attribute_gaps.py's own
    library functions (build_tag_index/load_squads/build_attribution),
    and writes gap-attribution.json atomically -- exactly what a
    standalone `uv run scripts/attribute_gaps.py` does, minus the extra
    process. Needs a real exiftool Perl lib and a real tag-comparison
    build, so no hermetic test calls this directly -- every test injects
    build_attribution_fn instead (same discipline as squad_merge_loop's
    validate_fn/comparison_fn injection points).
    """
    import attribute_gaps
    report_path = run_full_comparison(cache_dir, repo_root=repo_root)
    report = load_comparison_report(report_path)
    perl_lib_dir = Path(perl_lib) if perl_lib else attribute_gaps.default_perl_lib()
    index, modules = attribute_gaps.build_tag_index(perl_lib_dir)
    module_to_squad, squad_names = attribute_gaps.load_squads(squads_toml_path)
    attribution = attribute_gaps.build_attribution(report, index, modules, module_to_squad, squad_names)
    attribute_gaps.write_atomic(out_path, attribution)
    return attribution


def squad_open_gaps_from_attribution(attribution):
    """{squad: open_gaps} straight from gap-attribution.json's "squads"
    summary -- allocate_squad_slots's own input shape. Missing/None
    attribution reads as "no squad has any open gaps" (empty dict), not
    an error -- the caller (run_squad_round) treats an empty allocation
    as "nothing to dispatch this round", exactly like discover_formats
    returning no formats today."""
    squads = (attribution or {}).get("squads") or {}
    return {name: agg.get("open_gaps", 0) for name, agg in squads.items()}


def squad_worker_formats(squad, attribution, squads_toml_path):
    """Formats this squad's worker slots round-robin through this round
    (spec S3: model_fix_loop.py supports only one format per process
    invocation, and a squad may own several -- cycling slot n through
    formats[(n-1) % len(formats)] is the stated interim behavior until
    model_fix_loop.py itself grows multi-format support).

    Prefers the LIVE per-round formats attribute_gaps.py just derived
    for this squad (attribution["squads"][squad]["formats"]) -- a squad
    only ever gets slots when its open_gaps > 0, so this should
    ordinarily be non-empty -- falling back to squads.toml's advisory
    "formats" list (squad_merge_loop.squad_formats) when attribution has
    nothing recorded for this squad yet. Empty means "nothing to
    dispatch this squad this round" -- the caller skips it rather than
    spawning a worker with no format to work.
    """
    squads = (attribution or {}).get("squads") or {}
    live = squads.get(squad, {}).get("formats") or []
    if live:
        return list(live)
    try:
        return _sml().squad_formats(squads_toml_path, squad)
    except (OSError, ValueError):
        return []


def process_squad_worker(squad, n, fmt, repo_root, base_ref, worktree_base, log_base, cache_dir, timeout,
                          config_path=DEFAULT_CONFIG_PATH, squad_status_path=None):
    """Create this squad SLOT's worktree/branch, run its worker against
    `fmt` (this invocation's slice of the squad's round-robin format
    list -- see squad_worker_formats), report what happened. Mirrors
    process_format exactly, with the two squad-mode differences spec
    S2/S5 call for: worker identity is the SLOT ("<squad>-<n>"), stable
    across whichever format it cycles through, not the format itself;
    and create_worktree is wired with squad_status_path so the consume
    handshake (spec M2/M5) guards this slot's `checkout -B` reset --
    legacy per-format mode (process_format) never passes this. Never
    raises -- failures are reported in the returned dict's status.
    """
    worker_id = f"{squad}-{n}"
    path = squad_worktree_path(worktree_base, squad, n)
    branch = squad_branch_name(squad, n)
    log_path = log_base / f"{worker_id}.log"

    try:
        create_worktree(repo_root, path, branch, base_ref, config_path=config_path,
                        squad_status_path=squad_status_path)
    except subprocess.CalledProcessError as e:
        return worker_id, {"status": "worktree_failed", "error": e.stderr, "squad": squad, "format": fmt}

    try:
        returncode = run_worker(fmt, path, cache_dir, log_path, timeout=timeout, worker_id=worker_id)
    except subprocess.TimeoutExpired:
        return worker_id, {
            "status": "timeout", "worktree": path, "branch": branch, "log": log_path,
            "squad": squad, "format": fmt,
        }

    return worker_id, {
        "status": "worker_done", "returncode": returncode,
        "worktree": path, "branch": branch, "log": log_path, "squad": squad, "format": fmt,
    }


def run_squad_round(args, config_path, build_attribution_fn=None, ensure_staging_branch_fn=None):
    """One squad-mode dispatch round (spec S2/S5): allocate this round's
    total_slots (args.max_parallel) across squads by live open-gap share
    (allocate_squad_slots), spawn one per-slot worker per squad branched
    from THAT SQUAD's own staging branch (squad/<squad> -- never local
    main), and report what happened.

    Unlike run_round, there is no merge phase here: consuming a squad
    worker's commits is squad_merge_loop.py's job (a separate per-squad
    daemon process polling independently, spec M2) -- this function's
    only job past dispatch is making sure every worktree it resets goes
    through the consume handshake (squad_status_path, spec M2/M5) so a
    not-yet-consumed commit is never silently reset away. run_round
    (legacy per-format mode) is completely unchanged by any of this --
    it never passes squad_status_path and keeps doing its own
    round-end merge exactly as before.

    build_attribution_fn(cache_dir) -> attribution dict, default
    real_build_attribution (a real corpus comparison + exiftool Perl
    lib scan). ensure_staging_branch_fn(repo_root, squad, home, log_fn)
    -> branch name, default ensure_squad_staging_branch. Both are
    injectable for hermetic tests, same discipline as every other
    side-effectful entry point in this fleet.

    Returns True iff no worktree_failed/timeout occurred across every
    dispatched worker -- an allocation with nothing to dispatch (no
    squad currently has open gaps, or attribution regeneration produced
    nothing) is reported and treated as success, mirroring run_round's
    "no formats with gaps" case.
    """
    build_attribution_fn = build_attribution_fn or (
        lambda cache_dir: real_build_attribution(cache_dir, args.squads_toml, args.gap_attribution_path)
    )
    ensure_staging_branch_fn = ensure_staging_branch_fn or ensure_squad_staging_branch

    # M5, same as run_round: keep local main a faithful mirror of
    # origin/main before anything else this round. Squad-mode workers
    # never branch from local main (they branch from squad/<squad>), but
    # the fast-forward is still the round's one shared "make sure the
    # dispatcher's own checkout reflects reality" step.
    _updated, ff_message = fast_forward_local_main(REPO_ROOT)
    print(f"local main mirror: {ff_message}")

    home = Path(args.home)
    attribution = build_attribution_fn(args.cache_dir)
    if attribution is None:
        print("squad round: attribution regeneration produced nothing -- skipping dispatch this round")
        return False

    squad_gaps = squad_open_gaps_from_attribution(attribution)
    allocation = allocate_squad_slots(squad_gaps, args.max_parallel)
    if not allocation:
        print("squad round: no squad currently has open gaps -- nothing to dispatch")
        return True

    print(
        f"squad slot allocation (total_slots={args.max_parallel}): "
        + ", ".join(f"{squad}={n}" for squad, n in sorted(allocation.items()))
    )

    worktree_base = Path(args.worktree_dir)
    worktree_base.mkdir(parents=True, exist_ok=True)
    log_base = Path(args.log_dir)
    log_base.mkdir(parents=True, exist_ok=True)

    jobs = []  # (squad, n, fmt, base_ref, squad_status_path)
    for squad in sorted(allocation):
        formats = squad_worker_formats(squad, attribution, args.squads_toml)
        if not formats:
            print(f"[{squad}] allocated {allocation[squad]} slot(s) but has no attributed "
                  "format to work -- skipping this round")
            continue
        base_ref = ensure_staging_branch_fn(REPO_ROOT, squad, home, print)
        squad_status_path = _sml().squad_status_file(home, squad)
        for n in range(1, allocation[squad] + 1):
            fmt = formats[(n - 1) % len(formats)]
            jobs.append((squad, n, fmt, base_ref, squad_status_path))

    if not jobs:
        print("squad round: every allocated squad had no dispatchable format -- nothing to dispatch")
        return True

    results = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.max_parallel) as pool:
        futures = {
            pool.submit(
                process_squad_worker, squad, n, fmt, REPO_ROOT, base_ref, worktree_base, log_base,
                args.cache_dir, args.timeout, config_path=config_path, squad_status_path=squad_status_path,
            ): f"{squad}-{n}"
            for squad, n, fmt, base_ref, squad_status_path in jobs
        }
        for future in concurrent.futures.as_completed(futures):
            worker_id, result = future.result()
            results[worker_id] = result
            extra = f" (exit {result['returncode']})" if "returncode" in result else ""
            print(f"[{worker_id}] {result['status']}{extra} "
                  f"(squad={result.get('squad')} format={result.get('format')})")

    failed = [(worker_id, r["status"]) for worker_id, r in results.items() if r["status"] != "worker_done"]
    print(f"\nsquad round done: {len(results) - len(failed)}/{len(results)} worker(s) finished cleanly")
    for worker_id, status in failed:
        print(f"  {worker_id}: {status}")
    print("(no merge phase in squad mode -- squad_merge_loop.py's per-squad mergers consume these branches)")

    _run_janitor_safely(janitor_kwargs={"repo_root": REPO_ROOT, "home": home, "worktree_base": worktree_base})

    return not failed


# ---------------------------------------------------------------------------
# Janitor (spec M5 "dispatcher round step") -- shared by run_round and
# run_squad_round
# ---------------------------------------------------------------------------

ORIGIN_MAIN_REF = "origin/main"

# Same ">3 days" figure squad_merge_loop.py's own re-cut trigger uses
# absent a more specific number in the spec.
JANITOR_WORKTREE_STALENESS_SECONDS = 3 * 24 * 3600

DEFAULT_DASHBOARD_LOG_PATH = OXIDEX_HOME / "logs" / "dashboard.log"
DEFAULT_DASHBOARD_LOG_ROTATE_BYTES = 50 * 1024 * 1024

DEFAULT_MODEL_FIX_REQUESTS_DIR = OXIDEX_HOME / "logs" / "model-fix-requests"
DEFAULT_MODEL_FIX_REQUESTS_MAX_AGE_SECONDS = 14 * 24 * 3600

_SQUAD_BRANCH_RE = re.compile(r"^model-fix-parallel-(?P<squad>.+)-(?P<n>\d+)$")


def squad_from_branch(branch):
    """Squad name from a squad-mode worker branch
    ("model-fix-parallel-canon-2" -> "canon"), or None for a legacy
    per-format branch ("model-fix-parallel-jpeg" -- format names in
    this fleet never end in a "-<digits>" slot suffix, so this never
    misfires on one)."""
    m = _SQUAD_BRANCH_RE.match(branch)
    return m.group("squad") if m else None


def staging_branch_or_origin(branch, origin_ref=ORIGIN_MAIN_REF):
    """The ref a stale, fully-resolved worktree should be reset onto:
    its squad's own staging branch for a squad-mode worker branch (spec
    S5: squad workers branch from squad/<squad>, never local main/
    origin), origin_ref for a legacy per-format branch."""
    squad = squad_from_branch(branch)
    return f"squad/{squad}" if squad else origin_ref


def parse_worktree_list(porcelain_text):
    """`git worktree list --porcelain` -> one record per worktree:
    {"path": Path, "branch": str|None, "detached": bool, "bare": bool,
    "prunable": bool, "locked": bool}.

    Records are separated by a blank line and always OPEN with the
    "worktree <path>" line, which is what the split below keys on; every
    other line is an optional attribute of the record in progress.
    Branch refs are reported fully qualified ("branch refs/heads/main")
    and are shortened here, since every caller compares against short
    names.

    Shared by the janitor's discover_worktree_candidates (which wants
    only branch-tracked worktrees under one base dir) and the
    auto-publish worktree sync (which wants every worktree of the repo,
    detached ones included) -- one parser, so the two can never disagree
    about what git reported.
    """
    entries = []
    current = None
    for line in porcelain_text.splitlines():
        if line.startswith("worktree "):
            if current:
                entries.append(current)
            current = {
                "path": Path(line[len("worktree "):]), "branch": None,
                "detached": False, "bare": False, "prunable": False, "locked": False,
            }
        elif current is None:
            continue
        elif line.startswith("branch "):
            ref = line[len("branch "):]
            current["branch"] = ref[len("refs/heads/"):] if ref.startswith("refs/heads/") else ref
        elif line == "detached":
            current["detached"] = True
        elif line == "bare":
            current["bare"] = True
        elif line == "prunable" or line.startswith("prunable "):
            current["prunable"] = True
        elif line == "locked" or line.startswith("locked "):
            current["locked"] = True
    if current:
        entries.append(current)
    return entries


def discover_worktree_candidates(worktree_base, repo_root=REPO_ROOT):
    """Every git worktree currently registered (`git worktree list
    --porcelain`, not a directory scan -- so a leftover directory git no
    longer tracks, or vice versa, is never treated as live) whose path
    lives under worktree_base, paired with the branch it's checked out
    on. Both legacy (model-fix-<fmt>) and squad-mode
    (model-fix-<squad>-<n>) worktrees are created under the same
    --worktree-dir, so one scan covers both dispatch modes. A detached
    worktree (no branch line) is skipped -- the janitor only ever
    resets a branch-tracked worker worktree.

    Returns [{"path": Path, "branch": str}, ...].
    """
    result = subprocess.run(  # nosec B603
        ["git", "worktree", "list", "--porcelain"],
        cwd=repo_root, capture_output=True, text=True,
    )
    if result.returncode != 0:
        return []
    entries = parse_worktree_list(result.stdout)

    worktree_base = Path(worktree_base).resolve()
    out = []
    for entry in entries:
        path, branch = entry.get("path"), entry.get("branch")
        if not path or not branch:
            continue
        try:
            path.resolve().relative_to(worktree_base)
        except (ValueError, OSError):
            continue
        out.append({"path": path, "branch": branch})
    return out


def is_worktree_stale_and_resolved(*, repo_root, branch, origin_ref=ORIGIN_MAIN_REF, squad_status=None,
                                    quarantine_entries=None, staleness_seconds=JANITOR_WORKTREE_STALENESS_SECONDS,
                                    now_fn=time.time):
    """spec M5 janitor bullet 1's eligibility check: True iff `branch`'s
    merge-base with origin_ref is older than staleness_seconds AND every
    commit it carries beyond origin_ref is accounted for -- already
    landed on origin_ref by patch-id (novel_commits/is_patch_novel_against
    say so), recorded in its squad's squad-status heads dict (any status
    -- consumed or quarantined, both mean "the merger has already looked
    at this"), or present in the quarantine ledger by patch-id.

    A branch carrying even ONE commit that fails all three checks is
    NOT eligible -- this fails closed on every ambiguous case (no branch,
    no merge-base, unparseable commit date, "too young") by returning
    False, exactly like _branch_has_undiscardable_commits errs on the
    side of refusing a reset elsewhere in this module.
    """
    if not _branch_exists(repo_root, branch):
        return False
    merge_base = _git(["merge-base", branch, origin_ref], repo_root)
    if merge_base.returncode != 0:
        return False
    base_sha = merge_base.stdout.strip()
    if not base_sha:
        return False
    committed = _git(["log", "-1", "--format=%ct", base_sha], repo_root)
    if committed.returncode != 0 or not committed.stdout.strip():
        return False
    try:
        commit_ts = int(committed.stdout.strip())
    except ValueError:
        return False
    if (now_fn() - commit_ts) < staleness_seconds:
        return False

    shas_result = _git(["log", f"{origin_ref}..{branch}", "--format=%H"], repo_root)
    if shas_result.returncode != 0:
        return False
    shas = [line for line in shas_result.stdout.splitlines() if line]

    quarantine_entries = quarantine_entries or {}
    squad_heads = (squad_status or {}).get("heads") or {}
    sml = _sml()
    for sha in shas:
        landed = not sml.is_patch_novel_against(repo_root, origin_ref, sha)
        if landed or sha in squad_heads:
            continue
        patch_id = sml.compute_patch_id_for_sha(repo_root, sha)
        if patch_id in quarantine_entries:
            continue
        return False
    return True


def reset_stale_worktree(repo_root, path, branch, base_ref, log_fn=print):
    """Actually perform the reset: discard uncommitted state (never
    touches gitignored paths, so the worktree's own target/ build cache
    survives -- same as clean_worktree everywhere else), then `checkout
    -B branch base_ref`. Only ever called after
    is_worktree_stale_and_resolved has confirmed every commit the
    branch carries is landed/consumed/quarantined, so nothing this call
    discards is unswept work."""
    clean_worktree(path)
    subprocess.run(["git", "checkout", "-B", branch, base_ref], cwd=path, check=True)  # nosec B603
    log_fn(f"janitor: reset stale, fully-resolved worktree {path} ({branch} -> {base_ref})")


def janitor_reset_stale_worktrees(*, repo_root, worktree_candidates, home, origin_ref=ORIGIN_MAIN_REF,
                                  base_ref_for=staging_branch_or_origin,
                                  staleness_seconds=JANITOR_WORKTREE_STALENESS_SECONDS,
                                  now_fn=time.time, log_fn=print, reset_fn=reset_stale_worktree):
    """spec M5 janitor bullet 1: auto-reset any worktree whose
    merge-base with origin_ref is >staleness_seconds old AND whose
    commits are all consumed-or-quarantined (per its squad's
    squad-status, or the global quarantine ledger) -- never touches a
    worktree carrying even one unresolved commit. Retires
    model-fix-gif-class time bombs without destroying unswept work.

    Returns the list of (path, branch) pairs actually reset.
    """
    sml = _sml()
    quarantine_entries = sml.load_quarantine(sml.quarantine_ledger_path(home))
    squad_status_cache = {}
    reset = []
    for entry in worktree_candidates:
        path, branch = entry["path"], entry["branch"]
        squad = squad_from_branch(branch)
        squad_status = None
        if squad is not None:
            if squad not in squad_status_cache:
                squad_status_cache[squad] = sml.load_squad_status(sml.squad_status_file(home, squad))
            squad_status = squad_status_cache[squad]
        if is_worktree_stale_and_resolved(
            repo_root=repo_root, branch=branch, origin_ref=origin_ref, squad_status=squad_status,
            quarantine_entries=quarantine_entries, staleness_seconds=staleness_seconds, now_fn=now_fn,
        ):
            reset_fn(repo_root, path, branch, base_ref_for(branch, origin_ref), log_fn=log_fn)
            reset.append((path, branch))
    return reset


def clear_held_by_foundation(tag_state_path, repo_root, origin_ref=ORIGIN_MAIN_REF, is_ancestor_fn=None,
                             log_fn=print):
    """spec M5 janitor bullet 2 / S3 T4: clear a tag-state entry's
    held_by_foundation flag once its recorded foundation commit has
    landed on origin_ref.

    held_by_foundation is a MINIMAL new tag-state field this change
    introduces: {"job": <foundation job name>, "sha": <foundation
    commit sha>} on any tag-state entry (see model_fix_loop.py's
    load_tag_state/save_tag_state). The T4 FOUNDATION-UNLOCK job that
    WRITES this field is out of scope here (Phase 4 work) -- this is
    only the clearing half, and it is a safe no-op on every entry today
    (nothing sets the field yet).

    is_ancestor_fn(sha) -> bool, default `git merge-base --is-ancestor
    sha origin_ref` in repo_root -- injectable so tests don't need a
    real origin/main history. Goes through model_fix_loop._state_locked
    (the same flock'd read-modify-write every other tag-state mutation
    in the fleet uses), so this can run safely alongside a live worker
    claiming/updating the same file. Returns the list of cleared tag_keys.
    """
    if is_ancestor_fn is None:
        def is_ancestor_fn(sha):
            result = _git(["merge-base", "--is-ancestor", sha, origin_ref], repo_root)
            return result.returncode == 0

    def mutate(state):
        cleared = []
        for tag_key, entry in state.items():
            if not isinstance(entry, dict):
                continue
            held = entry.get("held_by_foundation")
            if not isinstance(held, dict):
                continue
            sha = held.get("sha")
            if sha and is_ancestor_fn(sha):
                entry.pop("held_by_foundation", None)
                cleared.append(tag_key)
        return state, cleared

    cleared = _state_locked(tag_state_path, mutate)
    for tag_key in cleared:
        log_fn(f"janitor: cleared held_by_foundation on {tag_key!r} (foundation commit landed on {origin_ref})")
    return cleared


def rotate_dashboard_log(path=DEFAULT_DASHBOARD_LOG_PATH, max_bytes=DEFAULT_DASHBOARD_LOG_ROTATE_BYTES,
                         log_fn=print):
    """spec M5 janitor bullet 3: single rotation (never logrotate-style
    N-deep) once `path` exceeds max_bytes -- copytruncate, NOT rename.

    dashboard.log (186 MB today) is a long-running dispatcher's
    redirected stdout: one fd held open for the process's entire life,
    with no SIGHUP/reopen hook of any kind. A plain rename
    (`os.replace(path, path + ".1")`) only moves the DIRECTORY ENTRY --
    the writer's already-open fd still points at the same inode, which
    now lives at the renamed path, so it keeps appending there forever.
    dashboard.log itself would stay empty from that point on (nothing
    ever writes to the fresh inode a later `open()` would create), while
    ".1" grows unbounded and is never itself re-rotated -- silently and
    permanently defeating the whole point of periodic rotation after
    the very first rotation of this process's lifetime.

    Copying the current bytes to "<name>.1" (clobbering any previous
    one, same single-rotation semantics as before) and then truncating
    `path` IN PLACE (same inode, same fd) keeps the writer's fd valid
    and pointed at the live, now-empty path, so growth resumes being
    bounded by this same check on every later janitor pass. A small
    window between the copy and the truncate can drop a few bytes the
    writer appends mid-rotation -- the standard, accepted copytruncate
    tradeoff (see `logrotate --copytruncate`), and enormously better
    than losing rotation altogether. A missing file or one under the
    threshold is a safe no-op. Returns True iff a rotation happened."""
    path = Path(path)
    try:
        size = path.stat().st_size
    except FileNotFoundError:
        return False
    if size <= max_bytes:
        return False
    rotated = path.with_name(path.name + ".1")
    shutil.copyfile(path, rotated)
    with open(path, "r+b") as f:
        f.truncate(0)
    log_fn(f"janitor: rotated {path} ({size} bytes) -> {rotated} (copytruncate -- writer fd stays valid)")
    return True


def prune_model_fix_requests(dir_path, max_age_seconds=DEFAULT_MODEL_FIX_REQUESTS_MAX_AGE_SECONDS,
                             now_fn=time.time, log_fn=print, keep_names=("manifest.log", "cache-stats.log")):
    """spec M5 janitor bullet 4: delete files under `dir_path`
    (model_fix_loop.py's req_log_dir -- per-call request/response
    artifacts) whose mtime is older than max_age_seconds. keep_names are
    never pruned regardless of age (the running manifest/cache-stats
    logs -- only dashboard.log gets rotation per spec, these just keep
    growing and are out of scope here). Missing directory is a safe
    no-op. Returns the list of pruned paths."""
    dir_path = Path(dir_path)
    if not dir_path.is_dir():
        return []
    cutoff = now_fn() - max_age_seconds
    pruned = []
    for entry in sorted(dir_path.iterdir()):
        if not entry.is_file() or entry.name in keep_names:
            continue
        try:
            mtime = entry.stat().st_mtime
        except OSError:
            continue
        if mtime < cutoff:
            entry.unlink()
            pruned.append(entry)
    if pruned:
        log_fn(f"janitor: pruned {len(pruned)} stale model-fix-requests/ entries (older than "
               f"{max_age_seconds}s)")
    return pruned


def run_janitor(*, repo_root=REPO_ROOT, home=None, worktree_base=None, tag_state_path=None,
               dashboard_log_path=None, requests_dir=None, origin_ref=ORIGIN_MAIN_REF,
               worktree_staleness_seconds=JANITOR_WORKTREE_STALENESS_SECONDS,
               requests_max_age_seconds=DEFAULT_MODEL_FIX_REQUESTS_MAX_AGE_SECONDS,
               dashboard_max_bytes=DEFAULT_DASHBOARD_LOG_ROTATE_BYTES,
               now_fn=time.time, log_fn=print):
    """spec M5 janitor: the dispatcher round step, callable from both
    run_round (legacy per-format) and run_squad_round (spec S2/S5) --
    neither mode needs anything special from it, since every sub-action
    here is scoped generically by worktree/branch naming
    (squad_from_branch) and OXIDEX_HOME-relative paths common to both.

    Each of the four sub-actions is fully independent and individually
    safe to no-op when nothing qualifies (no stale-and-resolved
    worktrees, no held_by_foundation entries yet, a small dashboard.log,
    an empty/missing requests dir) -- one sub-action never blocks
    another.

    Returns {"worktrees_reset": [...], "held_by_foundation_cleared": [...],
    "dashboard_rotated": bool, "requests_pruned": [...]}.
    """
    home = Path(home) if home else OXIDEX_HOME
    worktree_base = Path(worktree_base) if worktree_base else (OXIDEX_HOME / "worktrees" / "parallel-fix")
    tag_state_path = Path(tag_state_path) if tag_state_path else DEFAULT_TAG_STATE_PATH
    dashboard_log_path = Path(dashboard_log_path) if dashboard_log_path else DEFAULT_DASHBOARD_LOG_PATH
    requests_dir = Path(requests_dir) if requests_dir else DEFAULT_MODEL_FIX_REQUESTS_DIR

    worktree_candidates = discover_worktree_candidates(worktree_base, repo_root)
    reset = janitor_reset_stale_worktrees(
        repo_root=repo_root, worktree_candidates=worktree_candidates, home=home, origin_ref=origin_ref,
        staleness_seconds=worktree_staleness_seconds, now_fn=now_fn, log_fn=log_fn,
    )
    cleared = clear_held_by_foundation(tag_state_path, repo_root, origin_ref=origin_ref, log_fn=log_fn)
    rotated = rotate_dashboard_log(dashboard_log_path, max_bytes=dashboard_max_bytes, log_fn=log_fn)
    pruned = prune_model_fix_requests(requests_dir, max_age_seconds=requests_max_age_seconds,
                                      now_fn=now_fn, log_fn=log_fn)

    return {
        "worktrees_reset": reset,
        "held_by_foundation_cleared": cleared,
        "dashboard_rotated": rotated,
        "requests_pruned": pruned,
    }


def _run_janitor_safely(janitor_fn=run_janitor, janitor_kwargs=None, log_fn=print):
    """run_janitor, wrapped so a housekeeping hiccup NEVER sinks a
    dispatch round's own result -- the round already did its real work
    (dispatch workers, and for run_round, merge) by the time this runs;
    janitor failures are logged and swallowed, exactly like every other
    best-effort background-maintenance call in this fleet (e.g.
    _write_fix_gap_lesson in model_fix_loop.py)."""
    try:
        janitor_fn(**(janitor_kwargs or {}))
    except Exception as e:  # noqa: BLE001 -- deliberately broad, see docstring
        log_fn(f"janitor: round-step housekeeping raised {e!r} -- continuing "
               "(this round's own result is unaffected)")


def run_round(args, config_path):
    """One discover -> dispatch -> merge cycle across every requested
    format. Returns True iff the round had no unresolved failures
    (worktree_failed/timeout/merge-conflict/test-regression) -- callers in
    --infinite mode use this only for logging, never to stop the loop,
    since a format that can't currently be fixed is expected, not fatal.
    """
    # M5, before anything else this round: keep local main a faithful
    # mirror of origin/main (ff-only, loud skip on divergence), and make
    # sure the round's merges land on an integration branch, never on
    # main itself.
    _updated, ff_message = fast_forward_local_main(REPO_ROOT)
    print(f"local main mirror: {ff_message}")
    base_ref = ensure_integration_branch(REPO_ROOT)
    if base_ref is None:
        # No safe integration target (see ensure_integration_branch's
        # docstring) -- skip the whole round rather than dispatch workers
        # whose merges would have nowhere M5-legal to land. In --infinite
        # mode the next round retries, so a dirty checkout stalls rounds
        # until fixed instead of killing the dispatcher.
        print("no usable integration branch this round -- skipping dispatch (see warning above)")
        return False

    if args.formats:
        formats = [f.strip() for f in args.formats.split(",") if f.strip()]
    else:
        print("Discovering formats with gaps (full comparison run)...")
        formats = discover_formats(args.cache_dir)

    if not formats:
        print("No formats with gaps found.")
        return True

    print(f"{len(formats)} formats to process, up to {args.max_parallel} in parallel, merging into {base_ref!r}")

    worktree_base = Path(args.worktree_dir)
    worktree_base.mkdir(parents=True, exist_ok=True)
    log_base = Path(args.log_dir)
    log_base.mkdir(parents=True, exist_ok=True)

    results = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.max_parallel) as pool:
        futures = {
            pool.submit(
                process_format, fmt, REPO_ROOT, base_ref, worktree_base, log_base, args.cache_dir, args.timeout,
                config_path=config_path,
            ): fmt
            for fmt in formats
        }
        for future in concurrent.futures.as_completed(futures):
            fmt, result = future.result()
            results[fmt] = result
            extra = f" (exit {result['returncode']})" if "returncode" in result else ""
            print(f"[{fmt}] {result['status']}{extra}")

    print("\nMerging completed worker branches...")
    merged, failed, empty, dup = [], [], [], []
    for fmt in formats:
        result = results[fmt]
        if result["status"] != "worker_done":
            failed.append((fmt, result["status"]))
            continue

        commits = commits_on_branch(REPO_ROOT, base_ref, result["branch"])
        if not commits:
            empty.append(fmt)
            remove_worktree(REPO_ROOT, result["worktree"])
            delete_branch(REPO_ROOT, result["branch"])
            continue

        # Patch-id gate: if every commit's change is already in base_ref (a
        # "dirty dup" -- the worker re-derived a fix another format's sweep
        # already landed while this worktree sat on an older base), the branch
        # adds no new diff. Merging it would only pile on redundant no-op
        # commits, so drop it here just like an empty branch. This is the
        # airtight guard against dup pollution regardless of how stale the
        # worker's base was; re-anchoring worktrees to fresh main only narrows
        # the window, it can't close it once a sweep lands mid-run.
        if not novel_commits(REPO_ROOT, base_ref, result["branch"]):
            dup.append(fmt)
            remove_worktree(REPO_ROOT, result["worktree"])
            delete_branch(REPO_ROOT, result["branch"])
            continue

        ok, message = merge_branch(REPO_ROOT, result["branch"])
        if ok:
            merged.append((fmt, len(commits)))
            remove_worktree(REPO_ROOT, result["worktree"])
            delete_branch(REPO_ROOT, result["branch"])
        else:
            failed.append((fmt, message))
            # worktree and branch deliberately left in place for inspection

    print(f"\nmerged:  {len(merged)} formats ({sum(c for _, c in merged)} commits)")
    for fmt, count in merged:
        print(f"  {fmt}: {count} commits")
    print(f"empty:   {len(empty)} formats (no commits, worktree cleaned up)")
    print(f"dup:     {len(dup)} formats (only already-merged changes by patch-id, skipped)")
    print(f"failed:  {len(failed)} formats" + (" (worktree left for inspection)" if failed else ""))
    for fmt, reason in failed:
        print(f"  {fmt}: {reason}")

    # spec M5 frames the janitor as a generic "dispatcher round step",
    # but this PR's own --squad-mode flag help text promises "Default:
    # off (run_round, today's per-format behavior, unaffected)" -- the
    # CURRENTLY RUNNING per-format fleet uses exactly this legacy path,
    # so wiring the janitor in here unconditionally would make that
    # promise false the moment this code lands and the live dispatcher
    # is next restarted (auto-resetting worktrees, rotating a
    # dashboard.log already past its threshold, pruning
    # model-fix-requests/ -- none of that today). --enable-janitor keeps
    # run_round's behavior byte-for-byte unchanged by default; an
    # operator opts in explicitly once ready, same rollout discipline as
    # --squad-mode itself. run_squad_round (a new, not-yet-live
    # entrypoint) always runs it -- there is no existing behavior for it
    # to change.
    if getattr(args, "enable_janitor", False):
        _run_janitor_safely(janitor_kwargs={
            "repo_root": REPO_ROOT, "home": Path(getattr(args, "home", None) or OXIDEX_HOME),
            "worktree_base": worktree_base,
        })

    return not failed


# ---------------------------------------------------------------------------
# Auto-publish: sweep -> cargo fmt -> PR -> merge on green -> sync every
# worktree, run by the dispatcher itself after every round
# ---------------------------------------------------------------------------
#
# WHY this lives in the dispatcher at all: scripts/overlord_sweep.py has
# been the complete publish machine since it landed (cut a fresh branch,
# merge every green squad head, semantic recheck, mechanical bisection,
# cargo test --workspace, evidence-table PR body, push, gh pr create) --
# and NOTHING has ever called it. `grep -rn overlord_sweep scripts/`
# finds only its own module, its own tests, and prose: it runs when, and
# only when, a human types the command. Measured cost of that (2026-07):
# 4330 open tag gaps, thousands of worker commits validated onto squad
# branches, 2 tag fixes ever landed on main, and 1 sweep PR (#124) ever
# opened -- which then sat red anyway, because nothing in the publish
# path ever formatted it (CI run 30186389305: Build & Test green, Lint &
# Audit red on `cargo fmt --all -- --check`). A fix pipeline whose last
# mile is manual is a fix pipeline that does not ship.
#
# So: --infinite runs the sweep in-process at the end of every round.
# Everything side-effectful (the sweep itself, the git runner, the gh
# runner, the worktree sync, sleep/clock) is injected, exactly like
# run_round's cargo_test_fn/checkout_fn seams, so each step is testable
# without a network, a gh binary, or a real remote.

# The sweep runs in its OWN worktree, never the dispatcher's checkout --
# see ensure_sweep_worktree for why that separation is load-bearing.
# Same path overlord_sweep.py's own usage line suggests.
DEFAULT_SWEEP_WORKTREE_DIR = OXIDEX_HOME / "worktrees" / "overlord-sweep"

# CI check polling. 30s matches the granularity GitHub updates check runs
# at (polling faster just burns API quota against the same answer). 45
# minutes is ~3.7x the slowest completed CI run measured on this repo
# (12.2 min, `gh run list --workflow=ci.yml`), leaving room for runner
# queueing while still bounding how long one wedged workflow can stall
# an --infinite dispatcher.
DEFAULT_PR_CHECKS_INTERVAL_SECONDS = 30
DEFAULT_PR_CHECKS_TIMEOUT_SECONDS = 45 * 60

# `gh pr checks --json bucket` normalizes every check-run/status state
# into one of these five buckets. "skipping" is green-equivalent and that
# is load-bearing, not a nicety: measured on PR #124, this repo's own CI
# reports "Multi-platform Build" and "Benchmarks & Coverage" as SKIPPED
# on every ordinary PR, so treating skipped as anything but green would
# leave every sweep PR "pending" forever and nothing would ever merge.
# Anything not listed in either set (a bucket gh adds later) is treated
# as still-pending, which can only ever delay a merge, never cause one.
CHECK_BUCKETS_RED = {"fail", "cancel"}
CHECK_BUCKETS_GREEN = {"pass", "skipping"}


def default_run_gh(args, repo_root):
    """`gh <args>` in repo_root -> (returncode, stdout, stderr) -- the
    same tuple shape as the git runners above, so one test fake can
    stand in for both."""
    result = subprocess.run(  # nosec B603
        ["gh", *args], cwd=repo_root, capture_output=True, text=True,
    )
    return result.returncode, result.stdout, result.stderr


def _is_git_worktree(path, run_git):
    """True only when `path` is the top level of a real git working
    tree. Deliberately compares `rev-parse --show-toplevel` against the
    path itself rather than just checking the exit code: a bare `mkdir`
    made INSIDE some other repo answers rc=0 with that repo's toplevel,
    and treating it as the sweep worktree would run the sweep's
    checkouts against a checkout it does not own."""
    path = Path(path)
    if not path.is_dir():
        return False
    rc, out, _err = run_git(["rev-parse", "--show-toplevel"], path)
    if rc != 0 or not out.strip():
        return False
    try:
        return Path(out.strip()).resolve() == path.resolve()
    except OSError:
        return False


def ensure_sweep_worktree(repo_root, path, run_git=default_run_git, origin_ref=ORIGIN_MAIN_REF,
                          log_fn=print):
    """The dedicated worktree every auto-publish sweep runs in.
    Returns (path, message) or (None, message) when one can't be made.

    A sweep does real checkouts: cut_fresh_sweep_branch checks out the
    new branch, and the pre/post recheck detaches to origin/main and
    back. Running that inside the DISPATCHER's own checkout would move
    HEAD out from under the very next round -- ensure_integration_branch
    reads `git rev-parse --abbrev-ref HEAD`, which answers the literal
    string "HEAD" for the detached checkout a sweep leaves behind, and
    would then merge that round's workers onto a detached HEAD. A
    separate worktree makes the two completely independent.

    Reused in place across rounds, never torn down: the post-merge
    recheck and `cargo test --workspace` both build here, so recreating
    it every round would pay a full cold cargo build every round -- the
    same reasoning create_worktree spells out for worker worktrees.
    Reuse resets it to a clean detached origin_ref checkout, because it
    arrives sitting on the PREVIOUS round's sweep branch, which
    `git branch <new> origin/main` + checkout would otherwise trip over.
    Detaching discards nothing: the previous sweep branch ref stays
    exactly where it was (merged, pushed, or left for inspection).

    Provisioning happens on the first auto-publish round whether or not
    that round has anything to sweep -- deliberately, so that "is there
    news?" has exactly one authority (run_sweep's own sweep-state.json
    cursor) instead of a second, subtly-different pre-check here that
    could silently decide never to publish. The cost is one checkout,
    once, and two cheap git calls per round thereafter.

    Reuse is gated on the path actually BEING a git worktree, not merely
    on the directory existing. `if path.is_dir():` (what this did until
    2026-07-26) took the reuse branch for a directory git has never
    heard of -- a half-failed `worktree add`, a `worktree remove` that
    left the directory behind, a plain `mkdir`, or a human who ran
    overlord_sweep.py's own `--repo <dir>` usage line by hand. All three
    git calls in the reuse branch then fail, this returns (None, ...),
    and auto-publish is silently and permanently disabled for the life
    of the dispatcher -- indistinguishable, in the log, from a healthy
    no-news round. The prune + `worktree add` self-heal below is the
    recovery for exactly that state, and it was unreachable while the
    directory existed.
    """
    path = Path(path)
    if _is_git_worktree(path, run_git):
        # Same discard-local-mess pair clean_worktree uses (git clean -fd
        # never touches gitignored paths, so this worktree's own target/
        # build cache survives), then detach onto a fresh origin/main.
        run_git(["checkout", "--", "."], path)
        run_git(["clean", "-fd"], path)
        rc, _out, err = run_git(["checkout", "--force", "--detach", origin_ref], path)
        if rc != 0:
            message = f"could not reset the sweep worktree at {path} to {origin_ref}: {err.strip()}"
            log_fn(f"WARNING: {message} -- skipping auto-publish this round")
            return None, message
        return path, f"reused sweep worktree at {path}"

    path.parent.mkdir(parents=True, exist_ok=True)
    # A worktree whose directory was deleted out from under git (a wiped
    # /tmp, a manual rm -rf) stays REGISTERED, and its stale registration
    # makes `worktree add` refuse that same path forever. Pruning first
    # costs nothing when there is nothing stale to prune. This is also
    # the recovery for a directory that exists but is not a registered
    # worktree (see the docstring): prune drops the half-written
    # registration, and `worktree add` accepts an existing EMPTY
    # directory. A non-empty unregistered directory still fails here,
    # deliberately and loudly -- nothing in this fleet deletes a
    # directory it did not create, and git's own error message ("... is
    # not an empty directory") says exactly what a human has to do.
    run_git(["worktree", "prune"], repo_root)
    rc, _out, err = run_git(["worktree", "add", "--detach", str(path), origin_ref], repo_root)
    if rc != 0:
        message = f"could not create the sweep worktree at {path}: {err.strip()}"
        log_fn(f"WARNING: {message} -- skipping auto-publish this round")
        return None, message
    return path, f"created sweep worktree at {path}"


def default_sweep_fn(**kwargs):
    """overlord_sweep.run_sweep with its two real side-effect runners
    (the per-format comparison and the recheck's checkout) filled in --
    the same pair overlord_sweep.main() wires up for the human-driven
    CLI, so the automated and manual paths run identical code.

    The imports are deliberately INSIDE the function: overlord_sweep
    imports squad_merge_loop, which imports branch_name/novel_commits
    from THIS module at its own module level. A top-level
    `import overlord_sweep` here would therefore be a genuine circular
    import that blows up whenever this module is imported first -- which
    is every dispatcher run.
    """
    import overlord_sweep
    import squad_merge_loop

    def comparison_fn(repo, cache_dir, fmt, suffix):
        return squad_merge_loop.real_format_match(repo, cache_dir, fmt, suffix)

    return overlord_sweep.run_sweep(
        comparison_fn=comparison_fn, checkout_fn=overlord_sweep.real_checkout, **kwargs,
    )


def pr_ref_from_result(pr_result, branch):
    """What to hand `gh pr checks` / `gh pr merge` for the PR run_sweep
    just created. overlord_sweep.real_create_pr returns gh's own stdout
    (the PR URL) under "stdout"; other create_pr_fn implementations
    return it under "url". The branch name is the fallback rather than
    an error, because `gh pr <cmd> <branch>` resolves a PR by head
    branch and the branch is always pushed before the PR is created."""
    if isinstance(pr_result, dict):
        for key in ("url", "stdout"):
            for line in str(pr_result.get(key) or "").splitlines():
                line = line.strip()
                if line.startswith("http"):
                    return line
    return branch


def pr_checks_state(pr_ref, repo_root, run_gh=default_run_gh):
    """("green"|"red"|"pending"|"unknown", detail) for one PR's checks.

    The JSON is parsed rather than the exit code interpreted: gh exits 8
    for "pending" and 1 for "failing", but it also exits 1 for auth
    failures, an unknown PR, and network errors -- and a merge decision
    must never rest on a status this code cannot tell apart. gh's own
    `bucket` field is the normalization of the dozen underlying state
    strings, so it is what gets classified here.

    An EMPTY check list reads as "pending", never "green": in the first
    seconds after a push GitHub has not registered the workflows yet,
    and a repo whose checks never appear must time out with its PR left
    open rather than merge something nothing ever tested.
    """
    rc, out, err = run_gh(["pr", "checks", pr_ref, "--json", "bucket,name,state"], repo_root)
    try:
        checks = json.loads(out)
    except ValueError:
        detail = (err or out).strip() or f"gh pr checks exited {rc} with no JSON output"
        return "unknown", detail
    if not isinstance(checks, list):
        return "unknown", f"gh pr checks returned {type(checks).__name__}, expected a list"
    checks = [c for c in checks if isinstance(c, dict)]
    if not checks:
        return "pending", "no checks reported yet"
    detail = ", ".join(f"{c.get('name')}={c.get('bucket')}" for c in checks)
    buckets = [str(c.get("bucket") or "").lower() for c in checks]
    if any(b in CHECK_BUCKETS_RED for b in buckets):
        return "red", detail
    if any(b not in CHECK_BUCKETS_GREEN for b in buckets):
        return "pending", detail
    return "green", detail


def wait_for_pr_checks(pr_ref, repo_root, run_gh=default_run_gh, sleep_fn=time.sleep,
                       now_fn=time.monotonic, timeout_seconds=DEFAULT_PR_CHECKS_TIMEOUT_SECONDS,
                       interval_seconds=DEFAULT_PR_CHECKS_INTERVAL_SECONDS, max_unknown_polls=3,
                       log_fn=print):
    """Poll a PR's checks until they settle. Returns (state, detail)
    with state in "green"/"red"/"timeout"/"unknown".

    Bounded on purpose. An --infinite dispatcher runs for weeks and must
    not park forever on one wedged workflow (a stuck runner, a queue
    backlog, a PR whose checks are never created); on timeout the PR is
    left OPEN and the loop moves on, since the next round's sweep cuts
    an independent branch. Repeated `unknown` answers (auth expired, gh
    not installed, network down) give up after max_unknown_polls rather
    than burning the full timeout on a question that is not going to get
    a different answer -- one-off blips still just retry.

    now_fn defaults to time.monotonic, never time.time: a wall-clock
    step (NTP correction, DST) must not silently extend or truncate a CI
    wait.
    """
    deadline = now_fn() + timeout_seconds
    unknowns = 0
    while True:
        state, detail = pr_checks_state(pr_ref, repo_root, run_gh)
        if state in ("green", "red"):
            return state, detail
        if state == "unknown":
            unknowns += 1
            if unknowns >= max_unknown_polls:
                return "unknown", detail
        else:
            unknowns = 0
        if now_fn() >= deadline:
            return "timeout", detail
        log_fn(f"auto-publish: checks for {pr_ref} are {state} ({detail}) -- "
               f"re-polling in {interval_seconds}s")
        sleep_fn(interval_seconds)


def pr_state(pr_ref, repo_root, run_gh=default_run_gh):
    """GitHub's own view of a PR: "OPEN"/"MERGED"/"CLOSED", or None when
    that can't be determined."""
    rc, out, _err = run_gh(["pr", "view", pr_ref, "--json", "state"], repo_root)
    if rc != 0:
        return None
    try:
        data = json.loads(out)
    except ValueError:
        return None
    state = data.get("state") if isinstance(data, dict) else None
    return str(state).upper() if state else None


def merge_pr(pr_ref, repo_root, run_gh=default_run_gh):
    """Squash-merge a PR whose checks are already green. Returns
    (ok, message).

    `gh pr merge --auto` is deliberately NOT used: repository-level
    auto-merge is DISABLED on this repo, so --auto is rejected outright
    ("Auto merge is not allowed for this repository") and the poll-then-
    merge path above is the only one that works here. --delete-branch
    keeps origin clean, which costs nothing: next_sweep_branch_name
    never reuses a branch name anyway.

    A non-zero exit is NOT taken as proof the PR is unmerged. gh does
    its branch cleanup AFTER the API merge call -- deleting the local
    branch, moving the checkout to the default branch -- and in a
    multi-worktree repo like this fleet's that cleanup can legitimately
    fail on its own (`git checkout main` refuses while main is checked
    out in another worktree). Believing it would report merge_failed for
    a main that really did advance, and skip the worktree sync that
    should follow. So a failure re-asks GitHub what actually happened.
    """
    rc, out, err = run_gh(["pr", "merge", pr_ref, "--squash", "--delete-branch"], repo_root)
    message = (out + err).strip()
    if rc == 0:
        return True, message
    if pr_state(pr_ref, repo_root, run_gh) == "MERGED":
        return True, f"gh exited {rc} during post-merge cleanup, but the PR reports MERGED: {message}"
    return False, message


# Sweep branches are cut by overlord_sweep.next_sweep_branch_name as
# "sweep/tags-<date>-<n>"; adoption only ever touches this automation's
# own namespace.
SWEEP_BRANCH_PREFIX = "sweep/"


def list_open_sweep_prs(repo_root, run_gh=default_run_gh):
    """Open PRs whose head branch is a sweep branch, oldest (lowest PR
    number) first. [] on any failure -- an expired token or a missing
    `gh` must cost one skipped adoption pass, never an exception inside
    an --infinite dispatcher."""
    try:
        _rc, out, _err = run_gh(
            ["pr", "list", "--state", "open", "--json", "number,url,headRefName", "--limit", "50"],
            repo_root,
        )
    except OSError:
        # `gh` not installed, or repo_root gone. This is the FIRST gh
        # call of every round now -- including rounds that have no news
        # and previously made none at all -- so it must not be the thing
        # that turns a quiet round into a raised exception.
        return []
    try:
        prs = json.loads(out)
    except ValueError:
        return []
    if not isinstance(prs, list):
        return []
    sweeps = [
        pr for pr in prs
        if isinstance(pr, dict) and str(pr.get("headRefName") or "").startswith(SWEEP_BRANCH_PREFIX)
    ]
    return sorted(sweeps, key=lambda pr: pr.get("number") or 0)


def adopt_open_sweep_prs(*, repo_root, run_gh=default_run_gh, log_fn=print):
    """Merge any ALREADY-OPEN sweep PR whose checks are green right now.
    Returns one dict per open sweep PR:
    {"pr", "branch", "checks", "action": "merged"|"left_open"|"merge_failed",
     "message"}.

    Without this, an abandoned sweep PR was abandoned forever. Nothing
    ever revisited a PR after the round that created it moved on, and
    overlord_sweep.run_sweep persists its sweep-state cursor BEFORE the
    push and before PR creation -- so the stamps that fed that PR are
    already consumed and no later sweep re-cuts those commits. A run
    whose checks went green 46 minutes after the 45-minute
    wait_for_pr_checks timeout (or that was red and then fixed by hand,
    or whose `gh pr create` needed a manual retry) stranded every fix in
    it on origin permanently.

    Deliberately NOT a wait: one `gh pr checks` read per open sweep PR,
    then move on. The round's own sweep still has to run, and a PR that
    is genuinely mid-CI gets picked up by the next round -- blocking
    here would just move the 45-minute stall to the front of the round.

    Deliberately scoped to sweep/* head branches. This dispatcher runs
    unattended for weeks; squash-merging a human's PR because its checks
    happened to be green is not a mistake it gets to make.

    Cursor advancement is left exactly as run_sweep has it (durable once
    the branch is cut, semantically rechecked and workspace-tested,
    regardless of push/PR outcome). Adopting the PR is strictly better
    than un-advancing the cursor would be: the commits are already
    pushed on their sweep branch, so re-sweeping the same stamps would
    open a SECOND PR carrying the same fixes and leave two branches
    racing to merge the same content.
    """
    adopted = []
    for pr in list_open_sweep_prs(repo_root, run_gh):
        ref = pr.get("url") or str(pr.get("number"))
        branch = pr.get("headRefName")
        state, detail = pr_checks_state(ref, repo_root, run_gh)
        if state != "green":
            log_fn(f"auto-publish: adopting {ref} ({branch}) -- checks are {state} ({detail}); "
                   "leaving it open for a later round")
            adopted.append({"pr": ref, "branch": branch, "checks": state, "action": "left_open",
                            "message": detail})
            continue
        log_fn(f"auto-publish: adopting {ref} ({branch}) from an earlier round -- checks are green, "
               "merging it now")
        merged, message = merge_pr(ref, repo_root, run_gh=run_gh)
        adopted.append({"pr": ref, "branch": branch, "checks": state,
                        "action": "merged" if merged else "merge_failed", "message": message})
        if not merged:
            log_fn(f"AUTO-PUBLISH: could not merge adopted PR {ref}: {message} -- left open")
    return adopted


def sync_worktrees_to_origin_main(*, repo_root=REPO_ROOT, run_git=default_run_git,
                                  origin_ref=ORIGIN_MAIN_REF, fetch=True, log_fn=print):
    """Fast-forward every worktree of this repo onto the just-merged
    origin/main. Returns {"updated": [...], "current": [...],
    "skipped": [(path, reason)], "failed": [(path, reason)]}.

    `git worktree list --porcelain` enumerates them (~100 live ones
    across ~/.oxidex/worktrees and ~/.claude/worktrees), and two rules
    are absolute:

      * ff-only, and never touch a worktree with anything to lose. A
        worktree whose HEAD is not an ancestor of origin/main carries
        commits origin/main doesn't have -- an unswept worker commit, a
        squad branch ahead of main, the dispatcher's own
        model-fix-sweep-local -- and is SKIPPED with a logged reason.
        This is fast_forward_local_main's no-discard invariant applied
        worktree by worktree.
      * A worktree with modified tracked files is skipped: that is a
        worker's in-progress edit, and discarding one to save a
        fast-forward is never the right trade.

    Untracked-but-unignored files deliberately do NOT count as dirty
    here (`--untracked-files=no`). Long-lived worktrees accumulate stray
    artifacts -- comparison reports written next to the source, editor
    droppings, *.orig files from a resolved conflict -- and treating
    those as "in progress" would make a large share of the fleet
    permanently un-syncable. Nothing is risked by ignoring them: git
    itself refuses to overwrite an untracked file during a merge or
    checkout, so a genuine collision surfaces as a recorded failure
    below, never as a silent loss.

    A detached worktree fast-forwards via `git checkout --detach`, which
    is the only way a detached HEAD can express a fast-forward; the
    ancestor check above has already proven it discards nothing.
    """
    summary = {"updated": [], "current": [], "skipped": [], "failed": []}
    if fetch:
        rc, _out, err = run_git(["fetch", "origin", "main"], repo_root)
        if rc != 0:
            log_fn(f"worktree sync: `git fetch origin main` failed ({err.strip()}) -- syncing "
                   f"against the last-known {origin_ref} instead")
    rc, target, err = run_git(["rev-parse", "--verify", "--quiet", origin_ref], repo_root)
    target = target.strip()
    if rc != 0 or not target:
        log_fn(f"worktree sync: cannot resolve {origin_ref} ({err.strip()}) -- nothing synced")
        return summary
    rc, listing, err = run_git(["worktree", "list", "--porcelain"], repo_root)
    if rc != 0:
        log_fn(f"worktree sync: `git worktree list` failed ({err.strip()}) -- nothing synced")
        return summary

    for entry in parse_worktree_list(listing):
        path = entry["path"]
        if entry["bare"]:
            summary["skipped"].append((path, "bare worktree (no working copy to fast-forward)"))
            continue
        if not path.is_dir():
            summary["skipped"].append((path, "directory is gone (registration is prunable)"))
            continue
        # --untracked-files=no: see the docstring -- untracked droppings
        # are not in-progress work, and git protects them on its own.
        rc, status_out, err = run_git(["status", "--porcelain", "--untracked-files=no"], path)
        if rc != 0:
            summary["failed"].append((path, f"git status failed: {err.strip()}"))
            continue
        if status_out.strip():
            summary["skipped"].append((path, "dirty (modified tracked files -- work in progress)"))
            continue
        rc, head, err = run_git(["rev-parse", "HEAD"], path)
        if rc != 0:
            summary["failed"].append((path, f"could not resolve HEAD: {err.strip()}"))
            continue
        if head.strip() == target:
            summary["current"].append(path)
            continue
        rc, _out, _err = run_git(["merge-base", "--is-ancestor", "HEAD", origin_ref], path)
        if rc != 0:
            summary["skipped"].append(
                (path, f"has commits not on {origin_ref} (unpushed/unswept work -- no-discard)"),
            )
            continue
        if entry["branch"]:
            rc, _out, err = run_git(["merge", "--ff-only", origin_ref], path)
        else:
            rc, _out, err = run_git(["checkout", "--detach", origin_ref], path)
        if rc != 0:
            summary["failed"].append((path, f"fast-forward failed: {err.strip()}"))
            continue
        summary["updated"].append(path)

    log_fn(
        f"worktree sync: {len(summary['updated'])} fast-forwarded, "
        f"{len(summary['current'])} already current, {len(summary['skipped'])} skipped, "
        f"{len(summary['failed'])} failed"
    )
    for path, reason in summary["skipped"] + summary["failed"]:
        log_fn(f"  {path}: {reason}")
    return summary


def auto_publish_round(*, repo_root=REPO_ROOT, cache_dir, home=None, squads_toml_path=None,
                       sweep_worktree_dir=None, sweep_fn=None, ensure_worktree_fn=None,
                       sync_fn=None, fmt_fn=None, run_git=default_run_git, run_gh=default_run_gh,
                       sleep_fn=time.sleep, now_fn=time.monotonic,
                       checks_timeout_seconds=DEFAULT_PR_CHECKS_TIMEOUT_SECONDS,
                       checks_interval_seconds=DEFAULT_PR_CHECKS_INTERVAL_SECONDS,
                       origin_ref=ORIGIN_MAIN_REF, log_fn=print):
    """One end-to-end publish pass: adopt any stale sweep PR -> sweep ->
    cargo fmt -> push -> PR -> merge on green -> fast-forward every
    worktree.

    Returns a summary dict whose "status" is either one of run_sweep's
    own statuses passed straight through ("no_news",
    "branch_cut_failed", "nothing_merged", "sweep_aborted",
    "reattach_failed", "workspace_tests_failed", "push_failed",
    "pr_create_failed"), or one of this function's own: "no_worktree",
    "checks_red", "checks_timeout", "checks_unknown", "merge_failed",
    "merged". Every status also carries "adopted": what the round-start
    adoption pass did with each already-open sweep PR.

    A round that produced nothing new writes nothing: run_sweep's
    sweep-state.json cursor reports "no_news" and returns before cutting
    a branch, before touching a single ref -- so nothing is committed,
    nothing is pushed, and no PR of this round's is touched. The one
    thing such a round still does is the read-only `gh pr list` adoption
    pass below, which can merge an ALREADY-OPEN sweep PR that has since
    gone green; that is the point of it, and it is the only way a
    stranded PR ever lands (see adopt_open_sweep_prs).

    Red or timed-out checks leave the PR OPEN and return normally: the
    dispatcher loop must never block forever on CI, and must never merge
    a PR whose checks are failing or still pending.
    """
    home = Path(home) if home else OXIDEX_HOME
    sweep_worktree_dir = Path(sweep_worktree_dir) if sweep_worktree_dir else DEFAULT_SWEEP_WORKTREE_DIR
    sweep_fn = sweep_fn or default_sweep_fn
    ensure_worktree_fn = ensure_worktree_fn or ensure_sweep_worktree
    sync_fn = sync_fn or sync_worktrees_to_origin_main

    def sync():
        return sync_fn(repo_root=repo_root, run_git=run_git, origin_ref=origin_ref, log_fn=log_fn)

    # Round start, BEFORE the sweep cuts anything: a sweep PR left open
    # by an earlier round may have gone green since, and nothing else in
    # this system will ever look at it again. Runs against the
    # dispatcher's own repo_root rather than the sweep worktree so that
    # a worktree this round cannot provision (below) does not also
    # strand every previously-open PR.
    adopted = adopt_open_sweep_prs(repo_root=repo_root, run_gh=run_gh, log_fn=log_fn)
    adopted_merges = [a for a in adopted if a["action"] == "merged"]
    adopted_sync = sync() if adopted_merges else None

    sweep_repo, message = ensure_worktree_fn(
        repo_root, sweep_worktree_dir, run_git=run_git, origin_ref=origin_ref, log_fn=log_fn,
    )
    if sweep_repo is None:
        return {"status": "no_worktree", "message": message, "adopted": adopted,
                "adopted_sync": adopted_sync}

    sweep_kwargs = {
        "repo_root": sweep_repo, "home": home, "cache_dir": cache_dir,
        "fmt_fn": fmt_fn, "run_git": run_git, "log_fn": log_fn,
        # run_sweep derives quarantine.jsonl and the sweep-review log from
        # `home`, but its sweep-state.json default is the bare
        # OXIDEX_HOME constant -- passing it explicitly keeps the whole
        # publish state under whatever --home the dispatcher was given
        # (identical to the default when that is OXIDEX_HOME) instead of
        # reading one operator's cursor while writing another's stamps.
        "sweep_state_path": home / "logs" / "sweep-state.json",
    }
    if squads_toml_path:
        sweep_kwargs["squads_toml_path"] = squads_toml_path
    result = sweep_fn(**sweep_kwargs)
    common = {"sweep": result, "adopted": adopted, "adopted_sync": adopted_sync}
    status = result.get("status")
    if status != "ok":
        log_fn(f"auto-publish: sweep finished with status {status!r} -- no PR to merge this round")
        return {"status": status, **common}

    branch = result.get("branch")
    pr_ref = pr_ref_from_result(result.get("pr"), branch)
    log_fn(f"auto-publish: PR open for {branch} ({pr_ref}) -- waiting for checks")
    state, detail = wait_for_pr_checks(
        pr_ref, sweep_repo, run_gh=run_gh, sleep_fn=sleep_fn, now_fn=now_fn,
        timeout_seconds=checks_timeout_seconds, interval_seconds=checks_interval_seconds,
        log_fn=log_fn,
    )
    if state == "green":
        # One re-read immediately before the merge. .github/workflows/
        # ci.yml:11-13 sets `concurrency: cancel-in-progress: true`, so a
        # push to the same ref or a workflow re-run moves an in-flight
        # job into the "cancel" bucket -- which pr_checks_state classifies
        # as RED -- in the window between the poll that returned green
        # and `gh pr merge`. Without this, that window merges a PR whose
        # checks are no longer green. Costs one gh call per publish.
        state, detail = pr_checks_state(pr_ref, sweep_repo, run_gh=run_gh)
        if state != "green":
            log_fn(f"auto-publish: checks for {pr_ref} changed to {state.upper()} ({detail}) between "
                   "the green poll and the merge -- not merging")
    if state != "green":
        log_fn(f"AUTO-PUBLISH: checks for {pr_ref} are {state.upper()} ({detail}) -- leaving the PR "
               "OPEN for a human and continuing the loop; nothing is merged on anything but green")
        return {"status": f"checks_{state}", "pr_ref": pr_ref, "checks": detail, **common}

    merged, merge_message = merge_pr(pr_ref, sweep_repo, run_gh=run_gh)
    if not merged:
        log_fn(f"AUTO-PUBLISH: `gh pr merge --squash` failed for {pr_ref}: {merge_message} -- "
               "PR left open")
        return {"status": "merge_failed", "pr_ref": pr_ref, "message": merge_message, **common}

    log_fn(f"auto-publish: squash-merged {pr_ref} into main -- fast-forwarding worktrees")
    return {"status": "merged", "pr_ref": pr_ref, "sync": sync(), **common}


def _run_auto_publish_safely(publish_fn=auto_publish_round, publish_kwargs=None, log_fn=print):
    """auto_publish_round, wrapped so a publish hiccup NEVER sinks the
    dispatcher -- same discipline as _run_janitor_safely. The round's
    own work (dispatch + merge onto squad/integration branches) is
    already durable by the time this runs, and an --infinite dispatcher
    lives for weeks: a transient gh outage or a git error must cost one
    round's publish, not the whole loop."""
    try:
        return publish_fn(**(publish_kwargs or {}))
    except Exception as e:  # noqa: BLE001 -- deliberately broad, see docstring
        log_fn(f"auto-publish: publish step raised {e!r} -- continuing (this round's fixes stay "
               "on their squad branches and the next round's sweep retries them)")
        return {"status": "raised", "error": repr(e)}


def main(argv=None, run_round_fn=run_round, run_squad_round_fn=run_squad_round, sleep_fn=time.sleep,
         lock_path=None, pgids_path=None, reap_fn=reap_orphan_worker_pgids,
         auto_publish_fn=auto_publish_round):
    # The same buffering issue fixed for workers (PYTHONUNBUFFERED in
    # run_worker's env) also applies to this wrapper process itself when its
    # own stdout is redirected to a file (e.g. `nohup ... > out.log &`)
    # rather than a TTY -- confirmed live: its print() status lines sat
    # completely unflushed, making it impossible to tell what it was doing
    # (mid-merge? stuck? just sleeping?) without attaching a debugger. See
    # parallel_tag_fix_loop.py's main() for the sibling fix.
    sys.stdout.reconfigure(line_buffering=True)
    # An interrupted wrapper (Ctrl-C, SIGTERM) must not leave worker
    # process trees (cargo build/test, rustc) running unsupervised.
    signal.signal(signal.SIGINT, _handle_shutdown_signal)
    signal.signal(signal.SIGTERM, _handle_shutdown_signal)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--config", default=str(DEFAULT_CONFIG_PATH),
        help="Path to config.toml, copied into every worker's worktree (see config.example.toml)",
    )
    parser.add_argument(
        "--max-parallel", type=int,
        default=int(os.environ.get("MODEL_FIX_MAX_PARALLEL", str(DEFAULT_MAX_PARALLEL))),
        help=f"Default: min(20, CPU count) = {DEFAULT_MAX_PARALLEL} on this machine. Each worker "
             "runs a full `cargo test --workspace` before committing -- oversubscribing past the "
             "core count makes those test runs contend for CPU and risks spurious regressions "
             "that aren't actually caused by the fix being tested.",
    )
    parser.add_argument(
        "--formats",
        help="Comma-separated format list; default: auto-discover every format with gaps, "
             "re-discovered fresh every round when combined with --infinite",
    )
    parser.add_argument(
        "--infinite", action="store_true",
        help="Keep running discover -> dispatch -> merge rounds back to back, forever, until "
             "interrupted (Ctrl-C/SIGTERM). Each round re-discovers formats with gaps from "
             "scratch (unless --formats pins a fixed list), so newly-exposed or still-unfixed "
             "gaps keep getting retried across rounds. A round with zero formats or zero "
             "successful fixes is not a stop condition -- only an interrupt stops this mode.",
    )
    parser.add_argument(
        "--round-delay", type=float, default=0,
        help="Seconds to sleep between rounds in --infinite mode (default: 0, back to back)",
    )
    # Fixed /tmp defaults are a race-condition concern on shared multi-user
    # systems; this is a single-developer local CLI tool, and every one of
    # these is overridable via its env var or flag.
    parser.add_argument(
        "--cache-dir",
        default=os.environ.get("EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache"),  # nosec B108
    )
    parser.add_argument("--timeout", type=int, default=None, help="Per-worker timeout in seconds (default: none)")
    parser.add_argument(
        "--worktree-dir",
        default=os.environ.get("MODEL_FIX_WORKTREE_DIR", str(OXIDEX_HOME / "worktrees" / "parallel-fix")),
    )
    parser.add_argument(
        "--log-dir",
        default=os.environ.get("MODEL_FIX_LOG_DIR", str(DEFAULT_LOG_DIR)),
        help=f"Default: {DEFAULT_LOG_DIR} -- deliberately NOT under /tmp, which is wiped on "
             "reboot and excluded from Time Machine, so a run's worker logs are the one thing "
             "that survives to explain what happened after the fact.",
    )
    parser.add_argument(
        "--enable-janitor", action="store_true",
        help="spec M5 janitor round-step (worktree auto-reset, held_by_foundation clearing, "
             "dashboard.log rotation, model-fix-requests/ pruning) in LEGACY per-format mode "
             "(run_round). Default: off, so run_round's behavior is byte-for-byte unchanged by "
             "this flag's mere existence -- the currently-running per-format fleet is unaffected "
             "until an operator opts in here. --squad-mode's run_squad_round always runs the "
             "janitor regardless of this flag (a new entrypoint, nothing to leave unaffected).",
    )
    parser.add_argument(
        "--squad-mode", action="store_true",
        help="Spec Phase 3 cutover: dispatch per-SQUAD worker slots (run_squad_round) instead of "
             "one worker per format (run_round). Squads own ExifTool modules -- and therefore "
             "every container format those modules serve -- and workers branch from their "
             "squad's own staging branch (squad/<squad>, created by squad_merge_loop.py), never "
             "from local main. Default: off (run_round, today's per-format behavior, unaffected).",
    )
    parser.add_argument(
        "--squads-toml", default=str(DEFAULT_SQUADS_TOML),
        help="Squad manifest (spec S2); consulted in --squad-mode and by the auto-publish sweep "
             "(which walks every squad's status file looking for green stamps).",
    )
    parser.add_argument(
        "--home", default=str(OXIDEX_HOME),
        help="OXIDEX_HOME override for squad-status/staging-worktree paths; consulted in "
             "--squad-mode and by the auto-publish sweep (squad-status, sweep-state.json, "
             "quarantine.jsonl all hang off it).",
    )
    parser.add_argument(
        "--auto-publish", action=argparse.BooleanOptionalAction, default=None,
        help="After every round, run the overlord sweep IN-PROCESS (overlord_sweep.run_sweep): "
             "cut a fresh sweep branch from origin/main, merge every squad head stamped green "
             "since the last sweep, semantically recheck it, cargo fmt it, push, open a PR, wait "
             "for CI, squash-merge on all-green, then fast-forward every worktree to the new "
             "origin/main. DEFAULT: ON with --infinite (the whole point of --infinite is an "
             "unattended pipeline), off for a single round; --no-auto-publish opts out either "
             "way. A round with no new green stamps is a complete no-op -- no commits, no "
             "pushes, no PR touched. NOTE: the green stamps come from the per-squad mergers "
             "(scripts/squad_merge_loop.py); with no merger running there is nothing to sweep "
             "and this reports no_news every round.",
    )
    parser.add_argument(
        "--sweep-worktree-dir", default=str(DEFAULT_SWEEP_WORKTREE_DIR),
        help=f"Where the auto-publish sweep runs (default: {DEFAULT_SWEEP_WORKTREE_DIR}). "
             "Deliberately NOT the dispatcher's own checkout -- a sweep detaches HEAD, which "
             "would break the next round's integration-branch resolution.",
    )
    parser.add_argument(
        "--pr-checks-timeout", type=float, default=DEFAULT_PR_CHECKS_TIMEOUT_SECONDS,
        help=f"Seconds to wait for a sweep PR's checks before giving up (default: "
             f"{DEFAULT_PR_CHECKS_TIMEOUT_SECONDS}). On timeout the PR is left OPEN and the loop "
             "continues -- a wedged workflow must not stall an --infinite dispatcher forever.",
    )
    parser.add_argument(
        "--pr-checks-interval", type=float, default=DEFAULT_PR_CHECKS_INTERVAL_SECONDS,
        help=f"Seconds between `gh pr checks` polls (default: {DEFAULT_PR_CHECKS_INTERVAL_SECONDS}).",
    )
    parser.add_argument(
        "--gap-attribution-path", default=str(DEFAULT_GAP_ATTRIBUTION_PATH),
        help="Where the per-round gap-attribution.json regeneration is written/read; only "
             "consulted in --squad-mode.",
    )
    args = parser.parse_args(argv)

    config_path = Path(args.config)
    if not config_path.is_file():
        print(f"{config_path} not found -- see config.example.toml", file=sys.stderr)
        return 1

    # Dispatcher singleton: fail fast and loudly if another instance
    # already holds the flock. dispatcher_lock is deliberately kept in a
    # local for the rest of main() -- the flock lives exactly as long as
    # this open file object / this process does.
    lock_path = Path(lock_path) if lock_path else DISPATCHER_LOCK_PATH
    pgids_path = Path(pgids_path) if pgids_path else DISPATCHER_PGIDS_PATH
    dispatcher_lock = acquire_dispatcher_lock(lock_path)
    if dispatcher_lock is None:
        print(
            f"another dispatcher already holds {lock_path} -- refusing to start a second one "
            "(two dispatchers double-spawn workers and race each other's merges; "
            "use scripts/stop_parallel_fix.py to stop the running one first)",
            file=sys.stderr,
        )
        return 1

    # Only the singleton holder gets here, so anything in the pgid file
    # belongs to a dead dispatcher: reap it, then start persisting our
    # own spawns to the same file for whoever has to reap us one day.
    reap_fn(pgids_path)
    _set_pgids_persist_path(pgids_path)

    dispatch_fn = run_squad_round_fn if args.squad_mode else run_round_fn

    # --auto-publish is tri-state on purpose (BooleanOptionalAction with
    # default=None): unset means "on for --infinite, off otherwise", so
    # `uv run scripts/parallel_model_fix_loop.py --infinite` publishes
    # with no extra flag, while a one-shot debugging round still stays
    # local unless an operator asks for a publish explicitly.
    auto_publish = args.infinite if args.auto_publish is None else args.auto_publish
    publish_kwargs = {
        "repo_root": REPO_ROOT,
        "cache_dir": args.cache_dir,
        "home": Path(args.home),
        "squads_toml_path": Path(args.squads_toml),
        "sweep_worktree_dir": Path(args.sweep_worktree_dir),
        "checks_timeout_seconds": args.pr_checks_timeout,
        "checks_interval_seconds": args.pr_checks_interval,
    }

    try:
        round_num = 0
        last_round_ok = True
        while True:
            round_num += 1
            if args.infinite:
                print(f"\n{'=' * 20} round {round_num} {'=' * 20}")
            last_round_ok = dispatch_fn(args, config_path)
            if auto_publish:
                # After dispatch, never before: the sweep only ever sees
                # squad heads the mergers have already stamped green, so
                # running it here gives this round's fixes their first
                # chance to ship while costing a no-op when there are none.
                _run_auto_publish_safely(publish_fn=auto_publish_fn, publish_kwargs=publish_kwargs)
            if not args.infinite:
                return 0 if last_round_ok else 1
            if args.round_delay:
                sleep_fn(args.round_delay)
    finally:
        # Dispatching is over either way (single round done, or the
        # infinite loop is unwinding on an exception/interrupt) --
        # release the singleton so the next dispatcher can start.
        dispatcher_lock.close()


if __name__ == "__main__":
    sys.exit(main())
