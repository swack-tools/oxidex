#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Live colored dashboard for scripts/parallel_model_fix_loop.py's (per-format)
or scripts/parallel_tag_fix_loop.py's (per-tag) workers.

Tails every worker's log file (model_fix_loop.py's own stdout, redirected
there by the parallel wrapper), and redraws a one-line-per-worker status
view every --interval seconds. Auto-detects which parallel wrapper is
running by log filename shape:

  - worker-<N>.log (parallel_tag_fix_loop.py): shows each worker's current
    round number and the tag it's on (from model_fix_loop.py's "round N:
    attempting TAG" line), its most recent build/gap/review status, and
    (in the header) the aggregate count of tags found across every
    worker so far -- read from the shared --tags-found-log.
  - <FORMAT>.log (parallel_model_fix_loop.py): the original per-format
    view -- build result, gap-count delta, review verdict, done/failed
    summary. No round/tag/aggregate-count columns, since that wrapper
    doesn't track any of those per-worker.

This only reads log files -- it never touches worktrees, git, or the model
API, so it's safe to run in a separate terminal alongside an in-flight
parallel run, and does nothing but wait if neither is running yet.

Usage:
    uv run scripts/watch_parallel_fix.py
    uv run scripts/watch_parallel_fix.py --log-dir logs/parallel-tag-fix --interval 0.5
    uv run scripts/watch_parallel_fix.py --log-dir /tmp/oxidex-parallel-fix-logs  # old per-format mode
"""
import argparse
import re
import sys
import time
from pathlib import Path

RESET = "\x1b[0m"
BOLD = "\x1b[1m"
GREEN = "\x1b[32m"
RED = "\x1b[31m"
YELLOW = "\x1b[33m"
CYAN = "\x1b[36m"
DIM = "\x1b[2m"

# Matched against a log file's lines, most recent first -- the first
# pattern to hit wins, so more specific/terminal states (STOPPED, FIXED)
# must be listed ahead of the general per-attempt GAP_DELTA line they'd
# otherwise also match.
STOPPED_RE = re.compile(r"^stopped after (\d+) rounds")
FIXED_RE = re.compile(r"FIXED: closed (\d+) gaps")
REJECT_RE = re.compile(r"review REJECTED")
REGRESSED_RE = re.compile(r"(gap count did not decrease|cargo test --workspace regressed)")
BUILD_FAILED_RE = re.compile(r"build failed")
GAP_DELTA_RE = re.compile(r"gaps (\d+) -> (\d+)")

# scripts/model_fix_loop.py's run_tag_loop logs exactly one of these per
# round, right when it picks a tag to work on this round.
ROUND_TAG_RE = re.compile(r"round (\d+): attempting (\S+)")

WORKER_LOG_RE = re.compile(r"^worker-(\d+)\.log$")


def parse_status(log_path):
    """Return (label, color, detail) describing a worker's most recent
    understood state, scanning its log file from the end. A missing or
    empty file just means the worker hasn't started writing yet -- not an
    error -- so it's reported as "waiting", not a failure.
    """
    try:
        lines = log_path.read_text(errors="replace").splitlines()
    except OSError:
        return "waiting", DIM, ""
    if not lines:
        return "waiting", DIM, ""

    for line in reversed(lines):
        if STOPPED_RE.search(line):
            return "done", CYAN, line.strip()
        fixed_match = FIXED_RE.search(line)
        if fixed_match:
            return "fixed", GREEN, f"+{fixed_match.group(1)} gaps closed"
        if REJECT_RE.search(line):
            return "rejected", YELLOW, line.strip()
        if REGRESSED_RE.search(line):
            return "reverted", RED, line.strip()
        if BUILD_FAILED_RE.search(line):
            return "build-fail", RED, line.strip()
        m = GAP_DELTA_RE.search(line)
        if m:
            before, after = int(m.group(1)), int(m.group(2))
            delta = before - after
            sign = f"+{delta}" if delta > 0 else str(delta)
            color = GREEN if delta > 0 else (RED if delta < 0 else YELLOW)
            return "attempt", color, f"gaps {before}->{after} ({sign})"

    return "busy", DIM, lines[-1].strip()[:60]


def parse_round_and_tag(log_path):
    """Return (round_num, tag_key) from the most recent "round N:
    attempting TAG" line in a worker's log, or (None, None) if it hasn't
    logged one yet (e.g. still building/comparing before its first pick).
    """
    try:
        lines = log_path.read_text(errors="replace").splitlines()
    except OSError:
        return None, None
    for line in reversed(lines):
        m = ROUND_TAG_RE.search(line)
        if m:
            return int(m.group(1)), m.group(2)
    return None, None


def discover_formats(log_dir):
    return sorted(p.stem for p in log_dir.glob("*.log") if not WORKER_LOG_RE.match(p.name))


def discover_workers(log_dir):
    """Worker ids (ints) with a worker-<N>.log present, sorted numerically."""
    ids = []
    for p in log_dir.glob("worker-*.log"):
        m = WORKER_LOG_RE.match(p.name)
        if m:
            ids.append(int(m.group(1)))
    return sorted(ids)


def count_tags_found(tags_found_log):
    """Number of tags fixed so far across every worker -- one line per fix
    in the shared log every worker appends to (see model_fix_loop.py's
    --tags-found-log). 0 if the log doesn't exist yet."""
    try:
        return sum(1 for line in tags_found_log.read_text(errors="replace").splitlines() if line.strip())
    except OSError:
        return 0


def render(log_dir, formats):
    lines = [f"{BOLD}parallel_model_fix_loop.py -- watching {log_dir}{RESET}", ""]
    for fmt in formats:
        label, color, detail = parse_status(log_dir / f"{fmt}.log")
        lines.append(f"  {fmt:<10} {color}{label:<10}{RESET} {detail}")
    return "\n".join(lines)


def render_workers(log_dir, worker_ids, tags_found_log):
    total_found = count_tags_found(tags_found_log)
    lines = [
        f"{BOLD}parallel_tag_fix_loop.py -- watching {log_dir}{RESET}",
        f"{BOLD}tags found so far (all workers): {GREEN}{total_found}{RESET}{BOLD} "
        f"(see {tags_found_log}){RESET}",
        "",
    ]
    for worker_id in worker_ids:
        log_path = log_dir / f"worker-{worker_id}.log"
        round_num, tag = parse_round_and_tag(log_path)
        label, color, detail = parse_status(log_path)
        round_str = f"round {round_num}" if round_num is not None else "round -"
        tag_str = tag or "(none yet)"
        lines.append(
            f"  worker-{worker_id:<3} {round_str:<10} {tag_str:<28} "
            f"{color}{label:<10}{RESET} {detail}"
        )
    return "\n".join(lines)


def main(argv=None, sleep_fn=time.sleep, stdout=sys.stdout):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--log-dir",
        default="/tmp/oxidex-parallel-fix-logs",  # nosec B108
        help="Directory of per-format .log files (parallel_model_fix_loop.py's --log-dir) or "
             "per-worker worker-<N>.log files (parallel_tag_fix_loop.py's --log-dir) -- "
             "auto-detected by filename shape.",
    )
    parser.add_argument(
        "--tags-found-log",
        default=None,
        help="Shared tags-found log (parallel_tag_fix_loop.py's --tags-found-log). Default: "
             "<log-dir's parent>/tags-found.log, matching that wrapper's own default layout.",
    )
    parser.add_argument("--interval", type=float, default=0.5, help="Redraw interval in seconds")
    args = parser.parse_args(argv)

    log_dir = Path(args.log_dir)
    tags_found_log = (
        Path(args.tags_found_log) if args.tags_found_log else log_dir.parent / "tags-found.log"
    )
    stdout.write(f"Waiting for logs to appear in {log_dir}...\n")
    stdout.flush()
    while not log_dir.is_dir() or not any(log_dir.glob("*.log")):
        sleep_fn(args.interval)

    try:
        while True:
            worker_ids = discover_workers(log_dir)
            stdout.write("\x1b[2J\x1b[H")  # clear screen, cursor home
            if worker_ids:
                stdout.write(render_workers(log_dir, worker_ids, tags_found_log) + "\n")
            else:
                formats = discover_formats(log_dir)
                stdout.write(render(log_dir, formats) + "\n")
            stdout.flush()
            sleep_fn(args.interval)
    except KeyboardInterrupt:
        return 0


if __name__ == "__main__":
    sys.exit(main())
