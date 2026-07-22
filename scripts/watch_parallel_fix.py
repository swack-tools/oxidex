#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Live colored dashboard for scripts/parallel_model_fix_loop.py's (per-format)
or scripts/parallel_tag_fix_loop.py's (per-tag) workers.

Tails every worker's log file (model_fix_loop.py's own stdout, redirected
there by the parallel wrapper), and redraws every --interval seconds.
Auto-detects which parallel wrapper is running by log filename shape:

  - worker-<N>.log (parallel_tag_fix_loop.py): a full dashboard --
    aggregate tags-found/blacklist stats (last found, blacklisted in the
    last hour/24h/total), a colored per-format progress bar for every
    format with cached tag-comparison data (not just whichever one is
    currently --only-format-scoped), and a per-worker table (current
    round/tag, live status, when this tag was launched, and -- read from
    the wrapper's own never-truncated log plus the shared tag-state file,
    since a worker's own log is overwritten on every respawn and can't
    answer lifetime questions -- how many times it's restarted and how
    many tags it has personally blacklisted).
  - <FORMAT>.log (parallel_model_fix_loop.py): the original per-format
    view -- build result, gap-count delta, review verdict, done/failed
    summary. No round/tag/aggregate-count columns, since that wrapper
    doesn't track any of those per-worker.

This only reads log/state files -- it never touches worktrees, git, or
the model API, so it's safe to run in a separate terminal alongside an
in-flight parallel run, and does nothing but wait if neither is running.

Usage:
    uv run scripts/watch_parallel_fix.py
    uv run scripts/watch_parallel_fix.py --log-dir ~/.oxidex/logs/parallel-tag-fix --interval 2
    uv run scripts/watch_parallel_fix.py --log-dir ~/.oxidex/logs/parallel-model-fix  # old per-format mode
"""
import argparse
import datetime
import json
import os
import re
import shutil
import sys
import time
import tomllib
from pathlib import Path

RESET = "\x1b[0m"
BOLD = "\x1b[1m"
DIM = "\x1b[2m"
GREEN = "\x1b[32m"
RED = "\x1b[31m"
YELLOW = "\x1b[33m"
CYAN = "\x1b[36m"
BLUE = "\x1b[34m"
MAGENTA = "\x1b[35m"
BRIGHT_GREEN = "\x1b[92m"
BRIGHT_RED = "\x1b[91m"
BRIGHT_YELLOW = "\x1b[93m"
BRIGHT_CYAN = "\x1b[96m"
BRIGHT_WHITE = "\x1b[97m"

BULLET = "●"  # ●

DEFAULT_REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_TAGCMP_DIR = "/tmp"  # nosec B108 -- find_tag_gaps.run_format_comparison's own hardcoded output location

# Kept in sync with find_tag_gaps.py's own OXIDEX_HOME -- not imported directly
# since this script is meant to run standalone against any worktree's logs.
OXIDEX_HOME = Path(os.environ.get("OXIDEX_HOME", str(Path.home() / ".oxidex")))
# One worktree base per wrapper -- see their own --worktree-dir defaults.
# main() picks between these by detected mode when --worktree-dir isn't
# passed explicitly, the same way --log-dir auto-detects (see find_active_log_dir).
DEFAULT_WORKTREE_DIR_BY_MODE = {
    "tag": str(OXIDEX_HOME / "worktrees" / "parallel-tag-fix"),
    "format": str(OXIDEX_HOME / "worktrees" / "parallel-fix"),
}

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

# --- Per-tag-mode-specific log vocabulary (run_tag_loop's own lines,
# distinct from fix_gap's shared per-format lines already matched above).
TRACEBACK_MARKER = "Traceback (most recent call last):"
BLACKLISTED_RE = re.compile(r"^\[(\S+)\] blacklisted after (\d+) failed attempts")
TAG_FIXED_RE = re.compile(r"^\[(\S+)\] FIXED\s*$")
FAILED_ATTEMPT_RE = re.compile(r"^\[(\S+)\] failed attempt (\d+)/(\d+)")
MODEL_RETRY_RE = re.compile(r"model call retry (\d+)/(\d+)")
NO_WORK_RE = re.compile(r"(All tags found|claimed by other workers|max_distinct_tags=\d+ for this process)")
LOG_TIMESTAMP_RE = re.compile(r"^\[(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})\]")

# logs/tags-found.log's one line per fix (see model_fix_loop.py's
# log_tag_found): "<iso-ts> worker=<id> tag=<key> gaps_closed=<n>".
TAGS_FOUND_LINE_RE = re.compile(r"^(\S+) worker=(\S+) tag=(\S+) gaps_closed=(\d+)")

# The wrapper's own (never-truncated, append-only) stdout -- see
# parallel_tag_fix_loop.py's spawn_worker/pass-2 cleanup prints. Unlike a
# worker's own worker-<N>.log (overwritten on every respawn), this is the
# only place a lifetime restart/crash count can be reconstructed from.
WORKER_STARTED_RE = re.compile(r"^\[worker (\d+)\] started \(pid")
WORKER_CRASHED_RE = re.compile(r"^\[worker (\d+)\] CRASHED")

# tag-comparison JSON reports (both the single-format ones
# find_tag_gaps.run_format_comparison writes to /tmp/tagcmp-<FMT>.json,
# and the full-corpus comparison.json from `just compare-exiftool-full`)
# share this ComparisonReport shape.
TAGCMP_FILENAME_RE = re.compile(r"^tagcmp-.+\.json$")

# The shared logs/model-fix-requests/manifest.log every worker/format
# process appends to -- one completed (OK or ERROR) API call per line,
# phase- and worker-tagged (see model_fix_loop.py's make_logging_call_model).
# RETRY lines use a different shape entirely and are intentionally not
# matched by this -- see parse_manifest_log.
MANIFEST_ENTRY_RE = re.compile(
    r"^(?P<ts>\S+) phase=(?P<phase>fixer|reviewer) worker=(?P<worker>\S+) model=(?P<model>\S+) "
    r"prompt_chars=(?P<prompt_chars>\d+) elapsed=(?P<elapsed>[\d.]+)s "
    r"(?:reply_chars=\d+ )?(?P<rest>OK|ERROR=.*)$"
)


def parse_worker_log_status(log_path):
    """(label, color, detail) describing a per-tag worker's most recent
    understood state -- like parse_status, but tailored to
    model_fix_loop.py's per-tag-mode vocabulary: run_tag_loop's own
    round/blacklist/fixed/failed-attempt lines, model call retry
    messages, and a crashed process (an uncaught exception ended it
    before it ever got to its own error handling -- previously
    indistinguishable from ordinary "busy" output, which is exactly what
    reads as an unexplained error on a dashboard: the raw exception text
    shown under a "busy" label rather than a clear "CRASHED" state).
    fix_gap's shared per-format lines (build failed, gap delta, review
    verdict) still apply unchanged in single-tag mode -- gap["format"] is
    still a plain format name there, not the tag key.
    """
    try:
        text = log_path.read_text(errors="replace")
    except OSError:
        return "waiting", DIM, ""
    if not text.strip():
        return "waiting", DIM, ""
    lines = text.splitlines()

    if TRACEBACK_MARKER in text:
        return "crashed", BRIGHT_RED, lines[-1].strip()[:100]

    for line in reversed(lines):
        if STOPPED_RE.search(line):
            return "done", CYAN, line.strip()
        if BLACKLISTED_RE.search(line):
            return "blacklisted", RED, line.strip()
        if TAG_FIXED_RE.search(line):
            return "fixed", BRIGHT_GREEN, line.strip()
        fixed_match = FIXED_RE.search(line)
        if fixed_match:
            return "fixed", BRIGHT_GREEN, f"+{fixed_match.group(1)} gaps closed"
        if FAILED_ATTEMPT_RE.search(line):
            return "retrying", YELLOW, line.strip()
        if REJECT_RE.search(line):
            return "rejected", YELLOW, line.strip()
        if REGRESSED_RE.search(line):
            return "reverted", RED, line.strip()
        if BUILD_FAILED_RE.search(line):
            return "build-fail", RED, line.strip()
        if MODEL_RETRY_RE.search(line):
            return "retrying", YELLOW, line.strip()[:100]
        m = GAP_DELTA_RE.search(line)
        if m:
            before, after = int(m.group(1)), int(m.group(2))
            delta = before - after
            sign = f"+{delta}" if delta > 0 else str(delta)
            color = GREEN if delta > 0 else (RED if delta < 0 else YELLOW)
            return "attempt", color, f"gaps {before}->{after} ({sign})"
        if NO_WORK_RE.search(line):
            return "waiting", DIM, line.strip()

    return "busy", DIM, lines[-1].strip()[:60]


def parse_current_tag_progress(log_path):
    """(round_num, tag_key, launched_at_epoch) for whatever tag this
    worker incarnation has most recently logged a "round N: attempting
    TAG" line for. launched_at_epoch is when it FIRST started attempting
    this exact tag -- the earliest contiguous "attempting TAG" line for
    the same key, scanning back from the most recent one, stopping the
    instant a different tag key is found -- so round_num (which, bounded
    by --max-tags-per-process, already equals "which attempt is this on
    this tag") gets a concrete wall-clock anchor alongside it. All three
    are None if no such line has been logged yet.
    """
    try:
        lines = log_path.read_text(errors="replace").splitlines()
    except OSError:
        return None, None, None
    last_round = None
    last_tag = None
    launched_at = None
    for line in reversed(lines):
        m = ROUND_TAG_RE.search(line)
        if not m:
            continue
        round_num, tag_key = int(m.group(1)), m.group(2)
        if last_tag is None:
            last_round, last_tag = round_num, tag_key
        elif tag_key != last_tag:
            break
        ts_match = LOG_TIMESTAMP_RE.match(line)
        if ts_match:
            launched_at = parse_timestamp(ts_match.group(1))
    return last_round, last_tag, launched_at


def find_active_log_dir(candidates):
    """Of the given candidate directories, return whichever has *.log files,
    preferring the one with the most recently modified file if more than one
    does (the other is presumably stale output left over from an earlier,
    now-finished run). None if none of them have any log files yet."""
    best, best_mtime = None, None
    for candidate in candidates:
        if not candidate.is_dir():
            continue
        mtimes = [p.stat().st_mtime for p in candidate.glob("*.log")]
        if not mtimes:
            continue
        mtime = max(mtimes)
        if best_mtime is None or mtime > best_mtime:
            best, best_mtime = candidate, mtime
    return best


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


def parse_timestamp(ts_str):
    """Epoch seconds for an ISO-ish timestamp string. Both naive local
    (time.strftime's own "%Y-%m-%dT%H:%M:%S", used throughout this
    pipeline's own logs) and timezone-aware (the tag-comparison Rust
    binary's own RFC3339-with-offset timestamps) forms appear across this
    pipeline's files -- fromisoformat/.timestamp() handle both correctly.
    None if unparseable (never crash a dashboard over one bad line)."""
    try:
        return datetime.datetime.fromisoformat(ts_str).timestamp()
    except ValueError:
        return None


def format_relative(seconds_ago):
    """"3m ago" / "2.4h ago" style rendering. "never" for None (no event
    seen at all yet) -- distinct from "0s ago" (an event just happened)."""
    if seconds_ago is None:
        return "never"
    if seconds_ago < 0:
        seconds_ago = 0
    if seconds_ago < 5:
        return "just now"
    if seconds_ago < 60:
        return f"{int(seconds_ago)}s ago"
    if seconds_ago < 3600:
        return f"{int(seconds_ago // 60)}m ago"
    if seconds_ago < 86400:
        return f"{seconds_ago / 3600:.1f}h ago"
    return f"{seconds_ago / 86400:.1f}d ago"


def load_tag_state(path):
    try:
        return json.loads(Path(path).read_text())
    except (OSError, json.JSONDecodeError):
        return {}


def blacklist_stats(state, now):
    """{"total", "last_hour", "last_24h", "per_worker": {worker_id_str: n}}
    from tag-state.json's blacklisted/blacklisted_at/blacklisted_by
    fields (see run_tag_loop) -- the persistent, never-truncated source
    for "when" and "by whom", since a worker's own log is overwritten on
    every respawn and can't be trusted to still hold this history.
    Entries blacklisted before those two fields existed (no
    blacklisted_at) still count toward "total", just not toward the
    time-windowed or per-worker breakdowns.
    """
    total = last_hour = last_24h = 0
    per_worker = {}
    for entry in state.values():
        if not entry.get("blacklisted"):
            continue
        total += 1
        ts = entry.get("blacklisted_at")
        if isinstance(ts, (int, float)):
            age = now - ts
            if age <= 3600:
                last_hour += 1
            if age <= 86400:
                last_24h += 1
        worker = entry.get("blacklisted_by")
        if worker is not None:
            key = str(worker)
            per_worker[key] = per_worker.get(key, 0) + 1
    return {"total": total, "last_hour": last_hour, "last_24h": last_24h, "per_worker": per_worker}


def tag_iteration(state, tag_key):
    """Cumulative attempt number for tag_key -- state[tag_key]["fails"]
    (completed failed rounds, persisted in tag-state.json and so visible
    across worker respawns) plus 1 for whichever round is in progress
    right now. None if tag_key is falsy (no tag picked yet).

    Deliberately NOT the current worker process's own round_num counter
    (see parse_current_tag_progress): that resets to 1 every time a new
    worker process picks up a tag fresh, even when the tag already has a
    real fail history from a previous worker's attempt on it (release-
    then-reclaim between rounds is normal -- a tag's claim is only held
    for one round at a time, not its whole multi-round retry lifetime).
    Showing the process-local counter as "iteration" would understate a
    stubborn tag's true attempt count any time a different worker resumed
    it after an earlier one's attempt failed and released the claim.
    """
    if not tag_key:
        return None
    return state.get(tag_key, {}).get("fails", 0) + 1


def parse_tags_found_log(path):
    """[(timestamp_str, worker_id_str, tag_key, gaps_closed_int), ...] in
    file order (oldest first). Skips any line that doesn't match --
    a truncated final line from a killed process is possible, and
    shouldn't crash the dashboard."""
    try:
        text = Path(path).read_text(errors="replace")
    except OSError:
        return []
    out = []
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        m = TAGS_FOUND_LINE_RE.match(line)
        if m:
            out.append((m.group(1), m.group(2), m.group(3), int(m.group(4))))
    return out


def found_stats(entries, now):
    """{"total", "last_hour", "last_24h", "last_at", "last_tag", "last_worker"}
    from parse_tags_found_log's entries. last_at/last_tag/last_worker are
    None if no entry has a parseable timestamp. Computes the max
    timestamp explicitly rather than trusting the final line's position,
    in case two workers appended within the same instant out of file
    order (cheap safety, not a correctness assumption worth relying on)."""
    total = len(entries)
    last_hour = last_24h = 0
    last_at = None
    last_tag = None
    last_worker = None
    for ts_str, worker, tag, _closed in entries:
        t = parse_timestamp(ts_str)
        if t is None:
            continue
        if last_at is None or t > last_at:
            last_at, last_tag, last_worker = t, tag, worker
        age = now - t
        if age <= 3600:
            last_hour += 1
        if age <= 86400:
            last_24h += 1
    return {
        "total": total, "last_hour": last_hour, "last_24h": last_24h,
        "last_at": last_at, "last_tag": last_tag, "last_worker": last_worker,
    }


def parse_wrapper_log(path):
    """{worker_id_str: {"restarts": n, "crashes": n}} tallied across the
    wrapper's whole append-only log (never truncated, unlike each
    worker's own worker-<N>.log, which is overwritten on every respawn --
    see parallel_tag_fix_loop.py's start_worker). "restarts" is starts
    beyond the first (a worker still on its original spawn shows 0, not
    1); "crashes" is a lifetime total, including ones already recovered
    from by a later successful respawn.
    """
    try:
        text = Path(path).read_text(errors="replace")
    except OSError:
        return {}
    starts = {}
    crashes = {}
    for line in text.splitlines():
        m = WORKER_STARTED_RE.match(line)
        if m:
            wid = m.group(1)
            starts[wid] = starts.get(wid, 0) + 1
            continue
        m = WORKER_CRASHED_RE.match(line)
        if m:
            wid = m.group(1)
            crashes[wid] = crashes.get(wid, 0) + 1
    all_ids = set(starts) | set(crashes)
    return {
        wid: {"restarts": max(0, starts.get(wid, 0) - 1), "crashes": crashes.get(wid, 0)}
        for wid in all_ids
    }


def _model_names(models_table):
    return [m if isinstance(m, str) else m.get("name", "?") for m in (models_table or [])]


def worker_worktree_name(worker_id, mode):
    """The worktree subdirectory name for one worker/format's own
    persistent checkout -- model-fix-tag-worker-<N> for
    parallel_tag_fix_loop.py's numeric worker ids (see its own
    worktree_path), model-fix-<lowercase format> for
    parallel_model_fix_loop.py's format-name ids (see its own, separate
    worktree_path -- same helper name, different naming scheme)."""
    if mode == "format":
        return f"model-fix-{str(worker_id).lower()}"
    return f"model-fix-tag-worker-{worker_id}"


def worker_log_path(log_dir, worker_id, mode):
    """This worker/format's own combined stdout/stderr log -- worker-<N>.log
    for tag mode (parallel_tag_fix_loop.py), <FORMAT>.log for format mode
    (parallel_model_fix_loop.py)."""
    if mode == "format":
        return log_dir / f"{worker_id}.log"
    return log_dir / f"worker-{worker_id}.log"


def discover_worker_ids(log_dir):
    """(worker_ids, mode) for whichever wrapper's logs are present in
    log_dir: numeric ids with mode "tag" for parallel_tag_fix_loop.py's
    worker-<N>.log files, or format-name ids with mode "format" for
    parallel_model_fix_loop.py's <FORMAT>.log files. The two wrappers
    write to different log dirs (see find_active_log_dir), so in practice
    exactly one shape is ever present; tag ids win if somehow both are."""
    tag_ids = discover_workers(log_dir)
    if tag_ids:
        return tag_ids, "tag"
    return discover_formats(log_dir), "format"


def load_worker_model_config(worktree_dir, worker_id, mode="tag"):
    """(fixer_models, fixer_reasoning, reviewer_models, reviewer_reasoning)
    read directly from this worker's own config.toml copy -- each
    worktree gets one at creation time (see
    parallel_tag_fix_loop.py's create_worktree/spawn_worker), since
    config.toml is gitignored and never present in a fresh git checkout
    on its own. This is the one reliable source for "what model pool and
    reasoning level is this worker actually configured to use": both are
    static per [worker]/[reviewer] table (reasoning_effort applies to
    every model in a pool, and pick_model_fn picks randomly among
    models[] on every individual call -- there's no cheap, unambiguous
    way to tell a fixer call from a reviewer call apart after the fact
    in a shared request log when both pools happen to name the same
    model), so reading config.toml directly is both simpler and more
    trustworthy than trying to reconstruct this from logs.

    [reviewer] falls back to [worker] entirely when absent, matching
    model_fix_loop.py's own _normalize_model_config default. All four
    values are None if config.toml can't be read (worktree gone, or this
    worker id never started).
    """
    path = Path(worktree_dir) / worker_worktree_name(worker_id, mode) / "config.toml"
    try:
        with open(path, "rb") as f:
            data = tomllib.load(f)
    except (OSError, tomllib.TOMLDecodeError):
        return None, None, None, None
    worker_table = data.get("worker") or {}
    reviewer_table = data.get("reviewer") or worker_table
    return (
        _model_names(worker_table.get("models")), worker_table.get("reasoning_effort", "max"),
        _model_names(reviewer_table.get("models")), reviewer_table.get("reasoning_effort", "max"),
    )


def parse_manifest_log(path):
    """[(timestamp_str, phase, elapsed_seconds, ok, worker), ...] in file
    order, from the shared manifest.log every worker/format process
    appends to (see model_fix_loop.py's make_logging_call_model) -- one
    entry per COMPLETED API call, fixer or reviewer, success or failure,
    each with its own elapsed time.

    RETRY lines (call_model's own internal retry, logged before the
    retried attempt actually happens) are deliberately excluded: they
    don't represent a finished call and have no elapsed time of their
    own to report a latency for -- the eventual OK/ERROR line that ends
    that whole (possibly-retried) logical call already has the real,
    total elapsed time, including every retry's wait.
    """
    try:
        text = Path(path).read_text(errors="replace")
    except OSError:
        return []
    entries = []
    for line in text.splitlines():
        m = MANIFEST_ENTRY_RE.match(line)
        if m:
            entries.append((
                m.group("ts"), m.group("phase"), float(m.group("elapsed")),
                m.group("rest") == "OK", m.group("worker"),
            ))
    return entries


def entries_for_worker(entries, worker_id):
    """Filter parse_manifest_log's entries down to just one worker/format's
    own calls, dropping the worker tag back off to match request_stats'
    plain (ts, phase, elapsed, ok) shape. Necessary because every worker
    now shares one manifest.log (model_fix_loop.py's req_log_dir is a
    single OXIDEX_HOME-fixed location, not per-worktree) -- parse the
    shared file once with parse_manifest_log and reuse that same list both
    unfiltered (the dashboard-wide aggregate) and filtered through this
    (each worker's own row)."""
    label = str(worker_id)
    return [(ts, phase, elapsed, ok) for ts, phase, elapsed, ok, worker in entries if worker == label]


def _mean(values):
    return sum(values) / len(values) if values else None


def _median(values):
    if not values:
        return None
    ordered = sorted(values)
    mid = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[mid]
    return (ordered[mid - 1] + ordered[mid]) / 2


def request_stats(entries, since=None):
    """{"fixer": {"count", "mean", "median"}, "reviewer": {...}, "last":
    {"phase", "elapsed", "at"} or None} from parse_manifest_log's
    entries. since, if given, restricts to entries at or after that
    epoch timestamp (used for "requests this round" -- see
    parse_current_round_start) -- None (the default) covers every entry
    in the log, i.e. this worker's whole current lifetime/"iteration" on
    its current tag (a fresh manifest.log per worktree means this is
    naturally scoped to one worker incarnation, same caveat as every
    other per-worktree file this dashboard reads).
    """
    by_phase = {"fixer": [], "reviewer": []}
    last = None
    for ts, phase, elapsed, _ok in entries:
        t = parse_timestamp(ts)
        if since is not None and (t is None or t < since):
            continue
        by_phase.setdefault(phase, []).append(elapsed)
        if t is not None and (last is None or t > last["at"]):
            last = {"phase": phase, "elapsed": elapsed, "at": t}
    return {
        phase: {"count": len(latencies), "mean": _mean(latencies), "median": _median(latencies)}
        for phase, latencies in by_phase.items()
    } | {"last": last}


def parse_current_round_start(log_path):
    """Epoch timestamp of the most recent "round N: attempting TAG" line
    -- when the round in progress right now actually started, as opposed
    to parse_current_tag_progress's launched_at (which anchors to the
    EARLIEST same-tag line, i.e. this whole multi-round attempt's start).
    None if no such line has been logged yet.
    """
    try:
        lines = log_path.read_text(errors="replace").splitlines()
    except OSError:
        return None
    for line in reversed(lines):
        if ROUND_TAG_RE.search(line):
            ts_match = LOG_TIMESTAMP_RE.match(line)
            return parse_timestamp(ts_match.group(1)) if ts_match else None
    return None


def _format_latency_stats(stats, color):
    """"12 reqs (mean 34.2s, median 11.0s)" -- or just "0 reqs" when
    count is zero, since mean/median are None with nothing to average."""
    if stats["count"] == 0:
        return f"{DIM}0 reqs{RESET}"
    return (
        f"{color}{stats['count']}{RESET} reqs "
        f"{DIM}(mean{RESET} {stats['mean']:.1f}s {DIM}median{RESET} {stats['median']:.1f}s{DIM}){RESET}"
    )


def discover_format_progress(tagcmp_dir, repo_root=None):
    """{format_name: {"matched", "total", "mtime", "source"}} for every
    format with cached tag-comparison data -- not just whatever the
    current run's --only-format is scoped to, so the dashboard can show
    progress toward "every format done" even for formats nobody's
    actively working on right this second.

    Reads comparison.json (the full-corpus report from `just
    compare-exiftool-full`, if repo_root is given and it exists) and
    every <tagcmp_dir>/tagcmp-*.json (find_tag_gaps.run_format_comparison's
    own per-format output, always written to a fixed /tmp path -- NOT
    find_tag_gaps.py's own --cache-dir, a differently-named, unrelated
    directory holding the ExifTool binary/sample corpus) -- both share
    the same ComparisonReport shape (a "by_format" map). Per format key
    found, whichever source file has the newest mtime wins, so a stale
    leftover (e.g. from an old sample set, or an unrelated ad hoc test
    run that happens to share this directory) naturally loses to a
    fresher real comparison rather than needing an explicit allowlist of
    "real" filenames.
    """
    candidates = []
    if repo_root is not None:
        comparison_path = Path(repo_root) / "comparison.json"
        if comparison_path.is_file():
            candidates.append(comparison_path)
    tagcmp_dir = Path(tagcmp_dir)
    if tagcmp_dir.is_dir():
        candidates.extend(sorted(p for p in tagcmp_dir.glob("tagcmp-*.json") if TAGCMP_FILENAME_RE.match(p.name)))

    progress = {}
    for path in candidates:
        try:
            data = json.loads(path.read_text())
            mtime = path.stat().st_mtime
        except (OSError, json.JSONDecodeError):
            continue
        for fmt, comp in (data.get("by_format") or {}).items():
            existing = progress.get(fmt)
            if existing is not None and existing["mtime"] >= mtime:
                continue
            matched = comp.get("matched_tags")
            matched_count = len(matched) if isinstance(matched, list) else int(matched or 0)
            total = int(comp.get("total_exiftool_tags") or 0)
            if total <= 0:
                continue
            progress[fmt] = {"matched": matched_count, "total": total, "mtime": mtime, "source": path}
    return progress


def bar_color(pct):
    if pct >= 100:
        return BRIGHT_GREEN
    if pct >= 75:
        return GREEN
    if pct >= 40:
        return YELLOW
    return RED


def render_progress_bar(matched, total, width=40):
    pct = (matched / total * 100) if total > 0 else 0.0
    filled = max(0, min(width, round(pct / 100 * width)))
    color = bar_color(pct)
    bar = f"{color}{'█' * filled}{DIM}{'░' * (width - filled)}{RESET}"
    check = f" {BRIGHT_GREEN}✓{RESET}" if pct >= 100 else ""
    return f"[{bar}] {BOLD}{matched}{RESET}{DIM}/{RESET}{BOLD}{total}{RESET} {color}{pct:5.1f}%{RESET}{check}"


def render_format_progress(progress, width=40):
    """One colored progress-bar line per format, least-complete first --
    the formats needing the most attention surface at the top. Empty
    progress produces a single explanatory line rather than a blank
    section (no cached comparison exists yet)."""
    if not progress:
        return [f"  {DIM}no tag-comparison data cached yet -- run a comparison first{RESET}"]
    name_width = max(len(f) for f in progress) + 1
    lines = []
    for fmt in sorted(progress, key=lambda f: (progress[f]["matched"] / progress[f]["total"], f)):
        p = progress[fmt]
        lines.append(f"  {BOLD}{fmt:<{name_width}}{RESET} {render_progress_bar(p['matched'], p['total'], width)}")
    return lines




def _box_line(text, width, color=BRIGHT_WHITE):
    inner = width - 2
    return f"{color}║{RESET}{text.center(inner)}{color}║{RESET}"


def render_dashboard(log_dir, worker_ids, tags_found_log, tag_state_path, wrapper_log_path,
                      format_progress, max_tag_fails, now, term_width=100, worktree_dir=None,
                      manifest_path=None, mode="tag"):
    """The full dashboard: header, aggregate found/blacklist stats, a
    colored progress bar per known format, then one detail row per
    worker/format (status, current round/tag, when it launched onto that
    tag, lifetime restart/crash/personal-blacklist counts, and its
    configured fixer/reviewer model pool + reasoning level, read fresh
    from that worker's own config.toml copy -- see
    load_worker_model_config).

    mode is "tag" for parallel_tag_fix_loop.py's numeric worker ids
    (worker-<N>.log) or "format" for parallel_model_fix_loop.py's
    format-name ids (<FORMAT>.log) -- see discover_worker_ids, which
    picks it automatically from whatever's actually in log_dir. Every
    other per-mode difference (log path, worktree subdirectory name)
    flows from this one flag."""
    width = max(60, term_width)
    state = load_tag_state(tag_state_path)
    bl_stats = blacklist_stats(state, now)
    found_entries = parse_tags_found_log(tags_found_log)
    fnd_stats = found_stats(found_entries, now)
    wrapper_stats = parse_wrapper_log(wrapper_log_path)

    lines = []
    lines.append(f"{BRIGHT_WHITE}╔{'═' * (width - 2)}╗{RESET}")
    header = f"OXIDEX TAG-FIX DASHBOARD -- {time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(now))}"
    lines.append(_box_line(f"{BOLD}{header}{RESET}", width))
    lines.append(f"{BRIGHT_WHITE}╚{'═' * (width - 2)}╝{RESET}")
    lines.append("")

    last_found_str = "never"
    if fnd_stats["last_at"] is not None:
        last_found_str = (
            f"{format_relative(now - fnd_stats['last_at'])} "
            f"({fnd_stats['last_tag']}, worker {fnd_stats['last_worker']})"
        )
    lines.append(
        f"  {BOLD}Tags found:{RESET} {BRIGHT_GREEN}{fnd_stats['total']}{RESET} total  "
        f"{DIM}|{RESET}  {GREEN}{fnd_stats['last_hour']}{RESET} last hour  "
        f"{DIM}|{RESET}  {GREEN}{fnd_stats['last_24h']}{RESET} last 24h  "
        f"{DIM}|{RESET}  last: {CYAN}{last_found_str}{RESET}"
    )
    lines.append(
        f"  {BOLD}Blacklisted:{RESET} {BRIGHT_RED}{bl_stats['total']}{RESET} total  "
        f"{DIM}|{RESET}  {YELLOW}{bl_stats['last_hour']}{RESET} last hour  "
        f"{DIM}|{RESET}  {YELLOW}{bl_stats['last_24h']}{RESET} last 24h"
    )
    all_entries = parse_manifest_log(manifest_path) if manifest_path is not None else []
    if manifest_path is not None:
        # request_stats wants request_stats' plain 4-tuple shape; all_entries
        # (parse_manifest_log's raw output) carries a 5th worker field, kept
        # around so the per-worker loop below can filter it via entries_for_worker.
        agg_stats = request_stats([entry[:4] for entry in all_entries])
        last_str = "never"
        if agg_stats["last"] is not None:
            last_str = (
                f"{format_relative(now - agg_stats['last']['at'])} "
                f"({agg_stats['last']['phase']}, took {agg_stats['last']['elapsed']:.1f}s)"
            )
        lines.append(
            f"  {BOLD}API requests:{RESET} fixer {_format_latency_stats(agg_stats['fixer'], MAGENTA)}  "
            f"{DIM}|{RESET}  reviewer {_format_latency_stats(agg_stats['reviewer'], BLUE)}  "
            f"{DIM}|{RESET}  last: {CYAN}{last_str}{RESET}"
        )
    lines.append("")

    lines.append(f"  {BOLD}{BRIGHT_CYAN}FORMAT PROGRESS{RESET}")
    lines.append(f"  {DIM}{'─' * (width - 4)}{RESET}")
    lines.extend(render_format_progress(format_progress, width=min(80, width - 35)))
    lines.append("")

    lines.append(f"  {BOLD}{BRIGHT_CYAN}WORKERS{RESET}")
    lines.append(f"  {DIM}{'─' * (width - 4)}{RESET}")
    if not worker_ids:
        lines.append(f"  {DIM}no workers found in {log_dir}{RESET}")
    tag_width = 34
    for worker_id in worker_ids:
        log_path = worker_log_path(log_dir, worker_id, mode)
        _round_num, tag_key, launched_at = parse_current_tag_progress(log_path)
        label, color, detail = parse_worker_log_status(log_path)
        wid = str(worker_id)
        w_stats = wrapper_stats.get(wid, {"restarts": 0, "crashes": 0})
        personal_blacklisted = bl_stats["per_worker"].get(wid, 0)

        iteration = tag_iteration(state, tag_key)
        iter_str = f"{iteration}/{max_tag_fails}" if iteration is not None else "-"
        tag_str = (tag_key or "-")[:tag_width]
        launched_str = format_relative(now - launched_at) if launched_at is not None else "-"
        row_label = f"worker-{worker_id:<3}" if mode == "tag" else f"{worker_id:<10}"

        lines.append(
            f"  {BOLD}{row_label}{RESET} "
            f"{color}{BULLET}{RESET} {color}{label:<11}{RESET} "
            f"iter {BOLD}{iter_str:<6}{RESET} "
            f"{CYAN}{tag_str:<{tag_width}}{RESET} "
            f"launched {launched_str:<10} "
            f"{DIM}restarts:{RESET}{w_stats['restarts']:<3} "
            f"{DIM}crashes:{RESET}{w_stats['crashes']:<3} "
            f"{DIM}blacklisted:{RESET}{personal_blacklisted}"
        )
        if worktree_dir is not None:
            fixer_models, fixer_reasoning, reviewer_models, reviewer_reasoning = load_worker_model_config(
                worktree_dir, worker_id, mode
            )
            if fixer_models is not None:
                lines.append(
                    f"      {DIM}Fixer:{RESET} {MAGENTA}{'/'.join(fixer_models)}{RESET} "
                    f"{DIM}@{fixer_reasoning}{RESET}   "
                    f"{DIM}Reviewer:{RESET} {BLUE}{'/'.join(reviewer_models)}{RESET} "
                    f"{DIM}@{reviewer_reasoning}{RESET}"
                )

        if manifest_path is not None:
            manifest_entries = entries_for_worker(all_entries, worker_id)
            if manifest_entries:
                lifetime_stats = request_stats(manifest_entries)
                round_start = parse_current_round_start(log_path)
                round_stats = request_stats(manifest_entries, since=round_start)
                round_count = round_stats["fixer"]["count"] + round_stats["reviewer"]["count"]
                last_str = "never"
                if lifetime_stats["last"] is not None:
                    last_str = (
                        f"{format_relative(now - lifetime_stats['last']['at'])} "
                        f"({lifetime_stats['last']['phase']}, took {lifetime_stats['last']['elapsed']:.1f}s)"
                    )
                lines.append(
                    f"      {DIM}Requests:{RESET} fixer {_format_latency_stats(lifetime_stats['fixer'], MAGENTA)}  "
                    f"{DIM}|{RESET} reviewer {_format_latency_stats(lifetime_stats['reviewer'], BLUE)}  "
                    f"{DIM}|{RESET} this round: {BOLD}{round_count}{RESET}  "
                    f"{DIM}|{RESET} last: {CYAN}{last_str}{RESET}"
                )
        if detail and label in ("crashed", "build-fail", "reverted", "blacklisted"):
            lines.append(f"      {DIM}{detail[:width - 8]}{RESET}")

    return "\n".join(lines)


def main(argv=None, sleep_fn=time.sleep, stdout=sys.stdout, now_fn=time.time):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--log-dir",
        default=None,
        help="Directory of per-format .log files (parallel_model_fix_loop.py's --log-dir) or "
             "per-worker worker-<N>.log files (parallel_tag_fix_loop.py's --log-dir) -- "
             "auto-detected by filename shape. Default: auto-detect which wrapper is actually "
             f"running by checking both {OXIDEX_HOME / 'logs' / 'parallel-tag-fix'} and "
             f"{OXIDEX_HOME / 'logs' / 'parallel-model-fix'}, picking whichever has the more "
             "recently modified log file.",
    )
    parser.add_argument(
        "--tags-found-log",
        default=None,
        help="Shared tags-found log (parallel_tag_fix_loop.py's --tags-found-log). Default: "
             "<log-dir's parent>/tags-found.log, matching that wrapper's own default layout.",
    )
    parser.add_argument(
        "--tag-state-path",
        default=None,
        help="Shared per-tag blacklist state (parallel_tag_fix_loop.py's --tag-state-path). "
             "Default: <log-dir's parent>/model-fix-tag-state.json.",
    )
    parser.add_argument(
        "--wrapper-log",
        default=None,
        help="The parallel wrapper's own stdout (e.g. `... > logs/parallel-wrapper.log`), used "
             "for lifetime per-worker restart/crash counts -- a worker's own worker-<N>.log is "
             "overwritten on every respawn and can't answer that by itself. Default: "
             "<log-dir's parent>/parallel-wrapper.log.",
    )
    parser.add_argument(
        "--manifest-log",
        default=None,
        help="The shared request-audit manifest.log every worker/format appends to (see "
             "model_fix_loop.py's make_logging_call_model) -- one location for both wrappers, "
             "since model_fix_loop.py's own req_log_dir is OXIDEX_HOME-fixed, not per-worktree. "
             "Default: <log-dir's parent>/model-fix-requests/manifest.log.",
    )
    parser.add_argument(
        "--tagcmp-dir",
        default=DEFAULT_TAGCMP_DIR,
        help="Directory holding tagcmp-<FORMAT>.json comparison reports -- "
             "find_tag_gaps.run_format_comparison writes these to a fixed /tmp path unconditionally "
             f"(NOT under find_tag_gaps.py's own --cache-dir, a different, unrelated directory that "
             f"holds the ExifTool binary/sample corpus instead). Default: {DEFAULT_TAGCMP_DIR}",
    )
    parser.add_argument(
        "--repo-root",
        default=str(DEFAULT_REPO_ROOT),
        help="Repo root to look for a full-corpus comparison.json in, alongside --tagcmp-dir's "
             "per-format files, for the format progress bars.",
    )
    parser.add_argument(
        "--max-tag-fails", type=int, default=10,
        help="Display-only denominator for each worker's 'iteration N/M' -- match whatever "
             "--max-tag-fails the fixer run itself was launched with (default: 10).",
    )
    parser.add_argument(
        "--worktree-dir",
        default=None,
        help="Base directory of each worker/format's own persistent worktree (parallel_tag_fix_loop.py's "
             "or parallel_model_fix_loop.py's own --worktree-dir), used to read each worker's "
             "config.toml copy for its fixer/reviewer model pool and reasoning level. Default: "
             "auto-detect by mode, same as --log-dir -- "
             f"{DEFAULT_WORKTREE_DIR_BY_MODE['tag']} for tag mode, "
             f"{DEFAULT_WORKTREE_DIR_BY_MODE['format']} for format mode.",
    )
    parser.add_argument("--interval", type=float, default=0.5, help="Redraw interval in seconds")
    args = parser.parse_args(argv)

    if args.log_dir:
        log_dir = Path(args.log_dir)
        stdout.write(f"Waiting for logs to appear in {log_dir}...\n")
        stdout.flush()
        while not log_dir.is_dir() or not any(log_dir.glob("*.log")):
            sleep_fn(args.interval)
    else:
        candidates = [OXIDEX_HOME / "logs" / "parallel-tag-fix", OXIDEX_HOME / "logs" / "parallel-model-fix"]
        stdout.write(f"Waiting for logs to appear in {candidates[0]} or {candidates[1]}...\n")
        stdout.flush()
        log_dir = find_active_log_dir(candidates)
        while log_dir is None:
            sleep_fn(args.interval)
            log_dir = find_active_log_dir(candidates)

    tags_found_log = (
        Path(args.tags_found_log) if args.tags_found_log else log_dir.parent / "tags-found.log"
    )
    tag_state_path = (
        Path(args.tag_state_path) if args.tag_state_path else log_dir.parent / "model-fix-tag-state.json"
    )
    wrapper_log_path = (
        Path(args.wrapper_log) if args.wrapper_log else log_dir.parent / "parallel-wrapper.log"
    )
    manifest_path = (
        Path(args.manifest_log) if args.manifest_log else log_dir.parent / "model-fix-requests" / "manifest.log"
    )

    try:
        while True:
            worker_ids, mode = discover_worker_ids(log_dir)
            worktree_dir = args.worktree_dir or DEFAULT_WORKTREE_DIR_BY_MODE[mode]
            term_width = shutil.get_terminal_size(fallback=(100, 24)).columns
            format_progress = discover_format_progress(args.tagcmp_dir, args.repo_root)
            stdout.write("\x1b[2J\x1b[H")  # clear screen, cursor home
            stdout.write(render_dashboard(
                log_dir, worker_ids, tags_found_log, tag_state_path, wrapper_log_path,
                format_progress, args.max_tag_fails, now_fn(), term_width, worktree_dir,
                manifest_path, mode,
            ) + "\n")
            stdout.flush()
            sleep_fn(args.interval)
    except KeyboardInterrupt:
        return 0


if __name__ == "__main__":
    sys.exit(main())
