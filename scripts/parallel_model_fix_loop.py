#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""Run scripts/model_fix_loop.py in parallel across formats, each in its
own git worktree with its own target/ dir (never shared -- CARGO_TARGET_DIR
is explicitly stripped from each worker's environment), then merge
completed work back sequentially once each worker finishes.

Config: same MODEL_FIX_*/REVIEW_* env vars as model_fix_loop.py, loaded
from .env automatically (same loader) and passed through to every worker
unchanged.

Usage:
    uv run scripts/parallel_model_fix_loop.py
    uv run scripts/parallel_model_fix_loop.py --max-parallel 8
    uv run scripts/parallel_model_fix_loop.py --formats JPEG,NEF,DNG
"""
import argparse
import concurrent.futures
import os
import signal
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
import threading
import time
from pathlib import Path

from find_tag_gaps import REPO_ROOT, group_gaps_by_format, load_comparison_report, run_full_comparison
from model_fix_loop import _load_dotenv

# Every in-flight worker's process group, so an interrupted wrapper
# (Ctrl-C, SIGTERM) can force-terminate all of them rather than leaving
# cargo/rustc grandchildren running unsupervised.
_active_pgids = set()
_active_pgids_lock = threading.Lock()


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


def create_worktree(repo_root, path, branch, base_ref):
    subprocess.run(  # nosec B603
        ["git", "worktree", "add", "-b", branch, str(path), base_ref],
        cwd=repo_root, check=True, capture_output=True, text=True,
    )


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


def merge_branch(repo_root, branch, cargo_test_fn=None):
    """Merge branch into repo_root's current branch. On merge success, run
    the full test suite; if it regresses, roll back just this merge (never
    the commits before it). Returns (merged: bool, message: str).

    cargo_test_fn, if provided, overrides the real `cargo test --workspace`
    call for testing -- must return True/False like the real check would.
    """
    merge = subprocess.run(  # nosec B603
        ["git", "merge", "--no-ff", branch, "-m", f"merge: {branch}"],
        cwd=repo_root, capture_output=True, text=True,
    )
    if merge.returncode != 0:
        subprocess.run(["git", "merge", "--abort"], cwd=repo_root, capture_output=True, text=True)  # nosec B603
        return False, f"merge conflict: {merge.stderr.strip()}"

    tests_pass = cargo_test_fn() if cargo_test_fn else _real_cargo_test(repo_root)
    if not tests_pass:
        subprocess.run(["git", "reset", "--hard", "HEAD~1"], cwd=repo_root, check=True)  # nosec B603
        return False, "cargo test --workspace regressed after merge, rolled back"

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


def _unregister_pgid(pgid):
    with _active_pgids_lock:
        _active_pgids.discard(pgid)


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
    with open(log_path, "w") as log_file:
        proc = subprocess.Popen(  # nosec B603
            ["uv", "run", "scripts/model_fix_loop.py", "--only-format", fmt],
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


def process_format(fmt, repo_root, base_ref, worktree_base, log_base, cache_dir, timeout):
    """Create fmt's worktree, run its worker, report what happened. Never
    raises -- failures are reported in the returned dict's status."""
    path = worktree_path(worktree_base, fmt)
    branch = branch_name(fmt)
    log_path = log_base / f"{fmt}.log"

    try:
        create_worktree(repo_root, path, branch, base_ref)
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


def main(argv=None):
    # An interrupted wrapper (Ctrl-C, SIGTERM) must not leave worker
    # process trees (cargo build/test, rustc) running unsupervised.
    signal.signal(signal.SIGINT, _handle_shutdown_signal)
    signal.signal(signal.SIGTERM, _handle_shutdown_signal)
    _load_dotenv(REPO_ROOT / ".env")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--max-parallel", type=int,
        default=int(os.environ.get("MODEL_FIX_MAX_PARALLEL", "20")),
    )
    parser.add_argument(
        "--formats",
        help="Comma-separated format list; default: auto-discover every format with gaps",
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
        default=os.environ.get("MODEL_FIX_WORKTREE_DIR", "/tmp/oxidex-parallel-fix"),  # nosec B108
    )
    parser.add_argument(
        "--log-dir",
        default=os.environ.get("MODEL_FIX_LOG_DIR", "/tmp/oxidex-parallel-fix-logs"),  # nosec B108
    )
    args = parser.parse_args(argv)

    if args.formats:
        formats = [f.strip() for f in args.formats.split(",") if f.strip()]
    else:
        print("Discovering formats with gaps (full comparison run)...")
        formats = discover_formats(args.cache_dir)

    if not formats:
        print("No formats with gaps found.")
        return 0

    base_ref = subprocess.run(  # nosec B603
        ["git", "rev-parse", "--abbrev-ref", "HEAD"],
        cwd=REPO_ROOT, capture_output=True, text=True, check=True,
    ).stdout.strip()

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
            ): fmt
            for fmt in formats
        }
        for future in concurrent.futures.as_completed(futures):
            fmt, result = future.result()
            results[fmt] = result
            extra = f" (exit {result['returncode']})" if "returncode" in result else ""
            print(f"[{fmt}] {result['status']}{extra}")

    print("\nMerging completed worker branches...")
    merged, failed, empty = [], [], []
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
    print(f"failed:  {len(failed)} formats" + (" (worktree left for inspection)" if failed else ""))
    for fmt, reason in failed:
        print(f"  {fmt}: {reason}")

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
