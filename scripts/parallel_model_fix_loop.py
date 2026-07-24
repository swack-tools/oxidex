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

Usage:
    uv run scripts/parallel_model_fix_loop.py
    uv run scripts/parallel_model_fix_loop.py --max-parallel 8
    uv run scripts/parallel_model_fix_loop.py --formats JPEG,NEF,DNG
"""
import argparse
import concurrent.futures
import fcntl
import json
import os
import shutil
import signal
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
import tempfile
import threading
import time
from pathlib import Path

from find_tag_gaps import OXIDEX_HOME, REPO_ROOT, group_gaps_by_format, load_comparison_report, run_full_comparison
from model_fix_loop import DEFAULT_CONFIG_PATH

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
    # Advisory only (the flock is the actual mutex): record who holds it
    # so a human staring at the file can find the process.
    lock_f.seek(0)
    lock_f.truncate()
    lock_f.write(f"{os.getpid()}\n")
    lock_f.flush()
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

    Errs on the safe side: if git can't answer (bad ref, unreadable
    output), the commits are treated as present and the reset refused.
    """
    exclude = [f"^{base_ref}"]
    origin_main = _git(["rev-parse", "--verify", "--quiet", "refs/remotes/origin/main"], repo_root)
    if origin_main.returncode == 0:
        exclude.append("^refs/remotes/origin/main")
    result = _git(["rev-list", "--count", branch, *exclude], repo_root)
    if result.returncode != 0:
        return True
    try:
        return int(result.stdout.strip()) > 0
    except (TypeError, ValueError):
        return True


def create_worktree(repo_root, path, branch, base_ref, config_path=DEFAULT_CONFIG_PATH):
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


def run_worker(fmt, worktree, cache_dir, log_path, timeout=None):
    """Run model_fix_loop.py --only-format <fmt> inside worktree, logging
    combined stdout/stderr to log_path. Returns the process's exit code.

    Launched in its own process group (POSIX) so this function can
    positively confirm -- and if needed, force-terminate -- the worker's
    entire process tree before returning, not just the immediate `uv run`
    child. See _wait_for_process_group_exit for why that distinction
    matters.
    """
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
            ["uv", "run", "scripts/model_fix_loop.py", "--only-format", fmt, "--worker-id", fmt],
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

    return not failed


def main(argv=None, run_round_fn=run_round, sleep_fn=time.sleep,
         lock_path=None, pgids_path=None, reap_fn=reap_orphan_worker_pgids):
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

    try:
        round_num = 0
        last_round_ok = True
        while True:
            round_num += 1
            if args.infinite:
                print(f"\n{'=' * 20} round {round_num} {'=' * 20}")
            last_round_ok = run_round_fn(args, config_path)
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
