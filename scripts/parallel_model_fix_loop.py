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
import subprocess
import sys
from pathlib import Path

from find_tag_gaps import REPO_ROOT, group_gaps_by_format, load_comparison_report, run_full_comparison
from model_fix_loop import _load_dotenv


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


def create_worktree(repo_root, path, branch, base_ref):
    subprocess.run(
        ["git", "worktree", "add", "-b", branch, str(path), base_ref],
        cwd=repo_root, check=True, capture_output=True, text=True,
    )


def remove_worktree(repo_root, path):
    subprocess.run(["git", "worktree", "remove", "--force", str(path)], cwd=repo_root, check=True)


def delete_branch(repo_root, branch):
    subprocess.run(["git", "branch", "-D", branch], cwd=repo_root, check=True)


def commits_on_branch(repo_root, base_ref, branch):
    """Commit subjects unique to branch vs base_ref, oldest first (empty
    if the worker made no commits)."""
    result = subprocess.run(
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
    merge = subprocess.run(
        ["git", "merge", "--no-ff", branch, "-m", f"merge: {branch}"],
        cwd=repo_root, capture_output=True, text=True,
    )
    if merge.returncode != 0:
        subprocess.run(["git", "merge", "--abort"], cwd=repo_root, capture_output=True, text=True)
        return False, f"merge conflict: {merge.stderr.strip()}"

    tests_pass = cargo_test_fn() if cargo_test_fn else _real_cargo_test(repo_root)
    if not tests_pass:
        subprocess.run(["git", "reset", "--hard", "HEAD~1"], cwd=repo_root, check=True)
        return False, "cargo test --workspace regressed after merge, rolled back"

    return True, "merged"


def _real_cargo_test(repo_root):
    result = subprocess.run(["cargo", "test", "--workspace"], cwd=repo_root, capture_output=True, text=True)
    return result.returncode == 0


def run_worker(fmt, worktree, cache_dir, log_path, timeout=None):
    """Run model_fix_loop.py --only-format <fmt> inside worktree, logging
    combined stdout/stderr to log_path. Returns the process's exit code."""
    env = dict(os.environ)
    env.pop("CARGO_TARGET_DIR", None)  # each worktree gets its own default target/, never shared
    env["EXIFTOOL_CACHE_DIR"] = str(cache_dir)
    with open(log_path, "w") as log_file:
        result = subprocess.run(
            ["uv", "run", "scripts/model_fix_loop.py", "--only-format", fmt],
            cwd=worktree, env=env, stdout=log_file, stderr=subprocess.STDOUT,
            timeout=timeout,
        )
    return result.returncode


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
    parser.add_argument("--cache-dir", default=os.environ.get("EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache"))
    parser.add_argument("--timeout", type=int, default=None, help="Per-worker timeout in seconds (default: none)")
    parser.add_argument("--worktree-dir", default=os.environ.get("MODEL_FIX_WORKTREE_DIR", "/tmp/oxidex-parallel-fix"))
    parser.add_argument("--log-dir", default=os.environ.get("MODEL_FIX_LOG_DIR", "/tmp/oxidex-parallel-fix-logs"))
    args = parser.parse_args(argv)

    if args.formats:
        formats = [f.strip() for f in args.formats.split(",") if f.strip()]
    else:
        print("Discovering formats with gaps (full comparison run)...")
        formats = discover_formats(args.cache_dir)

    if not formats:
        print("No formats with gaps found.")
        return 0

    base_ref = subprocess.run(
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
