#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Local live dashboard for the OxiDex harness.

Run read-only: ``uv run scripts/harness_dashboard.py``.
Add ``--enable-controls`` for confirmed localhost-only terminate/restart actions.
"""

from __future__ import annotations

import argparse
import datetime as dt
import html
import json
import os
import plistlib
import re
import secrets
import shutil
import signal
import subprocess  # nosec B404 -- all commands use fixed argv
import sys
import tempfile
import threading
import time
from collections import defaultdict, deque
from collections.abc import Callable
from concurrent.futures import ThreadPoolExecutor
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_HOME = Path(os.environ.get("OXIDEX_HOME", Path.home() / ".oxidex"))
MANIFEST_RE = re.compile(r"^(?P<timestamp>\S+) phase=(?P<phase>fixer|reviewer|critique) worker=(?P<worker>\S+).*?(?P<outcome>\b(?:OK|ERROR=\S+|RETRY)\b)")
HTTP_STATUS_RE = re.compile(r"(?:\bhttp_status=|\bHTTP(?:\s+Error)?\s*|\bstatus(?:\s*code)?\s*[:=]?\s*)(?P<status>[1-5]\d{2})\b", re.I)
PR_RE = re.compile(r"\b(?:created|opened|published|merged|squash-merged).*?\bPR\s*#(?P<number>\d+)\b", re.I)
ALLOWED = ("fleet_up.sh", "parallel_model_fix_loop.py", "model_fix_loop.py", "squad_merge_loop.py", "overlord_sweep.py")
FLEET_LABEL = "com.oxidex.fleet"
FLEET_PLIST = Path.home() / "Library/LaunchAgents/com.oxidex.fleet.plist"
MIN_WORKERS, MAX_WORKERS = 1, 64        # plist historically held 3 and 60; the build semaphore
                                        # caps builds at 5, the governor at 30 calls/min -- the
                                        # ceiling is a safety feature
MIN_MERGERS, MAX_MERGERS = 0, 32        # 0 = all squads; values >= squad count behave as all
LAUNCHCTL_PRINT_TIMEOUT = 10
BOOTSTRAP_TIMEOUT = 30
BOOTOUT_TIMEOUT = 120                   # bootout drains the supervisor's SIGTERM->SIGKILL
                                        # ladder (FLEET_GRACE_SECONDS=20 per tier, serially)
DISPATCHER_PGIDS = DEFAULT_HOME / "logs" / "dispatcher-pgids.json"
FLEET_PROCESS_MARKERS = ("fleet_up.sh", "parallel_model_fix_loop.py", "squad_merge_loop.py")
FLEET_CONFIG_TTL_SECONDS = 10.0
_fleet_config_cache: tuple[float, dict[str, Any]] | None = None
API_RECENT_REQUESTS = 20
_api_recent_requests: deque[dict[str, Any]] = deque(maxlen=API_RECENT_REQUESTS)
WORKTREE_CACHE_SECONDS = 60.0
FROZEN_AFTER_SECONDS = 30 * 60
CLAIM_STALE_SECONDS = 2 * 60 * 60
_worktree_cache: dict[str, tuple[float, dict[str, Any] | None]] = {}
_publisher_cache: tuple[float, int, dict[str, Any]] | None = None
_github_pr_cache: tuple[float, str, list[dict[str, Any]]] | None = None
_cpu_samples: dict[int, tuple[float, float]] = {}
_manifest_cache: dict[str, dict[str, Any]] = {}
_fleet_log_cache: dict[str, dict[str, Any]] = {}
_lesson_cache: dict[str, dict[str, Any]] = {}
LESSON_EVENTS = ("review_rejected", "critique")
RECENT_LOG_SIZE = 40
RATE_WINDOW_SECONDS = 3600.0
_tag_claim_cache: tuple[tuple[int, int, int], dict[str, list[tuple[float, str]]]] | None = None
# Diff files are write-once, so an mtime read once never needs re-statting.
_diff_mtime_cache: dict[str, float] = {}
# Distinct tags each worker has ever been seen claiming: the union of the tag
# state file's current records (fresh and stale) across dashboard polls.
_tags_seen: dict[str, set[str]] = defaultdict(set)


def text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""


def json_file(path: Path, default: Any) -> Any:
    try:
        return json.loads(text(path))
    except ValueError:
        return default


def epoch(value: str) -> float | None:
    try:
        return dt.datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp()
    except ValueError:
        return None


def cpu_seconds(value: str) -> float | None:
    """Parse the portable ps TIME format (``[[dd-]hh:]mm:ss.xx``)."""
    try:
        days, clock = (value.split("-", 1) + [""])[:2] if "-" in value else ("0", value)
        parts = clock.split(":")
        seconds = float(parts.pop())
        factor = 60.0
        for part in reversed(parts):
            seconds += int(part) * factor
            factor *= 60.0
        return seconds + int(days) * 86400.0
    except ValueError:
        return None


def process_table() -> dict[int, dict[str, Any]]:
    """Live PID/RSS and interval CPU readings, sampled once per API poll."""
    now = time.monotonic()
    try:
        out = subprocess.run(["ps", "-axo", "pid=,ppid=,time=,rss=,etime=,command="], capture_output=True, text=True, timeout=5, check=False).stdout
    except (OSError, subprocess.SubprocessError):
        return {}
    rows: dict[int, dict[str, Any]] = {}
    next_samples: dict[int, tuple[float, float]] = {}
    for line in out.splitlines():
        fields = line.strip().split(None, 5)
        if len(fields) != 6:
            continue
        try:
            pid, ppid, rss = int(fields[0]), int(fields[1]), int(fields[3])
        except ValueError:
            continue
        cpu = cpu_seconds(fields[2])
        if cpu is None:
            continue
        previous = _cpu_samples.get(pid)
        percent = 0.0
        if previous and cpu >= previous[0] and now > previous[1]:
            percent = max(0.0, (cpu - previous[0]) * 100.0 / (now - previous[1]))
        next_samples[pid] = (cpu, now)
        rows[pid] = {"pid": pid, "ppid": ppid, "cpu_percent": percent, "memory_bytes": rss * 1024, "elapsed": fields[4], "elapsed_seconds": cpu_seconds(fields[4]) or 0.0, "command": fields[5]}
    _cpu_samples.clear()
    _cpu_samples.update(next_samples)
    return rows


def worker_runtime_state(process: dict[str, Any], last_task: dict[str, Any] | None, now: float | None = None) -> tuple[str, str]:
    """Return a conservative state for a live worker, avoiding restart false positives."""
    now = now if now is not None else time.time()
    elapsed = float(process.get("elapsed_seconds", 0.0) or 0.0)
    if elapsed < FROZEN_AFTER_SECONDS:
        return "running", "Live worker process found."
    started_at = now - elapsed
    last_activity = (last_task or {}).get("epoch")
    progress_at = max(started_at, last_activity) if isinstance(last_activity, (int, float)) else started_at
    silent_for = now - progress_at
    cpu = float(process.get("cpu_percent", 0.0) or 0.0)
    if silent_for >= FROZEN_AFTER_SECONDS and cpu <= 0.1:
        return "frozen", f"Potentially frozen: PID {process['pid']} has no recorded task progress for {int(silent_for // 60)}m and used {cpu:.2f}% CPU in the last second."
    return "running", "Live worker process found."


def incremental_lines(path: Path, cache: dict[str, dict[str, Any]]) -> tuple[dict[str, Any], list[str]]:
    """Read only appended complete lines; reset safely if a log was rotated."""
    key = str(path)
    try:
        stat = path.stat()
    except OSError:
        return cache.setdefault(key, {"offset": 0, "remainder": ""}), []
    state = cache.get(key)
    if state is None or state.get("device") != stat.st_dev or state.get("inode") != stat.st_ino or stat.st_size < state.get("offset", 0):
        state = {"device": stat.st_dev, "inode": stat.st_ino, "offset": 0, "remainder": ""}
        cache[key] = state
    try:
        with path.open("rb") as handle:
            handle.seek(state["offset"])
            appended = handle.read().decode("utf-8", errors="replace")
    except OSError:
        return state, []
    state["offset"] = stat.st_size
    combined = state["remainder"] + appended
    parts = combined.splitlines(keepends=True)
    state["remainder"] = ""
    if parts and not parts[-1].endswith(("\n", "\r")):
        state["remainder"] = parts.pop()
    return state, [line.rstrip("\r\n") for line in parts]


def _manifest_fields(line: str) -> dict[str, str]:
    """key=value tokens from a manifest line.

    Newer lines carry tier/provider/model/prompt_chars/elapsed/reply_chars;
    older ones do not, so every field stays optional.
    """
    fields: dict[str, str] = {}
    for token in line.split():
        if "=" in token:
            key, _, value = token.partition("=")
            fields[key] = value
    return fields


def _manifest_int(fields: dict[str, str], key: str) -> int | None:
    value = fields.get(key, "")
    return int(value) if value.isdigit() else None


def manifest_stats(path: Path) -> dict[str, dict[str, Any]]:
    """Per-worker model-call counters, plus a bounded recent-call log.

    ``recent`` only grows from newly appended lines each poll (the same
    incremental design as everything else here), so it stays cheap even
    though the dashboard process runs indefinitely.
    """
    state, lines = incremental_lines(path, _manifest_cache)
    result = state.setdefault("result", defaultdict(lambda: {"fixer_calls": 0, "reviewer_calls": 0, "critique_calls": 0, "last_task": None, "last_by_phase": {}, "recent": deque(maxlen=RECENT_LOG_SIZE), "call_times": deque()}))
    for line in lines:
        match = MANIFEST_RE.match(line)
        if not match:
            continue
        stat = result[match["worker"]]
        stat[f"{match['phase']}_calls"] += 1
        at = epoch(match["timestamp"])
        if at is not None:
            stat["call_times"].append(at)
        status_match = HTTP_STATUS_RE.search(line)
        task = {
            "timestamp": match["timestamp"], "epoch": at, "phase": match["phase"], "outcome": match["outcome"],
            "http_status": int(status_match["status"]) if status_match else None,
        }
        stat["recent"].append(task)
        fields = _manifest_fields(line)
        # Global last-N request feed for the Model API dialog. The tag is
        # stamped later in snapshot() from the live claims of the same poll
        # ("at ingest"): claims are transient, so a lazy lookup would lie.
        _api_recent_requests.append({
            **task, "worker": match["worker"],
            "tier": fields.get("tier"), "provider": fields.get("provider"), "model": fields.get("model"),
            "prompt_chars": _manifest_int(fields, "prompt_chars"), "reply_chars": _manifest_int(fields, "reply_chars"),
            "elapsed": fields.get("elapsed"), "tag": None, "unstamped": True,
        })
        old = stat["last_task"]
        if at is not None and (old is None or at > old["epoch"]):
            stat["last_task"] = task
        old_for_phase = stat["last_by_phase"].get(match["phase"])
        if at is not None and (old_for_phase is None or at > old_for_phase["epoch"]):
            stat["last_by_phase"][match["phase"]] = task
    return dict(result)


def latest_phase_task(stat: dict[str, Any], *phases: str) -> dict[str, Any] | None:
    """Return the most recent request among the requested model phases."""
    by_phase = stat.get("last_by_phase", {})
    tasks = [by_phase.get(phase) for phase in phases]
    tasks = [task for task in tasks if task]
    return max(tasks, key=lambda task: task.get("epoch") or 0) if tasks else None


def recent_call_stats(stat: dict[str, Any], now: float | None = None) -> dict[str, Any]:
    """Rates over one worker's already-collected model-call records.

    ``calls_last_hour`` prunes and counts the incremental epoch deque, so it is
    an exact last-hour volume; the error share and HTTP mix cover the bounded
    ``recent`` window (last ``RECENT_LOG_SIZE`` calls). No new I/O happens here.
    """
    now = time.time() if now is None else now
    times = stat.get("call_times")
    if times is not None:
        while times and now - times[0] > RATE_WINDOW_SECONDS:
            times.popleft()
    recent = list(stat.get("recent", ()))
    fixer = [task for task in recent if task.get("phase") == "fixer"]
    result: dict[str, Any] = {"calls_last_hour": len(times) if times is not None else 0}
    if fixer:
        result["fixer_error_pct"] = round(100.0 * sum(1 for task in fixer if task.get("outcome") != "OK") / len(fixer), 1)
    for bucket in (2, 4, 5):
        result[f"recent_http_{bucket}xx"] = sum(1 for task in recent if isinstance(task.get("http_status"), int) and task["http_status"] // 100 == bucket)
    return result


def lesson_stats(path: Path) -> dict[str, dict[str, Any]]:
    """Per-worker review-rejection/critique counts and the latest human reason.

    A diff filed as ``-applied.diff`` only means ``git apply`` succeeded --
    it can still fail cargo build, the gap recheck, or be rejected by the
    reviewer afterward. The only durable, human-readable record of *why* a
    patch didn't land is this K1 lesson ledger (``logs/lessons.jsonl``),
    written once per rejected round by ``model_fix_loop.py``. Diff filenames
    and manifest.log entries cannot answer "why" -- only this file can.
    """
    state, lines = incremental_lines(path, _lesson_cache)
    result = state.setdefault("result", defaultdict(lambda: {"review_rejected": 0, "critique": 0, "last_reason": None, "recent": deque(maxlen=RECENT_LOG_SIZE), "rejection_times": deque()}))
    for line in lines:
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        if not isinstance(entry, dict):
            continue
        worker, event = entry.get("worker"), entry.get("event")
        if not isinstance(worker, str) or not worker or event not in LESSON_EVENTS:
            continue
        stat = result[worker]
        stat[event] += 1
        at = epoch(entry["ts"]) if isinstance(entry.get("ts"), str) else None
        if event == "review_rejected" and at is not None:
            stat["rejection_times"].append(at)
        reason = entry.get("reason")
        if not isinstance(reason, str) or not reason:
            continue
        record = {"event": event, "reason": reason, "timestamp": entry.get("ts"), "epoch": at, "tag_key": entry.get("tag_key")}
        stat["recent"].append(record)
        old = stat["last_reason"]
        if old is None or (at or 0) >= (old.get("epoch") or 0):
            stat["last_reason"] = record
    return dict(result)


def rejections_last_hour(lesson: dict[str, Any], now: float | None = None) -> int:
    """Review-rejected verdicts in the rate window, from the lesson ledger only.

    Prunes and counts the incremental epoch deque, same pattern as
    ``recent_call_stats``: exact volume, no new I/O.
    """
    now = time.time() if now is None else now
    times = lesson.get("rejection_times")
    if times is None:
        return 0
    while times and now - times[0] > RATE_WINDOW_SECONDS:
        times.popleft()
    return len(times)


def known_workers(home: Path, manifest: dict[str, Any]) -> list[str]:
    names = set(manifest)
    try:
        names.update(path.name.removeprefix("model-fix-") for path in (home / "worktrees" / "parallel-fix").iterdir() if path.is_dir())
    except OSError:
        pass
    return sorted(name for name in names if name and name != "parallel-fix")


def patch_stats(diff_dir: Path, workers: list[str]) -> dict[str, dict[str, int]]:
    """Diffs a worker generated, split by whether ``git apply`` accepted them.

    The ``-applied``/``-rejected`` filename suffix records only a mechanical
    ``git apply`` verdict, taken *before* any build, test, or reviewer ever
    sees the diff -- an "applied" diff can still fail cargo build or be
    turned down by the reviewer afterward. So ``patches_applied`` means
    "applied to the worktree," not "accepted." The real accept/reject verdict
    lives in the K1 lesson ledger; see ``lesson_stats``.
    """
    result = {name: {"patches_found": 0, "patches_applied": 0, "patches_apply_failed": 0, "patches_last_hour": 0} for name in workers}
    try:
        files = list(diff_dir.glob("*.diff"))
    except OSError:
        return result
    cutoff = time.time() - RATE_WINDOW_SECONDS
    def written_recently(path: Path) -> bool:
        mtime = _diff_mtime_cache.get(path.name)
        if mtime is None:
            try:
                mtime = path.stat().st_mtime
            except OSError:
                mtime = 0.0
            _diff_mtime_cache[path.name] = mtime
        return mtime >= cutoff
    for worker in workers:
        for path in files:
            if path.name.endswith(f"-{worker}-applied.diff"):
                result[worker]["patches_found"] += 1
                result[worker]["patches_applied"] += 1
            elif path.name.endswith(f"-{worker}-rejected.diff"):
                result[worker]["patches_found"] += 1
                result[worker]["patches_apply_failed"] += 1
            else:
                continue
            if written_recently(path):
                result[worker]["patches_last_hour"] += 1
    return result


def active_tag_claims(path: Path, now: float | None = None) -> dict[str, dict[str, Any]]:
    """Return each live worker's primary claimed tag and clustered siblings.

    The shared tag state is the durable source of truth for work in progress.
    Claims are heartbeated while a worker is active, so old ownership records
    are omitted rather than being presented as current work.
    """
    global _tag_claim_cache
    now = time.time() if now is None else now
    try:
        stat = path.stat()
    except OSError:
        return {}
    cache_key = (stat.st_ino, stat.st_size, stat.st_mtime_ns)
    if _tag_claim_cache is None or _tag_claim_cache[0] != cache_key:
        state = json_file(path, {})
        claims: dict[str, list[tuple[float, str]]] = defaultdict(list)
        if isinstance(state, dict):
            for key, entry in state.items():
                if not isinstance(entry, dict):
                    continue
                worker, claimed_at = entry.get("claimed_by"), entry.get("claimed_at")
                if not isinstance(worker, str) or not worker or not isinstance(claimed_at, (int, float)):
                    continue
                tag = entry.get("tag_key") or key
                if isinstance(tag, str) and tag:
                    claims[worker].append((float(claimed_at), tag))
                    _tags_seen[worker].add(tag)
        # Python's stable sort preserves the state-file order for a cluster
        # claimed in the same instant; its first tag is the worker's leader.
        _tag_claim_cache = (cache_key, {
            worker: sorted(entries, key=lambda entry: entry[0], reverse=True)
            for worker, entries in claims.items()
        })
    result: dict[str, dict[str, Any]] = {}
    for worker, entries in _tag_claim_cache[1].items():
        fresh = [(claimed_at, tag) for claimed_at, tag in entries if now - claimed_at < CLAIM_STALE_SECONDS]
        if fresh:
            result[worker] = {"tag": fresh[0][1], "tags": [tag for _, tag in fresh], "claimed_at": fresh[0][0]}
    return result


def tags_claimed_totals() -> dict[str, int]:
    """Distinct tags per worker observed in the tag state (fresh and stale).

    Cheap history: the state file keeps stale ownership records around for a
    while, and ``_tags_seen`` accumulates everything observed since this
    dashboard started, so the count only grows within a dashboard session.
    """
    return {worker: len(tags) for worker, tags in _tags_seen.items()}


def git_inventory() -> dict[str, dict[str, str]]:
    """Get every registered worktree's branch and head in one Git call."""
    try:
        out = subprocess.run(["git", "-C", str(REPO_ROOT), "worktree", "list", "--porcelain"], capture_output=True, text=True, timeout=5, check=False).stdout
    except (OSError, subprocess.SubprocessError):
        return {}
    result: dict[str, dict[str, str]] = {}
    entry: dict[str, str] = {}
    for line in [*out.splitlines(), ""]:
        if not line:
            if entry.get("path"):
                result[str(Path(entry["path"]).resolve())] = entry
            entry = {}
            continue
        key, _, value = line.partition(" ")
        if key == "worktree":
            entry["path"] = value
        elif key in {"HEAD", "branch"}:
            entry[key] = value
        elif key == "detached":
            entry["detached"] = "true"
    return result


def parse_origin_main_divergence(output: str) -> tuple[int, int] | None:
    """Return (behind, ahead) from `git rev-list --left-right --count` output."""
    values = output.split()
    if len(values) != 2:
        return None
    try:
        return int(values[0]), int(values[1])
    except ValueError:
        return None


def git_worktrees(paths: list[Path]) -> dict[str, dict[str, Any] | None]:
    """Refresh many worktrees concurrently, while keeping hot API polls cheap."""
    now = time.monotonic()
    unique = {str(path.resolve()): path for path in paths}
    result: dict[str, dict[str, Any] | None] = {}
    stale: dict[str, Path] = {}
    for key, path in unique.items():
        cached = _worktree_cache.get(key)
        if cached and now - cached[0] < WORKTREE_CACHE_SECONDS:
            result[key] = cached[1]
        else:
            stale[key] = path
    if not stale:
        return result
    inventory = git_inventory()
    def inspect(key: str, path: Path) -> tuple[str, dict[str, Any] | None]:
        if not path.exists():
            return key, None
        entry = inventory.get(key, {})
        try:
            dirty = subprocess.run(["git", "-C", str(path), "status", "--porcelain=v1"], capture_output=True, text=True, timeout=5, check=False).stdout.count("\n")
        except (OSError, subprocess.SubprocessError):
            return key, {"path": str(path), "error": "git inspection failed"}
        try:
            comparison = subprocess.run(["git", "-C", str(path), "rev-list", "--left-right", "--count", "origin/main...HEAD"], capture_output=True, text=True, timeout=5, check=False)
            divergence = parse_origin_main_divergence(comparison.stdout) if comparison.returncode == 0 else None
        except (OSError, subprocess.SubprocessError):
            divergence = None
        branch = entry.get("branch", "").removeprefix("refs/heads/") or "(detached)"
        return key, {
            "path": str(path), "branch": branch, "head": entry.get("HEAD", "")[:12] or None, "dirty_files": dirty,
            "behind": divergence[0] if divergence else None, "ahead": divergence[1] if divergence else None,
        }
    with ThreadPoolExecutor(max_workers=min(8, len(stale))) as pool:
        for key, value in pool.map(lambda item: inspect(*item), stale.items()):
            _worktree_cache[key] = (now, value)
            result[key] = value
    return result


def fleet_rows(home: Path) -> dict[str, dict[str, Any]]:
    entries = {}
    for line in text(home / "logs" / "fleet-up.state").splitlines():
        fields = line.split("\t", 3)
        if len(fields) != 4:
            continue
        try:
            pid = int(fields[1])
        except ValueError:
            pid = None
        entries[fields[0]] = {"pid": pid, "state": fields[2], "expected": fields[3]}
    return entries


def fleet_log_summary(path: Path) -> dict[str, Any]:
    """Index the append-only fleet log once, then process only new events."""
    state, lines = incremental_lines(path, _fleet_log_cache)
    state.setdefault("last", {})
    state.setdefault("counts", defaultdict(int))
    state.setdefault("recent", defaultdict(lambda: deque(maxlen=RECENT_LOG_SIZE)))
    state.setdefault("pr_events", {})
    for line in lines:
        stamp = line.split(" ", 1)[0]
        at = epoch(stamp)
        if at is None:
            continue
        event = {"timestamp": stamp, "epoch": at, "phase": "activity", "outcome": line[-120:]}
        markers = []
        if "[fleet-up]" in line:
            markers.append("[fleet-up]")
        if "[dispatcher]" in line:
            markers.append("[dispatcher]")
        if "auto-publish:" in line:
            markers.append("auto-publish:")
        if merger := re.search(r"\[merger:([^]]+)\]", line):
            markers.append(f"[merger:{merger.group(1)}]")
        for marker in markers:
            state["counts"][marker] += 1
            state["recent"][marker].append(event)
            old = state["last"].get(marker)
            if old is None or at >= old["epoch"]:
                state["last"][marker] = event
        if match := PR_RE.search(line):
            pr_event = {"number": match["number"], "timestamp": stamp, "epoch": at, "name": None, "title": None, "url": None}
            old = state["pr_events"].get(match["number"])
            if old is None or at >= old["epoch"]:
                state["pr_events"][match["number"]] = pr_event
    return state


def github_repo_name(repo_root: Path) -> str | None:
    try:
        remote = subprocess.run(["git", "-C", str(repo_root), "remote", "get-url", "origin"], capture_output=True, text=True, timeout=5, check=False).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return None
    match = re.search(r"github\.com[/:](?P<repo>[^/\s]+/[^/\s]+?)(?:\.git)?$", remote)
    return match["repo"] if match else None


def github_recent_prs(repo_root: Path, limit: int = 20) -> list[dict[str, Any]]:
    """Read the recent PR list once per short interval, with no dashboard failure on GitHub outage."""
    global _github_pr_cache
    repo = github_repo_name(repo_root)
    if repo is None:
        return []
    now = time.monotonic()
    if _github_pr_cache and _github_pr_cache[1] == repo and now - _github_pr_cache[0] < 30:
        return _github_pr_cache[2]
    try:
        result = subprocess.run(
            ["gh", "pr", "list", "--repo", repo, "--state", "all", "--limit", str(limit), "--json", "number,title,createdAt,mergedAt,updatedAt,headRefName,url"],
            capture_output=True, text=True, timeout=10, check=False,
        )
        raw_prs = json.loads(result.stdout) if result.returncode == 0 else []
    except (OSError, subprocess.SubprocessError, ValueError):
        raw_prs = []
    prs = []
    for raw in raw_prs if isinstance(raw_prs, list) else []:
        if not isinstance(raw, dict) or not isinstance(raw.get("number"), int):
            continue
        timestamp = raw.get("mergedAt") or raw.get("createdAt") or raw.get("updatedAt")
        if not isinstance(timestamp, str):
            continue
        prs.append({
            "number": str(raw["number"]), "name": raw.get("headRefName") or "—", "title": raw.get("title") or "—",
            "timestamp": timestamp, "epoch": epoch(timestamp) or 0.0, "url": raw.get("url"),
        })
    prs.sort(key=lambda entry: entry["epoch"], reverse=True)
    _github_pr_cache = (now, repo, prs[:limit])
    return _github_pr_cache[2]


def publisher_stats(path: Path, repo_root: Path) -> dict[str, Any]:
    """PR creation events reconciled with the newer durable mainline record."""
    global _publisher_cache
    log_summary = fleet_log_summary(path)
    now = time.monotonic()
    log_size = log_summary.get("offset", 0)
    if _publisher_cache and _publisher_cache[1] == log_size and now - _publisher_cache[0] < 30:
        return _publisher_cache[2]
    events = dict(log_summary["pr_events"])
    try:
        history = subprocess.run(["git", "-C", str(repo_root), "log", "origin/main", "--format=%cI%x1f%s"], capture_output=True, text=True, timeout=10, check=False).stdout
    except (OSError, subprocess.SubprocessError):
        history = ""
    mainline_events = 0
    for line in history.splitlines():
        stamp, _, subject = line.partition("\x1f")
        match = re.search(r"\(#(?P<number>\d+)\)", subject)
        if not match:
            continue
        mainline_events += 1
        event = {
            "number": match["number"], "timestamp": stamp, "epoch": epoch(stamp) or 0.0,
            "name": "origin/main", "title": re.sub(r"\s*\(#\d+\)\s*$", "", subject).strip() or subject, "url": None,
        }
        if match["number"] not in events or event["epoch"] >= events[match["number"]]["epoch"]:
            events[match["number"]] = event
    recent_prs = github_recent_prs(repo_root)
    used_github = bool(recent_prs)
    if not recent_prs:
        recent_prs = [
            {"number": event["number"], "name": event.get("name") or "—", "title": event.get("title") or f"PR #{event['number']}", "timestamp": event["timestamp"], "epoch": event["epoch"], "url": event.get("url")}
            for event in sorted(events.values(), key=lambda entry: entry["epoch"], reverse=True)[:20]
        ]
    source = "GitHub + fleet log + origin/main" if used_github else "fleet log + origin/main" if mainline_events else "fleet log"
    latest = max(events.values(), key=lambda entry: entry["epoch"], default=None)
    day_start = dt.datetime.now().astimezone().replace(hour=0, minute=0, second=0, microsecond=0).timestamp()
    merged_today = sum(1 for event in events.values() if event["epoch"] >= day_start)
    value = {"prs_made": len(events), "prs_merged_today": merged_today, "last_pr": latest, "recent_prs": recent_prs, "source": source}
    _publisher_cache = (now, log_size, value)
    return value


def queue_stats(home: Path) -> dict[str, Any]:
    """Current judgment depth and blocked merger batches, keyed by durable IDs."""
    newest: dict[str, dict[str, Any]] = {}
    event_count = 0
    for line in text(home / "logs" / "judgment-queue.jsonl").splitlines():
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        patch_id = entry.get("patch_id")
        if not isinstance(patch_id, str):
            continue
        event_count += 1
        previous = newest.get(patch_id)
        if previous is None or entry.get("ts_epoch", 0) >= previous.get("ts_epoch", 0):
            newest[patch_id] = entry
    queued = [entry for entry in newest.values() if entry.get("verdict") == "queued"]
    blocked: list[dict[str, Any]] = []
    batch_commits_by_squad: dict[str, int] = {}
    for path in (home / "logs" / "squad-status").glob("*-batch.json"):
        entry = json_file(path, {})
        if not isinstance(entry, dict):
            continue
        squad = path.stem.removesuffix("-batch")
        batch_commits_by_squad[squad] = entry.get("commits_since", 0) if isinstance(entry.get("commits_since"), int) else 0
        if entry.get("blocked") is True:
            blocked.append({"squad": squad, "last_batch_ts": entry.get("last_batch_ts")})
    details = [{"patch_id": entry["patch_id"][:12], "format": entry.get("format"), "squad": entry.get("squad"), "reason": entry.get("reason"), "timestamp": entry.get("ts")} for entry in sorted(queued, key=lambda item: item.get("ts_epoch", 0), reverse=True)]
    return {"judgment_depth": len(queued), "event_count": event_count, "blocked_squads": blocked, "batch_commits": sum(batch_commits_by_squad.values()), "batch_commits_by_squad": batch_commits_by_squad, "queued": details}


def flow_stats(components: list[dict[str, Any]], manifest: dict[str, dict[str, Any]], publisher: dict[str, Any], queue: dict[str, Any], claimed_tags: int = 0) -> dict[str, Any]:
    """Graph-ready, group-level throughput across the harness's real hand-offs."""
    calls = {phase: sum(stat.get(f"{phase}_calls", 0) for stat in manifest.values()) for phase in ("fixer", "reviewer", "critique")}
    calls["total"] = sum(calls.values())
    grouped = {role: [component for component in components if component["role"] == role] for role in ("supervisor", "dispatcher", "worker", "reviewer", "merger", "publisher")}
    worker_patches = sum(component["metrics"].get("patches_found", 0) for component in grouped["worker"])
    worker_patches_last_hour = sum(component["metrics"].get("patches_last_hour", 0) for component in grouped["worker"])
    worker_calls_last_hour = sum(component["metrics"].get("calls_last_hour", 0) for component in grouped["worker"])
    http_mix = {key: sum(component["metrics"].get(key, 0) for component in grouped["worker"]) for key in ("recent_http_2xx", "recent_http_4xx", "recent_http_5xx")}
    reviewer_sent = sum(component["metrics"].get("patches_sent", 0) for component in grouped["reviewer"])
    reviewer_applied = sum(component["metrics"].get("patches_applied", 0) for component in grouped["reviewer"])
    reviewer_apply_failed = sum(component["metrics"].get("patches_apply_failed", 0) for component in grouped["reviewer"])
    reviewer_review_rejected = sum(component["metrics"].get("review_rejected", 0) for component in grouped["reviewer"])
    reviewer_critique_events = sum(component["metrics"].get("critique_events", 0) for component in grouped["reviewer"])
    def members(role: str) -> list[dict[str, Any]]:
        return [{"id": component["id"], "role": component["role"], "label": component["label"], "status": component["status"], "pid": component["pid"], "pid_note": component["pid_note"], "process": component["process"], "metrics": component["metrics"], "current_tag": component["current_tag"], "last_task": component["last_task"], "last_reason": component.get("last_reason"), "worktree": component["worktree"]} for component in grouped[role]]
    def state(role: str) -> tuple[int, int]:
        running = sum(component["status"] == "running" for component in grouped[role])
        return running, len(grouped[role]) - running
    def group_node(role: str, label: str, headline: str, summary: str, detail: dict[str, Any]) -> dict[str, Any]:
        running, inactive = state(role)
        return {"label": label, "headline": headline, "summary": summary, "detail": {"processes": len(grouped[role]), "running": running, "inactive": inactive, **detail}, "members": members(role)}
    supervisor_running, _ = state("supervisor")
    dispatcher_running, _ = state("dispatcher")
    worker_running, _ = state("worker")
    reviewer_running, _ = state("reviewer")
    merger_running, _ = state("merger")
    publisher_running, _ = state("publisher")
    publisher_status = grouped["publisher"][0]["status"] if grouped["publisher"] else "stopped"
    dispatcher_events = sum(component["metrics"].get("events", 0) for component in grouped["dispatcher"])
    worker_frozen = sum(component["status"] == "frozen" for component in grouped["worker"])
    worker_summary = f"{worker_patches:,} patches · {calls['fixer']:,} fixer calls" + (f" · {worker_frozen} frozen" if worker_frozen else "")
    reviewer_detail = {"calls": calls["reviewer"] + calls["critique"], "patches_sent": reviewer_sent, "patches_applied": reviewer_applied, "patches_apply_failed": reviewer_apply_failed, "review_rejected": reviewer_review_rejected, "critique_events": reviewer_critique_events, "rejections_last_hour": sum(component["metrics"].get("rejections_last_hour", 0) for component in grouped["reviewer"])}
    if reviewer_sent:
        reviewer_detail["review_rejection_pct"] = round(100.0 * reviewer_review_rejected / reviewer_sent, 1)
    return {
        "nodes": {
            "supervisor": group_node("supervisor", "Fleet supervisor", f"{supervisor_running}/{len(grouped['supervisor'])} running", "owns fleet lifecycle", {}),
            "dispatcher": group_node("dispatcher", "Dispatchers", f"{dispatcher_running}/{len(grouped['dispatcher'])} running", f"{worker_running} active workers · {dispatcher_events:,} events", {"active_workers": worker_running, "events": dispatcher_events}),
            "workers": group_node("worker", "Fixer workers", f"{len(grouped['worker'])} workers · {worker_running} active", worker_summary, {"patches_found": worker_patches, "fixer_calls": calls["fixer"], "frozen": worker_frozen, "patches_last_hour": worker_patches_last_hour, "claimed_tags": claimed_tags}),
            "api": {"label": "Model API", "headline": f"{calls['total']:,} total calls", "summary": f"{calls['fixer']:,} fixer · {calls['reviewer'] + calls['critique']:,} review", "detail": {**calls, "calls_last_hour": worker_calls_last_hour, **http_mix}},
            "reviewers": group_node("reviewer", "Review gates", f"{len(grouped['reviewer'])} gates · {reviewer_running} active", f"{reviewer_applied:,} git-applied · {reviewer_review_rejected:,} review-rejected of {reviewer_sent:,} sent", reviewer_detail),
            "mergers": group_node("merger", "Squad mergers", f"{merger_running}/{len(grouped['merger'])} running", f"{queue['batch_commits']:,} unbatched · {len(queue['blocked_squads'])} blocked", {"blocked_squads": len(queue["blocked_squads"]), "batch_commits": queue["batch_commits"]}),
            "publisher": group_node("publisher", "Publish sweep", f"{publisher['prs_made']:,} PRs made", f"{('sweep active' if publisher_running else 'waiting for dispatcher' if publisher_status == 'waiting' else 'dispatcher unavailable')} · latest {('#' + publisher['last_pr']['number']) if publisher['last_pr'] else '—'}", {"prs_merged_today": publisher.get("prs_merged_today", 0), "last_pr": publisher["last_pr"], "recent_prs": publisher.get("recent_prs", []), "source": publisher["source"], "status": publisher_status}),
            "queue": {"label": "Judgment queue", "headline": f"{queue['judgment_depth']:,} queued now", "summary": f"{queue['event_count']:,} events · advisory", "detail": {"event_count": queue["event_count"], "blocked_squads": queue["blocked_squads"], "queued": queue["queued"]}},
            "main": {"label": "origin/main", "headline": f"{publisher['prs_made']:,} merged PRs", "summary": f"latest {('#' + publisher['last_pr']['number']) if publisher['last_pr'] else '—'}", "detail": {"merged_prs": publisher["prs_made"], "last_pr": publisher["last_pr"]}},
        }
    }


def item(component_id: str, role: str, label: str, process: dict[str, Any] | None, *, worktree: dict[str, Any] | None = None, last_task: dict[str, Any] | None = None, current_tag: dict[str, Any] | None = None, metrics: dict[str, Any] | None = None, hint: str = "", status: str | None = None, pid_note: str | None = None, last_reason: dict[str, Any] | None = None, log: list[dict[str, Any]] | None = None) -> dict[str, Any]:
    display_pid = None
    if process is not None:
        # A worker's useful live identity is its deepest executable child
        # (rustc/mold/cargo), not the long-lived Python wrapper. Keep the
        # wrapper in ``process`` so terminate() can still safely stop the
        # complete worker process group.
        display_pid = process.get("active_pid", process["pid"]) if role in {"worker", "reviewer"} else process["pid"]
    return {"id": component_id, "role": role, "label": label, "status": status or ("running" if process else "offline"), "pid": display_pid, "pid_note": pid_note, "process": process, "worktree": worktree, "last_task": last_task, "current_tag": current_tag, "metrics": metrics or {}, "action_hint": hint, "last_reason": last_reason, "log": log or []}


def worker_from_command(command: str) -> str | None:
    match = re.search(r"--worker-id\s+([A-Za-z0-9_.-]+)", command)
    if match:
        return match.group(1)
    match = re.search(r"--(?:only-)?format\s+([A-Za-z0-9_.-]+)", command)
    return match.group(1) if match else None


def squad_from_command(command: str) -> str | None:
    match = re.search(r"--squad\s+([A-Za-z0-9_.-]+)", command)
    return match.group(1) if match else None


def runs_script(command: str, script: str) -> bool:
    """True only when a process is executing ``scripts/<script>``.

    A plain substring match turns dashboard/terminal commands that merely
    mention a script name into fake harness components.
    """
    return bool(re.search(rf"(?<!\S)(?:\S+/)?scripts/{re.escape(script)}(?:\s|$)", command))


def repo_from_script_command(command: str, script: str) -> Path | None:
    """Return the checkout containing an absolute script path in a command."""
    match = re.search(rf"(?<!\S)(?P<path>\S+)/scripts/{re.escape(script)}(?:\s|$)", command)
    if not match:
        return None
    candidate = Path(match["path"])
    return candidate if candidate.is_dir() else None


def deepest_descendant(processes: dict[int, dict[str, Any]], process: dict[str, Any] | None) -> dict[str, Any] | None:
    """Find the deepest live descendant, preferring the busiest one at the same depth."""
    if process is None:
        return None
    children: dict[int, list[int]] = defaultdict(list)
    for row in processes.values():
        children[row["ppid"]].append(row["pid"])
    selected, selected_rank = process, (0, float(process.get("cpu_percent", 0.0) or 0.0), process["pid"])
    pending, seen = [(process["pid"], 0)], set()
    while pending:
        pid, depth = pending.pop()
        if pid in seen or pid not in processes:
            continue
        seen.add(pid)
        row = processes[pid]
        rank = (depth, float(row.get("cpu_percent", 0.0) or 0.0), pid)
        if rank > selected_rank:
            selected, selected_rank = row, rank
        pending.extend((child, depth + 1) for child in children.get(pid, []))
    return selected


def process_with_active_child(processes: dict[int, dict[str, Any]], process: dict[str, Any] | None) -> dict[str, Any] | None:
    """Keep the component PID while annotating its deepest live child command."""
    if process is None:
        return None
    active = deepest_descendant(processes, process) or process
    return {**process, "active_pid": active["pid"], "active_command": active["command"]}


def child_process_usage(processes: dict[int, dict[str, Any]], process: dict[str, Any] | None) -> dict[str, Any] | None:
    """Include a worker's compiler/test children, without rolling up the fleet."""
    annotated = process_with_active_child(processes, process)
    if annotated is None:
        return None
    total_cpu, total_memory = 0.0, 0
    children: dict[int, list[int]] = defaultdict(list)
    for row in processes.values():
        children[row["ppid"]].append(row["pid"])
    pending, seen = [annotated["pid"]], set()
    while pending:
        pid = pending.pop()
        if pid in seen or pid not in processes:
            continue
        seen.add(pid)
        row = processes[pid]
        total_cpu += row["cpu_percent"]
        total_memory += row["memory_bytes"]
        pending.extend(children.get(pid, []))
    return {**annotated, "cpu_percent": total_cpu, "memory_bytes": total_memory}


def format_task_entry(task: dict[str, Any]) -> dict[str, Any]:
    status = task.get("http_status")
    detail = f"HTTP {status}" if isinstance(status, int) else "no HTTP status recorded"
    return {"timestamp": task.get("timestamp"), "epoch": task.get("epoch"), "text": f"{task.get('phase')} request: {task.get('outcome')} ({detail})"}


def format_reason_entry(entry: dict[str, Any]) -> dict[str, Any]:
    label = "Critique" if entry.get("event") == "critique" else "Review rejected"
    tag = f" [{entry['tag_key']}]" if entry.get("tag_key") else ""
    return {"timestamp": entry.get("timestamp"), "epoch": entry.get("epoch"), "text": f"{label}{tag}: {entry.get('reason')}"}


def worker_log(stat: dict[str, Any], lesson: dict[str, Any], *, phases: tuple[str, ...]) -> list[dict[str, Any]]:
    """Merge recent model-call and rejection-reason events into one timeline, newest first."""
    entries = [format_task_entry(task) for task in stat.get("recent", ()) if task.get("phase") in phases]
    entries += [format_reason_entry(reason) for reason in lesson.get("recent", ())]
    entries.sort(key=lambda entry: entry.get("epoch") or 0, reverse=True)
    return entries[:RECENT_LOG_SIZE]


def fleet_marker_log(fleet_events: dict[str, Any], marker: str) -> list[dict[str, Any]]:
    recent = fleet_events.get("recent", {}).get(marker, ())
    return [{"timestamp": event["timestamp"], "epoch": event["epoch"], "text": event["outcome"]} for event in reversed(list(recent))]


def snapshot(home: Path, repo_root: Path) -> dict[str, Any]:
    processes = process_table()
    dispatcher = next((row for row in processes.values() if runs_script(row["command"], "parallel_model_fix_loop.py")), None)
    dispatcher_repo = (
        repo_from_script_command(dispatcher["command"], "parallel_model_fix_loop.py")
        if dispatcher else None
    ) or home / "worktrees" / "fleet-main"
    manifest = manifest_stats(home / "logs" / "model-fix-requests" / "manifest.log")
    workers = known_workers(home, manifest)
    patches = patch_stats(home / "logs" / "model-fix-diffs", workers)
    lessons = lesson_stats(home / "logs" / "lessons.jsonl")
    state = fleet_rows(home)
    tag_claims = active_tag_claims(home / "logs" / "model-fix-tag-state.json")
    for request_record in _api_recent_requests:
        if request_record.pop("unstamped", None):
            claim = tag_claims.get(request_record["worker"])
            request_record["tag"] = claim["tag"] if claim else None
    tag_totals = tags_claimed_totals()
    queue = queue_stats(home)
    fleet_log = home / "logs" / "fleet-up.log"
    fleet_events = fleet_log_summary(fleet_log)
    last_events: dict[str, dict[str, Any]] = fleet_events["last"]
    event_counts: dict[str, int] = fleet_events["counts"]
    base = home / "worktrees" / "parallel-fix"
    locks: list[tuple[str, dict[str, Any]]] = []
    for path in sorted((home / "logs" / "knowledge").glob("merger-*.lock")):
        data = json_file(path, {})
        locks.append((path.stem.removeprefix("merger-"), data if isinstance(data, dict) else {}))
    live_merger_squads = {squad_from_command(row["command"]) for row in processes.values() if runs_script(row["command"], "squad_merge_loop.py")}
    worktree_paths = [dispatcher_repo, repo_root, *(base / f"model-fix-{worker}" for worker in workers), *(home / "worktrees" / "squad-staging" / squad for squad, _ in locks), *(home / "worktrees" / "squad-staging" / squad for squad in live_merger_squads if squad)]
    worktrees = git_worktrees(worktree_paths)
    def worktree(path: Path) -> dict[str, Any] | None:
        return worktrees.get(str(path.resolve()))
    components: list[dict[str, Any]] = []
    supervisor = state.get("supervisor", {})
    components.append(item("fleet", "supervisor", "Fleet supervisor", process_with_active_child(processes, processes.get(supervisor.get("pid"))), worktree=worktree(dispatcher_repo), last_task=last_events.get("[fleet-up]"), metrics={"recorded_state": supervisor.get("state", "not recorded")}, hint="Start bootstraps the configured LaunchAgent; Scale edits its plist (the source of truth for worker count) and restarts the fleet; work is salvaged before clean workers sync.", log=fleet_marker_log(fleet_events, "[fleet-up]")))
    components.append(item("dispatcher", "dispatcher", "Task dispatcher", process_with_active_child(processes, dispatcher), worktree=worktree(dispatcher_repo), last_task=last_events.get("[dispatcher]"), metrics={"events": event_counts.get("[dispatcher]", 0)}, hint="Workers are dispatcher-owned; restart recreates the fleet safely.", log=fleet_marker_log(fleet_events, "[dispatcher]")))
    running_workers: dict[str, dict[str, Any]] = {}
    for row in processes.values():
        if runs_script(row["command"], "model_fix_loop.py") and not runs_script(row["command"], "parallel_model_fix_loop.py"):
            if name := worker_from_command(row["command"]):
                current = running_workers.get(name)
                if current is None or (current["command"].startswith("uv run") and not row["command"].startswith("uv run")):
                    running_workers[name] = row
    for worker in workers:
        stat = manifest.get(worker, {})
        patch = patches[worker]
        lesson = lessons.get(worker, {})
        process = child_process_usage(processes, running_workers.get(worker))
        worktree_data = worktree(base / f"model-fix-{worker}")
        worker_status, worker_hint = worker_runtime_state(process, stat.get("last_task")) if process else ("offline", "A prior worker record has no live process or PID.") if stat else ("archived", "Historical worktree entry with no recorded worker activity.")
        worker_task = latest_phase_task(stat, "fixer")
        reviewer_task = latest_phase_task(stat, "reviewer", "critique")
        current_tag = tag_claims.get(worker) if process else None
        last_reason = lesson.get("last_reason")
        call_rates = recent_call_stats(stat)
        worker_metrics = {**patch, "fixer_calls": stat.get("fixer_calls", 0), "review_rejected": lesson.get("review_rejected", 0), "critique_events": lesson.get("critique", 0), **call_rates, "tags_claimed_total": tag_totals.get(worker, 0)}
        reviewer_metrics = {"patches_sent": patch["patches_found"], "patches_applied": patch["patches_applied"], "patches_apply_failed": patch["patches_apply_failed"], "review_rejected": lesson.get("review_rejected", 0), "rejections_last_hour": rejections_last_hour(lesson), "critique_events": lesson.get("critique", 0), "reviewer_calls": stat.get("reviewer_calls", 0), "critique_calls": stat.get("critique_calls", 0)}
        if patch["patches_found"]:
            reviewer_metrics["review_rejection_pct"] = round(100.0 * lesson.get("review_rejected", 0) / patch["patches_found"], 1)
        if worker_status == "frozen":
            worker_metrics["freeze_reason"] = worker_hint
            reviewer_metrics["freeze_reason"] = worker_hint
        components.append(item(f"worker:{worker}", "worker", worker, process, worktree=worktree_data, last_task=worker_task, current_tag=current_tag, metrics=worker_metrics, hint=worker_hint, status=worker_status, last_reason=last_reason, log=worker_log(stat, lesson, phases=("fixer",))))
        components.append(item(f"reviewer:{worker}", "reviewer", f"{worker} reviewer", process, worktree=worktree_data, last_task=reviewer_task, current_tag=current_tag, metrics=reviewer_metrics, hint=worker_hint, status=worker_status, last_reason=last_reason, log=worker_log(stat, lesson, phases=("reviewer", "critique"))))
    merger_seen: set[str] = set()
    blocked_squads = {entry["squad"] for entry in queue["blocked_squads"]}
    def merger_metrics(squad: str) -> dict[str, Any]:
        # Batch state comes from the squad-status files: unbatched commit
        # count always, plus an explicit blocked flag only when set.
        metrics: dict[str, Any] = {"batch_commits": queue["batch_commits_by_squad"].get(squad, 0)}
        if squad in blocked_squads:
            metrics["batch_blocked"] = "yes"
        return metrics
    for squad, data in locks:
        pid = data.get("pid")
        merger_seen.add(squad)
        components.append(item(f"merger:{squad}", "merger", f"{squad} merger", child_process_usage(processes, processes.get(pid)), worktree=worktree(home / "worktrees" / "squad-staging" / squad), last_task=last_events.get(f"[merger:{squad}]"), metrics={"heartbeat_ts": data.get("heartbeat_ts"), **merger_metrics(squad)}, hint="A live supervisor respawns a terminated merger; Restart relaunches the fleet.", log=fleet_marker_log(fleet_events, f"[merger:{squad}]")))
    for row in processes.values():
        if not runs_script(row["command"], "squad_merge_loop.py"):
            continue
        squad = squad_from_command(row["command"]) or f"pid-{row['pid']}"
        if squad not in merger_seen:
            components.append(item(f"merger:{squad}", "merger", f"{squad} merger", child_process_usage(processes, row), worktree=worktree(home / "worktrees" / "squad-staging" / squad), metrics=merger_metrics(squad), log=fleet_marker_log(fleet_events, f"[merger:{squad}]")))
    publisher = next((row for row in processes.values() if runs_script(row["command"], "overlord_sweep.py")), None)
    publisher_repo = (
        repo_from_script_command(publisher["command"], "overlord_sweep.py")
        if publisher else None
    ) or dispatcher_repo
    publisher_metric = publisher_stats(fleet_log, publisher_repo)
    publisher_task = last_events.get("auto-publish:")
    last_pr = publisher_metric["last_pr"]
    if last_pr and (publisher_task is None or last_pr["epoch"] > publisher_task["epoch"]):
        publisher_task = {"timestamp": last_pr["timestamp"], "epoch": last_pr["epoch"], "phase": "PR merged", "outcome": f"#{last_pr['number']} ({publisher_metric['source']})"}
    publisher_status = "running" if publisher else "waiting" if dispatcher else "stopped"
    publisher_process = child_process_usage(processes, publisher) if publisher else {**dispatcher, "active_pid": dispatcher["pid"], "active_command": dispatcher["command"], "display_name": "dispatcher owner"} if dispatcher else None
    publisher_note = None if publisher else "dispatcher owner" if dispatcher else None
    publisher_hint = "A dispatcher-owned publish sweep is active." if publisher else f"No publish child is running; dispatcher owner PID {dispatcher['pid']} is waiting to invoke the next sweep." if dispatcher else "The dispatcher is not running, so no publish sweep can be invoked."
    publisher_log = fleet_marker_log(fleet_events, "auto-publish:") + [
        {"timestamp": pr["timestamp"], "epoch": pr["epoch"], "text": f"PR #{pr['number']}: {pr['title']} ({pr['name']})"}
        for pr in publisher_metric.get("recent_prs", [])
    ]
    publisher_log.sort(key=lambda entry: entry.get("epoch") or 0, reverse=True)
    components.append(item("publisher", "publisher", "PR publisher", publisher_process, worktree=worktree(publisher_repo), last_task=publisher_task, metrics=publisher_metric, hint=publisher_hint, status=publisher_status, pid_note=publisher_note, log=publisher_log[:RECENT_LOG_SIZE]))
    running = sum(component["status"] == "running" for component in components)
    waiting = sum(component["status"] == "waiting" for component in components)
    frozen = sum(component["status"] == "frozen" for component in components)
    offline = sum(component["status"] == "offline" for component in components)
    archived = sum(component["status"] == "archived" for component in components)
    # Worker and reviewer cards share one wrapper process (and a waiting
    # publisher shows its dispatcher owner), so fleet totals dedupe by PID.
    unique_processes = {component["process"]["pid"]: component["process"] for component in components if component["process"]}
    summary = {
        "components": len(components), "running": running, "waiting": waiting, "frozen": frozen, "offline": offline, "archived": archived,
        "idle": len(components) - running - waiting - frozen - offline - archived,
        "active_workers": sum(component["role"] == "worker" and component["status"] == "running" for component in components),
        "total_cpu_percent": round(sum(float(process.get("cpu_percent", 0.0) or 0.0) for process in unique_processes.values()), 1),
        "total_memory_bytes": sum(int(process.get("memory_bytes", 0) or 0) for process in unique_processes.values()),
        "claimed_tags": sum(len(claim["tags"]) for claim in tag_claims.values()),
        "judgment_depth": queue["judgment_depth"],
        "patches_last_hour": sum(entry["patches_last_hour"] for entry in patches.values()),
        "model_calls_last_hour": sum(component["metrics"].get("calls_last_hour", 0) for component in components if component["role"] == "worker"),
        "review_rejections_last_hour": sum(component["metrics"].get("rejections_last_hour", 0) for component in components if component["role"] == "reviewer"),
        "prs_merged_today": publisher_metric.get("prs_merged_today", 0),
    }
    # Configured-versus-live fleet counts. workers_active counts this round's
    # dispatcher worker pgids (0 also means between-rounds idle, not zero
    # capacity); mergers_alive counts live squad_merge_loop.py processes.
    # Never derived from fleet-up.state (stale by design after hard kills).
    pgids = dispatcher_pgids(home)
    fleet_configuration = {
        **fleet_config_cached(),
        "workers_active": len(pgids),
        "mergers_alive": sum(1 for row in processes.values() if runs_script(row["command"], "squad_merge_loop.py")),
        "mid_round": bool(pgids),
    }
    return {"generated_at": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"), "summary": summary, "fleet_config": fleet_configuration, "api_requests": list(reversed(_api_recent_requests)), "flow": flow_stats(components, manifest, publisher_metric, queue, claimed_tags=summary["claimed_tags"]), "components": components}


def safe_process(data: dict[str, Any], component_id: str) -> tuple[dict[str, Any], dict[str, Any] | None]:
    component = next((entry for entry in data["components"] if entry["id"] == component_id), None)
    if component is None:
        raise ValueError("unknown component")
    process = component["process"]
    # Match the same script-path predicate used to detect harness processes,
    # not a loose substring: the supervisor slot is filled from a PID read out
    # of the stale-by-design fleet-up.state, so a reused PID must be verified
    # as actually running a harness script before any signal is sent to it.
    if process and not any(runs_script(process["command"], marker) for marker in ALLOWED):
        raise ValueError("refusing to control a process outside the harness")
    return component, process


def terminate(process: dict[str, Any]) -> str:
    pid, command = process["pid"], process["command"]
    try:
        if "model_fix_loop.py" in command and "parallel_model_fix_loop.py" not in command:
            os.killpg(os.getpgid(pid), signal.SIGTERM)
            return f"sent SIGTERM to worker process group {pid}"
        os.kill(pid, signal.SIGTERM)
        return f"sent SIGTERM to PID {pid}"
    except ProcessLookupError:
        return "process already exited"


class FleetControlError(Exception):
    """A control-plane failure carrying an HTTP status and a structured JSON payload."""

    def __init__(self, status: HTTPStatus, payload: dict[str, Any]):
        super().__init__(str(payload.get("error", "fleet control error")))
        self.status, self.payload = status, payload


def _valid_count(value: Any, lo: int, hi: int) -> bool:
    """Strict count validation: bools (a subclass of int), floats, and numeric strings all fail."""
    return isinstance(value, int) and not isinstance(value, bool) and lo <= value <= hi


def _result_detail(result: subprocess.CompletedProcess[str]) -> str:
    return ((result.stderr or "").strip() or (result.stdout or "").strip() or f"exit {result.returncode}")[-300:]


def _invalidate_fleet_config_cache() -> None:
    global _fleet_config_cache
    _fleet_config_cache = None


def fleet_service_target() -> str:
    return f"gui/{os.getuid()}/{FLEET_LABEL}"


def fleet_service_loaded() -> bool:
    """True when launchd has the service bootstrapped (returncode only; never parse print output)."""
    try:
        return subprocess.run(["launchctl", "print", fleet_service_target()], capture_output=True, text=True, timeout=LAUNCHCTL_PRINT_TIMEOUT, check=False).returncode == 0
    except (OSError, subprocess.SubprocessError):
        return False


def fleet_service_disabled() -> bool:
    try:
        out = subprocess.run(["launchctl", "print-disabled", f"gui/{os.getuid()}"], capture_output=True, text=True, timeout=LAUNCHCTL_PRINT_TIMEOUT, check=False).stdout
    except (OSError, subprocess.SubprocessError):
        return False
    return f'"{FLEET_LABEL}" => disabled' in out


def fleet_plist_read() -> dict[str, Any]:
    with FLEET_PLIST.open("rb") as handle:
        return plistlib.load(handle)


def fleet_deployed_script(args: list[Any]) -> Path | None:
    """The deployed fleet_up.sh named by the plist's ProgramArguments."""
    return next((Path(token) for token in args if isinstance(token, str) and token.endswith("fleet_up.sh")), None)


def fleet_capabilities(args: list[Any] | None = None) -> dict[str, Any]:
    """Probe the DEPLOYED fleet_up.sh text for optional flags.

    Behavior at launch is whatever is on disk at the plist's ProgramArguments
    path (the fleet-main checkout), not this worktree's copy, so capabilities
    must be probed there and never hardcoded.
    """
    if args is None:
        try:
            args = fleet_plist_read().get("ProgramArguments") or []
        except (OSError, plistlib.InvalidFileException):
            args = []
    script = fleet_deployed_script(args if isinstance(args, list) else [])
    return {"mergers_flag": script is not None and "--mergers" in text(script)}


def fleet_squad_total(args: list[Any]) -> int | None:
    """Count the squads the deployed fleet will actually run.

    Post-#624 they are [squads.<name>] tables in the plist's --config file;
    older deployments kept a separate scripts/squads.toml beside fleet_up.sh.
    """
    candidates: list[Path] = [Path(args[index + 1]) for index, token in enumerate(args) if token == "--config" and index + 1 < len(args) and isinstance(args[index + 1], str)]
    script = fleet_deployed_script(args)
    if script is not None:
        candidates.append(script.parent / "squads.toml")
    for candidate in candidates:
        count = sum(1 for line in text(candidate).splitlines() if line.startswith("[squads."))
        if count:
            return count
    return None


def fleet_parse_arguments(args: list[Any]) -> dict[str, Any]:
    parsed: dict[str, Any] = {"configured_workers": None, "configured_mergers": None, "squad_mode": False}
    for index, token in enumerate(args):
        following = args[index + 1] if index + 1 < len(args) else None
        if token == "--workers" and isinstance(following, str) and following.isdigit():
            parsed["configured_workers"] = int(following)
        elif token == "--mergers" and isinstance(following, str) and following.isdigit():
            parsed["configured_mergers"] = int(following)
        elif token == "--squad-mode":
            parsed["squad_mode"] = True
    return parsed


def fleet_config() -> dict[str, Any]:
    """Persistent fleet configuration; the LaunchAgent plist is the single source of truth."""
    config: dict[str, Any] = {
        "plist_present": FLEET_PLIST.exists(), "service_loaded": fleet_service_loaded(), "service_disabled": fleet_service_disabled(),
        "configured_workers": None, "configured_mergers": None, "squad_mode": False,
        "capabilities": {"mergers_flag": False}, "plist_mtime": None, "squad_total": None,
    }
    if not config["plist_present"]:
        return config
    try:
        config["plist_mtime"] = int(FLEET_PLIST.stat().st_mtime)
        data = fleet_plist_read()
    except (OSError, plistlib.InvalidFileException):
        return config
    args = data.get("ProgramArguments")
    if isinstance(args, list):
        config.update(fleet_parse_arguments(args))
        config["capabilities"] = fleet_capabilities(args)
        config["squad_total"] = fleet_squad_total(args)
    return config


def fleet_config_cached() -> dict[str, Any]:
    """A short-TTL copy for /api/status so `launchctl print` is not spawned every poll."""
    global _fleet_config_cache
    now = time.monotonic()
    if _fleet_config_cache is None or now - _fleet_config_cache[0] >= FLEET_CONFIG_TTL_SECONDS:
        _fleet_config_cache = (now, fleet_config())
    return _fleet_config_cache[1]


def _fleet_plist_shape_ok(data: Any) -> bool:
    """Only edit the exact hand-maintained shape: right Label plus a --workers <digits> pair."""
    if not isinstance(data, dict) or data.get("Label") != FLEET_LABEL:
        return False
    args = data.get("ProgramArguments")
    if not isinstance(args, list):
        return False
    for index, token in enumerate(args):
        if token == "--workers":
            following = args[index + 1] if index + 1 < len(args) else None
            return isinstance(following, str) and following.isdigit()
    return False


def fleet_plist_write(mutate: Callable[[dict[str, Any]], None]) -> str:
    """Atomically edit the LaunchAgent plist; returns the backup filename.

    plistlib only (never regex/string-template the XML), timestamped .bak,
    mtime_ns conflict guard (concurrent hand-editors are real on this machine),
    temp-file round-trip validation (~ plutil -lint), then os.replace. A
    malformed plist bricks the agent -- bootstrap fails and no fleet can start
    until a human repairs the file -- so every step here fails closed.
    """
    try:
        before = FLEET_PLIST.stat()
        data = fleet_plist_read()
    except OSError as exc:
        raise FleetControlError(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": "~/Library/LaunchAgents/com.oxidex.fleet.plist not found or unreadable; the dashboard will not create one"}) from exc
    except plistlib.InvalidFileException as exc:
        raise FleetControlError(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": "plist structure not recognized; refusing to edit ~/Library/LaunchAgents/com.oxidex.fleet.plist"}) from exc
    if not _fleet_plist_shape_ok(data):
        raise FleetControlError(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": "plist structure not recognized; refusing to edit ~/Library/LaunchAgents/com.oxidex.fleet.plist"})
    mutate(data)
    try:
        unchanged = FLEET_PLIST.stat().st_mtime_ns == before.st_mtime_ns
    except OSError:
        unchanged = False
    if not unchanged:
        raise FleetControlError(HTTPStatus.CONFLICT, {"error": "plist changed on disk during edit (concurrent editor); no changes made"})
    backup = FLEET_PLIST.with_name(f"{FLEET_PLIST.name}.bak-{time.strftime('%H%M%S')}")
    temp_name = None
    try:
        shutil.copy2(FLEET_PLIST, backup)
        handle_fd, temp_name = tempfile.mkstemp(prefix=f".{FLEET_PLIST.name}.", dir=str(FLEET_PLIST.parent))
        with os.fdopen(handle_fd, "wb") as handle:
            plistlib.dump(data, handle)
        with open(temp_name, "rb") as handle:
            plistlib.load(handle)
        os.replace(temp_name, FLEET_PLIST)
    except (OSError, OverflowError, TypeError, ValueError, plistlib.InvalidFileException) as exc:
        if temp_name is not None:
            try:
                os.unlink(temp_name)
            except OSError:
                pass
        raise FleetControlError(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": f"failed to write the plist safely; the original is untouched: {exc}", "backup": backup.name}) from exc
    _invalidate_fleet_config_cache()
    return backup.name


def _scale_mutation(workers: int | None, mergers: int | None) -> Callable[[dict[str, Any]], None]:
    """Replace the value after --workers; replace the --mergers pair or insert it
    immediately after the --workers value (deterministic position). The only
    user-derived data ever written is str(validated_int)."""

    def mutate(data: dict[str, Any]) -> None:
        args = data["ProgramArguments"]
        workers_index = args.index("--workers")
        if workers is not None:
            args[workers_index + 1] = str(workers)
        if mergers is not None:
            if "--mergers" in args:
                mergers_index = args.index("--mergers")
                if mergers_index + 1 < len(args):
                    args[mergers_index + 1] = str(mergers)
                else:
                    args.append(str(mergers))
            else:
                args[workers_index + 2:workers_index + 2] = ["--mergers", str(mergers)]

    return mutate


def fleet_bootout() -> subprocess.CompletedProcess[str]:
    # The only durable stop while the agent is loaded: KeepAlive=true respawns
    # anything fleet_up.sh --down kills within ThrottleInterval=30s.
    return subprocess.run(["launchctl", "bootout", fleet_service_target()], capture_output=True, text=True, timeout=BOOTOUT_TIMEOUT, check=False)


def fleet_bootstrap() -> subprocess.CompletedProcess[str]:
    # RunAtLoad=true starts the fleet immediately; no kickstart needed afterwards.
    return subprocess.run(["launchctl", "bootstrap", f"gui/{os.getuid()}", str(FLEET_PLIST)], capture_output=True, text=True, timeout=BOOTSTRAP_TIMEOUT, check=False)


def dispatcher_pgids(home: Path | None = None) -> list[Any]:
    path = (home / "logs" / "dispatcher-pgids.json") if home is not None else DISPATCHER_PGIDS
    data = json_file(path, None)
    if isinstance(data, dict):
        data = data.get("pgids")
    return data if isinstance(data, list) else []


def fleet_mid_round(home: Path | None = None) -> bool:
    # The exact "safe window" gate fleet_up.sh's own dispatcher_workers_active
    # uses; reuse it, do not invent a new signal. An empty pgids file means
    # between-rounds idle, not zero capacity.
    return bool(dispatcher_pgids(home))


def live_fleet_pids(processes: dict[int, dict[str, Any]] | None = None) -> list[int]:
    """Argv-verified live fleet processes (the PID-reuse guard).

    Never trust ~/.oxidex/logs/fleet-up.state for liveness -- it is stale by
    design after hard kills and can claim tiers "running" with dead pids.
    """
    rows = process_table() if processes is None else processes
    return sorted(row["pid"] for row in rows.values() if any(runs_script(row["command"], marker) for marker in FLEET_PROCESS_MARKERS))


def fleet_down(repo_root: Path) -> str:
    """Durable fleet stop.

    While the agent is loaded, only bootout sticks: KeepAlive=true respawns a
    supervisor killed by fleet_up.sh --down within 30s. When the agent is not
    loaded, --down is pidfile-exact and correct for hand-launched fleets.
    """
    if fleet_service_loaded():
        result = fleet_bootout()
        if result.returncode:
            raise FleetControlError(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": f"bootout failed: {_result_detail(result)}", "step": "bootout"})
        _invalidate_fleet_config_cache()
        return "fleet stop requested (launchctl bootout)"
    subprocess.run([str(repo_root / "scripts" / "fleet_up.sh"), "--down"], cwd=repo_root, capture_output=True, text=True, timeout=45, check=False)
    return "fleet stop requested (fleet_up.sh --down)"


def fleet_start(repo_root: Path, home: Path) -> str:
    # The LaunchAgent plist is the single source of truth for fleet
    # configuration. Never recreate the fleet by running fleet_up.sh with its
    # defaults here: bare defaults are 32 workers / all mergers / timeout 2400
    # versus the configured values -- a silent 10x -- and a directly spawned
    # supervisor would be unsupervised by launchd, parented to a dashboard HTTP
    # thread, and collide with any later launchd start.
    loaded = fleet_service_loaded()
    pids = live_fleet_pids()
    if pids and not loaded:
        # Starting launchd here makes preflight_no_existing_fleet fail (exit 2)
        # and KeepAlive relaunches every 30s forever -- a silent crash-loop.
        listed = ", ".join(str(pid) for pid in pids[:8])
        raise FleetControlError(HTTPStatus.CONFLICT, {"error": f"a fleet is already running outside launchd (pids {listed}); stop it (fleet_up.sh --down / stop_parallel_fix.py) before starting the agent"})
    if loaded and pids:
        return "fleet already running"
    if loaded:
        # Throttle-window case: kickstart WITHOUT -k starts a stopped service
        # and does not disturb a running one.
        result = subprocess.run(["launchctl", "kickstart", fleet_service_target()], capture_output=True, text=True, timeout=BOOTSTRAP_TIMEOUT, check=False)
        if result.returncode:
            raise FleetControlError(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": f"kickstart failed: {_result_detail(result)}; see ~/.oxidex/logs/fleet-launchd.stderr.log"})
        _invalidate_fleet_config_cache()
        return "kickstarted"
    if not FLEET_PLIST.exists():
        raise FleetControlError(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": "~/Library/LaunchAgents/com.oxidex.fleet.plist not found; the dashboard will not create one"})
    result = fleet_bootstrap()
    if result.returncode:
        if fleet_service_disabled():
            # Persisted per-user override; operator intent required -- never auto-enable.
            raise FleetControlError(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": f"service is disabled; run: launchctl enable {fleet_service_target()}"})
        raise FleetControlError(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": f"bootstrap failed: {_result_detail(result)}; see ~/.oxidex/logs/fleet-launchd.stderr.log"})
    _invalidate_fleet_config_cache()
    workers = fleet_config().get("configured_workers")
    return f"bootstrapped; fleet starting with {workers if workers is not None else 'the configured number of'} workers"


def fleet_scale_action(request: dict[str, Any], home: Path) -> tuple[HTTPStatus, dict[str, Any]]:
    """Persist worker/merger counts in the plist, then apply via bootout + bootstrap.

    kickstart -k is forbidden here: launchd captures the service definition at
    bootstrap time, so kickstart re-runs the OLD cached spec -- a confident
    no-op that keeps the old worker count. Scale never starts a stopped fleet;
    that is start's job.
    """
    unexpected = sorted(str(key) for key in set(request) - {"id", "action", "workers", "mergers", "force"})
    if unexpected:
        raise FleetControlError(HTTPStatus.BAD_REQUEST, {"error": f"unexpected fields for scale: {', '.join(unexpected)}"})
    if "workers" not in request and "mergers" not in request:
        raise FleetControlError(HTTPStatus.BAD_REQUEST, {"error": "scale requires at least one of workers or mergers"})
    if "workers" in request and not _valid_count(request["workers"], MIN_WORKERS, MAX_WORKERS):
        raise FleetControlError(HTTPStatus.BAD_REQUEST, {"error": f"workers must be an integer between {MIN_WORKERS} and {MAX_WORKERS}"})
    if "mergers" in request and not _valid_count(request["mergers"], MIN_MERGERS, MAX_MERGERS):
        raise FleetControlError(HTTPStatus.BAD_REQUEST, {"error": f"mergers must be an integer between {MIN_MERGERS} and {MAX_MERGERS}"})
    force = request.get("force", False)
    if not isinstance(force, bool):
        raise FleetControlError(HTTPStatus.BAD_REQUEST, {"error": "force must be a boolean"})
    config = fleet_config()
    if not config["plist_present"]:
        raise FleetControlError(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": "~/Library/LaunchAgents/com.oxidex.fleet.plist not found; the dashboard will not create one"})
    if "mergers" in request and not config["capabilities"].get("mergers_flag"):
        raise FleetControlError(HTTPStatus.CONFLICT, {"error": "deployed fleet_up.sh does not support --mergers; update ~/.oxidex/worktrees/fleet-main first"})
    current_mergers = config["configured_mergers"] if config["configured_mergers"] is not None else 0
    workers = request["workers"] if "workers" in request and request["workers"] != config["configured_workers"] else None
    mergers = request["mergers"] if "mergers" in request and request["mergers"] != current_mergers else None
    if workers is None and mergers is None:
        described = " · ".join(filter(None, [
            f"{config['configured_workers']} workers" if "workers" in request else None,
            f"mergers {'all' if current_mergers == 0 else current_mergers}" if "mergers" in request else None,
        ]))
        return HTTPStatus.OK, {"message": f"already configured: {described}", "applied": False}
    changes = []
    if workers is not None:
        changes.append(f"workers {config['configured_workers']} -> {workers}")
    if mergers is not None:
        changes.append(f"mergers {'all' if current_mergers == 0 else current_mergers} -> {'all' if mergers == 0 else mergers}")
    changed = ", ".join(changes)
    try:
        backup = fleet_plist_write(_scale_mutation(workers, mergers))
    except FleetControlError as exc:
        exc.payload.setdefault("step", "plist_write")
        exc.payload.setdefault("fleet_state", "running-old-config" if config["service_loaded"] else "down-old-config")
        raise
    if not config["service_loaded"]:
        # Scale never starts a stopped fleet.
        return HTTPStatus.OK, {"message": f"saved: {changed}; takes effect on next start", "backup": backup, "applied": False}
    if fleet_mid_round(home) and not force:
        # The restart SIGKILLs worker process groups, skipping the
        # release-claims path: in-flight model spend is lost and tag claims
        # shadow re-claims for up to claim_stale_seconds (2h).
        raise FleetControlError(HTTPStatus.CONFLICT, {"error": "workers are mid-round (dispatcher-pgids.json non-empty); config saved but not applied — retry between rounds, or resend with force:true (loses in-flight work; tag claims may shadow up to 2h)", "backup": backup, "applied": False})
    try:
        result = fleet_bootout()
    except (OSError, subprocess.SubprocessError) as exc:
        raise FleetControlError(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": f"bootout failed: {exc}; config saved but the fleet still runs the old config", "step": "bootout", "fleet_state": "running-old-config", "backup": backup}) from exc
    if result.returncode:
        raise FleetControlError(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": f"bootout failed: {_result_detail(result)}; config saved but the fleet still runs the old config", "step": "bootout", "fleet_state": "running-old-config", "backup": backup})
    try:
        result = fleet_bootstrap()
    except (OSError, subprocess.SubprocessError) as exc:
        raise FleetControlError(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": f"bootstrap failed after bootout: {exc}; the fleet is DOWN with the new config staged — 'start' retries the bootstrap", "step": "bootstrap", "fleet_state": "down-new-config-staged", "backup": backup}) from exc
    if result.returncode:
        raise FleetControlError(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": f"bootstrap failed after bootout: {_result_detail(result)}; the fleet is DOWN with the new config staged — 'start' retries the bootstrap; see ~/.oxidex/logs/fleet-launchd.stderr.log", "step": "bootstrap", "fleet_state": "down-new-config-staged", "backup": backup})
    _invalidate_fleet_config_cache()
    return HTTPStatus.OK, {"message": f"scaled {changed} (persisted; fleet restarted)", "backup": backup, "applied": True}


HOST_ALLOWLIST = {"127.0.0.1", "localhost", "[::1]"}


def host_allowed(header: str | None) -> bool:
    """Loopback host names only, closing DNS rebinding.

    The per-process control token is same-origin-embedded in the page, so a
    rebinding page whose hostname resolves here becomes same-origin, reads /
    (and the token), and could invoke controls. Cheap check, fails closed.
    """
    if not header:
        return False
    host = header.strip().lower()
    host = host.split("]", 1)[0] + "]" if host.startswith("[") else host.split(":", 1)[0]
    return host in HOST_ALLOWLIST


def page(token: str, controls: bool) -> bytes:
    # Token is embedded only in this same-origin page.  Controls cannot be enabled on non-localhost.
    body = """<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>OxiDex Harness</title>
<style>:root{color-scheme:dark;--bg:#07111f;--card:#0d1b2d;--edge:#203652;--txt:#e5edf8;--muted:#95a8bf;--ok:#37d996;--idle:#f4bd4f;--blue:#62b0ff;--purple:#d481ff;--bad:#ff8e8e}*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--txt);font:14px/1.45 system-ui;overflow-x:hidden;-webkit-font-smoothing:antialiased}header{position:sticky;top:0;padding:14px 24px 12px;background:linear-gradient(180deg,#0b1b2ef5,#091625d9);backdrop-filter:blur(10px) saturate(1.3);-webkit-backdrop-filter:blur(10px) saturate(1.3);border-bottom:1px solid var(--edge);z-index:2}h1{font-size:18px;font-weight:650;letter-spacing:.01em;margin:0 0 8px}#summary,.muted{color:var(--muted)}#summary{display:flex;flex-wrap:wrap;gap:6px;font-size:12px;font-variant-numeric:tabular-nums}.chip{display:inline-flex;align-items:center;padding:3px 10px;border:1px solid var(--edge);border-radius:999px;background:#0d1b2d99;white-space:nowrap}.chip.c-ok{color:var(--ok);border-color:#1e5a41;background:rgba(55,217,150,.08)}.chip.c-blue{color:var(--blue);border-color:#2a5c8f;background:rgba(98,176,255,.08)}.chip.c-purple{color:var(--purple);border-color:#5d3c85;background:rgba(212,129,255,.08)}.chip.c-red{color:var(--bad);border-color:#7e3543;background:rgba(255,142,142,.08)}.chip.c-idle{color:var(--idle);border-color:#6d5a2c;background:rgba(244,189,79,.08)}main{padding:20px;max-width:2000px;margin:auto}button{color:var(--txt);background:#10243c;border:1px solid var(--edge);border-radius:8px;padding:6px 10px;cursor:pointer;font:inherit;font-size:13px;transition:border-color .15s ease,background .15s ease}button:hover{border-color:var(--blue);background:#14304e}button:active{transform:translateY(1px)}:is(button,input,select,a):focus-visible{outline:2px solid var(--blue);outline-offset:2px}input[type=checkbox]{accent-color:var(--blue);width:14px;height:14px}select{accent-color:var(--blue)}input::placeholder{color:#5c718a}.danger{border-color:#7e3543;color:#ffd9dc}.danger:hover{border-color:#c25563;background:#361723}.hidden,[hidden]{display:none!important}#notice{position:fixed;right:16px;bottom:16px;background:#183553;border:1px solid var(--blue);padding:10px 12px;border-radius:8px;max-width:500px;box-shadow:0 8px 24px #04080fa8}#fleet-banner{margin-top:8px;padding:8px 12px;border:1px solid #7e3543;background:rgba(255,142,142,.08);border-radius:8px;display:flex;align-items:center;gap:10px;font-size:13px}#fleet-scale-form{display:flex;flex-wrap:wrap;align-items:center;gap:10px;padding:12px;background:var(--bg);border:1px solid var(--edge);border-radius:8px}#fleet-scale-form label{display:inline-flex;align-items:center;gap:6px}#fleet-scale-form input[type=number]{width:70px;padding:6px 8px;background:#0d1b2d;border:1px solid var(--edge);border-radius:8px;color:var(--txt);font:inherit}#fleet-scale-form .scale-help{flex-basis:100%;margin:0;color:var(--muted);font-size:12px}.mid-round-badge{display:inline-block;margin-left:6px;padding:1px 7px;border:1px solid #6d5a2c;border-radius:999px;background:rgba(244,189,79,.1);color:var(--idle);font-size:10.5px;font-weight:600;letter-spacing:.04em}.role-icon{width:14px;height:14px;vertical-align:-2px;margin-right:7px;flex:none;opacity:.9}@media (prefers-reduced-motion:reduce){*,*::before,*::after,dialog[open],dialog[open]::backdrop{animation:none!important;transition:none!important}}
#cards{display:grid;grid-template-columns:repeat(auto-fill,minmax(300px,1fr));gap:12px}.pcard{position:relative;overflow:hidden;background:linear-gradient(180deg,#102138,var(--card) 58%);border:1px solid var(--edge);border-radius:10px;padding:12px 14px;display:flex;flex-direction:column;gap:6px;font-size:13px;min-width:0;cursor:pointer;transition:transform .16s ease,box-shadow .16s ease,border-color .16s ease}.pcard::before,.member-card::before{content:'';position:absolute;top:0;left:0;right:0;height:2px;background:var(--edge)}.pcard.running::before,.member-card.running::before{background:linear-gradient(90deg,var(--ok),rgba(55,217,150,0))}.pcard.waiting::before,.member-card.waiting::before{background:linear-gradient(90deg,var(--blue),rgba(98,176,255,0))}.pcard.frozen::before,.member-card.frozen::before{background:linear-gradient(90deg,var(--purple),rgba(212,129,255,0))}.pcard.offline::before,.pcard.stopped::before,.member-card.offline::before{background:linear-gradient(90deg,var(--bad),rgba(255,142,142,0))}.pcard.archived::before,.member-card.archived::before{background:linear-gradient(90deg,var(--muted),rgba(149,168,191,0))}.pcard:hover,.pcard:focus-visible{border-color:var(--blue);transform:translateY(-2px);box-shadow:0 10px 24px #04080fb3}.pcard:focus-visible{outline:2px solid var(--blue);outline-offset:1px}.pcard-actions{cursor:default}.pcard.running{border-color:#2c4a68}.pcard-head{display:flex;justify-content:space-between;align-items:baseline;gap:8px;min-width:0}.pcard-name{font-weight:650;font-size:15px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0}.pcard-role,.flow-stat-label,.chart-title{color:var(--muted);font-size:10.5px;text-transform:uppercase;letter-spacing:.07em;font-weight:600}.pcard-role{margin-left:6px}.pcard-status{display:inline-flex;align-items:center;gap:6px;white-space:nowrap;font-size:11px;font-weight:600;letter-spacing:.03em;flex:none;padding:2px 9px 2px 8px;border-radius:999px;background:rgba(149,168,191,.1)}.pcard.running .pcard-status,.member-card.running .pcard-status{background:rgba(55,217,150,.12)}.pcard.waiting .pcard-status,.member-card.waiting .pcard-status{background:rgba(98,176,255,.12)}.pcard.frozen .pcard-status,.member-card.frozen .pcard-status{background:rgba(212,129,255,.12)}.pcard.offline .pcard-status,.pcard.stopped .pcard-status,.member-card.offline .pcard-status{background:rgba(255,142,142,.1)}.dot{width:7px;height:7px;border-radius:50%;background:var(--idle);display:inline-block;flex:none}.pcard.running .dot,.member-card.running .dot{background:var(--ok);box-shadow:0 0 8px #37d99688;animation:pulse-dot 2.2s ease-in-out infinite}@keyframes pulse-dot{0%,100%{box-shadow:0 0 0 0 rgba(55,217,150,.5)}55%{box-shadow:0 0 0 5px rgba(55,217,150,0)}}.pcard.frozen .dot{background:var(--purple)}.pcard.offline .dot,.pcard.archived .dot,.pcard.stopped .dot{background:var(--bad)}.pcard-activity{margin:0;line-height:1.4;overflow:hidden;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;min-height:2.8em}.pcard-row{display:flex;justify-content:space-between;align-items:center;gap:10px;color:var(--muted);font-size:12px;min-width:0;font-variant-numeric:tabular-nums}.pcard-row span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0;flex:1}.pcard-row span:last-child:not(:only-child){flex:none}.pcard-reason{margin:0;font-size:12px;color:var(--idle);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.pcard-chart{width:64px;height:20px;background:#091626;border:1px solid #16283e;border-radius:5px;flex:none;display:block}.pcard-actions{display:flex;gap:6px;margin-top:2px}.pcard-actions button{margin:0;padding:4px 9px;font-size:12px}.pcard-hint{margin:0;color:var(--muted);font-size:11px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
</style>
<header><h1>OxiDex harness</h1><div id=summary>Loading…</div><div id=fleet-banner class=hidden></div></header><main><div id=filters></div><section id=cards></section></main><div id=notice class=hidden></div>
<script>const TOKEN=__TOKEN__,CONTROLS=__CONTROLS__;let hist={},cpuHist={},memHist={},paused=false,render=()=>{};const f=n=>n==null?'—':new Intl.NumberFormat().format(n),b=n=>n==null?'—':(n/1048576).toFixed(1)+' MiB',cpu=n=>n==null?'—':(n<1?n.toFixed(2):n.toFixed(1))+'%',e=s=>{let x=document.createElement('i');x.textContent=s??'';return x.innerHTML.replace(/"/g,'&quot;').replace(/'/g,'&#39;')},formatAge=s=>s<60?Math.round(s)+'s ago':s<3600?Math.round(s/60)+'m ago':s<86400?Math.round(s/3600)+'h ago':Math.round(s/86400)+'d ago',age=x=>{if(!x)return '—';return formatAge(Math.max(0,Date.now()/1000-new Date(x.timestamp||x).getTime()/1000))},epochAge=v=>typeof v!=='number'?'—':formatAge(Math.max(0,Date.now()/1000-v));async function poll(force){if(!force&&(paused||document.hidden))return;try{latest=await fetch('/api/status').then(r=>r.json());render(latest)}catch(x){notice.textContent='Dashboard request failed: '+x;notice.classList.remove('hidden')}}async function act(id,action,extra){let c=latest.components.find(x=>x.id===id)||{label:id,role:''},fc=latest.fleet_config||{},caps=fc.capabilities||{},msg;if(action==='start'){let mergers=caps.mergers_flag?(fc.configured_mergers?'first '+fc.configured_mergers+' squads':'all '+(fc.squad_total??'?')+' squads'):'all squads';msg='Start the fleet via launchd? Configured: '+(fc.configured_workers??'?')+' workers'+(fc.squad_mode?', squad-mode':'')+', mergers: '+mergers+'.'}else if(action==='scale'){let parts=[];if(extra&&extra.workers!=null)parts.push('workers '+(fc.configured_workers??'?')+' → '+extra.workers);if(extra&&extra.mergers!=null)parts.push('mergers '+(fc.configured_mergers||'all')+' → '+(extra.mergers===0?'all':extra.mergers));msg=fc.service_loaded?'Scale '+parts.join(', ')+'? This persists across restarts and restarts the fleet now (bootout + bootstrap). In-flight worker rounds are killed; tag claims may shadow for up to 2h.':'Save '+parts.join(', ')+' to the LaunchAgent? Takes effect on next start.'}else{msg=action+' '+c.label+'?'+(action==='restart'&&c.role!=='supervisor'?' This restarts the fleet to recreate dispatcher-owned work safely.':'')}if(!(extra&&extra.force)&&!confirm(msg))return;let r=await fetch('/api/control',{method:'POST',headers:{'content-type':'application/json','x-control-token':TOKEN},body:JSON.stringify({id,action,...(extra||{})})}),d=await r.json();notice.textContent=d.message||d.error;notice.classList.remove('hidden');setTimeout(()=>notice.classList.add('hidden'),6000);if(r.status===409&&action==='scale'&&!(extra&&extra.force)&&/mid-round/.test(d.error||'')&&confirm('Workers are mid-round. Apply anyway (loses in-flight work)?'))return act(id,'scale',{...extra,force:true});poll(true)}let latest;poll();setInterval(poll,1000);</script>"""
    return (body.replace("__TOKEN__", json.dumps(token)).replace("__CONTROLS__", "true" if controls else "false") + FLOW_SCRIPT).encode()


FLOW_SCRIPT = r"""
<style>
#flow-panel{margin:0 0 16px;padding:14px 16px;background:linear-gradient(180deg,#0f2036,var(--card) 120px);border:1px solid var(--edge);border-radius:10px}#flow-panel h2{font-size:16px;margin:0 0 5px}#flow-panel p{margin:0 0 8px;color:var(--muted)}#flow-host svg{display:block;width:100%;min-height:330px}#flow-host .edge{stroke:var(--muted);stroke-width:1.5;fill:none;color:var(--muted)}#flow-host .edge-label{fill:var(--muted);font:11px system-ui}#flow-host .node{fill:var(--bg);stroke:var(--edge);stroke-width:1.5;cursor:pointer}#flow-host .node:hover{stroke:var(--blue);stroke-width:2.5}#flow-host .node-title{fill:var(--txt);font:500 15px system-ui}#flow-host .node-headline{fill:var(--txt);font:12px system-ui}#flow-host .node-summary{fill:var(--muted);font:11px system-ui}#flow-host .node-action{fill:var(--blue);font:10px system-ui}#filters{display:flex;align-items:center;flex-wrap:wrap;gap:10px;margin:0 0 10px}#component-filter{min-width:min(360px,100%);padding:8px 10px;background:var(--bg);border:1px solid var(--edge);border-radius:8px;color:var(--txt);font:inherit;transition:border-color .15s ease}#component-filter:hover{border-color:#2c4a68}.show-dead-toggle{display:flex;align-items:center;gap:6px;color:var(--muted);white-space:nowrap;cursor:pointer}.show-dead-toggle input{cursor:pointer}.sort-control{display:flex;align-items:center;gap:6px;color:var(--muted);white-space:nowrap}.sort-control select{padding:7px 9px;background:var(--bg);border:1px solid var(--edge);border-radius:8px;color:var(--txt);font:inherit;cursor:pointer;transition:border-color .15s ease}.sort-control select:hover{border-color:#2c4a68}.status-running{color:var(--ok)}.status-idle{color:var(--idle)}dialog{box-sizing:border-box;width:min(1600px,98vw);max-width:98vw;max-height:90vh;overflow:auto;background:linear-gradient(180deg,#0f2036,var(--card) 260px);color:var(--txt);border:1px solid #2a466a;border-radius:12px;padding:18px 20px;box-shadow:0 24px 64px #02060cd9}dialog::backdrop{background:#04090fb0;backdrop-filter:blur(4px);-webkit-backdrop-filter:blur(4px)}dialog[open]{animation:dialog-in .2s cubic-bezier(.2,.9,.3,1)}dialog[open]::backdrop{animation:fade-in .2s ease}@keyframes dialog-in{from{opacity:0;transform:translateY(10px) scale(.985)}to{opacity:1;transform:none}}@keyframes fade-in{from{opacity:0}to{opacity:1}}dialog h2{margin:0 0 12px;font-size:19px;font-weight:650;letter-spacing:.01em}dialog h2 .role-icon{width:17px;height:17px;color:var(--blue)}dialog h3{margin:20px 0 10px;padding-top:14px;border-top:1px solid var(--edge);font-size:11px;font-weight:650;text-transform:uppercase;letter-spacing:.08em;color:var(--muted)}dialog table{border-collapse:collapse;font-size:13px;font-variant-numeric:tabular-nums}dialog th,dialog td{padding:6px 8px;text-align:left;vertical-align:top;overflow-wrap:anywhere;border-bottom:1px solid var(--edge)}dialog th{color:var(--muted);font-size:11px;text-transform:uppercase;letter-spacing:.06em}.flow-close{float:right}.flow-reason{max-width:420px;white-space:normal}.flow-guide{margin:0 0 16px;padding:12px;background:var(--bg);border:1px solid var(--edge);border-radius:8px}.flow-guide h3{margin:0 0 8px;padding-top:0;border-top:0;font-size:15px;font-weight:600;text-transform:none;letter-spacing:0;color:var(--txt)}.flow-guide svg{display:block;width:100%;min-width:720px;height:auto}.flow-guide-wrap{overflow:auto}.flow-guide .guide-node{fill:var(--card);stroke:var(--edge);stroke-width:1.5}.flow-guide .guide-node.focus{stroke:var(--blue);stroke-width:2.5}.flow-guide .guide-title{fill:var(--txt);font:600 12px system-ui}.flow-guide .guide-copy{fill:var(--muted);font:10px system-ui}.flow-guide .guide-edge{stroke:var(--muted);stroke-width:1.5;fill:none;color:var(--muted)}.flow-guide .guide-feedback{stroke:var(--blue);stroke-width:1.5;fill:none;color:var(--blue)}.flow-guide .guide-label{fill:var(--blue);font:10px system-ui}.flow-parts{display:grid;grid-template-columns:repeat(auto-fit,minmax(190px,1fr));gap:8px;margin-top:10px}.flow-part{padding:8px;border:1px solid var(--edge);border-radius:6px}.flow-part strong{display:block;margin-bottom:3px}.flow-part p{margin:0;color:var(--muted);font-size:12px}
</style>
<style>
.pcard.waiting{border-color:#3a5f88}.pcard.frozen{border-color:#6b4a94}.pcard.offline,.pcard.archived{opacity:.7}.status-waiting{color:var(--blue)}.status-frozen{color:#d481ff}.status-offline{color:#ff8e8e}.status-archived{color:var(--muted)}.status-stopped{color:#ff8e8e}.model-result-ok{color:var(--ok)}.model-result-error{color:#ff8e8e}.model-result-retry{color:var(--idle)}.model-result-unknown{color:var(--muted)}.flow-dialog-controls{display:flex;justify-content:flex-end;gap:8px;margin-bottom:12px}.flow-dialog-controls button[hidden]{display:none}.flow-specific{margin:0 0 16px;padding:12px;background:var(--bg);border:1px solid var(--edge);border-radius:8px}.flow-specific h3{margin:0 0 6px;padding-top:0;border-top:0;font-size:15px;font-weight:600;text-transform:none;letter-spacing:0;color:var(--txt)}.flow-specific p{margin:0 0 8px;color:var(--muted)}.flow-specific ul{margin:8px 0;padding-left:20px}.flow-specific li{margin:5px 0}.flow-state-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:8px;margin-top:10px}.flow-state{padding:7px;border:1px solid var(--edge);border-radius:6px}.flow-state strong{display:block}.flow-state span{color:var(--muted);font-size:12px}.flow-stat-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(170px,1fr));gap:8px}.flow-stat{padding:11px 12px;border:1px solid var(--edge);border-radius:8px;background:var(--bg)}.flow-stat-label{display:block;margin-bottom:5px}.flow-stat-value{font-size:17px;font-weight:650;overflow-wrap:anywhere;font-variant-numeric:tabular-nums}.flow-member-controls{display:flex;align-items:center;gap:10px;margin:0 0 9px}.flow-member-controls label{color:var(--muted)}#flow-member-filter{min-width:min(360px,100%);padding:7px 9px;background:var(--bg);border:1px solid var(--edge);border-radius:8px;color:var(--txt);font:inherit;transition:border-color .15s ease}#flow-member-filter:hover{border-color:#2c4a68}.flow-member-table-wrap{overflow-x:auto;overflow-y:auto;max-width:100%;border:1px solid var(--edge);border-radius:8px}.flow-member-hint{margin:0 0 9px;color:var(--muted);font-size:12px}.member-card-grid-host{display:grid;grid-template-columns:repeat(auto-fill,minmax(320px,1fr));gap:10px}.member-card{position:relative;overflow:hidden;background:linear-gradient(180deg,#0c1a2e,var(--bg) 58%);border:1px solid var(--edge);border-radius:10px;padding:11px 13px;display:flex;flex-direction:column;gap:6px;min-width:0;cursor:pointer;transition:transform .16s ease,box-shadow .16s ease,border-color .16s ease}.member-card:hover,.member-card:focus-visible{border-color:var(--blue);transform:translateY(-2px);box-shadow:0 8px 20px #04080fb3}.member-card:focus-visible{outline:2px solid var(--blue);outline-offset:1px}.member-card.running{border-color:#2c4a68}.member-card.offline,.member-card.archived{opacity:.7}.member-card .dot{background:var(--idle)}.member-card.running .dot{background:var(--ok);box-shadow:0 0 8px #37d99688}.member-card.frozen .dot{background:#d481ff}.member-card.offline .dot,.member-card.archived .dot{background:#ff8e8e}.member-card-grid{display:grid;grid-template-columns:max-content 1fr;gap:3px 10px;margin:0;font-size:12px;font-variant-numeric:tabular-nums}.member-card-grid dt{color:var(--muted)}.member-card-grid dd{margin:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0;border:0;padding:0}.member-card-reason{margin:0;font-size:12px;color:var(--idle);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.publisher-prs-note{margin:0 0 9px;color:var(--muted)}.publisher-prs-table{width:max-content;min-width:1180px;table-layout:auto}.publisher-prs-table th,.publisher-prs-table td{white-space:nowrap}.publisher-prs-table td:nth-child(4){white-space:normal;min-width:420px}.publisher-prs-table a{color:var(--blue)}
.detail-charts{display:grid;grid-template-columns:repeat(auto-fit,minmax(280px,1fr));gap:10px;margin:0 0 4px}.detail-chart{padding:10px;border:1px solid var(--edge);border-radius:7px;background:var(--bg);min-width:0}.detail-chart .chart-title{display:block;margin-bottom:6px}.detail-chart canvas{display:block;width:100%;height:130px}.segment-bar{display:block;width:100%;height:16px;border-radius:5px;overflow:hidden;background:#091626;margin:4px 0 7px}.chart-legend{display:flex;flex-wrap:wrap;gap:12px;color:var(--muted);font-size:12px;margin:0 0 4px;font-variant-numeric:tabular-nums}.chart-legend-item{display:inline-flex;align-items:center;gap:5px}.chart-swatch{width:10px;height:10px;border-radius:3px;display:inline-block;flex:none}.chart-note{margin:2px 0 0;color:var(--muted);font-size:11px}.flow-chart-block{margin:0 0 14px;padding:12px;background:var(--bg);border:1px solid var(--edge);border-radius:8px}.flow-chart-block h3{margin:0 0 8px;padding-top:0;border-top:0;font-size:14px;font-weight:600;text-transform:none;letter-spacing:0;color:var(--txt)}.rank-chart{display:block;width:100%;height:auto}.rank-chart text{font:11px system-ui}
#fleet-strip{display:grid;grid-template-columns:repeat(auto-fit,minmax(148px,1fr));gap:8px;margin:0 0 12px}.fleet-tile{position:relative;min-width:0;display:flex;flex-direction:column;gap:3px;padding:8px 10px;border:1px solid var(--edge);border-radius:8px;background:var(--bg);font-variant-numeric:tabular-nums}.fleet-tile-label{color:var(--muted);font-size:10.5px;text-transform:uppercase;letter-spacing:.07em;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.fleet-tile-value{font-size:16px;font-weight:650;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-height:1.35em}.fleet-tile-value a{color:var(--blue)}.fleet-tile-spark{display:block;width:100%;height:18px;margin-top:2px}.fleet-tile.clickable{cursor:pointer;transition:border-color .16s ease,transform .16s ease,box-shadow .16s ease}.fleet-tile.clickable:hover,.fleet-tile.clickable:focus-visible{border-color:var(--blue);transform:translateY(-1px);box-shadow:0 6px 16px #04080f8c}.fleet-tile.tile-active{border-color:var(--blue);box-shadow:0 0 0 1px var(--blue) inset}
.tag-link{color:var(--blue);cursor:pointer;text-decoration:underline dotted;text-underline-offset:2px}.tag-link:hover{text-decoration:underline}.pcard-role[data-role-filter]{cursor:pointer}.pcard-role[data-role-filter]:hover{color:var(--blue);text-decoration:underline}.chip[data-status-filter]{cursor:pointer}.chip[data-status-filter]:hover{border-color:var(--blue)}.chip-active{border-color:var(--blue)!important;box-shadow:0 0 0 1px var(--blue) inset}.active-filters{display:inline-flex;flex-wrap:wrap;gap:6px}button.chip{font-size:12px}.copy-btn{margin-left:6px;padding:3px 6px;line-height:0;vertical-align:middle;border-radius:6px}.copy-btn svg{width:12px;height:12px;display:inline-block}
</style>
<script>
(() => {
  let query = '', sortKey = localStorage.getItem('oxidex-sort-key') || 'name', sortDirection = Number(localStorage.getItem('oxidex-sort-direction')) || 1, showDead = localStorage.getItem('oxidex-show-dead') === '1', gridReady = false;
  const rows = new Map();
  let roleFilter = null, statusFilter = null, tagFilter = null, toastTimer = null;
  const absTime = value => { if (value == null) return ''; const ms = typeof value === 'number' ? value * 1000 : new Date(value.timestamp || value).getTime(); return Number.isFinite(ms) ? new Date(ms).toLocaleString() : ''; };
  function toast(message) { notice.textContent = message; notice.classList.remove('hidden'); clearTimeout(toastTimer); toastTimer = setTimeout(() => notice.classList.add('hidden'), 1600); }
  function copyText(value) { if (!navigator.clipboard) { toast('Clipboard unavailable'); return; } navigator.clipboard.writeText(value).then(() => toast('Copied'), () => toast('Copy failed')); }
  const COPY_ICON = '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5.5" y="5.5" width="8" height="8" rx="1.5"/><path d="M2.5 10.5v-7a1 1 0 0 1 1-1h7"/></svg>';
  const copyButton = value => `<button type="button" class="copy-btn" data-copy="${e(String(value))}" title="Copy to clipboard" aria-label="Copy to clipboard">${COPY_ICON}</button>`;
  const tagLinkHtml = component => { const tags = component.current_tag?.tags || (component.current_tag?.tag ? [component.current_tag.tag] : []); return tags.length ? `<span class="tag-link" data-tag-filter="${e(tags[0])}" title="Click to filter to this tag">${e(tags[0])}</span>${tags.length > 1 ? ` +${tags.length - 1}` : ''}` : ''; };
  function renderActiveFilters() {
    const host = document.getElementById('active-filters');
    if (!host) return;
    const chips = [];
    if (roleFilter) chips.push(`<button type="button" class="chip chip-filter" data-clear-filter="role" title="Clear the role filter">role: ${e(roleFilter)} ×</button>`);
    if (statusFilter) chips.push(`<button type="button" class="chip chip-filter" data-clear-filter="status" title="Clear the status filter">status: ${e(statusFilter)} ×</button>`);
    if (tagFilter) chips.push(`<button type="button" class="chip chip-filter" data-clear-filter="tag" title="Clear the tag filter">tag: ${e(tagFilter)} ×</button>`);
    if (chips.length > 1) chips.push('<button type="button" class="chip chip-filter" data-clear-filter="all" title="Clear every chip filter">clear all</button>');
    host.innerHTML = chips.join('');
  }
  function setRoleFilter(role) { roleFilter = roleFilter === role ? null : role; renderActiveFilters(); renderTable(latest); if (latest) updateFleetStrip(latest); }
  function setStatusFilter(status) { statusFilter = statusFilter === status ? null : status; renderActiveFilters(); renderTable(latest); }
  function setTagFilter(tag) { tagFilter = tag || null; renderActiveFilters(); renderTable(latest); }
  document.addEventListener('keydown', event => {
    const target = event.target;
    if (event.key === '/' && !event.ctrlKey && !event.metaKey && !event.altKey) {
      if (target && ((typeof target.matches === 'function' && target.matches('input,textarea,select')) || target.isContentEditable)) return;
      const openDialog = document.querySelector('dialog[open]');
      const input = (openDialog && openDialog.querySelector('#flow-member-filter')) || document.getElementById('component-filter');
      if (input) { event.preventDefault(); input.focus(); input.select(); }
      return;
    }
    if (event.key !== 'Escape' || !target) return;
    if (target.id === 'flow-member-filter' && target.value) { event.preventDefault(); target.value = ''; target.dispatchEvent(new Event('input')); return; }
    if (target.id === 'component-filter') {
      event.preventDefault();
      if (target.value) { target.value = ''; query = ''; renderTable(latest); }
      else if (roleFilter || statusFilter || tagFilter) { roleFilter = statusFilter = tagFilter = null; renderActiveFilters(); renderTable(latest); }
      else target.blur();
    }
  });
  document.addEventListener('visibilitychange', () => { if (!document.hidden && !paused) poll(true); else if (latest) renderTable(latest); });
  const SORT_OPTIONS = [['name','Name'],['type','Role'],['status','Status'],['activity','Doing now'],['cpu','CPU'],['memory','Memory'],['uptime','Alive'],['lifetime','Stats'],['tag','Current tag'],['reason','Last rejection'],['last','Last request'],['model','Last model result'],['worktree','Worktree'],['behind','Behind origin/main']];
  const isDead = component => component.status === 'offline' || component.status === 'archived';
  // Tiny hand-drawn role glyphs (stroke currentColor, no external assets).
  const ROLE_ICONS = {
    supervisor: '<path d="M8 1.7L13.3 3.7V7.6C13.3 11 11.1 13.6 8 14.6C4.9 13.6 2.7 11 2.7 7.6V3.7Z"/><path d="M5.9 8.2L7.4 9.7L10.2 6.6"/>',
    dispatcher: '<path d="M5.6 4.2H13.6M5.6 8H13.6M5.6 11.8H13.6"/><circle cx="2.7" cy="4.2" r="0.9" fill="currentColor" stroke="none"/><circle cx="2.7" cy="8" r="0.9" fill="currentColor" stroke="none"/><circle cx="2.7" cy="11.8" r="0.9" fill="currentColor" stroke="none"/>',
    worker: '<path d="M13.9 4.1A3.8 3.8 0 0 1 9.2 8.9L4.7 13.4A1.55 1.55 0 0 1 2.5 11.2L7 6.7A3.8 3.8 0 0 1 11.8 2L9.6 4.2L11.7 6.3L13.9 4.1Z"/>',
    reviewer: '<path d="M1.6 8C3.5 4.7 5.9 3.1 8 3.1C10.1 3.1 12.5 4.7 14.4 8C12.5 11.3 10.1 12.9 8 12.9C5.9 12.9 3.5 11.3 1.6 8Z"/><circle cx="8" cy="8" r="2.1"/>',
    merger: '<circle cx="4.2" cy="3.6" r="1.7"/><circle cx="4.2" cy="12.4" r="1.7"/><circle cx="12" cy="8" r="1.7"/><path d="M4.2 5.3V10.7"/><path d="M4.2 6.2C4.6 7.9 6.6 8 10.2 8"/>',
    publisher: '<path d="M8 1.6C10.4 3.3 11.4 6.2 10.7 9.3L8 11.9L5.3 9.3C4.6 6.2 5.6 3.3 8 1.6Z"/><circle cx="8" cy="6.1" r="1.2"/><path d="M6.1 12.4L5.4 14.3M9.9 12.4L10.6 14.3"/>',
  };
  const FLOW_NODE_ROLES = {supervisor:'supervisor', dispatcher:'dispatcher', workers:'worker', reviewers:'reviewer', mergers:'merger', publisher:'publisher'};
  const roleIcon = role => ROLE_ICONS[role] ? `<svg class="role-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ROLE_ICONS[role]}</svg>` : '';
  function activityText(component) {
    const status = component.status, role = component.role, task = component.last_task || {}, metrics = component.metrics || {}, process = component.process || {};
    const tags = component.current_tag?.tags || (component.current_tag?.tag ? [component.current_tag.tag] : []);
    // A tag claim can lapse (heartbeat window, or a round between claims)
    // while the process is still visibly busy -- CPU is ground truth for
    // "is it doing something," a stale/missing claim is not proof of idle.
    const busyCommand = (process.cpu_percent || 0) > 15 ? (processName(process) || 'a build step') : null;
    if (status === 'archived') return 'Archived — worktree kept, no recorded activity';
    if (status === 'offline') return 'Not running';
    if (status === 'frozen') return metrics.freeze_reason || 'Stalled — no progress recorded';
    if (role === 'worker') {
      if (tags.length) return `Fixing ${tags[0]}${tags.length > 1 ? ` (+${tags.length - 1} clustered)` : ''}`;
      if (busyCommand) return `Building or testing (running ${busyCommand}, ${cpu(process.cpu_percent)} CPU) — no live tag claim recorded`;
      return task.phase ? `Idle — last attempted a fix ${age(task)}` : 'Idle — no tag claimed yet';
    }
    if (role === 'reviewer') {
      if (tags.length) return `Reviewing the patch for ${tags[0]}`;
      if (busyCommand) return `Building or testing (running ${busyCommand}, ${cpu(process.cpu_percent)} CPU) — no live tag claim recorded`;
      return task.phase ? `Idle — last reviewed ${age(task)}` : 'Idle';
    }
    if (role === 'merger') return status === 'running' ? 'Merging approved squad commits' : 'Waiting for approved commits';
    if (role === 'dispatcher') return status === 'running' ? 'Assigning work to workers' : 'Not running';
    if (role === 'supervisor') return status === 'running' ? 'Watching fleet health' : 'Not running';
    if (role === 'publisher') return status === 'running' ? 'Opening or merging ready pull requests' : status === 'waiting' ? 'Waiting for the next dispatcher round' : 'Not running';
    return status === 'running' ? 'Running' : '—';
  }
  function chart(canvas, id, sample) {
    const values = hist[id] || (hist[id] = []);
    values.push(sample || 0); if (values.length > 60) values.shift();
    const width = Math.round((canvas.clientWidth || 64) * devicePixelRatio), height = Math.round((canvas.clientHeight || 20) * devicePixelRatio);
    if (canvas.width !== width || canvas.height !== height) { canvas.width = width; canvas.height = height; }
    const context = canvas.getContext('2d'); context.clearRect(0, 0, width, height); context.strokeStyle = '#62b0ff'; context.lineWidth = 2 * devicePixelRatio; context.beginPath();
    values.forEach((sampleValue, index) => { const x = index * width / 59, y = height - Math.min(sampleValue, 100) * height / 100; index ? context.lineTo(x, y) : context.moveTo(x, y); }); context.stroke();
  }
  // Larger CPU/memory histories for the detail-dialog charts: one sample per
  // poll, bounded, and pruned together with the sparkline history whenever a
  // component disappears from the snapshot (mirrors the rows Map cleanup).
  const HISTORY_LIMIT = 120;
  function sampleHistories(data) {
    const live = new Set();
    for (const component of data.components) {
      live.add(component.id);
      const process = component.process || {};
      const cpuValues = cpuHist[component.id] || (cpuHist[component.id] = []);
      const memValues = memHist[component.id] || (memHist[component.id] = []);
      cpuValues.push(process.cpu_percent || 0); if (cpuValues.length > HISTORY_LIMIT) cpuValues.shift();
      memValues.push(process.memory_bytes || 0); if (memValues.length > HISTORY_LIMIT) memValues.shift();
    }
    for (const id of Object.keys(cpuHist)) if (!live.has(id)) { delete cpuHist[id]; delete memHist[id]; delete hist[id]; }
    const summaryData = data.summary || {};
    const fleetSamples = {workers: summaryData.active_workers || 0, cpu: summaryData.total_cpu_percent || 0, memory: summaryData.total_memory_bytes || 0, calls: summaryData.model_calls_last_hour || 0, patches: summaryData.patches_last_hour || 0, rejections: summaryData.review_rejections_last_hour || 0, queue: summaryData.judgment_depth || 0, merged: summaryData.prs_merged_today || 0};
    for (const key of Object.keys(fleetSamples)) { const series = fleetHist[key] || (fleetHist[key] = []); series.push(fleetSamples[key]); if (series.length > FLEET_HISTORY_LIMIT) series.shift(); }
  }
  // Fleet overview strip: one rolling 60-sample series per tile (same bound
  // as the per-card hist sparklines), sampled once per poll above.
  const FLEET_HISTORY_LIMIT = 60;
  const fleetHist = {};
  const FLEET_TILES = [
    {key:'workers', label:'workers', color:'#37d996', action:'filter-workers', title:'Running fixer workers of all known workers. Click to toggle the worker role filter.'},
    {key:'cpu', label:'fleet CPU', color:'#62b0ff', title:'CPU summed across unique harness processes (deduped by PID).'},
    {key:'memory', label:'fleet memory', color:'#37d996', title:'Memory summed across unique harness processes (deduped by PID).'},
    {key:'calls', label:'model calls/h', color:'#62b0ff', action:'node-api', title:'Model API calls in the last hour. Click for the Model API detail.'},
    {key:'patches', label:'patches/h', color:'#62b0ff', action:'node-workers', title:'Diffs generated in the last hour. Click for the fixer workers detail.'},
    {key:'rejections', label:'rejections/h', color:'#f4bd4f', action:'node-reviewers', title:'Review rejections in the last hour, from the lesson ledger (the real reviewer verdict). Click for the review gates detail.'},
    {key:'queue', label:'judgment queue', color:'#d481ff', action:'node-queue', title:'Advisory judgments queued now. Click to open the judgment queue.'},
    {key:'merged', label:'PRs merged today', color:'#37d996', action:'node-publisher', title:'PRs recorded merged since local midnight. Click for the publish sweep detail.'},
    {key:'lastpr', label:'latest PR', spark:false, action:'node-publisher', title:'Most recent PR. Click for the publish sweep detail; the number links to GitHub when known.'},
  ];
  function stripAction(action) {
    if (action === 'filter-workers') { setRoleFilter('worker'); return; }
    if (action && action.indexOf('node-') === 0) showFlowNode(action.slice(5));
  }
  function ensureFleetStrip() {
    let strip = document.getElementById('fleet-strip');
    if (strip) return strip;
    strip = document.createElement('section');
    strip.id = 'fleet-strip';
    strip.setAttribute('aria-label', 'Fleet overview');
    strip.innerHTML = FLEET_TILES.map(tile => `<div class="fleet-tile${tile.action ? ' clickable' : ''}" data-tile="${tile.key}"${tile.action ? ` role="button" tabindex="0" data-strip-action="${tile.action}"` : ''} title="${e(tile.title)}"><span class="fleet-tile-label">${e(tile.label)}</span><span class="fleet-tile-value" data-fleet-value="${tile.key}">—</span>${tile.spark === false ? '' : `<canvas class="fleet-tile-spark" data-fleet-spark="${tile.key}"></canvas>`}</div>`).join('');
    const run = target => { const el = target.closest('[data-strip-action]'); if (el) stripAction(el.dataset.stripAction); };
    strip.addEventListener('click', event => { if (event.target.closest('a')) return; run(event.target); });
    strip.addEventListener('keydown', event => { if ((event.key === 'Enter' || event.key === ' ') && event.target.closest('[data-strip-action]')) { event.preventDefault(); run(event.target); } });
    const filtersEl = document.getElementById('filters');
    filtersEl.parentNode.insertBefore(strip, filtersEl);
    return strip;
  }
  function fleetSpark(canvas, values, color) {
    const ratio = devicePixelRatio || 1;
    const width = Math.round((canvas.clientWidth || 120) * ratio), height = Math.round((canvas.clientHeight || 18) * ratio);
    if (canvas.width !== width || canvas.height !== height) { canvas.width = width; canvas.height = height; }
    const context = canvas.getContext('2d');
    context.clearRect(0, 0, width, height);
    if (values.length < 2) return;
    const peak = Math.max(...values, 1), pad = 1.5 * ratio;
    context.strokeStyle = color; context.lineWidth = 1.5 * ratio; context.beginPath();
    values.forEach((sample, index) => {
      const x = index * width / (FLEET_HISTORY_LIMIT - 1);
      const y = height - pad - Math.min(sample / peak, 1) * (height - 2 * pad);
      index ? context.lineTo(x, y) : context.moveTo(x, y);
    });
    context.stroke();
  }
  function latestPrHtml(data) {
    const detail = data.flow?.nodes?.publisher?.detail || {};
    const pr = detail.last_pr;
    if (!pr || pr.number == null) return '—';
    const url = pr.url || ((detail.recent_prs || []).find(entry => entry && String(entry.number) === String(pr.number)) || {}).url;
    const label = `#${e(String(pr.number))}`;
    const when = pr.timestamp ? ` · ${e(age({timestamp: pr.timestamp}))}` : '';
    return (url ? `<a href="${e(url)}" target="_blank" rel="noreferrer" title="${e(pr.title || '')}">${label}</a>` : label) + when;
  }
  function updateFleetStrip(data) {
    const strip = ensureFleetStrip(), s = data.summary || {};
    const totalWorkers = data.components.filter(component => component.role === 'worker').length;
    const values = {
      workers: `${f(s.active_workers)}/${f(totalWorkers)} active`,
      cpu: cpu(s.total_cpu_percent),
      memory: b(s.total_memory_bytes),
      calls: f(s.model_calls_last_hour),
      patches: f(s.patches_last_hour),
      rejections: f(s.review_rejections_last_hour),
      queue: f(s.judgment_depth),
      merged: f(s.prs_merged_today),
      lastpr: latestPrHtml(data),
    };
    for (const key of Object.keys(values)) { const el = strip.querySelector(`[data-fleet-value="${key}"]`); if (el && el.innerHTML !== values[key]) el.innerHTML = values[key]; }
    const workersTile = strip.querySelector('[data-tile="workers"]');
    if (workersTile) { workersTile.classList.toggle('tile-active', roleFilter === 'worker'); workersTile.setAttribute('aria-pressed', String(roleFilter === 'worker')); }
    for (const tile of FLEET_TILES) {
      if (tile.spark === false) continue;
      const canvas = strip.querySelector(`[data-fleet-spark="${tile.key}"]`);
      if (canvas) fleetSpark(canvas, fleetHist[tile.key] || [], tile.color);
    }
  }
  function drawSeriesChart(canvas, values, options) {
    const ratio = devicePixelRatio || 1;
    const width = Math.round((canvas.clientWidth || 600) * ratio), height = Math.round((canvas.clientHeight || 130) * ratio);
    if (canvas.width !== width || canvas.height !== height) { canvas.width = width; canvas.height = height; }
    const context = canvas.getContext('2d');
    context.clearRect(0, 0, width, height);
    const top = 16 * ratio, bottom = height - 4 * ratio, plotHeight = bottom - top;
    const peak = Math.max(options.floor || 1, ...values);
    context.strokeStyle = '#203652'; context.lineWidth = 1;
    [0, 0.25, 0.5, 0.75, 1].forEach(fraction => { const y = bottom - plotHeight * fraction; context.beginPath(); context.moveTo(0, y); context.lineTo(width, y); context.stroke(); });
    const step = width / (HISTORY_LIMIT - 1);
    const pointX = index => width - (values.length - 1 - index) * step;
    const pointY = sampleValue => bottom - Math.min(sampleValue / peak, 1) * plotHeight;
    if (values.length > 1) {
      context.beginPath();
      values.forEach((sampleValue, index) => { const x = pointX(index), y = pointY(sampleValue); index ? context.lineTo(x, y) : context.moveTo(x, y); });
      context.strokeStyle = options.stroke; context.lineWidth = 2 * ratio; context.stroke();
      context.lineTo(pointX(values.length - 1), bottom); context.lineTo(pointX(0), bottom); context.closePath();
      context.fillStyle = options.fill; context.fill();
    }
    if (values.length) {
      context.beginPath(); context.arc(pointX(values.length - 1), pointY(values[values.length - 1]), 2.5 * ratio, 0, 2 * Math.PI);
      context.fillStyle = options.stroke; context.fill();
    }
    context.font = `${11 * ratio}px system-ui`;
    context.fillStyle = '#95a8bf'; context.textAlign = 'left';
    context.fillText(options.format(peak), 4 * ratio, 12 * ratio);
    context.fillText('0', 4 * ratio, bottom - 3 * ratio);
    if (values.length) { context.textAlign = 'right'; context.fillStyle = options.stroke; context.fillText('now ' + options.format(values[values.length - 1]), width - 4 * ratio, 12 * ratio); }
  }
  function segmentBar(segments, note) {
    const total = segments.reduce((sum, segment) => sum + segment.value, 0);
    if (!total) return '<p class="muted">Nothing recorded yet.</p>';
    let offset = 0;
    const parts = segments.filter(segment => segment.value > 0).map(segment => {
      const span = 100 * segment.value / total;
      const piece = `<rect x="${offset.toFixed(2)}" y="0" width="${span.toFixed(2)}" height="14" fill="${segment.color}"/>`;
      offset += span;
      return piece;
    }).join('');
    const legend = segments.map(segment => `<span class="chart-legend-item"><span class="chart-swatch" style="background:${segment.color}"></span>${e(segment.label)}: ${f(segment.value)}</span>`).join('');
    return `<svg class="segment-bar" viewBox="0 0 100 14" preserveAspectRatio="none" role="img">${parts}</svg><div class="chart-legend">${legend}</div>${note ? `<p class="chart-note">${e(note)}</p>` : ''}`;
  }
  const STATUS_COLORS = {running:'#37d996', waiting:'#62b0ff', frozen:'#d481ff', offline:'#ff8e8e', archived:'#95a8bf'};
  function workerRankChart(node) {
    const entries = (node.members || []).map(member => ({label: member.label, status: member.status, patches: (member.metrics || {}).patches_sent ?? (member.metrics || {}).patches_found ?? 0})).sort((left, right) => right.patches - left.patches).slice(0, 15);
    if (!entries.length) return '<p class="muted">No members recorded yet.</p>';
    const peak = Math.max(...entries.map(entry => entry.patches), 1);
    const width = 720, labelWidth = 172, valueSpace = 54, rowHeight = 24, chartLeft = labelWidth + 8, barSpan = width - chartLeft - valueSpace;
    const height = entries.length * rowHeight + 6;
    const rows = entries.map((entry, index) => {
      const y = index * rowHeight + 4, barLength = Math.max(entry.patches / peak * barSpan, entry.patches ? 2 : 0);
      const color = STATUS_COLORS[entry.status] || '#95a8bf';
      const inside = barLength > 64;
      const valueX = inside ? chartLeft + barLength - 6 : chartLeft + barLength + 6;
      const shortLabel = entry.label.length > 26 ? entry.label.slice(0, 25) + '…' : entry.label;
      return `<g><title>${e(entry.label)}: ${f(entry.patches)} patches sent (${e(entry.status)})</title><text x="${labelWidth}" y="${y + 14}" text-anchor="end" fill="var(--txt)">${e(shortLabel)}</text><rect x="${chartLeft}" y="${y + 2}" width="${barLength.toFixed(1)}" height="${rowHeight - 8}" rx="3" fill="${color}" fill-opacity="0.75"/><text x="${valueX.toFixed(1)}" y="${y + 14}" text-anchor="${inside ? 'end' : 'start'}" fill="${inside ? '#07111f' : 'var(--muted)'}">${f(entry.patches)}</text></g>`;
    }).join('');
    const statuses = [...new Set(entries.map(entry => entry.status))];
    const legend = statuses.map(status => `<span class="chart-legend-item"><span class="chart-swatch" style="background:${STATUS_COLORS[status] || '#95a8bf'}"></span>${e(status)}</span>`).join('');
    return `<svg class="rank-chart" viewBox="0 0 ${width} ${height}" role="img" aria-label="Workers ranked by patches sent">${rows}</svg><div class="chart-legend">${legend}</div>`;
  }
  function rankChartSection(node, key) {
    if (key !== 'workers' && key !== 'reviewers') return '';
    return `<section class="flow-chart-block"><h3>${key === 'workers' ? 'Workers' : 'Gates'} ranked by patches sent (top 15, colored by status)</h3><div id="flow-rank-chart">${workerRankChart(node)}</div></section>`;
  }
  function patchOutcomeSection(component) {
    if (component.role !== 'worker' && component.role !== 'reviewer') return '';
    const metrics = component.metrics || {};
    const bar = segmentBar([
      {label:'git-applied', value: metrics.patches_applied || 0, color:'#62b0ff'},
      {label:'apply failed', value: metrics.patches_apply_failed || 0, color:'#ff8e8e'},
      {label:'review-rejected', value: metrics.review_rejected || 0, color:'#f4bd4f'},
    ], 'git-applied only means the diff applied to the tree, not accepted; review-rejected is the reviewer verdict and overlaps git-applied.');
    return `<h3>Patch outcomes</h3>${bar}`;
  }
  function httpMixSection(component) {
    const metrics = component.metrics || {};
    const total = (metrics.recent_http_2xx || 0) + (metrics.recent_http_4xx || 0) + (metrics.recent_http_5xx || 0);
    if (!total) return '';
    const bar = segmentBar([
      {label:'HTTP 2xx', value: metrics.recent_http_2xx || 0, color:'#37d996'},
      {label:'HTTP 4xx', value: metrics.recent_http_4xx || 0, color:'#f4bd4f'},
      {label:'HTTP 5xx', value: metrics.recent_http_5xx || 0, color:'#ff8e8e'},
    ], 'Distribution over the recent model calls that recorded an HTTP status (bounded window).');
    return `<h3>Recent HTTP status mix</h3>${bar}`;
  }
  const value = (component, key) => {
    const process = component.process || {}, metrics = component.metrics || {}, task = component.last_task || {}, worktree = component.worktree || {};
    const lifetime = component.role === 'worker' ? metrics.patches_found : component.role === 'reviewer' ? metrics.patches_applied : component.role === 'publisher' ? metrics.prs_made : 0;
    return {type:component.role, name:component.label, status:component.status, activity:activityText(component), pid:component.pid || 0, uptime:process.elapsed_seconds || 0, process:activeCommand(process), cpu:process.cpu_percent || 0, memory:process.memory_bytes || 0, lifetime:lifetime || 0, tag:currentTag(component), reason:component.last_reason?.epoch || 0, last:task.epoch || 0, model:Number.isInteger(task.http_status) ? task.http_status : -1, worktree:worktree.path || '', behind:Number.isInteger(worktree.behind) ? worktree.behind : -1}[key] ?? '';
  };
  const activeCommand = process => process?.active_command || process?.command || '';
  const processName = process => process?.display_name || (activeCommand(process).trim().split(/\s+/)[0] || '—').split('/').pop();
  const processText = process => processName(process);
  const pidText = component => component.pid == null ? '—' : `${component.pid}${component.pid_note ? ` · ${component.pid_note}` : ''}`;
  const uptimeText = component => (component.process || {}).elapsed || '—';
  const lifetime = component => {
    const metrics = component.metrics || {};
    if (component.status === 'frozen') return metrics.freeze_reason || 'Potentially frozen: inspect PID, CPU, and last activity.';
    if (component.role === 'worker') return `sent ${f(metrics.patches_found)} · git-applied ${f(metrics.patches_applied)} · review-rejected ${f(metrics.review_rejected)} · ${f(metrics.patches_last_hour)} last hr`;
    if (component.role === 'reviewer') return `sent ${f(metrics.patches_sent)} · git-applied ${f(metrics.patches_applied)} · apply failed ${f(metrics.patches_apply_failed)} · review-rejected ${f(metrics.review_rejected)}`;
    if (component.role === 'publisher') return `PRs ${f(metrics.prs_made)}`;
    if (component.role === 'merger') return `${f(metrics.batch_commits)} unbatched${metrics.batch_blocked ? ' · batch blocked' : ''}${metrics.heartbeat_ts ? ` · last heartbeat ${epochAge(metrics.heartbeat_ts)}` : ''}`;
    if (component.role === 'dispatcher') return `${f(metrics.events)} dispatch events`;
    if (component.role === 'supervisor') return metrics.recorded_state ? `recorded state: ${metrics.recorded_state}` : '—';
    return '—';
  };
  const reasonText = component => {
    const reason = component.last_reason;
    if (!reason) return '—';
    const label = reason.event === 'critique' ? 'Critique' : 'Review rejected';
    return `${label} (${age(reason)}): ${reason.reason}`;
  };
  const currentTag = component => {
    const tags = component.current_tag?.tags || (component.current_tag?.tag ? [component.current_tag.tag] : []);
    return !tags.length ? '—' : `${tags[0]}${tags.length > 1 ? ` +${tags.length - 1}` : ''}`;
  };
  const currentTagTitle = component => (component.current_tag?.tags || []).join('\n');
  const lastFound = component => component.last_task ? `${component.last_task.phase} · ${age(component.last_task)}` : '—';
  const modelResult = component => {
    const task = component.last_task || {}, status = task.http_status;
    if (Number.isInteger(status)) return `${status}${status >= 200 && status < 400 ? ' OK' : task.outcome === 'RETRY' ? ' retrying' : ' error'}`;
    if (!task.phase) return '—';
    return task.outcome === 'OK' ? 'OK · code not recorded' : task.outcome === 'RETRY' ? 'retrying · code not recorded' : 'error · code not recorded';
  };
  const modelResultClass = component => {
    const task = component.last_task || {}, status = task.http_status;
    if (Number.isInteger(status)) return status >= 200 && status < 400 ? 'model-result-ok' : status >= 400 ? 'model-result-error' : 'model-result-retry';
    return task.outcome === 'RETRY' ? 'model-result-retry' : 'model-result-unknown';
  };
  const match = component => {
    if (roleFilter && component.role !== roleFilter) return false;
    if (statusFilter && component.status !== statusFilter) return false;
    if (tagFilter) { const tags = component.current_tag?.tags || (component.current_tag?.tag ? [component.current_tag.tag] : []); if (!tags.includes(tagFilter)) return false; }
    if (!showDead && isDead(component) && statusFilter !== component.status) return false;
    if (!query) return true;
    const process = component.process || {}, worktree = component.worktree || {};
    return [component.role, component.label, component.status, component.pid_note, process.pid, process.command, process.active_command, currentTag(component), currentTagTitle(component), worktree.path, modelResult(component), activityText(component), reasonText(component)].join(' ').toLowerCase().includes(query);
  };
  function updateSortButton() {
    const button = document.getElementById('sort-direction');
    if (button) button.textContent = sortDirection > 0 ? '↑ Ascending' : '↓ Descending';
  }
  function ensureGrid() {
    if (gridReady) return;
    gridReady = true;
    filters.innerHTML = `<label for="component-filter">Filter components</label><input id="component-filter" type="search" placeholder="PDF, reviewer, canon-13…" autocomplete="off" title="Press / to focus; Escape clears"><label class="show-dead-toggle"><input type="checkbox" id="show-dead"> Show dead workers</label><label class="sort-control" for="sort-key">Sort by<select id="sort-key">${SORT_OPTIONS.map(([key,label]) => `<option value="${key}">${e(label)}</option>`).join('')}</select></label><button type="button" id="sort-direction"></button><button type="button" id="pause-toggle" title="Pause or resume the 1-second live refresh">Pause</button><span id="active-filters" class="active-filters"></span>`;
    document.getElementById('component-filter').addEventListener('input', event => { query = event.target.value.trim().toLowerCase(); renderTable(latest); });
    const showDeadBox = document.getElementById('show-dead');
    showDeadBox.checked = showDead;
    showDeadBox.addEventListener('change', event => { showDead = event.target.checked; try { localStorage.setItem('oxidex-show-dead', showDead ? '1' : '0'); } catch (_) {} renderTable(latest); });
    const sortSelect = document.getElementById('sort-key');
    sortSelect.value = SORT_OPTIONS.some(([key]) => key === sortKey) ? sortKey : 'name';
    sortSelect.addEventListener('change', event => { sortKey = event.target.value; try { localStorage.setItem('oxidex-sort-key', sortKey); } catch (_) {} renderTable(latest); });
    document.getElementById('sort-direction').addEventListener('click', () => { sortDirection = -sortDirection; try { localStorage.setItem('oxidex-sort-direction', String(sortDirection)); } catch (_) {} updateSortButton(); renderTable(latest); });
    const pauseButton = document.getElementById('pause-toggle');
    pauseButton.addEventListener('click', () => { paused = !paused; pauseButton.textContent = paused ? 'Resume' : 'Pause'; if (latest) renderTable(latest); if (!paused) poll(true); });
    document.getElementById('active-filters').addEventListener('click', event => {
      const el = event.target.closest('[data-clear-filter]');
      if (!el) return;
      const kind = el.dataset.clearFilter;
      if (kind === 'role' || kind === 'all') roleFilter = null;
      if (kind === 'status' || kind === 'all') statusFilter = null;
      if (kind === 'tag' || kind === 'all') tagFilter = null;
      renderActiveFilters(); renderTable(latest);
    });
    const summaryHost = document.getElementById('summary');
    const summaryChipAction = target => { const el = target.closest('[data-status-filter]'); if (el) setStatusFilter(el.dataset.statusFilter); };
    summaryHost.addEventListener('click', event => summaryChipAction(event.target));
    summaryHost.addEventListener('keydown', event => { if ((event.key === 'Enter' || event.key === ' ') && event.target.closest('[data-status-filter]')) { event.preventDefault(); summaryChipAction(event.target); } });
    cards.addEventListener('click', event => {
      const roleEl = event.target.closest('[data-role-filter]');
      if (roleEl) { setRoleFilter(roleEl.dataset.roleFilter); return; }
      const tagEl = event.target.closest('[data-tag-filter]');
      if (tagEl) setTagFilter(tagEl.dataset.tagFilter);
    });
    updateSortButton();
  }
  function field(card, name) { return card.querySelector(`[data-field="${name}"]`); }
  function createCard(component) {
    const card = document.createElement('article');
    card.dataset.id = component.id;
    card.tabIndex = 0;
    card.title = 'Click for full details and history';
    card.innerHTML = '<div class="pcard-head"><div class="pcard-name" data-field="name"></div><div class="pcard-status" data-field="status"></div></div><p class="pcard-activity" data-field="activity"></p><div class="pcard-row"><span data-field="proc"></span><canvas class="pcard-chart" data-field="chart"></canvas></div><div class="pcard-row"><span data-field="stats"></span></div><div class="pcard-row" data-field="tagrow"><span data-field="tag"></span></div><p class="pcard-reason" data-field="reason" hidden></p><div class="pcard-row"><span data-field="worktree"></span></div><div class="pcard-row"><span data-field="last"></span><span data-field="model"></span></div><p class="pcard-hint" data-field="hint"></p>';
    card.addEventListener('click', event => { if (event.target.closest('button,[data-role-filter],[data-tag-filter]')) return; showComponentDetail(component.id); });
    card.addEventListener('keydown', event => { if ((event.key === 'Enter' || event.key === ' ') && !event.target.closest('button,[data-role-filter],[data-tag-filter]')) { event.preventDefault(); showComponentDetail(component.id); } });
    if (CONTROLS) {
      const actions = document.createElement('div');
      actions.className = 'pcard-actions';
      if (component.role === 'publisher') { actions.textContent = 'Dispatcher-owned'; actions.title = 'The publisher runs only inside a dispatcher round; it cannot be restarted independently.'; actions.classList.add('muted'); }
      else { const buttons = component.id === 'fleet' ? [['start','Start',''],['terminate','Terminate','danger'],['restart','Restart','']] : [['terminate','Terminate','danger'],['restart','Restart','']]; buttons.forEach(([action,label,className]) => { const button = document.createElement('button'); button.textContent = label; button.className = className; button.dataset.action = action; button.addEventListener('click', () => act(component.id, action)); actions.append(button); }); }
      card.append(actions);
    }
    cards.append(card);
    rows.set(component.id, card);
    return card;
  }
  function updateCard(card, component) {
    const process = component.process || {}, worktree = component.worktree || {};
    card.className = `pcard ${component.status}`;
    field(card,'name').innerHTML = `${roleIcon(component.role)}${e(component.label)}<span class="pcard-role" data-role-filter="${e(component.role)}" title="Click to filter to ${e(component.role)} components">${e(component.role)}</span>`;
    field(card,'status').innerHTML = `<span class="dot"></span><span class="status-${e(component.status)}">${e(component.status)}</span>` + (component.id === 'fleet' && latest.fleet_config && latest.fleet_config.mid_round ? '<span class="mid-round-badge" title="Workers hold this round\'s process groups; a scale-apply will ask for force">mid-round</span>' : '');
    const activityValue = activityText(component);
    const activity = field(card,'activity'); activity.textContent = activityValue; activity.title = activityValue;
    const proc = field(card,'proc'); proc.textContent = `${pidText(component)} · ${processText(process)} · alive ${uptimeText(component)} · ${cpu(process.cpu_percent)} / ${b(process.memory_bytes)}`; proc.title = activeCommand(process) || process.command || '';
    let lifetimeValue = lifetime(component);
    if (component.id === 'fleet' && latest.fleet_config) { const fc = latest.fleet_config; lifetimeValue = `${fc.configured_workers ?? '?'} workers configured · ${fc.workers_active ?? 0} active this round · mergers ${fc.mergers_alive ?? 0}${fc.squad_total != null ? '/' + fc.squad_total : ''} alive`; }
    const stats = field(card,'stats'); stats.textContent = lifetimeValue; stats.title = lifetimeValue;
    const tagText = currentTag(component), tagRow = field(card,'tagrow'), tag = field(card,'tag');
    tagRow.hidden = tagText === '—';
    tag.innerHTML = 'Tag: ' + (tagLinkHtml(component) || e(tagText)); tag.title = currentTagTitle(component);
    const reason = field(card,'reason');
    if (component.last_reason) { reason.hidden = false; reason.textContent = reasonText(component); reason.title = `${component.last_reason.reason || ''}${absTime(component.last_reason) ? '\n' + absTime(component.last_reason) : ''}`; }
    else { reason.hidden = true; }
    const worktreeEl = field(card,'worktree');
    worktreeEl.textContent = worktree.path ? `${worktree.path}${worktree.dirty_files ? ` · ${worktree.dirty_files} dirty` : ''}${Number.isInteger(worktree.behind) ? ` · ${f(worktree.behind)} behind` : ''}` : '—';
    worktreeEl.title = worktree.path || '';
    const lastEl = field(card,'last'); lastEl.textContent = lastFound(component); lastEl.title = component.last_task ? absTime(component.last_task) : '';
    const model = field(card,'model'); model.textContent = modelResult(component); model.className = modelResultClass(component);
    const hint = field(card,'hint'); hint.textContent = component.action_hint || ''; hint.title = component.action_hint || ''; hint.hidden = !component.action_hint;
    if (component.id === 'fleet') { const down = ['offline','stopped'].includes(component.status); card.querySelectorAll('.pcard-actions button').forEach(button => { button.hidden = button.dataset.action === 'start' ? !down : down; }); }
    chart(field(card,'chart'), component.id, process.cpu_percent);
  }
  function renderTable(data) {
    ensureGrid();
    const hiddenDead = !showDead ? data.components.filter(isDead).length : 0;
    const s = data.summary;
    const chipHtml = (text, cls, attrs) => `<span class="chip${cls ? ' ' + cls : ''}"${attrs || ''}>${e(text)}</span>`;
    const statusChip = (count, status, label, cls, always) => (always || count) ? chipHtml(`${count} ${label}`, `${cls}${statusFilter === status ? ' chip-active' : ''}`, ` data-status-filter="${status}" role="button" tabindex="0" title="Click to filter to ${label} components"`) : '';
    summary.innerHTML = [
      (paused || document.hidden) ? chipHtml(paused ? 'paused' : 'paused (tab hidden)', 'c-idle') : '',
      statusChip(s.running, 'running', 'running', 'c-ok', true),
      statusChip(s.waiting, 'waiting', 'waiting', 'c-blue'),
      statusChip(s.frozen, 'frozen', 'frozen', 'c-purple'),
      statusChip(s.offline, 'offline', 'offline', 'c-red'),
      statusChip(s.archived, 'archived', 'archived', ''),
      statusChip(s.idle, 'stopped', 'idle', 'c-idle'),
      chipHtml(`${s.components} components`, ''),
      hiddenDead ? chipHtml(`${hiddenDead} dead hidden`, '') : '',
      chipHtml(`${s.active_workers} active workers`, ''),
      chipHtml(`${f(s.claimed_tags)} tags claimed`, ''),
      chipHtml(`queue ${f(s.judgment_depth)}`, ''),
      chipHtml(`${f(s.patches_last_hour)} patches/h`, ''),
      chipHtml(`${f(s.model_calls_last_hour)} calls/h`, ''),
      chipHtml(`${cpu(s.total_cpu_percent)} CPU`, ''),
      chipHtml(b(s.total_memory_bytes), ''),
      chipHtml(new Date(data.generated_at).toLocaleTimeString(), ''),
      chipHtml(CONTROLS ? 'controls enabled' : 'read-only', ''),
    ].filter(Boolean).join('');
    const current = new Set(data.components.map(component => component.id));
    for (const [id,card] of rows) if (!current.has(id)) { card.remove(); rows.delete(id); }
    for (const component of data.components) updateCard(rows.get(component.id) || createCard(component), component);
    const visible = data.components.filter(match).sort((left,right) => { const a = value(left,sortKey), b = value(right,sortKey); return typeof a === 'number' && typeof b === 'number' ? sortDirection * (a - b) : sortDirection * String(a).localeCompare(String(b)); });
    const shown = new Set(visible.map(component => component.id));
    for (const [id,card] of rows) card.hidden = !shown.has(id);
    const fragment = document.createDocumentFragment(); visible.forEach(component => fragment.append(rows.get(component.id))); cards.append(fragment);
  }
  function updateFleetBanner(data) {
    const banner = document.getElementById('fleet-banner');
    if (!banner) return;
    const fc = data.fleet_config || {};
    const live = data.components.some(c => (c.id === 'fleet' || c.id === 'dispatcher') && ['running','waiting','frozen'].includes(c.status));
    const show = !!fc.plist_present && fc.service_loaded === false && !live;
    banner.classList.toggle('hidden', !show);
    if (!show) { banner.dataset.ready = ''; return; }
    if (banner.dataset.ready === '1') return;
    banner.innerHTML = `<span>Fleet is stopped — the LaunchAgent is not bootstrapped.</span>${CONTROLS ? '<button id="fleet-banner-start" type="button">Start fleet</button>' : '<span class="muted">(starting requires --enable-controls)</span>'}`;
    banner.dataset.ready = '1';
    const button = document.getElementById('fleet-banner-start');
    if (button) button.addEventListener('click', () => act('fleet', 'start'));
  }
  render = data => { sampleHistories(data); renderTable(data); updateFleetBanner(data); updateFleetStrip(data); drawFlow(data); refreshOpenMemberTable(); refreshOpenComponentDetail(); };
  const METRIC_LABELS = {patches_found:'sent', patches_sent:'sent', patches_applied:'git-applied', patches_apply_failed:'apply failed', review_rejected:'review-rejected', critique_events:'critiqued', fixer_calls:'fixer calls', reviewer_calls:'reviewer calls', critique_calls:'critique calls', events:'events', prs_made:'PRs made', heartbeat_ts:'last heartbeat', recorded_state:'recorded state', patches_last_hour:'patches (last hour)', calls_last_hour:'model calls (last hour)', fixer_error_pct:'fixer errors (recent)', review_rejection_pct:'review-rejected share', recent_http_2xx:'HTTP 2xx (recent)', recent_http_4xx:'HTTP 4xx (recent)', recent_http_5xx:'HTTP 5xx (recent)', tags_claimed_total:'distinct tags claimed', batch_commits:'unbatched commits', batch_blocked:'batch blocked', rejections_last_hour:'review-rejected (last hour)', prs_merged_today:'PRs merged today'};
  const formatMetricValue = (key, value) => typeof value === 'number' ? (/_ts$/.test(key) ? epochAge(value) : /_pct$/.test(key) ? value.toFixed(1) + '%' : f(value)) : String(value);
  function memberStats(metrics) {
    return Object.entries(metrics || {}).filter(([, value]) => typeof value === 'number' || typeof value === 'string').map(([key, value]) => `${METRIC_LABELS[key] || key}: ${formatMetricValue(key, value)}`).join(' · ') || '—';
  }
  function specificGuide(key) {
    const docs = {
      supervisor: ['Fleet supervisor', 'Owns the fleet lifecycle and starts the dispatcher, merger, and judgment tiers.', ['A restart is fleet-wide.', 'It does not create code changes itself.', 'Start bootstraps the configured LaunchAgent when the fleet is stopped; it never invents configuration.', 'Scale edits the LaunchAgent plist — the single source of truth for the worker count — then restarts the fleet (bootout + bootstrap) to apply it.']],
      dispatcher: ['Task dispatcher', 'Allocates worker slots, creates each worker process, and invokes the publish sweep after a round.', ['If it is stopped, no new fixer processes can be created.', 'It is the owner of the PR publisher lifecycle.']],
      workers: ['Fixer workers', 'A fixer is the worker process that asks the Model API for a candidate patch, applies it in its isolated worktree, and runs local validation.', ['A reviewed rejection returns a concrete critique to this same worker for another attempt.', 'An approved commit moves right to the squad merger.', '"git-applied" only means the diff applied cleanly to the tree -- it can still fail the build, the gap recheck, or the reviewer afterward. "Review-rejected" is the real accept/reject verdict, drawn from the harness\'s own rejection ledger.', 'Why (last rejection) shows the most recent human-readable reviewer or critique reason for this worker, when one was recorded.', 'Process and Active PID both show the deepest live child, such as rustc or mold, rather than the Python worker wrapper. CPU and Memory are totals for the full worker process tree; control actions still safely target the wrapper process group.', 'Last model result is the actual HTTP status for the last fixer request: 2xx/3xx is healthy; 4xx/5xx needs attention. Older requests honestly say when no status was recorded.', 'Behind origin/main compares each worktree to the locally fetched origin/main reference; a dashboard refresh does not fetch Git.']],
      reviewers: ['Review gates', 'The review phase checks a candidate patch after local validation. It runs inside the worker process, so it shares the worker PID.', ['Approved work proceeds to a squad merger.', 'A rejection becomes fixer feedback, not a terminal failure.', '"Sent" is every diff the fixer generated; "git-applied" is how many applied cleanly to the tree (not the same as accepted); "apply failed" is a mechanical git-apply failure with no reviewer involved; "review-rejected" is the reviewer\'s actual verdict.', 'Why (last rejection) shows the most recent human-readable reviewer or critique reason, when one was recorded.', 'Last model result shows the HTTP result for the latest reviewer or critique request.']],
      mergers: ['Squad mergers', 'One merger per squad gathers approved commits, removes duplicate patch identities, and stages a squad branch.', ['A merger is long-lived and polls independently of individual workers.']],
      publisher: ['PR publisher', 'The dispatcher invokes this short publish sweep after a round. It opens, adopts, checks, or merges publishable work, then exits.', ['Waiting means there is no sweep child process right now, while the dispatcher is alive between sweeps.', 'When waiting, PID / owner and Process show that live dispatcher owner.', 'It is dispatcher-owned and cannot be safely restarted as a separate daemon.']],
      api: ['Model API', 'Both fixer and reviewer phases make request/response calls here.', ['The arrows represent calls and replies, not a separate persistent worker.', 'HTTP status is captured for new calls; a 401 or 5xx is shown as an error, while historical log lines without an HTTP result remain explicitly unknown.']],
      queue: ['Judgment queue', 'Holds flagged changes for inspection.', ['It is advisory and does not block the normal publish path by itself.']],
      main: ['origin/main', 'The published destination after a ready PR is merged.', ['The dashboard reconciles this state with the durable publish log.']],
    };
    const [title, copy, steps] = docs[key] || ['Component', 'No component-specific documentation is available.', []];
    const states = key === 'workers' ? '<div class="flow-state-grid"><div class=flow-state><strong>running</strong><span>Live PID; actively working or between checks.</span></div><div class=flow-state><strong>idle</strong><span>Reserved for a live PID with no assigned work.</span></div><div class=flow-state><strong>frozen</strong><span>Live PID, no recorded progress for 30m, and near-zero sampled CPU. The diagnosis appears in Lifetime.</span></div><div class=flow-state><strong>offline</strong><span>Prior worker record, but no live PID.</span></div><div class=flow-state><strong>archived</strong><span>Historical worktree entry with no activity record.</span></div></div>' : '';
    return `<section class="flow-specific"><h3>${e(title)}</h3><p>${e(copy)}</p><ul>${steps.map(step => `<li>${e(step)}</li>`).join('')}</ul>${states}</section>`;
  }
  function flowGuide(focus) {
    const focused = key => focus === key ? 'focus' : '';
    const parts = [
      ['Fleet supervisor', 'Keeps the dispatcher and merger processes alive; it does not make code changes.'],
      ['Dispatcher', 'Allocates worker slots and hands each slot a format or squad task.'],
      ['Fixer worker', 'A worker’s fixer phase asks the Model API for a candidate diff, then runs local checks.'],
      ['Review gate', 'An independent review phase checks the candidate. Rejections feed a concrete critique back to the fixer.'],
      ['Squad merger', 'Takes approved worker commits, deduplicates them, and stages squad branches.'],
      ['PR publisher', 'A dispatcher-owned sweep that runs after each round, then waits for the next one. Waiting is healthy; it is not a separate daemon to restart.'],
      ['Model API', 'Request/response service used by both the fixer and reviewer phases.'],
      ['Judgment queue', 'Advisory record of flagged changes; it is visible for inspection and does not block publishing by itself.'],
    ];
    return `<section class="flow-guide"><h3>How a fixer worker reaches origin/main</h3><div class="flow-guide-wrap"><svg viewBox="0 0 830 150" role="img" aria-label="Fixer worker retry and publish flow"><defs><marker id="guide-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8" fill="currentColor"/></marker></defs><path class="guide-edge" marker-end="url(#guide-arrow)" d="M126 45 H150"/><path class="guide-edge" marker-end="url(#guide-arrow)" d="M266 45 H290"/><path class="guide-edge" marker-end="url(#guide-arrow)" d="M406 45 H430"/><path class="guide-edge" marker-end="url(#guide-arrow)" d="M546 45 H570"/><path class="guide-edge" marker-end="url(#guide-arrow)" d="M686 45 H710"/><path class="guide-edge" marker-end="url(#guide-arrow)" d="M208 70 V112 H338"/><path class="guide-edge" marker-end="url(#guide-arrow)" d="M348 70 V112"/><path class="guide-feedback" marker-end="url(#guide-arrow)" d="M348 70 V90 H208 V70"/><text class="guide-label" x="276" y="86" text-anchor="middle">rejected → revise</text><text class="guide-label" x="274" y="106" text-anchor="middle">fixer + review API requests</text><text class="guide-label" x="628" y="91" text-anchor="middle">runs after each dispatcher round</text><text class="guide-label" x="628" y="104" text-anchor="middle">then waits for the next round</text><g><rect class="guide-node ${focused('dispatcher')}" x="10" y="20" width="116" height="50" rx="6"/><text class="guide-title" x="68" y="42" text-anchor="middle">Dispatcher</text><text class="guide-copy" x="68" y="57" text-anchor="middle">assigns work</text></g><g><rect class="guide-node ${focused('workers')}" x="150" y="20" width="116" height="50" rx="6"/><text class="guide-title" x="208" y="42" text-anchor="middle">Fixer worker</text><text class="guide-copy" x="208" y="57" text-anchor="middle">diff + local checks</text></g><g><rect class="guide-node ${focused('reviewers')}" x="290" y="20" width="116" height="50" rx="6"/><text class="guide-title" x="348" y="42" text-anchor="middle">Review gate</text><text class="guide-copy" x="348" y="57" text-anchor="middle">approve or critique</text></g><g><rect class="guide-node ${focused('mergers')}" x="430" y="20" width="116" height="50" rx="6"/><text class="guide-title" x="488" y="42" text-anchor="middle">Squad merger</text><text class="guide-copy" x="488" y="57" text-anchor="middle">stages commits</text></g><g><rect class="guide-node ${focused('publisher')}" x="570" y="20" width="116" height="50" rx="6"/><text class="guide-title" x="628" y="42" text-anchor="middle">PR publisher</text><text class="guide-copy" x="628" y="57" text-anchor="middle">dispatcher sweep</text></g><g><rect class="guide-node ${focused('main')}" x="710" y="20" width="110" height="50" rx="6"/><text class="guide-title" x="765" y="42" text-anchor="middle">origin/main</text><text class="guide-copy" x="765" y="57" text-anchor="middle">landed work</text></g><g><rect class="guide-node ${focused('api')}" x="338" y="102" width="130" height="38" rx="6"/><text class="guide-title" x="403" y="126" text-anchor="middle">Model API</text></g></svg></div><div class="flow-parts">${parts.map(([name, copy]) => `<div class="flow-part"><strong>${e(name)}</strong><p>${e(copy)}</p></div>`).join('')}</div></section>`;
  }
  const memberColumns = [
    ['label', 'Component'], ['status', 'Status'], ['activity', 'Doing now'], ['pid', 'Active PID / owner'], ['uptime', 'Alive'], ['process', 'Process'], ['cpu', 'CPU (tree, 1s)'], ['memory', 'Memory (tree)'], ['behind', 'Behind origin/main'], ['lifetime', 'Stats'], ['tag', 'Current tag'], ['reason', 'Why (last rejection)'], ['last', 'Last request'], ['model', 'Last model result'], ['worktree', 'Worktree'],
  ];
  const memberDisplay = (member, key) => {
    const process = member.process || {}, worktree = member.worktree || {};
    if (key === 'label') return member.label;
    if (key === 'status') return member.status;
    if (key === 'activity') return activityText(member);
    if (key === 'pid') return member.pid == null ? '—' : `${member.pid}${member.pid_note ? ` · ${member.pid_note}` : ''}`;
    if (key === 'uptime') return uptimeText(member);
    if (key === 'process') return processText(process);
    if (key === 'cpu') return cpu(process.cpu_percent);
    if (key === 'memory') return b(process.memory_bytes);
    if (key === 'behind') return Number.isInteger(worktree.behind) ? `${f(worktree.behind)} commits` : '—';
    if (key === 'tag') return currentTag(member);
    if (key === 'reason') return reasonText(member);
    if (key === 'last') return member.last_task ? `${member.last_task.phase} · ${age(member.last_task)}` : '—';
    if (key === 'model') return modelResult(member);
    if (key === 'lifetime') return member.metrics?.freeze_reason || memberStats(member.metrics);
    if (key === 'worktree') return worktree.path || '—';
    return '—';
  };
  const memberSortValue = (member, key) => {
    const process = member.process || {}, worktree = member.worktree || {};
    return {label:member.label, status:member.status, activity:activityText(member), pid:member.pid || -1, uptime:process.elapsed_seconds || 0, process:activeCommand(process), cpu:process.cpu_percent || 0, memory:process.memory_bytes || 0, behind:Number.isInteger(worktree.behind) ? worktree.behind : -1, lifetime:memberDisplay(member, 'lifetime'), tag:currentTag(member), reason:member.last_reason?.epoch || 0, last:member.last_task?.epoch || 0, model:Number.isInteger(member.last_task?.http_status) ? member.last_task.http_status : -1, worktree:worktree.path || ''}[key] ?? '';
  };
  // Re-render hook for the open member grid. Kept as a closure so a poll
  // refreshes the data without discarding the user's filter and sort state.
  let refreshMemberCards = null;
  function refreshOpenMemberTable() {
    const dialog = document.getElementById('flow-detail');
    if (!dialog?.open || flowShowingOverview || flowShowingPrs || !selectedFlowNode || !refreshMemberCards) return;
    const node = latest.flow.nodes[selectedFlowNode];
    if (node?.members) refreshMemberCards(node);
  }
  let selectedComponentId = null;
  function componentDialog() {
    let dialog = document.getElementById('component-detail');
    if (dialog) return dialog;
    document.body.insertAdjacentHTML('beforeend', '<dialog id="component-detail"><div class="flow-dialog-controls"><button id="component-detail-close" type="button">Close</button></div><div id="component-detail-body"></div></dialog>');
    dialog = document.getElementById('component-detail');
    dialog.querySelector('#component-detail-close').addEventListener('click', event => { event.stopPropagation(); selectedComponentId = null; dialog.close(); });
    dialog.addEventListener('close', () => { selectedComponentId = null; });
    dialog.addEventListener('click', event => {
      const copyEl = event.target.closest('[data-copy]');
      if (copyEl) { copyText(copyEl.dataset.copy); return; }
      const tagEl = event.target.closest('[data-tag-filter]');
      if (tagEl) { setTagFilter(tagEl.dataset.tagFilter); dialog.close(); return; }
      const box = dialog.getBoundingClientRect();
      if (event.clientX < box.left || event.clientX > box.right || event.clientY < box.top || event.clientY > box.bottom) dialog.close();
    });
    return dialog;
  }
  function detailStatCards(component) {
    const process = component.process || {}, worktree = component.worktree || {};
    const tagsHtml = (component.current_tag?.tags || (component.current_tag?.tag ? [component.current_tag.tag] : [])).map(tag => `<span class="tag-link" data-tag-filter="${e(tag)}" title="Click to filter the grid to this tag">${e(tag)}</span>`).join(', ') || '—';
    const rows = [
      ['Doing now', e(activityText(component))],
      ['PID', e(pidText(component)) + (component.pid == null ? '' : copyButton(component.pid))],
      ['Process', e(activeCommand(process) || process.command || '—')],
      ['Alive', e(uptimeText(component))],
      ['CPU (tree, 1s)', e(cpu(process.cpu_percent))],
      ['Memory (tree)', e(b(process.memory_bytes))],
      ['Current tag', tagsHtml],
      ['Worktree', worktree.path ? e(worktree.path) + copyButton(worktree.path) : '—'],
      ['Dirty files', e(String(worktree.dirty_files ?? '—'))],
      ['Behind origin/main', e(Number.isInteger(worktree.behind) ? `${f(worktree.behind)} commits` : '—')],
      ['Last request', component.last_task ? `<span title="${e(absTime(component.last_task))}">${e(`${component.last_task.phase} · ${component.last_task.outcome} · ${age(component.last_task)}`)}</span>` : '—'],
      ['Last model result', e(modelResult(component))],
    ];
    return `<section class="flow-stat-grid">${rows.map(([label, html]) => `<div class="flow-stat"><span class="flow-stat-label">${e(label)}</span><span class="flow-stat-value">${html}</span></div>`).join('')}</section>`;
  }
  function detailMetrics(component) {
    const entries = Object.entries(component.metrics || {}).filter(([key, value]) => key !== 'freeze_reason' && (typeof value === 'number' || typeof value === 'string'));
    if (!entries.length) return '<p class="muted">No metrics recorded.</p>';
    return `<section class="flow-stat-grid">${entries.map(([key, value]) => `<div class="flow-stat"><span class="flow-stat-label">${e(METRIC_LABELS[key] || key)}</span><span class="flow-stat-value">${e(formatMetricValue(key, value))}</span></div>`).join('')}</section>`;
  }
  function detailLog(component) {
    if (!component.log || !component.log.length) return '<p class="muted">No recent history recorded.</p>';
    const prUrls = {};
    for (const pr of (component.metrics && component.metrics.recent_prs) || []) if (pr && pr.url && pr.number != null) prUrls[String(pr.number)] = pr.url;
    const eventHtml = text => {
      const prMatch = /^PR #(\d+)/.exec(text || '');
      const url = prMatch && prUrls[prMatch[1]];
      return url ? `<a href="${e(url)}" target="_blank" rel="noreferrer">${e(prMatch[0])}</a>${e(text.slice(prMatch[0].length))}` : e(text);
    };
    const rows = component.log.map(entry => `<tr><td title="${e(absTime(entry))}">${e(age(entry))}</td><td class="flow-reason">${eventHtml(entry.text)}</td></tr>`).join('');
    return `<div class="flow-member-table-wrap"><table class="publisher-prs-table"><thead><tr><th>Age</th><th>Event</th></tr></thead><tbody>${rows}</tbody></table></div>`;
  }
  function renderComponentDetail(component) {
    const dialog = componentDialog();
    const active = document.activeElement;
    if (dialog.open && active && dialog.contains(active) && /^fleet-scale/.test(active.id || '')) return;
    const fc = latest.fleet_config || {}, caps = fc.capabilities || {};
    let actionsHtml = '';
    if (CONTROLS) {
      if (component.role === 'publisher') actionsHtml = '<p class="muted">Dispatcher-owned; cannot be restarted independently.</p>';
      else {
        const startButton = component.id === 'fleet' && ['offline','stopped'].includes(component.status) ? '<button id="component-detail-start">Start</button>' : '';
        actionsHtml = `<div class="flow-dialog-controls" style="justify-content:flex-start">${startButton}<button class="danger" id="component-detail-terminate">Terminate</button><button id="component-detail-restart">Restart</button></div>`;
        if (component.id === 'fleet') {
          const squadTotal = fc.squad_total ?? 14;
          const mergersHtml = caps.mergers_flag
            ? `<label title="One merger per squad (${squadTotal} squads); the cap covers squads in squads.toml order. Squad membership itself changes by PR, not here."><input type="checkbox" id="fleet-scale-limit"${fc.configured_mergers ? ' checked' : ''}> Limit mergers to first</label><input type="number" id="fleet-scale-mergers" min="1" max="${squadTotal}" step="1" value="${fc.configured_mergers || squadTotal}"${fc.configured_mergers ? '' : ' disabled'}><span class="muted">of ${squadTotal} squads</span>`
            : '<span class="muted">merger cap unsupported by deployed fleet_up.sh</span>';
          actionsHtml = `<h3>Fleet scale</h3><div id="fleet-scale-form"><label for="fleet-scale-workers">Workers (dispatcher --max-parallel)</label><input type="number" id="fleet-scale-workers" min="1" max="64" step="1" value="${fc.configured_workers ?? ''}">${mergersHtml}<button id="fleet-scale-apply">Apply</button><p class="scale-help">Persists in the LaunchAgent plist (the source of truth); applying restarts the fleet via bootout + bootstrap. Added workers queue behind the 5-holder build semaphore and the 30-calls/min governor — large counts add risk, not linear throughput.</p></div>` + actionsHtml;
        }
      }
    }
    document.getElementById('component-detail-body').innerHTML = `<h2>${roleIcon(component.role)}${e(component.label)} <span class="pcard-role">${e(component.role)} · ${e(component.status)}</span></h2>${component.action_hint ? `<p class="muted">${e(component.action_hint)}</p>` : ''}<h3>Overview</h3>${detailStatCards(component)}<h3>Resource history</h3><div class="detail-charts"><div class="detail-chart"><span class="chart-title">CPU (tree) · last ${HISTORY_LIMIT} polls</span><canvas id="detail-cpu-chart"></canvas></div><div class="detail-chart"><span class="chart-title">Memory (tree) · last ${HISTORY_LIMIT} polls</span><canvas id="detail-mem-chart"></canvas></div></div><h3>Stats</h3>${detailMetrics(component)}${patchOutcomeSection(component)}${httpMixSection(component)}${component.last_reason ? `<h3>Last rejection reason</h3><p class="flow-reason">${e(reasonText(component))}</p>` : ''}<h3>Recent history</h3>${detailLog(component)}${actionsHtml}`;
    if (CONTROLS && component.role !== 'publisher') {
      document.getElementById('component-detail-terminate').addEventListener('click', () => act(component.id, 'terminate'));
      document.getElementById('component-detail-restart').addEventListener('click', () => act(component.id, 'restart'));
      const startButton = document.getElementById('component-detail-start');
      if (startButton) startButton.addEventListener('click', () => act(component.id, 'start'));
      const toast = message => { notice.textContent = message; notice.classList.remove('hidden'); setTimeout(() => notice.classList.add('hidden'), 6000); };
      const limitBox = document.getElementById('fleet-scale-limit');
      if (limitBox) limitBox.addEventListener('change', () => { document.getElementById('fleet-scale-mergers').disabled = !limitBox.checked; });
      const applyButton = document.getElementById('fleet-scale-apply');
      if (applyButton) applyButton.addEventListener('click', () => {
        const workers = parseInt(document.getElementById('fleet-scale-workers').value, 10);
        if (!Number.isInteger(workers) || workers < 1 || workers > 64) { toast('workers must be an integer between 1 and 64'); return; }
        const extra = {workers};
        if (limitBox) {
          if (limitBox.checked) {
            const mergers = parseInt(document.getElementById('fleet-scale-mergers').value, 10);
            if (!Number.isInteger(mergers) || mergers < 1) { toast('merger cap must be a positive integer'); return; }
            extra.mergers = mergers;
          } else extra.mergers = 0;
        }
        act(component.id, 'scale', extra);
      });
    }
    if (!dialog.open) dialog.showModal();
    const cpuCanvas = document.getElementById('detail-cpu-chart'), memCanvas = document.getElementById('detail-mem-chart');
    if (cpuCanvas) drawSeriesChart(cpuCanvas, cpuHist[component.id] || [], {stroke:'#62b0ff', fill:'rgba(98,176,255,0.16)', floor:100, format:cpu});
    if (memCanvas) drawSeriesChart(memCanvas, memHist[component.id] || [], {stroke:'#37d996', fill:'rgba(55,217,150,0.14)', floor:1048576, format:b});
  }
  window.showComponentDetail = id => {
    const component = latest.components.find(c => c.id === id);
    if (!component) return;
    selectedComponentId = id;
    renderComponentDetail(component);
  };
  function refreshOpenComponentDetail() {
    const dialog = document.getElementById('component-detail');
    if (!dialog?.open || !selectedComponentId) return;
    const component = latest.components.find(c => c.id === selectedComponentId);
    if (!component) { dialog.close(); return; }
    renderComponentDetail(component);
  }
  function memberCardHtml(member) {
    const process = member.process || {}, worktree = member.worktree || {};
    const rows = [
      ['Process', `${memberDisplay(member,'pid')} · ${processText(process)} · alive ${uptimeText(member)}`, activeCommand(process)],
      ['Resources', `${cpu(process.cpu_percent)} · ${b(process.memory_bytes)}`, ''],
      ['Stats', memberDisplay(member,'lifetime'), memberDisplay(member,'lifetime')],
      ['Tag', currentTag(member), currentTagTitle(member), tagLinkHtml(member)],
      ['Worktree', worktree.path ? `${worktree.path}${Number.isInteger(worktree.behind) ? ` · ${f(worktree.behind)} behind` : ''}` : '—', worktree.path || ''],
      ['Last request', memberDisplay(member,'last'), member.last_task ? absTime(member.last_task) : ''],
    ];
    const reason = member.last_reason ? `<p class="member-card-reason" title="${e(member.last_reason.reason || '')}${absTime(member.last_reason) ? e('\n' + absTime(member.last_reason)) : ''}">${e(reasonText(member))}</p>` : '';
    return `<article class="member-card ${e(member.status)}" data-member-open="${e(member.id)}" tabindex="0" title="Click for full details and history"><div class="pcard-head"><div class="pcard-name">${roleIcon(member.role)}${e(member.label)}</div><div class="pcard-status"><span class="dot"></span><span class="status-${e(member.status)}">${e(member.status)}</span></div></div><p class="pcard-activity" title="${e(activityText(member))}">${e(activityText(member))}</p><dl class="member-card-grid">${rows.map(([label, text, title, html]) => `<dt>${e(label)}</dt><dd${title ? ` title="${e(title)}"` : ''}>${html || e(text)}</dd>`).join('')}</dl>${reason}<div class="pcard-row"><span class="${modelResultClass(member)}">${e(modelResult(member))}</span></div></article>`;
  }
  function setupMemberTable(node) {
    const host = document.getElementById('flow-member-host'), filter = document.getElementById('flow-member-filter'), sortSelect = document.getElementById('flow-member-sort'), directionButton = document.getElementById('flow-member-direction');
    if (!host || !filter) return;
    let query = '', sortKey = 'label', sortDirection = 1;
    refreshMemberCards = fresh => { node = fresh; renderMembers(); };
    const renderMembers = () => {
      const members = node.members.filter(member => !query || [member.label, member.status, member.pid, member.pid_note, activeCommand(member.process || {}), currentTag(member), currentTagTitle(member), (member.worktree || {}).path, memberDisplay(member, 'model'), activityText(member), reasonText(member)].join(' ').toLowerCase().includes(query)).sort((left, right) => {
        const a = memberSortValue(left, sortKey), b = memberSortValue(right, sortKey);
        return typeof a === 'number' && typeof b === 'number' ? sortDirection * (a - b) : sortDirection * String(a).localeCompare(String(b));
      });
      host.innerHTML = members.length ? members.map(memberCardHtml).join('') : '<p class="muted">No members match this filter.</p>';
      const rankHost = document.getElementById('flow-rank-chart');
      if (rankHost) rankHost.innerHTML = workerRankChart(node);
      const statsHost = document.getElementById('flow-group-stats');
      if (statsHost) statsHost.innerHTML = readableStatCards(node.detail);
      host.querySelectorAll('[data-member-open]').forEach(card => {
        const open = () => { document.getElementById('flow-detail')?.close(); showComponentDetail(card.dataset.memberOpen); };
        card.addEventListener('click', event => { if (event.target.closest('[data-tag-filter]')) return; open(); });
        card.addEventListener('keydown', event => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); open(); } });
      });
    };
    directionButton.textContent = sortDirection > 0 ? '↑ Ascending' : '↓ Descending';
    directionButton.addEventListener('click', () => { sortDirection = -sortDirection; directionButton.textContent = sortDirection > 0 ? '↑ Ascending' : '↓ Descending'; renderMembers(); });
    sortSelect.addEventListener('change', event => { sortKey = event.target.value; renderMembers(); });
    filter.addEventListener('input', event => { query = event.target.value.trim().toLowerCase(); renderMembers(); });
    renderMembers();
  }
  function detailRows(node, key) {
    if (node.members) return `<h3>Group stats</h3><div id="flow-group-stats">${readableStatCards(node.detail)}</div>${rankChartSection(node, key)}<div class="flow-member-controls"><label for="flow-member-filter">Filter</label><input id="flow-member-filter" type="search" placeholder="name, process, PID, worktree…" autocomplete="off"><label class="sort-control" for="flow-member-sort">Sort by<select id="flow-member-sort">${memberColumns.map(([k,label]) => `<option value="${k}">${e(label)}</option>`).join('')}</select></label><button type="button" id="flow-member-direction"></button></div><p class="flow-member-hint">Click any member for its full stats and history.</p><div id="flow-member-host" class="member-card-grid-host"></div>`;
    if (key === 'queue') {
      const blocked = (node.detail.blocked_squads || []).map(x => e(x.squad)).join(', ') || 'none';
      const queued = (node.detail.queued || []).map(x => `<tr><td>${e(x.patch_id)}</td><td>${e(x.format)}</td><td>${e(x.squad)}</td><td class=flow-reason>${e(x.reason)}</td><td>${e(x.timestamp)}</td></tr>`).join('') || '<tr><td colspan=5>No queued judgments</td></tr>';
      return `<p>Advisory only: queued changes do not block the publish path.</p><p>Blocked squads: ${blocked}</p><table><thead><tr><th>Patch</th><th>Format</th><th>Squad</th><th>Reason</th><th>Queued</th></tr></thead><tbody>${queued}</tbody></table>`;
    }
    if (key === 'api') return readableStatCards(node.detail) + apiRequestTable();
    return readableStatCards(node.detail);
  }
  function apiRequestTable() {
    const requests = latest.api_requests || [];
    if (!requests.length) return '<h3>Last 20 requests</h3><p class="muted">No model requests recorded by this dashboard process yet; the feed fills as new manifest lines arrive.</p>';
    const resultClass = outcome => outcome === 'OK' ? 'model-result-ok' : /^ERROR/.test(outcome || '') ? 'model-result-error' : 'model-result-retry';
    const rows = requests.map(r => `<tr><td title="${e(r.timestamp || '')}">${r.epoch ? e(epochAge(r.epoch)) : '—'}</td><td>${e(r.phase || '—')}</td><td>${e(r.worker || '—')}</td><td>${r.tag ? `<span data-tag-filter="${e(r.tag)}" role="button" tabindex="0" title="Click to filter to this tag">${e(r.tag)}</span>` : '—'}</td><td>${e(r.tier || '—')}</td><td>${e(r.provider || '—')}${r.model ? ' / ' + e(r.model) : ''}</td><td>${f(r.prompt_chars)}</td><td>${f(r.reply_chars)}</td><td>${e(r.elapsed || '—')}</td><td class="${resultClass(r.outcome)}">${e(r.outcome || '—')}${r.http_status ? ' · HTTP ' + e(String(r.http_status)) : ''}</td></tr>`).join('');
    return `<h3>Last 20 requests</h3><p class="muted">Newest first. Tags are stamped from the worker's live claim when the request line is first seen, so they stay honest after the claim moves on.</p><div class="flow-member-table-wrap"><table class="publisher-prs-table"><thead><tr><th>Age</th><th>Phase</th><th>Worker</th><th>Tag</th><th>Tier</th><th>Provider / model</th><th>Prompt chars</th><th>Reply chars</th><th>Elapsed</th><th>Result</th></tr></thead><tbody>${rows}</tbody></table></div>`;
  }
  function readableStatCards(detail) {
    const labels = {processes:'Components', running:'Active processes', inactive:'Inactive processes', active_workers:'Active workers', events:'Dispatcher events', patches_found:'Patches found', fixer_calls:'Fixer calls', frozen:'Frozen workers', calls:'Model calls', patches_sent:'Patches sent for review', patches_applied:'Patches git-applied', patches_apply_failed:'Patches that failed to apply', review_rejected:'Patches rejected by review', critique_events:'Critique events', blocked_squads:'Blocked squads', batch_commits:'Unbatched commits', merged_prs:'Merged PRs', total:'Total model calls', fixer:'Fixer calls', reviewer:'Reviewer calls', critique:'Critique calls', event_count:'Recorded events', status:'Publisher state', source:'Data source', last_pr:'Latest PR', patches_last_hour:'Patches (last hour)', calls_last_hour:'Model calls (last hour)', claimed_tags:'Tags claimed now', review_rejection_pct:'Review-rejected share', rejections_last_hour:'Review rejections (last hour)', prs_merged_today:'PRs merged today', recent_http_2xx:'HTTP 2xx (recent calls)', recent_http_4xx:'HTTP 4xx (recent calls)', recent_http_5xx:'HTTP 5xx (recent calls)'};
    const value = (key, item) => {
      if (item == null) return '—';
      if (key === 'last_pr' && typeof item === 'object') return item.number ? `#${item.number}${item.timestamp ? ` · ${age(item)}` : ''}` : '—';
      if (Array.isArray(item)) return item.length ? `${item.length} recorded` : 'none';
      if (typeof item === 'number') return /_pct$/.test(key) ? item.toFixed(1) + '%' : f(item);
      if (typeof item === 'object') return 'recorded';
      return String(item).replaceAll('_', ' ');
    };
    const cards = Object.entries(detail || {}).filter(([key]) => key !== 'recent_prs').map(([key, item]) => { const title = key === 'last_pr' && item && item.timestamp ? absTime(item) : ''; return `<div class="flow-stat"><span class="flow-stat-label">${e(labels[key] || key.replaceAll('_', ' '))}</span><span class="flow-stat-value"${title ? ` title="${e(title)}"` : ''}>${e(value(key, item))}</span></div>`; }).join('');
    return `<section class="flow-stat-grid">${cards || '<div class="flow-stat"><span class="flow-stat-value">No current data.</span></div>'}</section>`;
  }
  function publisherPrTable(node) {
    const prs = (node.detail.recent_prs || []).slice(0, 20);
    if (!prs.length) return '<p class="publisher-prs-note">No recent PR records are available yet.</p>';
    const rows = prs.map(pr => {
      const number = pr.url ? `<a href="${e(pr.url)}" target="_blank" rel="noreferrer">#${e(pr.number)}</a>` : `#${e(pr.number)}`;
      return `<tr><td>${number}</td><td>${e(pr.name || '—')}</td><td title="${e(absTime(pr))}">${e(age({timestamp: pr.timestamp}))}</td><td>${e(pr.title || '—')}</td></tr>`;
    }).join('');
    return `<p class="publisher-prs-note">Latest 20 PRs · ${e(node.detail.source || 'durable publish record')}</p><div class="flow-member-table-wrap"><table class="publisher-prs-table"><thead><tr><th>PR #</th><th>PR name</th><th>Time ago</th><th>Description / title</th></tr></thead><tbody>${rows}</tbody></table></div>`;
  }
  let selectedFlowNode = null, flowShowingOverview = false, flowShowingPrs = false;
  function flowDialog() {
    let dialog = document.getElementById('flow-detail');
    if (dialog) return dialog;
    document.body.insertAdjacentHTML('beforeend', '<dialog id="flow-detail"><div class="flow-dialog-controls"><button id="flow-close" type="button">Close</button><button id="flow-overview" type="button">Overview</button><button id="flow-prs" type="button" hidden>PRs</button></div><div id="flow-detail-body"></div></dialog>');
    dialog = document.getElementById('flow-detail');
    dialog.querySelector('#flow-close').addEventListener('click', event => { event.stopPropagation(); dialog.close(); });
    dialog.querySelector('#flow-overview').addEventListener('click', event => { event.stopPropagation(); if (selectedFlowNode) renderFlowDialog(selectedFlowNode, flowShowingOverview ? 'detail' : 'overview'); });
    dialog.querySelector('#flow-prs').addEventListener('click', event => { event.stopPropagation(); if (selectedFlowNode === 'publisher') renderFlowDialog(selectedFlowNode, flowShowingPrs ? 'detail' : 'prs'); });
    dialog.addEventListener('click', event => {
      const tagEl = event.target.closest('[data-tag-filter]');
      if (tagEl) { setTagFilter(tagEl.dataset.tagFilter); dialog.close(); return; }
      const box = dialog.getBoundingClientRect();
      if (event.clientX < box.left || event.clientX > box.right || event.clientY < box.top || event.clientY > box.bottom) dialog.close();
    });
    return dialog;
  }
  function renderFlowDialog(key, mode = 'detail') {
    const node = latest.flow.nodes[key];
    if (!node) return;
    const overview = mode === 'overview', prs = mode === 'prs';
    selectedFlowNode = key; flowShowingOverview = overview; flowShowingPrs = prs;
    const dialog = flowDialog();
    dialog.querySelector('#flow-overview').textContent = overview ? `Back to ${node.label}` : 'Overview';
    const prsButton = dialog.querySelector('#flow-prs');
    prsButton.hidden = key !== 'publisher';
    prsButton.textContent = prs ? `Back to ${node.label}` : 'PRs';
    document.getElementById('flow-detail-body').innerHTML = `<h2>${prs ? 'Recent PRs' : overview ? 'Harness overview' : roleIcon(FLOW_NODE_ROLES[key]) + e(node.label)}</h2>${prs ? publisherPrTable(node) : overview ? flowGuide(key) : specificGuide(key) + detailRows(node, key)}`;
    if (!overview && !prs && node.members) setupMemberTable(node);
    if (!dialog.open) dialog.showModal();
  };
  window.showFlowNode = key => renderFlowDialog(key, 'detail');
  window.showFlowOverview = () => { if (selectedFlowNode) renderFlowDialog(selectedFlowNode, 'overview'); };
  function box(key, x, y, node) { return `<g onclick="showFlowNode('${key}')" role="button" aria-label="Show ${e(node.label)} details"><rect class=node x="${x}" y="${y}" width="180" height="100" rx="8"/><text class=node-title x="${x+90}" y="${y+25}" text-anchor="middle">${e(node.label)}</text><text class=node-headline x="${x+90}" y="${y+50}" text-anchor="middle">${e(node.headline)}</text><text class=node-summary x="${x+90}" y="${y+72}" text-anchor="middle">${e(node.summary)}</text><text class=node-action x="${x+90}" y="${y+90}" text-anchor="middle">click to inspect</text></g>`; }
  function drawFlow(data) {
    let host = document.getElementById('flow-host');
    if (!host) { const panel = document.createElement('section'); panel.id = 'flow-panel'; panel.innerHTML = '<h2>Harness flow</h2><p>Each box is a component group with aggregate statistics. Arrows show real outputs; click a group to inspect all of its members.</p><div id="flow-host"></div>'; document.querySelector('main').prepend(panel); host = document.getElementById('flow-host'); }
    const n = data.flow.nodes, pos = {supervisor:[20,20],dispatcher:[225,20],workers:[430,20],reviewers:[635,20],mergers:[840,20],publisher:[1045,20],main:[1250,20],api:[530,190],queue:[1050,190]};
    const edge = (a,b,label) => { const y = a[1]+50, x = (a[0]+180+b[0]-8)/2; return `<path class=edge marker-end="url(#arrow)" d="M ${a[0]+180} ${y} H ${b[0]-8}"/><text class=edge-label x="${x}" y="142" text-anchor="middle">${e(label)}</text>`; };
    const workerToApi = '<path class=edge marker-end="url(#arrow)" d="M520 120 V168 H560 V182"/><text class=edge-label x="500" y="160" text-anchor="end">fixer requests</text>';
    const reviewToApi = '<path class=edge marker-end="url(#arrow)" d="M725 120 V162 H690 V182"/><text class=edge-label x="748" y="160">review requests</text>';
    const publisherToQueue = '<path class=edge marker-end="url(#arrow)" d="M1135 120 V182"/><text class=edge-label x="1148" y="162">flagged changes</text>';
    host.innerHTML = `<svg viewBox="0 0 1450 320" aria-label="OxiDex harness group flow"><defs><marker id="arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8" fill="currentColor"/></marker></defs>${edge(pos.supervisor,pos.dispatcher,'fleet control')}${edge(pos.dispatcher,pos.workers,'task slots')}${edge(pos.workers,pos.reviewers,'reviewed patches')}${edge(pos.reviewers,pos.mergers,'validated commits')}${edge(pos.mergers,pos.publisher,'squad branches')}${edge(pos.publisher,pos.main,'merged PRs')}${workerToApi}${reviewToApi}${publisherToQueue}${Object.entries(pos).map(([key,xy]) => box(key,xy[0],xy[1],n[key])).join('')}</svg>`;
  }
})();
</script>
"""


class Server(ThreadingHTTPServer):
    def __init__(self, address: tuple[str, int], home: Path, controls: bool):
        super().__init__(address, Handler)
        self.home, self.controls, self.token = home, controls, secrets.token_urlsafe(32)
        self.repo_root = REPO_ROOT
        self.snapshot_lock = threading.Lock()
        # Single-flight guard for EVERY control action (terminate/restart/start/
        # scale): acquired non-blocking, so a scale during a restart's
        # bootout/edit/bootstrap sequence gets a 409 instead of interleaving.
        self.control_lock = threading.Lock()


class Handler(BaseHTTPRequestHandler):
    server: Server
    def log_message(self, fmt: str, *args: Any) -> None:
        sys.stderr.write("dashboard: " + fmt % args + "\n")
    def send_json(self, status: HTTPStatus, data: dict[str, Any]) -> None:
        body = json.dumps(data).encode(); self.send_response(status); self.send_header("content-type", "application/json"); self.send_header("cache-control", "no-store"); self.send_header("content-length", str(len(body))); self.end_headers(); self.wfile.write(body)
    def do_GET(self) -> None:  # noqa: N802
        if not host_allowed(self.headers.get("host")): self.send_json(HTTPStatus.FORBIDDEN, {"error": "forbidden Host header (loopback only)"}); return
        if urlparse(self.path).path == "/api/status":
            with self.server.snapshot_lock:
                data = snapshot(self.server.home, self.server.repo_root)
            self.send_json(HTTPStatus.OK, data); return
        if urlparse(self.path).path != "/": self.send_error(HTTPStatus.NOT_FOUND); return
        body = page(self.server.token, self.server.controls); self.send_response(HTTPStatus.OK); self.send_header("content-type", "text/html; charset=utf-8"); self.send_header("cache-control", "no-store"); self.send_header("content-length", str(len(body))); self.end_headers(); self.wfile.write(body)
    def do_POST(self) -> None:  # noqa: N802
        if not host_allowed(self.headers.get("host")): self.send_json(HTTPStatus.FORBIDDEN, {"error": "forbidden Host header (loopback only)"}); return
        if urlparse(self.path).path != "/api/control": self.send_error(HTTPStatus.NOT_FOUND); return
        if not self.server.controls: self.send_json(HTTPStatus.FORBIDDEN, {"error": "controls require --enable-controls"}); return
        if not secrets.compare_digest(self.headers.get("x-control-token", ""), self.server.token): self.send_json(HTTPStatus.FORBIDDEN, {"error": "invalid control token"}); return
        try:
            size = int(self.headers.get("content-length", "0")); request = json.loads(self.rfile.read(size))
            if not isinstance(request, dict): raise ValueError("request body must be a JSON object")
            component_id, action = request["id"], request["action"]
            if action not in {"terminate", "restart", "start", "scale"}: raise ValueError("action must be terminate, restart, start, or scale")
            if action in {"start", "scale"} and component_id != "fleet": self.send_json(HTTPStatus.BAD_REQUEST, {"error": f"action '{action}' applies only to the fleet component"}); return
            if action == "start" and set(request) - {"id", "action"}: self.send_json(HTTPStatus.BAD_REQUEST, {"error": "start takes no fields beyond id and action"}); return
            # Single-flight: covers the whole bootout/plist-edit/bootstrap
            # sequence, so e.g. a scale during a restart gets a 409 instead of
            # interleaving with it.
            if not self.server.control_lock.acquire(blocking=False): self.send_json(HTTPStatus.CONFLICT, {"error": "another control operation is in flight; retry shortly"}); return
            try:
                if action == "start":
                    self.send_json(HTTPStatus.OK, {"message": fleet_start(self.server.repo_root, self.server.home)}); return
                if action == "scale":
                    status, payload = fleet_scale_action(request, self.server.home); self.send_json(status, payload); return
                with self.server.snapshot_lock:
                    component, process = safe_process(snapshot(self.server.home, self.server.repo_root), component_id)
                if component_id == "publisher":
                    self.send_json(HTTPStatus.CONFLICT, {"error": "PR publishing is a dispatcher-owned, short-lived sweep and cannot be controlled independently."}); return
                if action == "terminate":
                    if component_id == "fleet": message = fleet_down(self.server.repo_root)
                    else: message = terminate(process) if process else "component is not running"
                else:
                    if process: terminate(process)
                    fleet_down(self.server.repo_root); message = f"{component['label']}: " + fleet_start(self.server.repo_root, self.server.home)
                self.send_json(HTTPStatus.OK, {"message": message})
            finally:
                self.server.control_lock.release()
        except FleetControlError as exc: self.send_json(exc.status, exc.payload)
        except (ValueError, KeyError, json.JSONDecodeError) as exc: self.send_json(HTTPStatus.BAD_REQUEST, {"error": str(exc)})
        except (OSError, subprocess.SubprocessError) as exc: self.send_json(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": str(exc)})


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument("--host", default="127.0.0.1"); parser.add_argument("--port", type=int, default=8765); parser.add_argument("--home", type=Path, default=DEFAULT_HOME); parser.add_argument("--enable-controls", action="store_true")
    args = parser.parse_args(argv)
    if args.enable_controls and args.host not in {"127.0.0.1", "::1", "localhost"}: parser.error("controls are restricted to localhost")
    server = Server((args.host, args.port), args.home.expanduser(), args.enable_controls)
    print(f"OxiDex harness dashboard ({'controls enabled' if args.enable_controls else 'read-only'}): http://{args.host}:{args.port}")
    try: server.serve_forever(poll_interval=0.5)
    except KeyboardInterrupt: pass
    finally: server.server_close()
    return 0


if __name__ == "__main__": raise SystemExit(main())
