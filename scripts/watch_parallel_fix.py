#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Live colored dashboard for scripts/parallel_model_fix_loop.py's workers.

Tails every <log-dir>/<FORMAT>.log file the parallel wrapper's workers write
to (model_fix_loop.py's own stdout, redirected there by run_worker), and
redraws a one-line-per-worker status view every --interval seconds: build
result, gap-count delta (green "+N" when gaps close, red "-N" when they
regress), review verdict, and the final done/fixed/failed summary.

This only reads log files -- it never touches worktrees, git, or the model
API, so it's safe to run in a separate terminal alongside an in-flight
`uv run scripts/parallel_model_fix_loop.py` and does nothing if none is
running (it just waits for logs to appear).

Usage:
    uv run scripts/watch_parallel_fix.py
    uv run scripts/watch_parallel_fix.py --log-dir /tmp/oxidex-parallel-fix-logs --interval 0.5
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


def discover_formats(log_dir):
    return sorted(p.stem for p in log_dir.glob("*.log"))


def render(log_dir, formats):
    lines = [f"{BOLD}parallel_model_fix_loop.py -- watching {log_dir}{RESET}", ""]
    for fmt in formats:
        label, color, detail = parse_status(log_dir / f"{fmt}.log")
        lines.append(f"  {fmt:<10} {color}{label:<10}{RESET} {detail}")
    return "\n".join(lines)


def main(argv=None, sleep_fn=time.sleep, stdout=sys.stdout):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--log-dir",
        default="/tmp/oxidex-parallel-fix-logs",  # nosec B108
        help="Directory of per-format .log files (parallel_model_fix_loop.py's own --log-dir)",
    )
    parser.add_argument("--interval", type=float, default=0.5, help="Redraw interval in seconds")
    args = parser.parse_args(argv)

    log_dir = Path(args.log_dir)
    stdout.write(f"Waiting for logs to appear in {log_dir}...\n")
    stdout.flush()
    while not log_dir.is_dir() or not any(log_dir.glob("*.log")):
        sleep_fn(args.interval)

    try:
        while True:
            formats = discover_formats(log_dir)
            stdout.write("\x1b[2J\x1b[H")  # clear screen, cursor home
            stdout.write(render(log_dir, formats) + "\n")
            stdout.flush()
            sleep_fn(args.interval)
    except KeyboardInterrupt:
        return 0


if __name__ == "__main__":
    sys.exit(main())
