#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Run scripts/model_fix_loop.py's per-tag fixer (run_tag_loop) across N
concurrent workers, each in its own persistent git worktree with its own
target/ dir, sharing one tag-state file so they coordinate which tag each
is working on (see model_fix_loop.py's --worker-id claim mechanism) rather
than duplicating effort. Unlike scripts/parallel_model_fix_loop.py (which
splits work by FORMAT and merges once each worker's subprocess exits),
these workers run --blacklist-full and can stay busy for a long time
working through many tags one at a time, so this periodically checks each
worker's branch for new commits and merges them into the base branch while
the workers keep running -- real progress lands well before a worker
finally exhausts its share of the tag pool and exits.

Config: config.toml (see config.example.toml), same file model_fix_loop.py
reads directly. Since config.toml is gitignored, each worker's worktree
gets its own copy at creation time.

Usage:
    uv run scripts/parallel_tag_fix_loop.py --workers 4
    uv run scripts/parallel_tag_fix_loop.py --workers 4 --only-format JPEG
"""
import argparse
import os
import re
import signal
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
import threading
import time
import tomllib
from pathlib import Path

from find_tag_gaps import OXIDEX_HOME, REPO_ROOT
from model_fix_loop import DEFAULT_CONFIG_PATH, DEFAULT_TAG_STATE_PATH

from parallel_model_fix_loop import (
    commits_on_branch,
    create_worktree,
    delete_branch,
    merge_branch,
    remove_worktree,
)

DEFAULT_LOG_DIR = OXIDEX_HOME / "logs" / "parallel-tag-fix"
DEFAULT_PROMPT_LOG_DIR = OXIDEX_HOME / "logs" / "tag-fix-prompts"
DEFAULT_TAGS_FOUND_LOG = OXIDEX_HOME / "logs" / "tags-found.log"

# Each worker should only ever hold one tag at a time -- respawning
# frequently is what makes the merge-then-respawn design (see the two-pass
# loop below) actually exercise itself often, instead of one worker sitting
# on a long private branch for hours before its work ever reaches base_ref.
DEFAULT_MAX_TAGS_PER_PROCESS = 1

# A worker that crashes (uncaught exception, e.g. a transient network
# failure building cargo) before ever printing its "stopped after N
# rounds" summary must never be mistaken for "the shared tag pool is
# empty" -- see classify_worker_exit. It's still worth retrying (the
# failure may be transient), but a slot that crashes over and over with no
# real work happening between crashes is a broken environment, not
# progress; cap consecutive crashes before giving up on that slot for good.
DEFAULT_MAX_CONSECUTIVE_CRASHES = 5

# Every in-flight worker's process group, so an interrupted wrapper
# (Ctrl-C, SIGTERM) can force-terminate all of them rather than leaving
# cargo/rustc grandchildren running unsupervised.
_active_pgids = set()
_active_pgids_lock = threading.Lock()


def worktree_path(base_dir, worker_id):
    return base_dir / f"model-fix-tag-worker-{worker_id}"


def branch_name(worker_id):
    return f"model-fix-tag-parallel-worker-{worker_id}"


def _process_group_alive(pgid):
    try:
        os.killpg(pgid, 0)
        return True
    except ProcessLookupError:
        return False


def _kill_process_group(pgid, sig=signal.SIGKILL):
    try:
        os.killpg(pgid, sig)
    except ProcessLookupError:
        pass  # already exited -- nothing to kill


def _register_pgid(pgid):
    with _active_pgids_lock:
        _active_pgids.add(pgid)


def _unregister_pgid(pgid):
    with _active_pgids_lock:
        _active_pgids.discard(pgid)


def _kill_all_active_workers():
    with _active_pgids_lock:
        pgids = list(_active_pgids)
    for pgid in pgids:
        _kill_process_group(pgid)


def _handle_shutdown_signal(signum, frame):
    _kill_all_active_workers()
    sys.exit(1)


def start_worker(worker_id, worktree, cache_dir, log_path, tag_state_path, prompt_log_dir,
                  max_tag_fails, only_format=None, max_tags_per_process=None,
                  tags_found_log=DEFAULT_TAGS_FOUND_LOG, base_ref=None):
    """Launch model_fix_loop.py --blacklist-full in worktree as a
    background process (own process group, POSIX), logging combined
    stdout/stderr to log_path. Returns the Popen handle -- callers poll it
    rather than blocking, since these workers can run for a long time.

    base_ref, if given, is passed through as --base-ref so the worker
    fast-forwards this worktree onto the shared branch's latest commits
    at the start of every round -- see model_fix_loop.py's
    refresh_worktree/run_tag_loop for why this matters: a tag retried
    across many rounds would otherwise keep comparing against an
    increasingly stale snapshot while other workers merge in fixes
    elsewhere, which is exactly what's produced every duplicate-fix
    merge conflict seen in this pipeline so far.
    """
    env = dict(os.environ)
    env.pop("CARGO_TARGET_DIR", None)  # each worktree gets its own default target/, never shared
    env["EXIFTOOL_CACHE_DIR"] = str(cache_dir)
    # stdout redirected to a regular file (not a TTY) makes Python default
    # to full block buffering instead of line buffering -- print() output
    # (including the "round N: attempting TAG" line watch_parallel_fix.py
    # tails) can sit unflushed for many lines/rounds behind the worker's
    # true progress. Force unbuffered so the log file -- and the live
    # dashboard reading it -- actually reflect real-time state.
    env["PYTHONUNBUFFERED"] = "1"
    argv = [
        "uv", "run", "scripts/model_fix_loop.py",
        "--blacklist-full",
        "--worker-id", str(worker_id),
        "--tag-state-path", str(tag_state_path),
        "--prompt-log-dir", str(prompt_log_dir),
        "--max-tag-fails", str(max_tag_fails),
        "--cache-dir", str(cache_dir),
        "--tags-found-log", str(tags_found_log),
    ]
    if only_format:
        argv += ["--only-format", only_format]
    if max_tags_per_process is not None:
        argv += ["--max-tags-per-process", str(max_tags_per_process)]
    if base_ref:
        argv += ["--base-ref", base_ref]
    log_file = open(log_path, "w")  # noqa: SIM115 -- kept open for the worker's lifetime, closed by caller
    proc = subprocess.Popen(  # nosec B603 # nosemgrep: python.lang.security.audit.dangerous-subprocess-use-audit.dangerous-subprocess-use-audit -- list-argv, no shell=True, argv built entirely from literals/internal paths above
        argv, cwd=worktree, env=env, stdout=log_file, stderr=subprocess.STDOUT,  # nosemgrep: python.lang.security.audit.dangerous-subprocess-use-tainted-env-args.dangerous-subprocess-use-tainted-env-args -- env is os.environ plus two internally-set keys, not external input
        start_new_session=True,
    )
    pgid = os.getpgid(proc.pid)
    _register_pgid(pgid)
    return proc, log_file, pgid


def wait_for_process_group_exit(pgid, poll_interval=0.5, force_after=30, sleep_fn=time.sleep):
    waited = 0.0
    while _process_group_alive(pgid):
        sleep_fn(poll_interval)
        waited += poll_interval
        if waited >= force_after:
            _kill_process_group(pgid)
            break


def merge_worker_progress(repo_root, base_ref, branch, merged_up_to):
    """Merge any commits on branch beyond merged_up_to into repo_root's
    current branch. Returns (new_commit_count, ok, message) -- new_commit_count
    is how many commits existed on branch at all (used to detect "nothing
    new since last check" without re-merging), ok/message describe the
    merge attempt only when there was something new to merge.
    """
    commits = commits_on_branch(repo_root, base_ref, branch)
    if len(commits) <= merged_up_to:
        return len(commits), True, "nothing new"
    ok, message = merge_branch(repo_root, branch)
    return len(commits), ok, message


FIXED_COUNT_RE = re.compile(r"^\s*fixed:\s+(\d+) tags", re.MULTILINE)
FAILED_COUNT_RE = re.compile(r"^\s*failed:\s+(\d+) attempts", re.MULTILINE)
SKIPPED_COUNT_RE = re.compile(r"^\s*skipped:\s+(\d+) tags", re.MULTILINE)


def parse_worker_summary(log_path):
    """(fixed_count, failed_count, skipped_count, has_summary) from a
    worker's final "  fixed:   N tags" / "  failed:  N attempts" /
    "  skipped: N tags" summary lines (see model_fix_loop.py main()'s
    prints). has_summary is False if the log doesn't have them at all
    (e.g. it crashed before printing a summary), in which case
    fixed/failed/skipped are just 0 placeholders, not a real "did
    nothing" report -- see classify_worker_exit, which is what actually
    tells those two situations apart.
    """
    try:
        text = log_path.read_text(errors="replace")
    except OSError:
        return 0, 0, 0, False
    fixed_match = FIXED_COUNT_RE.search(text)
    failed_match = FAILED_COUNT_RE.search(text)
    skipped_match = SKIPPED_COUNT_RE.search(text)
    has_summary = fixed_match is not None or failed_match is not None
    fixed = int(fixed_match.group(1)) if fixed_match else 0
    failed = int(failed_match.group(1)) if failed_match else 0
    skipped = int(skipped_match.group(1)) if skipped_match else 0
    return fixed, failed, skipped, has_summary


def classify_worker_exit(returncode, has_summary, fixed, failed, skipped=0):
    """What a just-exited worker's outcome actually means, given its
    process return code and parse_worker_summary's result -- the three
    possibilities the caller must tell apart:

      "crashed"  -- returncode != 0, or no summary was ever printed (an
                    uncaught exception, e.g. a transient network failure
                    mid-build). This must NEVER be treated as "the shared
                    tag pool is empty": a real crash produces exactly the
                    same (fixed=0, failed=0) as a clean no-work exit once
                    parse_worker_summary's fallback kicks in, and
                    conflating the two silently retires a slot that could
                    have kept making progress -- confirmed live: a worker
                    hit a crates.io DNS timeout building cargo, the
                    wrapper logged "found no work available", and retired
                    the slot while thousands of tags were still unfixed.
      "no_work"  -- exited cleanly (returncode 0, real summary present)
                    having fixed, failed, AND skipped nothing at all: the
                    shared pool was already exhausted, or every remaining
                    tag was claimed by other workers, the moment this one
                    looked. Respawning would just repeat that immediately,
                    forever.
      "respawn"  -- exited cleanly having done real work -- fixed and/or
                    failed attempts > 0, OR skipped a tag it found
                    already fixed elsewhere (see fix_gap's
                    detect_duplicate_fn) -- and hit
                    --max-tags-per-process, not "nothing left". A
                    duplicate-skip specifically must respawn, not retire:
                    the shared pool very much isn't empty, this worker's
                    own worktree was just stale about one specific tag.
    """
    if returncode != 0 or not has_summary:
        return "crashed"
    if fixed == 0 and failed == 0 and skipped == 0:
        return "no_work"
    return "respawn"


def main(argv=None):
    # Same buffering issue fixed for workers (PYTHONUNBUFFERED in
    # start_worker's env) also applies to this wrapper process itself when
    # its own stdout is redirected to a file (e.g. `nohup ... > out.log &`)
    # rather than a TTY -- confirmed live: its print() status lines sat
    # completely unflushed, making it impossible to tell what it was doing
    # (mid-merge? stuck? just sleeping?) without attaching a debugger.
    sys.stdout.reconfigure(line_buffering=True)
    signal.signal(signal.SIGINT, _handle_shutdown_signal)
    signal.signal(signal.SIGTERM, _handle_shutdown_signal)

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--workers", type=int, default=None,
        help="Number of concurrent workers. Default: [parallel].workers in config.toml, or 4 "
             "if that table/key is absent.",
    )
    parser.add_argument(
        "--config", default=str(DEFAULT_CONFIG_PATH),
        help="Path to config.toml, copied into every worker's worktree (see config.example.toml)",
    )
    parser.add_argument(
        "--cache-dir",
        default=os.environ.get("EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache"),  # nosec B108
    )
    parser.add_argument("--only-format", help="Scope every worker to a single format (e.g. JPEG)")
    parser.add_argument("--max-tag-fails", type=int, default=10)
    parser.add_argument(
        "--max-tags-per-process", type=int, default=None,
        help="Cap how many distinct tags each worker will start work on. Default: "
             f"[parallel].max_tags_per_process in config.toml, or {DEFAULT_MAX_TAGS_PER_PROCESS} if absent.",
    )
    parser.add_argument(
        "--tag-state-path", default=str(DEFAULT_TAG_STATE_PATH),
        help="Shared state file every worker claims tags in -- must be outside any worker's own "
             "worktree (which gets reset between rounds) so it actually persists and coordinates "
             f"across workers. Default: {DEFAULT_TAG_STATE_PATH}",
    )
    parser.add_argument("--worktree-dir", default=os.environ.get("MODEL_FIX_WORKTREE_DIR", str(OXIDEX_HOME / "worktrees" / "parallel-tag-fix")))
    parser.add_argument("--log-dir", default=os.environ.get("MODEL_FIX_LOG_DIR", str(DEFAULT_LOG_DIR)))
    parser.add_argument("--prompt-log-dir", default=str(DEFAULT_PROMPT_LOG_DIR))
    parser.add_argument(
        "--tags-found-log", default=str(DEFAULT_TAGS_FOUND_LOG),
        help="Shared log every worker appends to when it actually fixes a tag -- a single "
             f"running record across the whole parallel run. Default: {DEFAULT_TAGS_FOUND_LOG}",
    )
    parser.add_argument(
        "--merge-interval", type=float, default=30,
        help="Seconds between checks for new commits to merge from each worker's branch "
             "while they're all still running (default: 30)",
    )
    args = parser.parse_args(argv)

    config_path = Path(args.config)
    if not config_path.is_file():
        print(f"{config_path} not found -- see config.example.toml", file=sys.stderr)
        return 1

    with open(config_path, "rb") as f:
        parallel_table = tomllib.load(f).get("parallel") or {}
    num_workers = args.workers if args.workers is not None else parallel_table.get("workers", 4)
    max_tags_per_process = (
        args.max_tags_per_process if args.max_tags_per_process is not None
        else parallel_table.get("max_tags_per_process", DEFAULT_MAX_TAGS_PER_PROCESS)
    )

    tag_state_path = Path(args.tag_state_path)
    tag_state_path.parent.mkdir(parents=True, exist_ok=True)

    base_ref = subprocess.run(  # nosec B603
        ["git", "rev-parse", "--abbrev-ref", "HEAD"],
        cwd=REPO_ROOT, capture_output=True, text=True, check=True,
    ).stdout.strip()

    worktree_base = Path(args.worktree_dir)
    worktree_base.mkdir(parents=True, exist_ok=True)
    log_base = Path(args.log_dir)
    log_base.mkdir(parents=True, exist_ok=True)
    prompt_log_dir = Path(args.prompt_log_dir)
    prompt_log_dir.mkdir(parents=True, exist_ok=True)
    Path(args.tags_found_log).parent.mkdir(parents=True, exist_ok=True)

    print(
        f"{num_workers} workers, shared tag-state {tag_state_path}, "
        f"max_tags_per_process={max_tags_per_process or 'unbounded'}, "
        f"merging into {base_ref!r} every {args.merge_interval}s"
    )

    def spawn_worker(worker_id):
        """(Re)create worker_id's worktree/branch and start a fresh
        model_fix_loop.py in it. Returns the workers[worker_id] dict entry,
        or None if worktree setup failed. Called both for the initial
        batch and to refill a slot once its previous occupant exits --
        with --max-tags-per-process bounding each individual worker's
        lifetime, this is what keeps num_workers actually busy for the
        life of the whole run instead of each one doing exactly one tag
        and then sitting idle forever.
        """
        path = worktree_path(worktree_base, worker_id)
        branch = branch_name(worker_id)
        log_path = log_base / f"worker-{worker_id}.log"
        try:
            create_worktree(REPO_ROOT, path, branch, base_ref, config_path=config_path)
        except subprocess.CalledProcessError as e:
            print(f"[worker {worker_id}] worktree setup failed: {e.stderr}", file=sys.stderr)
            return None
        proc, log_file, pgid = start_worker(
            worker_id, path, args.cache_dir, log_path, tag_state_path, prompt_log_dir,
            args.max_tag_fails, only_format=args.only_format, max_tags_per_process=max_tags_per_process,
            tags_found_log=Path(args.tags_found_log), base_ref=base_ref,
        )
        print(f"[worker {worker_id}] started (pid {proc.pid}), worktree {path}")
        return {
            "path": path, "branch": branch, "log_path": log_path,
            "proc": proc, "log_file": log_file, "pgid": pgid, "merged_up_to": 0,
            "merge_broken": False,
        }

    # worker_id -> {"path", "branch", "log_path", "proc", "log_file", "pgid",
    # "merged_up_to", "merge_broken"}
    workers = {}
    # worker_id -> count of consecutive crashes on this slot (reset to 0 by
    # any exit that isn't itself a crash) -- see classify_worker_exit and
    # DEFAULT_MAX_CONSECUTIVE_CRASHES.
    crash_counts = {}
    # Human-readable lines describing any slot that ended for a reason
    # other than "genuinely found no work" -- drives the final summary
    # message so it never claims completeness it can't back up.
    retired_for_review = []
    for worker_id in range(1, num_workers + 1):
        entry = spawn_worker(worker_id)
        if entry:
            workers[worker_id] = entry

    if not workers:
        print("No workers started.", file=sys.stderr)
        return 1

    try:
        while workers:
            time.sleep(args.merge_interval)

            # Pass 1: merge every worker's new commits into base_ref, and
            # collect (don't yet act on) which workers have exited. This
            # must fully finish -- every pending merge applied -- before
            # any exited worker gets cleaned up/respawned in pass 2 below.
            # Doing merge-and-respawn together per worker in one pass (the
            # original design) let an earlier worker in iteration order
            # respawn against a base_ref that hadn't yet picked up a later
            # worker's merge from the very same tick -- confirmed live: two
            # respawned workers both re-picked tags (CAM3, CAM5) that two
            # other workers had just fixed, because their fresh worktrees
            # were checked out before those fixes were merged in.
            just_exited = []
            for worker_id in list(workers):
                w = workers[worker_id]
                if not w["merge_broken"]:
                    count, ok, message = merge_worker_progress(REPO_ROOT, base_ref, w["branch"], w["merged_up_to"])
                    if count > w["merged_up_to"]:
                        if ok:
                            print(f"[worker {worker_id}] {count - w['merged_up_to']} new commit(s) -> merged")
                            w["merged_up_to"] = count
                        else:
                            print(f"[worker {worker_id}] {count - w['merged_up_to']} new commit(s) -> MERGE FAILED: {message}")
                            # A conflict here won't resolve itself on a later
                            # tick -- nothing about this worker's commits or
                            # the target branch changes between retries, so
                            # retrying just re-runs `git merge`/`--abort`
                            # forever. Confirmed live: one conflicted worker
                            # got retried 150+ times over more than an hour,
                            # burying every other worker's real status in
                            # the log. One failure is enough to know this
                            # needs a human, not another attempt.
                            w["merge_broken"] = True
                            print(f"[worker {worker_id}] will not retry this merge automatically -- "
                                  "needs manual resolution once the worker itself exits")

                if w["proc"].poll() is not None:
                    just_exited.append(worker_id)

            # Pass 2: now that base_ref reflects every merge from this
            # tick, clean up and (maybe) respawn each worker that exited.
            for worker_id in just_exited:
                w = workers[worker_id]
                wait_for_process_group_exit(w["pgid"])
                _unregister_pgid(w["pgid"])
                w["log_file"].close()
                # Final sweep in case commits landed between pass 1's merge
                # check and the process actually exiting (skipped once a
                # merge has already failed once -- see pass 1 above).
                if not w["merge_broken"]:
                    count, ok, message = merge_worker_progress(REPO_ROOT, base_ref, w["branch"], w["merged_up_to"])
                    if count > w["merged_up_to"]:
                        if ok:
                            new_commits = count - w["merged_up_to"]
                            w["merged_up_to"] = count
                            print(f"[worker {worker_id}] final merge: {new_commits} commit(s)")
                        else:
                            w["merge_broken"] = True
                print(f"[worker {worker_id}] exited (code {w['proc'].returncode}) -- {w['log_path']}")

                if w["merge_broken"]:
                    print(f"[worker {worker_id}] worktree/branch left in place (merge issue): {w['path']}")
                    retired_for_review.append(f"worker {worker_id}: merge conflict at {w['path']}")
                    del workers[worker_id]
                    crash_counts.pop(worker_id, None)
                    continue

                fixed, failed, skipped, has_summary = parse_worker_summary(w["log_path"])
                outcome = classify_worker_exit(w["proc"].returncode, has_summary, fixed, failed, skipped)

                if outcome == "crashed":
                    # Never treat this as "the shared tag pool is empty" --
                    # see classify_worker_exit's docstring for why the two
                    # look identical from parse_worker_summary alone. Retry
                    # a bounded number of times (a transient failure, e.g. a
                    # DNS blip building cargo, deserves another chance) but
                    # don't let a genuinely broken environment masquerade as
                    # slow progress forever.
                    crash_counts[worker_id] = crash_counts.get(worker_id, 0) + 1
                    remove_worktree(REPO_ROOT, w["path"])
                    delete_branch(REPO_ROOT, w["branch"])
                    if crash_counts[worker_id] >= DEFAULT_MAX_CONSECUTIVE_CRASHES:
                        print(
                            f"[worker {worker_id}] CRASHED {crash_counts[worker_id]} times in a row "
                            f"(exit code {w['proc'].returncode}) -- giving up on this slot, see {w['log_path']}"
                        )
                        retired_for_review.append(f"worker {worker_id}: repeated crashes, see {w['log_path']}")
                        del workers[worker_id]
                        crash_counts.pop(worker_id, None)
                    else:
                        print(
                            f"[worker {worker_id}] CRASHED (exit code {w['proc'].returncode}), attempt "
                            f"{crash_counts[worker_id]}/{DEFAULT_MAX_CONSECUTIVE_CRASHES} -- see "
                            f"{w['log_path']} -- respawning"
                        )
                        entry = spawn_worker(worker_id)
                        if entry:
                            workers[worker_id] = entry
                        else:
                            del workers[worker_id]
                            crash_counts.pop(worker_id, None)
                elif outcome == "no_work":
                    # Exited having attempted nothing at all -- the shared
                    # tag pool was already exhausted (or every remaining
                    # tag already claimed) the moment this worker looked.
                    # Respawning would just repeat that immediately,
                    # forever; let this slot end.
                    crash_counts.pop(worker_id, None)
                    print(f"[worker {worker_id}] found no work available -- not respawning this slot")
                    remove_worktree(REPO_ROOT, w["path"])
                    delete_branch(REPO_ROOT, w["branch"])
                    del workers[worker_id]
                else:  # "respawn"
                    # Did real work and hit --max-tags-per-process, not
                    # "nothing left" -- refill the slot now that base_ref
                    # reflects every merge from this tick (including any
                    # other worker that also just exited), so this fresh
                    # worktree can't redo work another slot just landed.
                    crash_counts.pop(worker_id, None)
                    remove_worktree(REPO_ROOT, w["path"])
                    delete_branch(REPO_ROOT, w["branch"])
                    entry = spawn_worker(worker_id)
                    if entry:
                        workers[worker_id] = entry
                    else:
                        del workers[worker_id]
    except BaseException:
        for w in workers.values():
            _kill_process_group(w["pgid"])
        raise

    if retired_for_review:
        print(
            f"\nAll worker slots ended, but {len(retired_for_review)} need manual attention "
            "(merge conflicts or repeated crashes) -- NOT every tag is necessarily fixed or "
            "blacklisted:"
        )
        for line in retired_for_review:
            print(f"  - {line}")
    else:
        print("\nAll workers exited -- every tag is now either fixed or blacklisted.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
