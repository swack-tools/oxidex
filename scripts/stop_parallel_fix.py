#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Stop every running parallel fix-loop wrapper (both
parallel_tag_fix_loop.py and the older parallel_model_fix_loop.py) and
any worker process it started, cleanly and in one command.

Sends SIGTERM to each wrapper process first -- its own shutdown handler
(installed for both SIGINT and SIGTERM) already force-kills every worker
process group it's tracking before exiting, so this alone handles the
common case. As a follow-up safety pass (a wrapper that already died on
its own -- e.g. the shell it was launched from was closed -- leaves its
workers orphaned with nothing left to signal them), this also directly
finds and signals any still-running model_fix_loop.py worker process by
pattern. Workers are killed via their whole process group (they're each
spawned with start_new_session=True specifically so this is possible),
which also takes down any cargo/rustc build still running underneath
one at the moment it's killed -- a plain single-PID kill would leave
that as an orphan. Anything still alive after --grace-period seconds
gets escalated from SIGTERM to SIGKILL.

Worktrees and branches under /tmp/oxidex-parallel-tag-fix (or wherever
--worktree-dir pointed) are left untouched -- this only stops processes,
so a killed run's in-progress work stays inspectable, or the wrapper can
just be relaunched to pick its worktrees back up.

Usage:
    uv run scripts/stop_parallel_fix.py
    uv run scripts/stop_parallel_fix.py --dry-run
    uv run scripts/stop_parallel_fix.py --grace-period 10
"""
import argparse
import os
import signal
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
import time

# Matched against each process's full command line (`ps -o command=`) --
# specific enough to this project's own scripts that this can't collide
# with an unrelated process, even a generic bare "python3" one.
WRAPPER_PATTERNS = ["parallel_tag_fix_loop.py", "parallel_model_fix_loop.py"]
WORKER_PATTERN = "model_fix_loop.py"


def find_pids(pattern):
    """PIDs of running processes whose command line contains pattern, via
    `pgrep -f`. Empty list if none match (pgrep's own "no matches" exit
    code is 1, not an error) or pgrep itself isn't installed."""
    try:
        result = subprocess.run(  # nosec B603 B607
            ["pgrep", "-f", pattern], capture_output=True, text=True,
        )
    except FileNotFoundError:
        print("pgrep not found -- can't discover running processes on this system", file=sys.stderr)
        return []
    if result.returncode not in (0, 1):
        print(f"pgrep -f {pattern!r} failed: {result.stderr.strip()}", file=sys.stderr)
        return []
    return [int(pid) for pid in result.stdout.split()]


def command_line(pid):
    result = subprocess.run(  # nosec B603 B607
        ["ps", "-o", "command=", "-p", str(pid)], capture_output=True, text=True,
    )
    return result.stdout.strip()


def is_alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except (OSError, ProcessLookupError):
        return False


def signal_pid(pid, sig):
    """Signal just this one PID -- used for wrapper processes, which
    (unlike workers) aren't necessarily their own process group leader:
    a wrapper is typically launched with a plain `nohup ... &` from an
    interactive shell, not start_new_session=True, so killpg-ing it
    risks reaching an unrelated sibling job backgrounded in the same
    shell. Its own signal handler already scopes worker cleanup
    correctly via their individual process groups -- see signal_group.
    """
    try:
        os.kill(pid, sig)
    except ProcessLookupError:
        pass  # already exited -- nothing to signal


def signal_group(pid, sig):
    """Signal pid's whole process group -- used for worker processes,
    each deliberately spawned with start_new_session=True (making the
    worker its own group leader), so this also reaches any cargo/rustc
    build still running underneath it rather than orphaning it. Falls
    back to doing nothing if the group lookup itself fails (process
    already gone by the time we get here)."""
    try:
        pgid = os.getpgid(pid)
    except ProcessLookupError:
        return
    try:
        os.killpg(pgid, sig)
    except ProcessLookupError:
        pass  # already exited -- nothing to signal


def wait_until_dead(pids, timeout, is_alive_fn=is_alive, sleep_fn=time.sleep, now_fn=time.time):
    """pids still alive after up to timeout seconds of polling."""
    deadline = now_fn() + timeout
    while now_fn() < deadline and any(is_alive_fn(pid) for pid in pids):
        sleep_fn(0.2)
    return [pid for pid in pids if is_alive_fn(pid)]


def main(argv=None, find_pids_fn=find_pids, command_line_fn=command_line, is_alive_fn=is_alive,
         signal_pid_fn=signal_pid, signal_group_fn=signal_group, sleep_fn=time.sleep,
         now_fn=time.time, stdout=sys.stdout):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--dry-run", action="store_true", help="Show what would be signaled without doing it")
    parser.add_argument(
        "--grace-period", type=float, default=5,
        help="Seconds to wait after SIGTERM before escalating to SIGKILL for anything still alive (default: 5)",
    )
    args = parser.parse_args(argv)

    wrapper_pids = sorted(set(pid for pattern in WRAPPER_PATTERNS for pid in find_pids_fn(pattern)))
    worker_pids = [pid for pid in find_pids_fn(WORKER_PATTERN) if pid not in wrapper_pids]

    if not wrapper_pids and not worker_pids:
        stdout.write("Nothing running -- no wrapper or worker process found.\n")
        return 0

    verb = "Would signal" if args.dry_run else "Signaling"
    if wrapper_pids:
        stdout.write(
            f"{verb} {len(wrapper_pids)} wrapper process(es) with SIGTERM (each cleans up its own workers):\n"
        )
        for pid in wrapper_pids:
            stdout.write(f"  pid {pid}: {command_line_fn(pid)[:100]}\n")
            if not args.dry_run:
                signal_pid_fn(pid, signal.SIGTERM)
    else:
        stdout.write("No wrapper process found (parallel_tag_fix_loop.py / parallel_model_fix_loop.py).\n")

    if worker_pids:
        stdout.write(
            f"{verb} {len(worker_pids)} worker process(es) directly (in case they're orphaned) with SIGTERM:\n"
        )
        for pid in worker_pids:
            stdout.write(f"  pid {pid}: {command_line_fn(pid)[:100]}\n")
            if not args.dry_run:
                signal_group_fn(pid, signal.SIGTERM)
    else:
        stdout.write("No standalone worker process found (model_fix_loop.py).\n")

    if args.dry_run:
        stdout.write("\nDry run -- nothing was actually signaled.\n")
        return 0

    stdout.write(f"Waiting up to {args.grace_period:g}s for a clean exit...\n")
    still_alive = wait_until_dead(wrapper_pids + worker_pids, args.grace_period, is_alive_fn, sleep_fn, now_fn)

    if still_alive:
        stdout.write(f"{len(still_alive)} process(es) ignored SIGTERM -- escalating to SIGKILL:\n")
        for pid in still_alive:
            stdout.write(f"  pid {pid}\n")
            if pid in wrapper_pids:
                signal_pid_fn(pid, signal.SIGKILL)
            else:
                signal_group_fn(pid, signal.SIGKILL)
        sleep_fn(0.5)

    stdout.write(
        "\nDone. Worktrees/branches under the workers' --worktree-dir were left in place -- "
        "clean those up separately, or just relaunch the wrapper, which reuses them.\n"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
