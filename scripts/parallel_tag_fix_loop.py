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
import signal
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
import threading
import time
import tomllib
from pathlib import Path

from find_tag_gaps import REPO_ROOT
from model_fix_loop import DEFAULT_CONFIG_PATH, DEFAULT_TAG_STATE_PATH

from parallel_model_fix_loop import (
    commits_on_branch,
    create_worktree,
    delete_branch,
    merge_branch,
    remove_worktree,
)

DEFAULT_LOG_DIR = REPO_ROOT / "logs" / "parallel-tag-fix"
DEFAULT_PROMPT_LOG_DIR = REPO_ROOT / "logs" / "tag-fix-prompts"
DEFAULT_TAGS_FOUND_LOG = REPO_ROOT / "logs" / "tags-found.log"

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
        pass


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
                  tags_found_log=DEFAULT_TAGS_FOUND_LOG):
    """Launch model_fix_loop.py --blacklist-full in worktree as a
    background process (own process group, POSIX), logging combined
    stdout/stderr to log_path. Returns the Popen handle -- callers poll it
    rather than blocking, since these workers can run for a long time.
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
    log_file = open(log_path, "w")  # noqa: SIM115 -- kept open for the worker's lifetime, closed by caller
    proc = subprocess.Popen(  # nosec B603
        argv, cwd=worktree, env=env, stdout=log_file, stderr=subprocess.STDOUT,
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


def main(argv=None):
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
             "[parallel].max_tags_per_process in config.toml, or unbounded if absent.",
    )
    parser.add_argument(
        "--tag-state-path", default=str(DEFAULT_TAG_STATE_PATH),
        help="Shared state file every worker claims tags in -- must be outside any worker's own "
             "worktree (which gets reset between rounds) so it actually persists and coordinates "
             f"across workers. Default: {DEFAULT_TAG_STATE_PATH}",
    )
    parser.add_argument("--worktree-dir", default=os.environ.get("MODEL_FIX_WORKTREE_DIR", "/tmp/oxidex-parallel-tag-fix"))  # nosec B108
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
        else parallel_table.get("max_tags_per_process")
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

    workers = {}  # worker_id -> {"path", "branch", "log_path", "proc", "log_file", "pgid", "merged_up_to"}
    for worker_id in range(1, num_workers + 1):
        path = worktree_path(worktree_base, worker_id)
        branch = branch_name(worker_id)
        log_path = log_base / f"worker-{worker_id}.log"
        try:
            create_worktree(REPO_ROOT, path, branch, base_ref, config_path=config_path)
        except subprocess.CalledProcessError as e:
            print(f"[worker {worker_id}] worktree setup failed: {e.stderr}", file=sys.stderr)
            continue
        proc, log_file, pgid = start_worker(
            worker_id, path, args.cache_dir, log_path, tag_state_path, prompt_log_dir,
            args.max_tag_fails, only_format=args.only_format, max_tags_per_process=max_tags_per_process,
            tags_found_log=Path(args.tags_found_log),
        )
        workers[worker_id] = {
            "path": path, "branch": branch, "log_path": log_path,
            "proc": proc, "log_file": log_file, "pgid": pgid, "merged_up_to": 0,
        }
        print(f"[worker {worker_id}] started (pid {proc.pid}), worktree {path}")

    if not workers:
        print("No workers started.", file=sys.stderr)
        return 1

    try:
        while workers:
            time.sleep(args.merge_interval)
            for worker_id in list(workers):
                w = workers[worker_id]
                count, ok, message = merge_worker_progress(REPO_ROOT, base_ref, w["branch"], w["merged_up_to"])
                if count > w["merged_up_to"]:
                    status = "merged" if ok else f"MERGE FAILED: {message}"
                    print(f"[worker {worker_id}] {count - w['merged_up_to']} new commit(s) -> {status}")
                    if ok:
                        w["merged_up_to"] = count

                exited = w["proc"].poll() is not None
                if exited:
                    wait_for_process_group_exit(w["pgid"])
                    _unregister_pgid(w["pgid"])
                    w["log_file"].close()
                    # Final sweep in case commits landed between the last
                    # merge check and process exit.
                    count, ok, message = merge_worker_progress(REPO_ROOT, base_ref, w["branch"], w["merged_up_to"])
                    if count > w["merged_up_to"] and ok:
                        w["merged_up_to"] = count
                        print(f"[worker {worker_id}] final merge: {count - w['merged_up_to']} commit(s)")
                    print(f"[worker {worker_id}] exited (code {w['proc'].returncode}) -- {w['log_path']}")
                    if ok:
                        remove_worktree(REPO_ROOT, w["path"])
                        delete_branch(REPO_ROOT, w["branch"])
                    else:
                        print(f"[worker {worker_id}] worktree/branch left in place (merge issue): {w['path']}")
                    del workers[worker_id]
    except BaseException:
        for w in workers.values():
            _kill_process_group(w["pgid"])
        raise

    print("\nAll workers exited -- every tag is now either fixed or blacklisted.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
