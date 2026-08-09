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
import re
import secrets
import signal
import subprocess  # nosec B404 -- all commands use fixed argv
import sys
import threading
import time
from collections import defaultdict
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
WORKTREE_CACHE_SECONDS = 60.0
FROZEN_AFTER_SECONDS = 30 * 60
CLAIM_STALE_SECONDS = 2 * 60 * 60
_worktree_cache: dict[str, tuple[float, dict[str, Any] | None]] = {}
_publisher_cache: tuple[float, int, dict[str, Any]] | None = None
_github_pr_cache: tuple[float, str, list[dict[str, Any]]] | None = None
_cpu_samples: dict[int, tuple[float, float]] = {}
_manifest_cache: dict[str, dict[str, Any]] = {}
_fleet_log_cache: dict[str, dict[str, Any]] = {}
_tag_claim_cache: tuple[tuple[int, int, int], dict[str, list[tuple[float, str]]]] | None = None


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


def manifest_stats(path: Path) -> dict[str, dict[str, Any]]:
    state, lines = incremental_lines(path, _manifest_cache)
    result = state.setdefault("result", defaultdict(lambda: {"fixer_calls": 0, "reviewer_calls": 0, "critique_calls": 0, "last_task": None, "last_by_phase": {}}))
    for line in lines:
        match = MANIFEST_RE.match(line)
        if not match:
            continue
        stat = result[match["worker"]]
        stat[f"{match['phase']}_calls"] += 1
        at = epoch(match["timestamp"])
        status_match = HTTP_STATUS_RE.search(line)
        task = {
            "timestamp": match["timestamp"], "epoch": at, "phase": match["phase"], "outcome": match["outcome"],
            "http_status": int(status_match["status"]) if status_match else None,
        }
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


def known_workers(home: Path, manifest: dict[str, Any]) -> list[str]:
    names = set(manifest)
    try:
        names.update(path.name.removeprefix("model-fix-") for path in (home / "worktrees" / "parallel-fix").iterdir() if path.is_dir())
    except OSError:
        pass
    return sorted(name for name in names if name and name != "parallel-fix")


def patch_stats(diff_dir: Path, workers: list[str]) -> dict[str, dict[str, int]]:
    result = {name: {"patches_found": 0, "patches_applied": 0} for name in workers}
    try:
        files = list(diff_dir.glob("*.diff"))
    except OSError:
        return result
    for worker in workers:
        for path in files:
            if path.name.endswith(f"-{worker}-applied.diff"):
                result[worker]["patches_found"] += 1
                result[worker]["patches_applied"] += 1
            elif path.name.endswith(f"-{worker}-rejected.diff"):
                result[worker]["patches_found"] += 1
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
    value = {"prs_made": len(events), "last_pr": latest, "recent_prs": recent_prs, "source": source}
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
    commits_since = 0
    for path in (home / "logs" / "squad-status").glob("*-batch.json"):
        entry = json_file(path, {})
        if not isinstance(entry, dict):
            continue
        commits_since += entry.get("commits_since", 0) if isinstance(entry.get("commits_since"), int) else 0
        if entry.get("blocked") is True:
            blocked.append({"squad": path.stem.removesuffix("-batch"), "last_batch_ts": entry.get("last_batch_ts")})
    details = [{"patch_id": entry["patch_id"][:12], "format": entry.get("format"), "squad": entry.get("squad"), "reason": entry.get("reason"), "timestamp": entry.get("ts")} for entry in sorted(queued, key=lambda item: item.get("ts_epoch", 0), reverse=True)]
    return {"judgment_depth": len(queued), "event_count": event_count, "blocked_squads": blocked, "batch_commits": commits_since, "queued": details}


def flow_stats(components: list[dict[str, Any]], manifest: dict[str, dict[str, Any]], publisher: dict[str, Any], queue: dict[str, Any]) -> dict[str, Any]:
    """Graph-ready, group-level throughput across the harness's real hand-offs."""
    calls = {phase: sum(stat.get(f"{phase}_calls", 0) for stat in manifest.values()) for phase in ("fixer", "reviewer", "critique")}
    calls["total"] = sum(calls.values())
    grouped = {role: [component for component in components if component["role"] == role] for role in ("supervisor", "dispatcher", "worker", "reviewer", "merger", "publisher")}
    worker_patches = sum(component["metrics"].get("patches_found", 0) for component in grouped["worker"])
    reviewer_applied = sum(component["metrics"].get("patches_applied", 0) for component in grouped["reviewer"])
    def members(role: str) -> list[dict[str, Any]]:
        return [{"id": component["id"], "role": component["role"], "label": component["label"], "status": component["status"], "pid": component["pid"], "pid_note": component["pid_note"], "process": component["process"], "metrics": component["metrics"], "current_tag": component["current_tag"], "last_task": component["last_task"], "worktree": component["worktree"]} for component in grouped[role]]
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
    return {
        "nodes": {
            "supervisor": group_node("supervisor", "Fleet supervisor", f"{supervisor_running}/{len(grouped['supervisor'])} running", "owns fleet lifecycle", {}),
            "dispatcher": group_node("dispatcher", "Dispatchers", f"{dispatcher_running}/{len(grouped['dispatcher'])} running", f"{worker_running} active workers · {dispatcher_events:,} events", {"active_workers": worker_running, "events": dispatcher_events}),
            "workers": group_node("worker", "Fixer workers", f"{len(grouped['worker'])} workers · {worker_running} active", worker_summary, {"patches_found": worker_patches, "fixer_calls": calls["fixer"], "frozen": worker_frozen}),
            "api": {"label": "Model API", "headline": f"{calls['total']:,} total calls", "summary": f"{calls['fixer']:,} fixer · {calls['reviewer'] + calls['critique']:,} review", "detail": calls},
            "reviewers": group_node("reviewer", "Review gates", f"{len(grouped['reviewer'])} gates · {reviewer_running} active", f"{reviewer_applied:,} passed · {calls['reviewer'] + calls['critique']:,} calls", {"calls": calls["reviewer"] + calls["critique"], "patches_applied": reviewer_applied}),
            "mergers": group_node("merger", "Squad mergers", f"{merger_running}/{len(grouped['merger'])} running", f"{queue['batch_commits']:,} unbatched · {len(queue['blocked_squads'])} blocked", {"blocked_squads": len(queue["blocked_squads"]), "batch_commits": queue["batch_commits"]}),
            "publisher": group_node("publisher", "Publish sweep", f"{publisher['prs_made']:,} PRs made", f"{('sweep active' if publisher_running else 'waiting for dispatcher' if publisher_status == 'waiting' else 'dispatcher unavailable')} · latest {('#' + publisher['last_pr']['number']) if publisher['last_pr'] else '—'}", {"last_pr": publisher["last_pr"], "recent_prs": publisher.get("recent_prs", []), "source": publisher["source"], "status": publisher_status}),
            "queue": {"label": "Judgment queue", "headline": f"{queue['judgment_depth']:,} queued now", "summary": f"{queue['event_count']:,} events · advisory", "detail": {"event_count": queue["event_count"], "blocked_squads": queue["blocked_squads"], "queued": queue["queued"]}},
            "main": {"label": "origin/main", "headline": f"{publisher['prs_made']:,} merged PRs", "summary": f"latest {('#' + publisher['last_pr']['number']) if publisher['last_pr'] else '—'}", "detail": {"merged_prs": publisher["prs_made"], "last_pr": publisher["last_pr"]}},
        }
    }


def item(component_id: str, role: str, label: str, process: dict[str, Any] | None, *, worktree: dict[str, Any] | None = None, last_task: dict[str, Any] | None = None, current_tag: dict[str, Any] | None = None, metrics: dict[str, Any] | None = None, hint: str = "", status: str | None = None, pid_note: str | None = None) -> dict[str, Any]:
    display_pid = None
    if process is not None:
        # A worker's useful live identity is its deepest executable child
        # (rustc/mold/cargo), not the long-lived Python wrapper. Keep the
        # wrapper in ``process`` so terminate() can still safely stop the
        # complete worker process group.
        display_pid = process.get("active_pid", process["pid"]) if role in {"worker", "reviewer"} else process["pid"]
    return {"id": component_id, "role": role, "label": label, "status": status or ("running" if process else "offline"), "pid": display_pid, "pid_note": pid_note, "process": process, "worktree": worktree, "last_task": last_task, "current_tag": current_tag, "metrics": metrics or {}, "action_hint": hint}


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
    state = fleet_rows(home)
    tag_claims = active_tag_claims(home / "logs" / "model-fix-tag-state.json")
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
    components.append(item("fleet", "supervisor", "Fleet supervisor", process_with_active_child(processes, processes.get(supervisor.get("pid"))), worktree=worktree(dispatcher_repo), last_task=last_events.get("[fleet-up]"), metrics={"recorded_state": supervisor.get("state", "not recorded")}, hint="Restart launches the fleet; it salvages work before syncing clean workers."))
    components.append(item("dispatcher", "dispatcher", "Task dispatcher", process_with_active_child(processes, dispatcher), worktree=worktree(dispatcher_repo), last_task=last_events.get("[dispatcher]"), metrics={"events": event_counts.get("[dispatcher]", 0)}, hint="Workers are dispatcher-owned; restart recreates the fleet safely."))
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
        process = child_process_usage(processes, running_workers.get(worker))
        worktree_data = worktree(base / f"model-fix-{worker}")
        worker_status, worker_hint = worker_runtime_state(process, stat.get("last_task")) if process else ("offline", "A prior worker record has no live process or PID.") if stat else ("archived", "Historical worktree entry with no recorded worker activity.")
        worker_task = latest_phase_task(stat, "fixer")
        reviewer_task = latest_phase_task(stat, "reviewer", "critique")
        current_tag = tag_claims.get(worker) if process else None
        worker_metrics = {**patch, "fixer_calls": stat.get("fixer_calls", 0)}
        reviewer_metrics = {"patches_applied": patch["patches_applied"], "reviewer_calls": stat.get("reviewer_calls", 0), "critique_calls": stat.get("critique_calls", 0)}
        if worker_status == "frozen":
            worker_metrics["freeze_reason"] = worker_hint
            reviewer_metrics["freeze_reason"] = worker_hint
        components.append(item(f"worker:{worker}", "worker", worker, process, worktree=worktree_data, last_task=worker_task, current_tag=current_tag, metrics=worker_metrics, hint=worker_hint, status=worker_status))
        components.append(item(f"reviewer:{worker}", "reviewer", f"{worker} reviewer", process, worktree=worktree_data, last_task=reviewer_task, current_tag=current_tag, metrics=reviewer_metrics, hint=worker_hint, status=worker_status))
    merger_seen: set[str] = set()
    for squad, data in locks:
        pid = data.get("pid")
        merger_seen.add(squad)
        components.append(item(f"merger:{squad}", "merger", f"{squad} merger", child_process_usage(processes, processes.get(pid)), worktree=worktree(home / "worktrees" / "squad-staging" / squad), last_task=last_events.get(f"[merger:{squad}]"), metrics={"heartbeat_ts": data.get("heartbeat_ts")}, hint="A live supervisor respawns a terminated merger; Restart relaunches the fleet."))
    for row in processes.values():
        if not runs_script(row["command"], "squad_merge_loop.py"):
            continue
        squad = squad_from_command(row["command"]) or f"pid-{row['pid']}"
        if squad not in merger_seen:
            components.append(item(f"merger:{squad}", "merger", f"{squad} merger", child_process_usage(processes, row), worktree=worktree(home / "worktrees" / "squad-staging" / squad)))
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
    components.append(item("publisher", "publisher", "PR publisher", publisher_process, worktree=worktree(publisher_repo), last_task=publisher_task, metrics=publisher_metric, hint=publisher_hint, status=publisher_status, pid_note=publisher_note))
    running = sum(component["status"] == "running" for component in components)
    waiting = sum(component["status"] == "waiting" for component in components)
    frozen = sum(component["status"] == "frozen" for component in components)
    offline = sum(component["status"] == "offline" for component in components)
    archived = sum(component["status"] == "archived" for component in components)
    queue = queue_stats(home)
    return {"generated_at": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"), "summary": {"components": len(components), "running": running, "waiting": waiting, "frozen": frozen, "offline": offline, "archived": archived, "idle": len(components) - running - waiting - frozen - offline - archived}, "flow": flow_stats(components, manifest, publisher_metric, queue), "components": components}


def safe_process(data: dict[str, Any], component_id: str) -> tuple[dict[str, Any], dict[str, Any] | None]:
    component = next((entry for entry in data["components"] if entry["id"] == component_id), None)
    if component is None:
        raise ValueError("unknown component")
    process = component["process"]
    if process and not any(marker in process["command"] for marker in ALLOWED):
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


def fleet_down(repo_root: Path) -> None:
    subprocess.run([str(repo_root / "scripts" / "fleet_up.sh"), "--down"], cwd=repo_root, capture_output=True, text=True, timeout=45, check=False)


def fleet_start(repo_root: Path, home: Path) -> str:
    # The LaunchAgent holds the configured worker count and squad-mode flags.
    # Never recreate the fleet by running fleet_up.sh with its defaults here.
    service = f"gui/{os.getuid()}/com.oxidex.fleet"
    result = subprocess.run(["launchctl", "kickstart", "-k", service], capture_output=True, text=True, timeout=20, check=False)
    if result.returncode:
        detail = result.stderr.strip() or result.stdout.strip() or f"exit {result.returncode}"
        raise OSError(f"could not restart configured fleet LaunchAgent {service}: {detail}")
    return "fleet restart requested through the configured LaunchAgent"


def page(token: str, controls: bool) -> bytes:
    # Token is embedded only in this same-origin page.  Controls cannot be enabled on non-localhost.
    body = """<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>OxiDex Harness</title>
<style>:root{color-scheme:dark;--bg:#07111f;--card:#0d1b2d;--edge:#203652;--txt:#e5edf8;--muted:#95a8bf;--ok:#37d996;--idle:#f4bd4f;--blue:#62b0ff}*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--txt);font:14px system-ui}header{position:sticky;top:0;padding:16px 22px;background:#091625ed;border-bottom:1px solid var(--edge);z-index:2}h1{font-size:20px;margin:0}#summary,.muted,.role{color:var(--muted)}main{padding:18px;max-width:1800px;margin:auto}.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(320px,1fr));gap:12px}.card{background:var(--card);border:1px solid var(--edge);border-radius:10px;padding:14px;min-height:220px}.head{display:flex;justify-content:space-between;gap:10px}.label{font-weight:700;font-size:16px}.dot{display:inline-block;width:9px;height:9px;background:var(--idle);border-radius:50%;margin-right:5px}.running .dot{background:var(--ok);box-shadow:0 0 12px #37d99688}dl{display:grid;grid-template-columns:max-content 1fr;gap:5px 10px;margin:13px 0}dt{color:var(--muted)}dd{margin:0;word-break:break-word}canvas{display:block;width:100%;height:44px;background:#091626;border-radius:5px;margin:10px 0}button{color:var(--txt);background:#10243c;border:1px solid var(--edge);border-radius:7px;padding:6px 9px;cursor:pointer;margin:0 6px 10px 0}button:hover{border-color:var(--blue)}.danger{border-color:#7e3543;color:#ffd9dc}.hidden{display:none}#notice{position:fixed;right:16px;bottom:16px;background:#183553;border:1px solid var(--blue);padding:10px;border-radius:8px;max-width:500px}</style>
<header><h1>OxiDex harness</h1><div id=summary>Loading…</div></header><main><div id=filters></div><section id=cards class=grid></section></main><div id=notice class=hidden></div>
<script>const TOKEN=__TOKEN__,CONTROLS=__CONTROLS__;let active='all',hist={};const f=n=>n==null?'—':new Intl.NumberFormat().format(n),b=n=>n==null?'—':(n/1048576).toFixed(1)+' MiB',cpu=n=>n==null?'—':(n<1?n.toFixed(2):n.toFixed(1))+'%',e=s=>{let x=document.createElement('i');x.textContent=s??'';return x.innerHTML},age=x=>{if(!x)return '—';let s=Math.max(0,Date.now()/1000-new Date(x.timestamp||x).getTime()/1000);return s<60?Math.round(s)+'s ago':s<3600?Math.round(s/60)+'m ago':s<86400?Math.round(s/3600)+'h ago':Math.round(s/86400)+'d ago'};function chart(c,id,v){let a=hist[id]||(hist[id]=[]);a.push(v||0);if(a.length>60)a.shift();let z=c.getContext('2d'),w=c.width=c.clientWidth*devicePixelRatio,h=c.height=c.clientHeight*devicePixelRatio;z.clearRect(0,0,w,h);z.strokeStyle='#62b0ff';z.lineWidth=2*devicePixelRatio;z.beginPath();a.forEach((v,i)=>{let X=i*w/59,Y=h-Math.min(v,100)*h/100;i?z.lineTo(X,Y):z.moveTo(X,Y)});z.stroke()}function card(c){let p=c.process||{},m=c.metrics||{},t=c.last_task||{},w=c.worktree||{},life=c.role==='worker'?`found ${f(m.patches_found)} · applied ${f(m.patches_applied)}`:c.role==='reviewer'?`applied ${f(m.patches_applied)} · reviews ${f(m.reviewer_calls)}`:c.role==='publisher'?`PRs ${f(m.prs_made)} · last ${m.last_pr?'#'+m.last_pr.number+' '+age(m.last_pr):'—'} (${e(m.source||'event log')})`:'';let ctl=CONTROLS?`<div><button class=danger onclick="act(decodeURIComponent('${encodeURIComponent(c.id)}'),'terminate')">Terminate</button><button onclick="act(decodeURIComponent('${encodeURIComponent(c.id)}'),'restart')">Restart</button></div>`:'';return `<article class="card ${c.status}"><div class=head><div><div class=label>${e(c.label)}</div><div class=role>${e(c.role)}</div></div><div><span class=dot></span>${c.status}</div></div><dl><dt>PID</dt><dd>${p.pid??'—'} ${p.elapsed?'· '+e(p.elapsed):''}</dd><dt>CPU (1s) / memory</dt><dd>${cpu(p.cpu_percent)} / ${b(p.memory_bytes)}</dd><dt>Worktree</dt><dd title="${e(w.path||'')}">${e(w.path||'—')}${w.dirty_files?' · '+w.dirty_files+' dirty':''}</dd><dt>Last task</dt><dd>${t.phase?e(t.phase)+' ('+e(t.outcome)+'), '+age(t):'—'}</dd><dt>Lifetime</dt><dd>${life}</dd></dl><canvas data-id="${e(c.id)}"></canvas><div class=muted>${e(c.action_hint||'')}</div>${ctl}</article>`}function render(d){let roles=['all',...new Set(d.components.map(x=>x.role))];filters.innerHTML=roles.map(r=>`<button onclick="active='${r}';render(latest)">${r}</button>`).join('');let xs=d.components.filter(x=>active==='all'||x.role===active);cards.innerHTML=xs.map(card).join('');document.querySelectorAll('canvas').forEach(x=>{let c=xs.find(q=>q.id===x.dataset.id);chart(x,c.id,c.process?.cpu_percent)});summary.textContent=`${d.summary.running} running · ${d.summary.idle} idle · ${d.summary.components} components · ${new Date(d.generated_at).toLocaleTimeString()} · ${CONTROLS?'controls enabled':'read-only'}`}async function poll(){try{latest=await fetch('/api/status').then(r=>r.json());render(latest)}catch(x){notice.textContent='Dashboard request failed: '+x;notice.classList.remove('hidden')}}async function act(id,action){let c=latest.components.find(x=>x.id===id),extra=action==='restart'&&c.role!=='supervisor'?' This restarts the fleet to recreate dispatcher-owned work safely.':'';if(!confirm(action+' '+c.label+'?'+extra))return;let r=await fetch('/api/control',{method:'POST',headers:{'content-type':'application/json','x-control-token':TOKEN},body:JSON.stringify({id,action})}),d=await r.json();notice.textContent=d.message||d.error;notice.classList.remove('hidden');setTimeout(()=>notice.classList.add('hidden'),6000);poll()}let latest;poll();setInterval(poll,1000);</script>"""
    return (body.replace("__TOKEN__", json.dumps(token)).replace("__CONTROLS__", "true" if controls else "false") + FLOW_SCRIPT).encode()


FLOW_SCRIPT = r"""
<style>
#flow-panel{margin:0 0 16px;padding:12px;background:var(--card);border:1px solid var(--edge);border-radius:10px}#flow-panel h2{font-size:16px;margin:0 0 5px}#flow-panel p{margin:0 0 8px;color:var(--muted)}#flow-host svg{display:block;width:100%;min-height:330px}#flow-host .edge{stroke:var(--muted);stroke-width:1.5;fill:none;color:var(--muted)}#flow-host .edge-label{fill:var(--muted);font:11px system-ui}#flow-host .node{fill:var(--bg);stroke:var(--edge);stroke-width:1.5;cursor:pointer}#flow-host .node:hover{stroke:var(--blue);stroke-width:2.5}#flow-host .node-title{fill:var(--txt);font:500 15px system-ui}#flow-host .node-headline{fill:var(--txt);font:12px system-ui}#flow-host .node-summary{fill:var(--muted);font:11px system-ui}#flow-host .node-action{fill:var(--blue);font:10px system-ui}#filters{display:flex;align-items:center;gap:10px;margin:0 0 10px}#component-filter{min-width:min(360px,100%);padding:8px 10px;background:var(--bg);border:1px solid var(--edge);border-radius:7px;color:var(--txt);font:inherit}.table-wrap{overflow:auto;border:1px solid var(--edge);border-radius:10px;background:var(--card)}#component-table{width:max-content;min-width:1680px;border-collapse:collapse;font-size:13px}#component-table th{position:sticky;top:0;background:#0d1b2d;z-index:1;text-align:left;border-bottom:1px solid var(--edge);white-space:nowrap}#component-table th button{margin:0;border:0;padding:10px;background:transparent;color:var(--muted);font:inherit;font-weight:700}#component-table th button:hover{color:var(--txt)}.column-heading{display:flex;align-items:center;min-width:0}.column-heading button{overflow:hidden;text-overflow:ellipsis;flex:1;text-align:left}.column-resize{width:8px;align-self:stretch;cursor:col-resize;touch-action:none;background:linear-gradient(90deg,transparent 3px,var(--edge) 3px,var(--edge) 4px,transparent 4px)}.column-resize:hover,.column-resize.resizing{background:linear-gradient(90deg,transparent 2px,var(--blue) 2px,var(--blue) 5px,transparent 5px)}#component-table td{padding:8px 10px;border-bottom:1px solid var(--edge);vertical-align:middle;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}#component-table tr:last-child td{border-bottom:0}#component-table tr.running{background:#37d99609}.status-running{color:var(--ok)}.status-idle{color:var(--idle)}#component-table canvas{display:block;width:118px;height:30px;margin:0;background:#091626;border-radius:4px}.row-actions button{margin:0 4px 0 0;padding:4px 7px}dialog{box-sizing:border-box;width:min(1600px,98vw);max-width:98vw;max-height:90vh;overflow:auto;background:var(--card);color:var(--txt);border:1px solid var(--edge);border-radius:10px;padding:16px}dialog::backdrop{background:#0008}dialog table{border-collapse:collapse;font-size:13px}dialog th,dialog td{padding:6px;text-align:left;vertical-align:top;overflow-wrap:anywhere;border-bottom:1px solid var(--edge)}dialog th{color:var(--muted)}.flow-close{float:right}.flow-reason{max-width:420px;white-space:normal}.flow-guide{margin:0 0 16px;padding:12px;background:var(--bg);border:1px solid var(--edge);border-radius:8px}.flow-guide h3{margin:0 0 8px;font-size:15px}.flow-guide svg{display:block;width:100%;min-width:720px;height:auto}.flow-guide-wrap{overflow:auto}.flow-guide .guide-node{fill:var(--card);stroke:var(--edge);stroke-width:1.5}.flow-guide .guide-node.focus{stroke:var(--blue);stroke-width:2.5}.flow-guide .guide-title{fill:var(--txt);font:600 12px system-ui}.flow-guide .guide-copy{fill:var(--muted);font:10px system-ui}.flow-guide .guide-edge{stroke:var(--muted);stroke-width:1.5;fill:none;color:var(--muted)}.flow-guide .guide-feedback{stroke:var(--blue);stroke-width:1.5;fill:none;color:var(--blue)}.flow-guide .guide-label{fill:var(--blue);font:10px system-ui}.flow-parts{display:grid;grid-template-columns:repeat(auto-fit,minmax(190px,1fr));gap:8px;margin-top:10px}.flow-part{padding:8px;border:1px solid var(--edge);border-radius:6px}.flow-part strong{display:block;margin-bottom:3px}.flow-part p{margin:0;color:var(--muted);font-size:12px}
</style>
<style>
#component-table tr.waiting{background:#62b0ff09}#component-table tr.frozen{background:#d481ff14}#component-table tr.offline{background:#ff8e8e09}#component-table tr.archived{opacity:.72}.status-waiting{color:var(--blue)}.status-frozen{color:#d481ff}.status-offline{color:#ff8e8e}.status-archived{color:var(--muted)}.status-stopped{color:#ff8e8e}.model-result-ok{color:var(--ok)}.model-result-error{color:#ff8e8e}.model-result-retry{color:var(--idle)}.model-result-unknown{color:var(--muted)}.flow-dialog-controls{display:flex;justify-content:flex-end;gap:8px;margin-bottom:12px}.flow-dialog-controls button[hidden]{display:none}.flow-specific{margin:0 0 16px;padding:12px;background:var(--bg);border:1px solid var(--edge);border-radius:8px}.flow-specific h3{margin:0 0 6px}.flow-specific p{margin:0 0 8px;color:var(--muted)}.flow-specific ul{margin:8px 0;padding-left:20px}.flow-specific li{margin:5px 0}.flow-state-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:8px;margin-top:10px}.flow-state{padding:7px;border:1px solid var(--edge);border-radius:6px}.flow-state strong{display:block}.flow-state span{color:var(--muted);font-size:12px}.flow-stat-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(170px,1fr));gap:9px}.flow-stat{padding:11px;border:1px solid var(--edge);border-radius:7px;background:var(--bg)}.flow-stat-label{display:block;color:var(--muted);font-size:12px;margin-bottom:4px}.flow-stat-value{font-size:17px;font-weight:650;overflow-wrap:anywhere}.flow-member-controls{display:flex;align-items:center;gap:10px;margin:0 0 9px}.flow-member-controls label{color:var(--muted)}#flow-member-filter{min-width:min(360px,100%);padding:7px 9px;background:var(--bg);border:1px solid var(--edge);border-radius:7px;color:var(--txt);font:inherit}.flow-member-table-wrap{overflow-x:auto;overflow-y:auto;max-width:100%;border:1px solid var(--edge);border-radius:8px}.flow-member-table{width:max-content;min-width:1420px;table-layout:auto}.flow-member-table th{cursor:grab;white-space:nowrap;user-select:none}.flow-member-table th.dragging{opacity:.5}.flow-member-table th button{margin:0;padding:0;background:transparent;border:0;color:var(--muted);font:inherit;font-weight:700;cursor:pointer}.flow-member-table th button:hover{color:var(--txt)}.flow-member-hint{margin:0 0 9px;color:var(--muted);font-size:12px}.publisher-prs-note{margin:0 0 9px;color:var(--muted)}.publisher-prs-table{width:max-content;min-width:1180px;table-layout:auto}.publisher-prs-table th,.publisher-prs-table td{white-space:nowrap}.publisher-prs-table td:nth-child(4){white-space:normal;min-width:420px}.publisher-prs-table a{color:var(--blue)}
</style>
<script>
(() => {
  let query = '', sortKey = 'name', sortDirection = 1, tableReady = false;
  const rows = new Map();
  const columns = [['type','Type',100],['name','Name',150],['status','Status',100],['pid','Active PID',88],['process','Process',180],['cpu','CPU (tree, 1s)',104],['chart','CPU graph',138],['memory','Memory (tree)',120],['lifetime','Lifetime / diagnosis',260],['tag','Current tag',260],['last','Last found',150],['model','Last model result',150],['worktree','Worktree',320],['behind','Behind origin/main',130],['actions','Actions',145]];
  const minimumWidth = {type:80, name:100, status:80, pid:64, process:120, cpu:76, chart:128, memory:90, lifetime:140, tag:150, last:120, model:130, worktree:180, behind:120, actions:130};
  const savedWidths = (() => { try { return JSON.parse(localStorage.getItem('oxidex-component-column-widths') || '{}'); } catch (_) { return {}; } })();
  const widthFor = (key, fallback) => Math.max(minimumWidth[key], Number(savedWidths[key]) || fallback);
  function chart(canvas, id, sample) {
    const values = hist[id] || (hist[id] = []);
    values.push(sample || 0); if (values.length > 60) values.shift();
    const width = Math.round((canvas.clientWidth || 118) * devicePixelRatio), height = Math.round((canvas.clientHeight || 30) * devicePixelRatio);
    if (canvas.width !== width || canvas.height !== height) { canvas.width = width; canvas.height = height; }
    const context = canvas.getContext('2d'); context.clearRect(0, 0, width, height); context.strokeStyle = '#62b0ff'; context.lineWidth = 2 * devicePixelRatio; context.beginPath();
    values.forEach((sampleValue, index) => { const x = index * width / 59, y = height - Math.min(sampleValue, 100) * height / 100; index ? context.lineTo(x, y) : context.moveTo(x, y); }); context.stroke();
  }
  const value = (component, key) => {
    const process = component.process || {}, metrics = component.metrics || {}, task = component.last_task || {}, worktree = component.worktree || {};
    const lifetime = component.role === 'worker' ? metrics.patches_found : component.role === 'reviewer' ? metrics.patches_applied : component.role === 'publisher' ? metrics.prs_made : 0;
    return {type:component.role, name:component.label, status:component.status, pid:component.pid || 0, process:activeCommand(process), cpu:process.cpu_percent || 0, memory:process.memory_bytes || 0, lifetime:lifetime || 0, tag:currentTag(component), last:task.epoch || 0, model:Number.isInteger(task.http_status) ? task.http_status : -1, worktree:worktree.path || '', behind:Number.isInteger(worktree.behind) ? worktree.behind : -1}[key] ?? '';
  };
  const activeCommand = process => process?.active_command || process?.command || '';
  const processName = process => process?.display_name || (activeCommand(process).trim().split(/\s+/)[0] || '—').split('/').pop();
  const processText = process => processName(process);
  const pidText = component => component.pid == null ? '—' : `${component.pid}${component.pid_note ? ` · ${component.pid_note}` : ''}`;
  const lifetime = component => {
    const metrics = component.metrics || {};
    if (component.status === 'frozen') return metrics.freeze_reason || 'Potentially frozen: inspect PID, CPU, and last activity.';
    if (component.role === 'worker') return `found ${f(metrics.patches_found)} · applied ${f(metrics.patches_applied)}`;
    if (component.role === 'reviewer') return `applied ${f(metrics.patches_applied)} · reviews ${f(metrics.reviewer_calls)}`;
    if (component.role === 'publisher') return `PRs ${f(metrics.prs_made)}`;
    return '—';
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
    if (!query) return true;
    const process = component.process || {}, worktree = component.worktree || {};
    return [component.role, component.label, component.status, component.pid_note, process.pid, process.command, process.active_command, currentTag(component), currentTagTitle(component), worktree.path, modelResult(component)].join(' ').toLowerCase().includes(query);
  };
  function ensureTable() {
    if (tableReady) return;
    tableReady = true;
    filters.innerHTML = '<label for="component-filter">Filter components</label><input id="component-filter" type="search" placeholder="PDF, reviewer, canon-13…" autocomplete="off">';
    cards.className = 'table-wrap';
    cards.innerHTML = `<table id="component-table"><colgroup>${columns.map(([key,,fallback]) => `<col data-column="${key}" style="width:${widthFor(key, fallback)}px">`).join('')}</colgroup><thead><tr>${columns.map(([key,label]) => `<th><div class="column-heading">${key === 'chart' || key === 'actions' ? `<span>${e(label)}</span>` : `<button type="button" data-sort="${key}">${e(label)}</button>`}<span class="column-resize" data-column="${key}" role="separator" aria-label="Resize ${e(label)} column" title="Drag to resize"></span></div></th>`).join('')}</tr></thead><tbody></tbody></table>`;
    document.getElementById('component-filter').addEventListener('input', event => { query = event.target.value.trim().toLowerCase(); renderTable(latest); });
    cards.querySelectorAll('[data-sort]').forEach(button => button.addEventListener('click', () => { const key = button.dataset.sort; sortDirection = sortKey === key ? -sortDirection : 1; sortKey = key; renderTable(latest); }));
    cards.querySelectorAll('.column-resize').forEach(handle => handle.addEventListener('pointerdown', event => {
      event.preventDefault(); const key = handle.dataset.column, column = cards.querySelector(`col[data-column="${key}"]`), startX = event.clientX, startWidth = column.getBoundingClientRect().width;
      const resize = move => { const next = Math.max(minimumWidth[key], Math.round(startWidth + move.clientX - startX)); column.style.width = `${next}px`; savedWidths[key] = next; };
      const finish = () => { handle.classList.remove('resizing'); document.removeEventListener('pointermove', resize); document.removeEventListener('pointerup', finish); document.removeEventListener('pointercancel', finish); try { localStorage.setItem('oxidex-component-column-widths', JSON.stringify(savedWidths)); } catch (_) {} };
      handle.classList.add('resizing'); handle.setPointerCapture(event.pointerId); document.addEventListener('pointermove', resize); document.addEventListener('pointerup', finish); document.addEventListener('pointercancel', finish);
    }));
  }
  function cell(row, name) { return row.querySelector(`[data-field="${name}"]`); }
  function createRow(component) {
    const row = document.createElement('tr');
    row.dataset.id = component.id;
    row.innerHTML = '<td data-field="type"></td><td data-field="name"></td><td data-field="status"></td><td data-field="pid"></td><td class="process-name" data-field="process"></td><td data-field="cpu"></td><td><canvas data-field="chart"></canvas></td><td data-field="memory"></td><td data-field="lifetime"></td><td data-field="tag"></td><td data-field="last"></td><td data-field="model"></td><td><span class="worktree-path" data-field="worktree"></span></td><td data-field="behind"></td><td class="row-actions" data-field="actions"></td>';
    if (CONTROLS) { const actions = cell(row, 'actions'); if (component.role === 'publisher') { actions.textContent = 'Dispatcher-owned'; actions.title = 'The publisher runs only inside a dispatcher round; it cannot be restarted independently.'; } else { [['terminate','Terminate','danger'],['restart','Restart','']].forEach(([action,label,className]) => { const button = document.createElement('button'); button.textContent = label; button.className = className; button.addEventListener('click', () => act(component.id, action)); actions.append(button); }); } }
    rows.set(component.id, row);
    return row;
  }
  function updateRow(row, component) {
    const process = component.process || {}, worktree = component.worktree || {};
    row.className = component.status;
    cell(row,'type').textContent = component.role;
    cell(row,'name').textContent = component.label;
    const status = cell(row,'status'); status.textContent = component.status; status.className = `status-${component.status}`;
    cell(row,'pid').textContent = pidText(component);
    const processCell = cell(row,'process'); processCell.textContent = processText(process); processCell.title = activeCommand(process) || process.command || '';
    cell(row,'cpu').textContent = cpu(process.cpu_percent);
    cell(row,'memory').textContent = b(process.memory_bytes);
    cell(row,'lifetime').textContent = lifetime(component);
    const tag = cell(row,'tag'); tag.textContent = currentTag(component); tag.title = currentTagTitle(component);
    cell(row,'last').textContent = lastFound(component);
    const model = cell(row,'model'); model.textContent = modelResult(component); model.className = modelResultClass(component);
    const worktreeCell = cell(row,'worktree'); worktreeCell.textContent = worktree.path ? `${worktree.path}${worktree.dirty_files ? ` · ${worktree.dirty_files} dirty` : ''}` : '—'; worktreeCell.title = worktree.path || '';
    cell(row,'behind').textContent = Number.isInteger(worktree.behind) ? `${f(worktree.behind)} commits` : '—';
    chart(cell(row,'chart'), component.id, process.cpu_percent);
  }
  function renderTable(data) {
    ensureTable();
    const states = [`${data.summary.running} running`, data.summary.waiting ? `${data.summary.waiting} waiting` : '', data.summary.frozen ? `${data.summary.frozen} frozen` : '', data.summary.offline ? `${data.summary.offline} offline` : '', data.summary.archived ? `${data.summary.archived} archived` : '', data.summary.idle ? `${data.summary.idle} idle` : ''].filter(Boolean).join(' · ');
    summary.textContent = `${states} · ${data.summary.components} components · ${new Date(data.generated_at).toLocaleTimeString()} · ${CONTROLS ? 'controls enabled' : 'read-only'}`;
    const current = new Set(data.components.map(component => component.id));
    for (const [id,row] of rows) if (!current.has(id)) { row.remove(); rows.delete(id); }
    for (const component of data.components) updateRow(rows.get(component.id) || createRow(component), component);
    const visible = data.components.filter(match).sort((left,right) => { const a = value(left,sortKey), b = value(right,sortKey); return typeof a === 'number' && typeof b === 'number' ? sortDirection * (a - b) : sortDirection * String(a).localeCompare(String(b)); });
    const shown = new Set(visible.map(component => component.id));
    for (const [id,row] of rows) row.hidden = !shown.has(id);
    const body = cards.querySelector('tbody'); const fragment = document.createDocumentFragment(); visible.forEach(component => fragment.append(rows.get(component.id))); body.append(fragment);
  }
  render = data => { renderTable(data); drawFlow(data); refreshOpenMemberTable(); };
  function memberStats(metrics) {
    const names = {patches_found:'found', patches_applied:'applied', fixer_calls:'fixer calls', reviewer_calls:'reviewer calls', critique_calls:'critique calls', events:'events', prs_made:'PRs made'};
    return Object.entries(metrics || {}).filter(([, value]) => typeof value === 'number').map(([key, value]) => `${names[key] || key}: ${f(value)}`).join(' · ') || '—';
  }
  function specificGuide(key) {
    const docs = {
      supervisor: ['Fleet supervisor', 'Owns the fleet lifecycle and starts the dispatcher, merger, and judgment tiers.', ['A restart is fleet-wide.', 'It does not create code changes itself.']],
      dispatcher: ['Task dispatcher', 'Allocates worker slots, creates each worker process, and invokes the publish sweep after a round.', ['If it is stopped, no new fixer processes can be created.', 'It is the owner of the PR publisher lifecycle.']],
      workers: ['Fixer workers', 'A fixer is the worker process that asks the Model API for a candidate patch, applies it in its isolated worktree, and runs local validation.', ['A reviewed rejection returns a concrete critique to this same worker for another attempt.', 'An approved commit moves right to the squad merger.', 'Process and Active PID both show the deepest live child, such as rustc or mold, rather than the Python worker wrapper. CPU and Memory are totals for the full worker process tree; control actions still safely target the wrapper process group.', 'Last model result is the actual HTTP status for the last fixer request: 2xx/3xx is healthy; 4xx/5xx needs attention. Older requests honestly say when no status was recorded.', 'Behind origin/main compares each worktree to the locally fetched origin/main reference; a dashboard refresh does not fetch Git.']],
      reviewers: ['Review gates', 'The review phase checks a candidate patch after local validation. It runs inside the worker process, so it shares the worker PID.', ['Approved work proceeds to a squad merger.', 'A rejection becomes fixer feedback, not a terminal failure.', 'Last model result shows the HTTP result for the latest reviewer or critique request.']],
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
    ['label', 'Component'], ['status', 'Status'], ['pid', 'Active PID / owner'], ['process', 'Process'], ['cpu', 'CPU (tree, 1s)'], ['memory', 'Memory (tree)'], ['behind', 'Behind origin/main'], ['lifetime', 'Lifetime stats / diagnosis'], ['tag', 'Current tag'], ['last', 'Last activity'], ['model', 'Last model result'], ['worktree', 'Worktree'],
  ];
  const memberDisplay = (member, key) => {
    const process = member.process || {}, worktree = member.worktree || {};
    if (key === 'label') return member.label;
    if (key === 'status') return member.status;
    if (key === 'pid') return member.pid == null ? '—' : `${member.pid}${member.pid_note ? ` · ${member.pid_note}` : ''}`;
    if (key === 'process') return processText(process);
    if (key === 'cpu') return cpu(process.cpu_percent);
    if (key === 'memory') return b(process.memory_bytes);
    if (key === 'behind') return Number.isInteger(worktree.behind) ? `${f(worktree.behind)} commits` : '—';
    if (key === 'tag') return currentTag(member);
    if (key === 'last') return member.last_task ? `${member.last_task.phase} · ${age(member.last_task)}` : '—';
    if (key === 'model') return modelResult(member);
    if (key === 'lifetime') return member.metrics?.freeze_reason || memberStats(member.metrics);
    if (key === 'worktree') return worktree.path || '—';
    return '—';
  };
  const memberSortValue = (member, key) => {
    const process = member.process || {}, worktree = member.worktree || {};
    return {label:member.label, status:member.status, pid:member.pid || -1, process:activeCommand(process), cpu:process.cpu_percent || 0, memory:process.memory_bytes || 0, behind:Number.isInteger(worktree.behind) ? worktree.behind : -1, lifetime:memberDisplay(member, 'lifetime'), tag:currentTag(member), last:member.last_task?.epoch || 0, model:Number.isInteger(member.last_task?.http_status) ? member.last_task.http_status : -1, worktree:worktree.path || ''}[key] ?? '';
  };
  function updateMemberCell(cell, member, key) {
    const text = memberDisplay(member, key);
    const title = key === 'process' ? activeCommand(member.process || {}) : key === 'tag' ? currentTagTitle(member) : key === 'worktree' ? (member.worktree || {}).path || '' : '';
    cell.textContent = text;
    cell.className = key === 'model' ? modelResultClass(member) : '';
    if (title) cell.title = title; else cell.removeAttribute('title');
  }
  function refreshOpenMemberTable() {
    const dialog = document.getElementById('flow-detail'), table = document.getElementById('flow-member-table');
    if (!dialog?.open || !table || flowShowingOverview || flowShowingPrs || !selectedFlowNode) return;
    const node = latest.flow.nodes[selectedFlowNode];
    if (!node?.members) return;
    const members = new Map(node.members.map(member => [member.id, member]));
    table.querySelectorAll('tbody tr[data-member-id]').forEach(row => {
      const member = members.get(row.dataset.memberId);
      if (!member) return;
      row.querySelectorAll('[data-member-field]').forEach(cell => updateMemberCell(cell, member, cell.dataset.memberField));
    });
  }
  function memberColumnOrder() {
    const fallback = memberColumns.map(([key]) => key);
    try {
      const saved = JSON.parse(localStorage.getItem('oxidex-flow-member-column-order') || '[]');
      return Array.isArray(saved) && saved.length === fallback.length && saved.every(key => fallback.includes(key)) ? saved : fallback;
    } catch (_) { return fallback; }
  }
  function setupMemberTable(node) {
    const table = document.getElementById('flow-member-table'), filter = document.getElementById('flow-member-filter');
    if (!table || !filter) return;
    let query = '', sortKey = 'label', sortDirection = 1, dragKey = null, order = memberColumnOrder();
    const column = key => memberColumns.find(([candidate]) => candidate === key);
    const renderMembers = () => {
      const members = node.members.filter(member => !query || [member.label, member.status, member.pid, member.pid_note, activeCommand(member.process || {}), currentTag(member), currentTagTitle(member), (member.worktree || {}).path, memberDisplay(member, 'model')].join(' ').toLowerCase().includes(query)).sort((left, right) => {
        const a = memberSortValue(left, sortKey), b = memberSortValue(right, sortKey);
        return typeof a === 'number' && typeof b === 'number' ? sortDirection * (a - b) : sortDirection * String(a).localeCompare(String(b));
      });
      table.innerHTML = `<thead><tr>${order.map(key => { const [, label] = column(key); return `<th draggable="true" data-member-column="${key}" title="Drag to move this column"><button type="button" data-member-sort="${key}">${e(label)}${sortKey === key ? sortDirection > 0 ? ' ↑' : ' ↓' : ''}</button></th>`; }).join('')}</tr></thead><tbody>${members.map(member => `<tr data-member-id="${e(member.id)}">${order.map(key => { const text = memberDisplay(member, key), title = key === 'process' ? activeCommand(member.process || {}) : key === 'tag' ? currentTagTitle(member) : key === 'worktree' ? (member.worktree || {}).path || '' : ''; return `<td data-member-field="${key}"${key === 'model' ? ` class="${modelResultClass(member)}"` : ''}${title ? ` title="${e(title)}"` : ''}>${e(text)}</td>`; }).join('')}</tr>`).join('')}</tbody>`;
      table.querySelectorAll('[data-member-sort]').forEach(button => button.addEventListener('click', () => { const next = button.dataset.memberSort; sortDirection = sortKey === next ? -sortDirection : 1; sortKey = next; renderMembers(); }));
      table.querySelectorAll('[data-member-column]').forEach(header => {
        header.addEventListener('dragstart', event => { dragKey = header.dataset.memberColumn; header.classList.add('dragging'); event.dataTransfer.effectAllowed = 'move'; event.dataTransfer.setData('text/plain', dragKey); });
        header.addEventListener('dragend', () => { dragKey = null; header.classList.remove('dragging'); });
        header.addEventListener('dragover', event => { if (dragKey) event.preventDefault(); });
        header.addEventListener('drop', event => { event.preventDefault(); const target = header.dataset.memberColumn; if (!dragKey || dragKey === target) return; const from = order.indexOf(dragKey), to = order.indexOf(target); order.splice(from, 1); order.splice(to, 0, dragKey); try { localStorage.setItem('oxidex-flow-member-column-order', JSON.stringify(order)); } catch (_) {} renderMembers(); });
      });
    };
    filter.addEventListener('input', event => { query = event.target.value.trim().toLowerCase(); renderMembers(); });
    renderMembers();
  }
  function detailRows(node, key) {
    if (node.members) return '<div class="flow-member-controls"><label for="flow-member-filter">Filter members</label><input id="flow-member-filter" type="search" placeholder="name, process, PID, worktree…" autocomplete="off"></div><p class="flow-member-hint">Click a heading to sort. Drag a heading left or right to move that column.</p><div class="flow-member-table-wrap"><table id="flow-member-table"></table></div>';
    if (key === 'queue') {
      const blocked = (node.detail.blocked_squads || []).map(x => e(x.squad)).join(', ') || 'none';
      const queued = (node.detail.queued || []).map(x => `<tr><td>${e(x.patch_id)}</td><td>${e(x.format)}</td><td>${e(x.squad)}</td><td class=flow-reason>${e(x.reason)}</td><td>${e(x.timestamp)}</td></tr>`).join('') || '<tr><td colspan=5>No queued judgments</td></tr>';
      return `<p>Advisory only: queued changes do not block the publish path.</p><p>Blocked squads: ${blocked}</p><table><thead><tr><th>Patch</th><th>Format</th><th>Squad</th><th>Reason</th><th>Queued</th></tr></thead><tbody>${queued}</tbody></table>`;
    }
    return readableStatCards(node.detail);
  }
  function readableStatCards(detail) {
    const labels = {processes:'Components', running:'Active processes', inactive:'Inactive processes', active_workers:'Active workers', events:'Dispatcher events', patches_found:'Patches found', fixer_calls:'Fixer calls', frozen:'Frozen workers', calls:'Model calls', patches_applied:'Patches passed', blocked_squads:'Blocked squads', batch_commits:'Unbatched commits', merged_prs:'Merged PRs', total:'Total model calls', fixer:'Fixer calls', reviewer:'Reviewer calls', critique:'Critique calls', event_count:'Recorded events', status:'Publisher state', source:'Data source', last_pr:'Latest PR'};
    const value = (key, item) => {
      if (item == null) return '—';
      if (key === 'last_pr' && typeof item === 'object') return item.number ? `#${item.number}${item.timestamp ? ` · ${age(item)}` : ''}` : '—';
      if (Array.isArray(item)) return item.length ? `${item.length} recorded` : 'none';
      if (typeof item === 'number') return f(item);
      if (typeof item === 'object') return 'recorded';
      return String(item).replaceAll('_', ' ');
    };
    const cards = Object.entries(detail || {}).filter(([key]) => key !== 'recent_prs').map(([key, item]) => `<div class="flow-stat"><span class="flow-stat-label">${e(labels[key] || key.replaceAll('_', ' '))}</span><span class="flow-stat-value">${e(value(key, item))}</span></div>`).join('');
    return `<section class="flow-stat-grid">${cards || '<div class="flow-stat"><span class="flow-stat-value">No current data.</span></div>'}</section>`;
  }
  function publisherPrTable(node) {
    const prs = (node.detail.recent_prs || []).slice(0, 20);
    if (!prs.length) return '<p class="publisher-prs-note">No recent PR records are available yet.</p>';
    const rows = prs.map(pr => {
      const number = pr.url ? `<a href="${e(pr.url)}" target="_blank" rel="noreferrer">#${e(pr.number)}</a>` : `#${e(pr.number)}`;
      return `<tr><td>${number}</td><td>${e(pr.name || '—')}</td><td>${e(age({timestamp: pr.timestamp}))}</td><td>${e(pr.title || '—')}</td></tr>`;
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
    dialog.addEventListener('click', event => { const box = dialog.getBoundingClientRect(); if (event.clientX < box.left || event.clientX > box.right || event.clientY < box.top || event.clientY > box.bottom) dialog.close(); });
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
    document.getElementById('flow-detail-body').innerHTML = `<h2>${prs ? 'Recent PRs' : overview ? 'Harness overview' : e(node.label)}</h2>${prs ? publisherPrTable(node) : overview ? flowGuide(key) : specificGuide(key) + detailRows(node, key)}`;
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


class Handler(BaseHTTPRequestHandler):
    server: Server
    def log_message(self, fmt: str, *args: Any) -> None:
        sys.stderr.write("dashboard: " + fmt % args + "\n")
    def send_json(self, status: HTTPStatus, data: dict[str, Any]) -> None:
        body = json.dumps(data).encode(); self.send_response(status); self.send_header("content-type", "application/json"); self.send_header("cache-control", "no-store"); self.send_header("content-length", str(len(body))); self.end_headers(); self.wfile.write(body)
    def do_GET(self) -> None:  # noqa: N802
        if urlparse(self.path).path == "/api/status":
            with self.server.snapshot_lock:
                data = snapshot(self.server.home, self.server.repo_root)
            self.send_json(HTTPStatus.OK, data); return
        if urlparse(self.path).path != "/": self.send_error(HTTPStatus.NOT_FOUND); return
        body = page(self.server.token, self.server.controls); self.send_response(HTTPStatus.OK); self.send_header("content-type", "text/html; charset=utf-8"); self.send_header("cache-control", "no-store"); self.send_header("content-length", str(len(body))); self.end_headers(); self.wfile.write(body)
    def do_POST(self) -> None:  # noqa: N802
        if urlparse(self.path).path != "/api/control": self.send_error(HTTPStatus.NOT_FOUND); return
        if not self.server.controls: self.send_json(HTTPStatus.FORBIDDEN, {"error": "controls require --enable-controls"}); return
        if not secrets.compare_digest(self.headers.get("x-control-token", ""), self.server.token): self.send_json(HTTPStatus.FORBIDDEN, {"error": "invalid control token"}); return
        try:
            size = int(self.headers.get("content-length", "0")); request = json.loads(self.rfile.read(size)); component_id, action = request["id"], request["action"]
            if action not in {"terminate", "restart"}: raise ValueError("action must be terminate or restart")
            with self.server.snapshot_lock:
                component, process = safe_process(snapshot(self.server.home, self.server.repo_root), component_id)
            if component_id == "publisher":
                self.send_json(HTTPStatus.CONFLICT, {"error": "PR publishing is a dispatcher-owned, short-lived sweep and cannot be controlled independently."}); return
            if action == "terminate":
                if component_id == "fleet": fleet_down(self.server.repo_root); message = "fleet stop requested"
                else: message = terminate(process) if process else "component is not running"
            else:
                if process: terminate(process)
                fleet_down(self.server.repo_root); message = f"{component['label']}: " + fleet_start(self.server.repo_root, self.server.home)
            self.send_json(HTTPStatus.OK, {"message": message})
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
