#!/usr/bin/env python3
"""Durable single-writer controller for the Beta 1 functional fleet.

The atomic snapshot is the current recovery view.  The append-only event log
is its audit trail.  Remote branches and pull requests are reconciled inputs,
never inferred success.  Process identity is PID + kernel start time + task +
unique token + exact argv; a reused PID alone is never considered live.
"""

from __future__ import annotations

import argparse
import ctypes
import fcntl
import hashlib
import json
import os
import re
import shlex
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
import uuid
from collections.abc import Callable, Mapping
from contextlib import contextmanager
from datetime import UTC, datetime
from pathlib import Path, PurePath
from typing import Any


OPS_ROOT = Path("/Users/allen/oxidex-ops").resolve()
GIT_ROOT = Path("/Users/allen/git").resolve()
CONTROLLER_BASE = OPS_ROOT / "evidence/20260919-beta1-functional/controller"
EVIDENCE_BASE = OPS_ROOT / "evidence/20260919-beta1-functional"
TARGET_BASE = GIT_ROOT / "oxidex-beta1-targets"
SCHEMA_VERSION = 1
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
HASH_RE = re.compile(r"^[0-9a-f]{64}$")
REMOTE_BRANCH_RE = re.compile(r"^staging/beta1/[a-z0-9][a-z0-9._/-]*$")

SLUGS = {
    0: "durable-controller-oracle-bootstrap",
    1: "ownership-inventory",
    2: "typed-occurrence-core",
    3: "typed-consumers",
    4: "conv-registry",
    5: "upgrade-transaction",
    6: "conformance-receipts",
    7: "file-session",
    8: "generated-attribution",
    9: "exif-shared-pipeline",
    10: "refusal-closure",
    11: "olympus-pilot",
    12: "nikon-port",
    13: "pentax-panasonic-port",
    14: "dji-composite-xmp-port",
    15: "legacy-camera-tail",
    16: "trailer-tail",
    17: "walker-engine-consolidation",
    18: "proven-deletion",
    19: "version-transition-qualification",
    20: "frozen-candidate-evidence",
}
DEPENDENCIES = {
    0: [],
    1: [0],
    2: [0],
    3: [2],
    4: [0],
    5: [0],
    6: [0],
    7: [2, 4],
    8: [1, 3, 4, 7],
    9: [2, 4, 7, 8],
    10: [1, 4, 9],
    11: [3, 8, 10],
    12: [11],
    13: [11],
    14: [11, 13],
    15: [11],
    16: [11],
    17: [9, 11, 12, 13, 14, 15, 16],
    18: [8, 10, 11, 12, 13, 14, 15, 16, 17],
    19: [5, 6, 18],
    20: list(range(20)),
}

STATE_REQUIRED = {
    "schema_version",
    "plan_sha256",
    "spec_sha256",
    "target_ref",
    "target_sha",
    "merge_lease",
    "expected_merge_parent",
    "controller_identity",
    "tasks",
}
TASK_REQUIRED = {
    "number",
    "slug",
    "state",
    "state_history",
    "dependencies",
    "file_lease",
    "worker",
    "process",
    "launch_count",
    "prd_sha256",
    "report_sha256",
    "review_sha256",
    "base_sha",
    "head_sha",
    "pushed_sha",
    "merge_sha",
    "target_sha",
    "paths",
    "heartbeat",
    "pr_ci_state",
    "receipt_hashes",
    "ruling",
    "blocker",
    "expected_merge_parent",
    "next_command",
}
PATH_REQUIRED = {"worktree", "target", "evidence", "prd", "report", "review"}
WORKER_REQUIRED = {"kind", "model", "effort", "identity"}
PROCESS_REQUIRED = {
    "pid",
    "start_time",
    "token",
    "task",
    "executable",
    "model",
    "argv",
    "observed_command",
    "events",
    "final",
    "segment",
    "session_id",
    "offset",
    "launch_count",
}


class Refused(RuntimeError):
    """A safety fence rejected an operation."""


class Blocked(RuntimeError):
    """A valid operation cannot proceed until a prerequisite changes."""


def utc_now() -> str:
    return datetime.now(UTC).isoformat().replace("+00:00", "Z")


def reject_symlink_components(path: Path) -> None:
    """Refuse any existing symlink component before path resolution."""
    lexical = Path(os.path.abspath(path.expanduser()))
    current = Path(lexical.anchor)
    for part in lexical.parts[1:]:
        current /= part
        if current.is_symlink():
            raise Refused(
                "symlink path is refused because it may resolve outside durable roots: "
                f"{current}"
            )


def require_operational_path(path: Path) -> Path:
    reject_symlink_components(path)
    value = path.expanduser().resolve(strict=False)
    if not any(value == root or root in value.parents for root in (OPS_ROOT, GIT_ROOT)):
        raise Refused(f"path outside durable roots: {value}")
    return value


def require_sha(value: str, name: str = "SHA") -> str:
    if not SHA_RE.fullmatch(value):
        raise Refused(f"{name} must be a resolved 40-character lowercase SHA")
    return value


def require_hash(value: str, name: str = "SHA-256") -> str:
    if not HASH_RE.fullmatch(value):
        raise Refused(f"{name} must be a 64-character lowercase SHA-256")
    return value


def require_remote_branch(value: Any) -> str:
    components = value.removeprefix("staging/beta1/").split("/") if isinstance(value, str) else []
    if (
        not isinstance(value, str)
        or not REMOTE_BRANCH_RE.fullmatch(value)
        or ".." in value
        or "//" in value
        or any(
            not component
            or component.startswith(".")
            or component.endswith(".")
            or component.endswith(".lock")
            for component in components
        )
    ):
        raise Refused(
            "remote_branch must be a normalized staging/beta1 task branch"
        )
    return value


def remove_test_root(path: Path) -> None:
    value = require_operational_path(path)
    if "controller-tests" not in value.parts:
        raise Refused("test cleanup restricted to controller-tests")
    shutil.rmtree(value, ignore_errors=True)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def atomic_json(path: Path, value: Mapping[str, Any]) -> None:
    require_operational_path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, name = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.tmp-")
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as output:
            json.dump(value, output, indent=2, sort_keys=True)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        temporary.unlink(missing_ok=True)


def atomic_bytes(path: Path, value: bytes) -> None:
    require_operational_path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, name = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.tmp-")
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(value)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        temporary.unlink(missing_ok=True)


def validate_state(state: Mapping[str, Any]) -> None:
    missing = sorted(STATE_REQUIRED - state.keys())
    if missing:
        raise Refused(f"state missing required field {missing[0]}")
    if state["schema_version"] != SCHEMA_VERSION:
        raise Refused(f"schema_version must be {SCHEMA_VERSION}")
    require_sha(str(state["target_sha"]), "target_sha")
    require_sha(str(state["expected_merge_parent"]), "expected_merge_parent")
    tasks = state["tasks"]
    if not isinstance(tasks, Mapping):
        raise Refused("tasks must be an object")
    for key, raw_task in tasks.items():
        if not isinstance(raw_task, Mapping):
            raise Refused(f"task {key} must be an object")
        task_missing = sorted(TASK_REQUIRED - raw_task.keys())
        if task_missing:
            raise Refused(f"task {key} missing required field {task_missing[0]}")
        if str(raw_task["number"]) != str(key):
            raise Refused(f"task key {key} does not match number")
        if "remote_branch" in raw_task:
            require_remote_branch(raw_task["remote_branch"])
        worker_missing = sorted(WORKER_REQUIRED - raw_task["worker"].keys())
        if worker_missing:
            raise Refused(f"task {key} worker missing {worker_missing[0]}")
        path_missing = sorted(PATH_REQUIRED - raw_task["paths"].keys())
        if path_missing:
            raise Refused(f"task {key} paths missing {path_missing[0]}")
        for path in raw_task["paths"].values():
            require_operational_path(Path(path))
        process = raw_task["process"]
        if process is not None:
            process_missing = sorted(PROCESS_REQUIRED - process.keys())
            if process_missing:
                raise Refused(
                    f"task {key} process missing required field {process_missing[0]}"
                )


class StateStore:
    def __init__(self, root: Path):
        self.root = require_operational_path(root)
        self.root.mkdir(parents=True, exist_ok=True)
        self.snapshot_path = self.root / "fleet-state.json"
        self.events_path = self.root / "fleet-events.jsonl"
        self.receipt_index_path = self.root / "receipt-index.json"
        self.lock_path = self.root / ".controller.lock"
        self._lock_stream: Any | None = None
        self._lock_depth = 0

    @contextmanager
    def _locked(self):
        if self._lock_stream is not None:
            self._lock_depth += 1
            try:
                yield self._lock_stream
            finally:
                self._lock_depth -= 1
            return
        stream = self.lock_path.open("a+")
        fcntl.flock(stream.fileno(), fcntl.LOCK_EX)
        self._lock_stream = stream
        self._lock_depth = 1
        try:
            yield stream
        finally:
            self._lock_depth = 0
            self._lock_stream = None
            fcntl.flock(stream.fileno(), fcntl.LOCK_UN)
            stream.close()

    def _require_no_pending_repair(
        self, *, allowed_journal: Path | None = None
    ) -> None:
        transactions = self.root / "repair-transactions"
        if not transactions.is_dir():
            return
        for path in transactions.glob("*.json"):
            try:
                value = json.loads(path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError) as exc:
                raise Refused(f"malformed merge-receipt repair journal: {path}") from exc
            if (
                not isinstance(value, Mapping)
                or value.get("phase") != "complete"
            ) and path != allowed_journal:
                raise Blocked(
                    "pending merge-receipt repair must be resumed before controller mutation"
                )

    def _require_no_pending_refresh(
        self, *, allowed_journal: Path | None = None
    ) -> None:
        transactions = self.root / "refresh-transactions"
        if not transactions.is_dir():
            return
        for path in transactions.glob("*.json"):
            try:
                value = json.loads(path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError) as exc:
                raise Refused(f"malformed plan/spec refresh journal: {path}") from exc
            if (not isinstance(value, Mapping) or value.get("phase") != "complete") and path != allowed_journal:
                raise Blocked(
                    "pending plan/spec refresh must be resumed before controller mutation"
                )

    def write_snapshot(self, state: Mapping[str, Any]) -> None:
        self._require_no_pending_repair()
        self._require_no_pending_refresh()
        validate_state(state)
        atomic_json(self.snapshot_path, state)

    def read_snapshot(self) -> dict[str, Any]:
        if not self.snapshot_path.exists():
            raise Refused(f"controller is not initialized: {self.snapshot_path}")
        value = json.loads(self.snapshot_path.read_text(encoding="utf-8"))
        validate_state(value)
        return value

    def mutate(self, callback: Callable[[dict[str, Any]], Any]) -> Any:
        with self._locked():
            self._require_no_pending_repair()
            self._require_no_pending_refresh()
            state = self.read_snapshot()
            result = callback(state)
            validate_state(state)
            atomic_json(self.snapshot_path, state)
            return result

    def append_event(self, event: Mapping[str, Any]) -> dict[str, Any]:
        with self._locked() as lock:
            self._require_no_pending_repair()
            self._require_no_pending_refresh()
            lock.seek(0)
            sequence = 1
            if self.events_path.exists():
                with self.events_path.open(encoding="utf-8") as existing:
                    sequence += sum(1 for _ in existing)
            record = {"sequence": sequence, "timestamp": utc_now(), **event}
            with self.events_path.open("a", encoding="utf-8") as output:
                output.write(json.dumps(record, sort_keys=True) + "\n")
                output.flush()
                os.fsync(output.fileno())
            return record

    def write_receipt_index(self, value: Mapping[str, Any]) -> None:
        self._require_no_pending_repair()
        self._require_no_pending_refresh()
        atomic_json(self.receipt_index_path, value)

    def read_receipt_index(self) -> dict[str, Any]:
        if not self.receipt_index_path.exists():
            return {}
        return json.loads(self.receipt_index_path.read_text(encoding="utf-8"))


def _section(text: str, start: str, following_level: str = "## ") -> str:
    position = text.find(start)
    if position < 0:
        raise Refused(f"plan section missing: {start}")
    heading = re.compile(r"(?m)^##(?:#)?\s+")
    match = heading.search(text, position + len(start))
    next_position = match.start() if match else -1
    return text[position:] if next_position < 0 else text[position:next_position]


def parse_plan(plan: Path) -> tuple[str, dict[int, tuple[str, str]]]:
    require_operational_path(plan)
    text = plan.read_text(encoding="utf-8")
    global_constraints = _section(text, "## Global Constraints")
    matches = list(re.finditer(r"(?m)^### Task (\d+): ([^\n]+)$", text))
    if not matches:
        raise Refused("plan contains no Task sections")
    tasks: dict[int, tuple[str, str]] = {}
    for index, match in enumerate(matches):
        end = matches[index + 1].start() if index + 1 < len(matches) else len(text)
        execution = text.find("\n## Execution Handoff", match.end(), end)
        if execution >= 0:
            end = execution
        tasks[int(match.group(1))] = (match.group(2).strip(), text[match.start():end].rstrip() + "\n")
    return global_constraints.rstrip() + "\n", tasks


def _slug(title: str, number: int) -> str:
    if number in SLUGS:
        return SLUGS[number]
    return re.sub(r"[^a-z0-9]+", "-", title.lower()).strip("-")


def _parse_dependencies(section: str, number: int) -> list[int]:
    match = re.search(r"(?m)^\*\*Depends on:\*\*\s*(.*)$", section)
    if match:
        value = match.group(1).strip()
        if value.lower() in {"none", "controller setup"}:
            return []
        return [int(item) for item in re.findall(r"\d+", value)]
    return list(DEPENDENCIES.get(number, []))


def _parse_worker(section: str) -> dict[str, Any]:
    match = re.search(r"(?m)^\*\*Worker:\*\*\s*(.*)$", section)
    value = match.group(1) if match else "Codex CLI, `gpt-5.6-terra`, fast mode"
    model = re.search(r"`([^`]+)`", value)
    if "Controller-owned" in value:
        kind = "Controller"
        model_name = "gpt-6-astra"
    else:
        kind = "Desktop" if "Desktop" in value else "CLI"
        model_name = model.group(1) if model else "gpt-5.6-terra"
    effort = "fast" if kind == "Controller" or "fast" in value.lower() else "medium"
    return {
        "kind": kind,
        "model": model_name,
        "effort": effort,
        "identity": None,
    }


def _parse_file_lease(section: str) -> list[str]:
    files_match = re.search(r"(?ms)^\*\*Files:\*\*\s*(.*?)(?=^\*\*Interfaces:\*\*|^### |\Z)", section)
    if not files_match:
        return []
    paths: list[str] = []
    for line in files_match.group(1).splitlines():
        entry = re.match(r"\s*-\s+(.*)$", line)
        if not entry:
            continue
        value = entry.group(1).strip()
        if re.match(r"(?i)^do not\b", value):
            continue
        match = re.fullmatch(
            r"(?i)(?:Add|Create|Modify|Delete|Test|Replace or update):\s*`([^`]+)`",
            value,
        )
        if match:
            paths.append(match.group(1))
            continue
        if re.match(r"(?i)^(?:Add|Create|Modify|Delete|Test|Replace or update):", value):
            raise Refused(
                "file lease operation requires one literal backtick path: " + value
            )
    return sorted(set(paths))


def _task_paths(root: Path, number: int, slug: str) -> dict[str, str]:
    paths = {
        "worktree": str(GIT_ROOT / f"oxidex-beta1-{slug}"),
        "target": str(TARGET_BASE / slug),
        "evidence": str(EVIDENCE_BASE / slug),
        "prd": str(root / "prds" / f"{number:02d}-{slug}.md"),
        "report": str(root / "reports" / f"{number:02d}-{slug}.md"),
        "review": str(root / "reviews" / f"{number:02d}-{slug}.md"),
    }
    for path in paths.values():
        require_operational_path(Path(path))
    return paths


def _task_branch(task: Mapping[str, Any]) -> str:
    if "remote_branch" in task:
        return require_remote_branch(task["remote_branch"])
    return f"staging/beta1/{task['slug']}"


def initialize_state(
    plan: Path, spec: Path, target_ref: str, target_sha: str, root: Path
) -> dict[str, Any]:
    plan = require_operational_path(plan)
    spec = require_operational_path(spec)
    root = require_operational_path(root)
    require_sha(target_sha, "target_sha")
    _, sections = parse_plan(plan)
    tasks: dict[str, Any] = {}
    for number, (title, section) in sorted(sections.items()):
        slug = _slug(title, number)
        dependencies = _parse_dependencies(section, number)
        state_name = "launchable" if not dependencies else "blocked"
        tasks[str(number)] = {
            "number": number,
            "slug": slug,
            "state": state_name,
            "state_history": [{"state": state_name, "at": utc_now()}],
            "dependencies": dependencies,
            "file_lease": _parse_file_lease(section),
            "worker": _parse_worker(section),
            "process": None,
            "launch_count": 0,
            "prd_sha256": None,
            "report_sha256": None,
            "review_sha256": None,
            "base_sha": target_sha,
            "head_sha": None,
            "pushed_sha": None,
            "merge_sha": None,
            "target_sha": target_sha,
            "paths": _task_paths(root, number, slug),
            "heartbeat": None,
            "pr_ci_state": None,
            "receipt_hashes": {},
            "ruling": None,
            "blocker": None,
            "expected_merge_parent": target_sha,
            "next_command": f"materialize --task {number}",
        }
    value = {
        "schema_version": SCHEMA_VERSION,
        "plan_sha256": sha256_file(plan),
        "spec_sha256": sha256_file(spec),
        "target_ref": target_ref,
        "target_sha": target_sha,
        "merge_lease": None,
        "expected_merge_parent": target_sha,
        "controller_identity": f"controller-{uuid.uuid4().hex}",
        "tasks": tasks,
    }
    validate_state(value)
    return value


def initialize_controller(
    store: StateStore,
    plan: Path,
    spec: Path,
    target_ref: str,
    target_sha: str,
) -> dict[str, Any]:
    """Initialize once, or return an exactly matching durable snapshot."""
    candidate = initialize_state(plan, spec, target_ref, target_sha, store.root)
    if store.snapshot_path.exists():
        existing = store.read_snapshot()
        identity_fields = (
            "schema_version",
            "plan_sha256",
            "spec_sha256",
            "target_ref",
            "target_sha",
            "expected_merge_parent",
        )
        mismatches = [
            field
            for field in identity_fields
            if existing.get(field) != candidate.get(field)
        ]
        if mismatches:
            raise Refused(
                "controller already initialized with different "
                + ", ".join(mismatches)
            )
        return existing
    store.write_snapshot(candidate)
    if not store.receipt_index_path.exists():
        store.write_receipt_index({})
    store.append_event({"event": "init", "target_sha": target_sha})
    return candidate


def _refresh_journal_path(store: StateStore, plan_sha256: str, spec_sha256: str) -> Path:
    return store.root / "refresh-transactions" / f"{plan_sha256}-{spec_sha256}.json"


def _refresh_request(
    plan: Path,
    spec: Path,
    old_plan_sha256: str,
    old_spec_sha256: str,
    before_state_sha256: str,
) -> dict[str, Any]:
    plan = require_operational_path(plan)
    spec = require_operational_path(spec)
    for path, name in ((plan, "plan"), (spec, "spec")):
        if not path.is_file() or path.is_symlink():
            raise Refused(f"{name} must be a regular non-symlink file")
    old_plan_sha256 = require_hash(old_plan_sha256, "old plan SHA-256")
    old_spec_sha256 = require_hash(old_spec_sha256, "old spec SHA-256")
    before_state_sha256 = require_hash(before_state_sha256, "before-state SHA-256")
    return {
        "plan": str(plan),
        "spec": str(spec),
        "old_plan_sha256": old_plan_sha256,
        "old_spec_sha256": old_spec_sha256,
        "before_state_sha256": before_state_sha256,
        "new_plan_sha256": sha256_file(plan),
        "new_spec_sha256": sha256_file(spec),
    }


def _refresh_receipt_matches(journal: Mapping[str, Any], request: Mapping[str, Any]) -> bool:
    return all(journal.get(field) == request.get(field) for field in (
        "plan", "spec", "old_plan_sha256", "old_spec_sha256",
        "before_state_sha256", "new_plan_sha256", "new_spec_sha256",
    ))


def _resume_plan_spec_refresh(
    store: StateStore, journal_path: Path, journal: dict[str, Any]
) -> dict[str, Any]:
    before_state = journal["before_state_sha256"]
    after_state = journal["after_state_sha256"]
    before_events = journal["before_events_sha256"]
    after_events = journal["after_events_sha256"]
    state_hash = sha256_file(store.snapshot_path)
    events_bytes = store.events_path.read_bytes() if store.events_path.exists() else b""
    if state_hash not in {before_state, after_state}:
        raise Refused("refresh state matches neither journaled before nor after identity")
    if hashlib.sha256(events_bytes).hexdigest() not in {before_events, after_events}:
        raise Refused("refresh events match neither journaled before nor after identity")
    if state_hash == before_state:
        state = store.read_snapshot()
        if state["plan_sha256"] != journal["old_plan_sha256"] or state["spec_sha256"] != journal["old_spec_sha256"]:
            raise Refused("refresh old plan/spec identity no longer matches state")
        state["plan_sha256"] = journal["new_plan_sha256"]
        state["spec_sha256"] = journal["new_spec_sha256"]
        validate_state(state)
        atomic_json(store.snapshot_path, state)
        if sha256_file(store.snapshot_path) != after_state:
            raise Refused("refresh snapshot write did not produce journaled hash")
    if hashlib.sha256(events_bytes).hexdigest() == before_events:
        current = store.events_path.read_bytes() if store.events_path.exists() else b""
        if current != journal["before_events_bytes"].encode("utf-8"):
            raise Refused("refresh event prefix changed before append")
        atomic_bytes(store.events_path, journal["after_events_bytes"].encode("utf-8"))
        if sha256_file(store.events_path) != after_events:
            raise Refused("refresh event append did not produce journaled hash")
    journal["phase"] = "complete"
    atomic_json(journal_path, journal)
    return {**journal["receipt"], "idempotent": False}


def refresh_plan_spec(
    store: StateStore,
    plan: Path,
    spec: Path,
    *,
    old_plan_sha256: str,
    old_spec_sha256: str,
    before_state_sha256: str,
) -> dict[str, Any]:
    """Atomically refresh controller plan/spec identities with a durable journal."""
    request = _refresh_request(
        plan, spec, old_plan_sha256, old_spec_sha256, before_state_sha256
    )
    journal_path = _refresh_journal_path(
        store, request["new_plan_sha256"], request["new_spec_sha256"]
    )
    with store._locked():
        store._require_no_pending_repair()
        store._require_no_pending_refresh(allowed_journal=journal_path)
        if journal_path.exists():
            try:
                journal = json.loads(journal_path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError) as exc:
                raise Refused(f"malformed plan/spec refresh journal: {journal_path}") from exc
            if not isinstance(journal, dict) or not _refresh_receipt_matches(journal, request):
                raise Refused("different refresh request conflicts with existing journal")
            if journal.get("phase") == "complete":
                _resume_plan_spec_refresh(store, journal_path, journal)
                return {**journal["receipt"], "idempotent": True}
            if journal.get("phase") != "prepared":
                raise Refused("plan/spec refresh journal has invalid lifecycle")
            return _resume_plan_spec_refresh(store, journal_path, journal)

        state = store.read_snapshot()
        actual_before = sha256_file(store.snapshot_path)
        if actual_before != request["before_state_sha256"]:
            raise Refused("before-state SHA-256 does not match controller snapshot")
        if state["plan_sha256"] != request["old_plan_sha256"]:
            raise Refused("old plan SHA-256 does not match controller state")
        if state["spec_sha256"] != request["old_spec_sha256"]:
            raise Refused("old spec SHA-256 does not match controller state")
        before_events_bytes = store.events_path.read_bytes() if store.events_path.exists() else b""
        if before_events_bytes and not before_events_bytes.endswith(b"\n"):
            raise Refused("event log lacks a final newline before plan/spec refresh")
        sequence = len(before_events_bytes.splitlines()) + 1
        event = {
            "sequence": sequence,
            "timestamp": utc_now(),
            "event": "refresh-plan-spec",
            "before_state_sha256": request["before_state_sha256"],
            "old_plan_sha256": request["old_plan_sha256"],
            "old_spec_sha256": request["old_spec_sha256"],
            "new_plan_sha256": request["new_plan_sha256"],
            "new_spec_sha256": request["new_spec_sha256"],
        }
        encoded_event = (json.dumps(event, sort_keys=True) + "\n").encode("utf-8")
        after_events_bytes = before_events_bytes + encoded_event
        after_state = dict(state)
        after_state["plan_sha256"] = request["new_plan_sha256"]
        after_state["spec_sha256"] = request["new_spec_sha256"]
        validate_state(after_state)
        receipt = {
            "transaction": "refresh-plan-spec",
            "event_sequence": sequence,
            "before_state_sha256": request["before_state_sha256"],
            "old_plan_sha256": request["old_plan_sha256"],
            "old_spec_sha256": request["old_spec_sha256"],
            "new_plan_sha256": request["new_plan_sha256"],
            "new_spec_sha256": request["new_spec_sha256"],
        }
        journal = {
            "schema_version": 1,
            "phase": "prepared",
            **request,
            "before_events_sha256": hashlib.sha256(before_events_bytes).hexdigest(),
            "after_state_sha256": _bytes_hash(_json_document_bytes(after_state)),
            "after_events_sha256": hashlib.sha256(after_events_bytes).hexdigest(),
            "before_events_bytes": before_events_bytes,
            "after_events_bytes": after_events_bytes,
            "receipt": receipt,
        }
        # JSON cannot encode bytes; retain the exact event bytes as base64-free
        # UTF-8 text because the event log is UTF-8 JSONL by contract.
        journal["before_events_bytes"] = before_events_bytes.decode("utf-8")
        journal["after_events_bytes"] = after_events_bytes.decode("utf-8")
        atomic_json(journal_path, journal)
        return _resume_plan_spec_refresh(store, journal_path, journal)


def _transition(task: dict[str, Any], state_name: str, *, detail: str | None = None) -> None:
    if task["state"] == state_name and not detail:
        return
    task["state"] = state_name
    entry = {"state": state_name, "at": utc_now()}
    if detail:
        entry["detail"] = detail
    task["state_history"].append(entry)


def _task(store: StateStore, state: dict[str, Any], number: int) -> dict[str, Any]:
    try:
        return state["tasks"][str(number)]
    except KeyError as exc:
        raise Refused(f"unknown task {number} in {store.snapshot_path}") from exc


def require_dependencies(task: Mapping[str, Any], tasks: Mapping[Any, Any]) -> None:
    for number in task.get("dependencies", []):
        dependency = tasks.get(str(number), tasks.get(number, {}))
        if dependency.get("state") != "merged" or not dependency.get("merge_sha"):
            raise Blocked(f"dependency {number} lacks remote merge")
        expected = dependency.get("expected_merge_parent")
        actual = dependency.get("merge_parent", expected)
        validate_merge_parent(expected, actual)


def _launch_instruction(task: Mapping[str, Any]) -> tuple[str, str]:
    worker = task["worker"]
    kind = worker["kind"]
    if kind == "CLI":
        return (
            "bash",
            "python3 tools/release/fleet_controller.py launch "
            f"--root {CONTROLLER_BASE} "
            "--repo /Users/allen/git/oxidex-beta1-functional-integration "
            f"--task {task['number']}",
        )
    if kind == "Desktop":
        task_name = f"beta1_task_{int(task['number']):02d}_{task['slug'].replace('-', '_')}"
        message = (
            f"Execute Task {task['number']} from {task['paths']['prd']} in "
            f"{task['paths']['worktree']}; write {task['paths']['report']}; "
            "do not use subagents."
        )
        return (
            "json",
            "collaboration.spawn_agent({\n"
            f'  "task_name": "{task_name}",\n'
            '  "fork_turns": "none",\n'
            f'  "model": "{worker["model"]}",\n'
            f'  "reasoning_effort": "{worker["effort"]}",\n'
            f'  "message": {json.dumps(message)}\n'
            "})",
        )
    if kind == "Controller":
        return (
            "text",
            "Controller-owned task; do not launch an implementation worker. "
            "The authenticated controller executes this task directly after "
            "its dependencies and merge lease are satisfied.",
        )
    raise Refused(f"unsupported worker kind for Task {task['number']}: {kind}")


def _materialized_footer(task: Mapping[str, Any], base_sha: str) -> str:
    paths = task["paths"]
    lease = "\n".join(f"- `{path}`" for path in task["file_lease"]) or "- none"
    launch_language, launch = _launch_instruction(task)
    return f"""
## Materialized execution contract

- Resolved base SHA: `{base_sha}`
- base_sha: {base_sha}
- Worktree: `{paths['worktree']}`
- Cargo target: `{paths['target']}`
- Evidence: `{paths['evidence']}`
- Canonical PRD: `{paths['prd']}`
- Report: `{paths['report']}`
- Review: `{paths['review']}`
- Dependencies: `{task['dependencies']}`
- Worker policy: `{task['worker']['kind']}` / `{task['worker']['model']}` / `{task['worker']['effort']}`

### Exact file lease

{lease}

### Exact launch instruction

```{launch_language}
{launch}
```

### Handoff and report contract

Update the worktree-root `HANDOFF.md` after every meaningful transition and at
least every 15 minutes. Record task/base/head, exact commands and results,
receipt paths and hashes, blockers and rulings. Write the canonical report at
`{paths['report']}`. Return only terminal status, commit SHA, one-line test
summary, and concerns. The exact next action in the final handoff is
`RETURN_TO_CONTROLLER`.

### Required final checkpoint

```bash
git diff --check
git diff --name-only | sort -u
git diff --cached --name-only | sort -u
git ls-files --others --exclude-standard | sort -u
# Controller verifies every changed path is inside the exact lease.
git add -- {shlex.join(task['file_lease']) if task['file_lease'] else '--intent-to-add /dev/null'}
git commit -S -m "{task['slug']}"
git cat-file -p HEAD | rg '^gpgsig '
test -z "$(git status --short)"
```
"""


def materialize_prd(
    store: StateStore,
    plan: Path,
    task_number: int,
    base_sha: str,
    paths: Mapping[str, str] | None = None,
) -> Path:
    require_sha(base_sha, "base SHA")
    global_constraints, sections = parse_plan(require_operational_path(plan))
    if task_number not in sections:
        raise Refused(f"plan has no Task {task_number}")
    state = store.read_snapshot()
    task = _task(store, state, task_number)
    require_dependencies(task, state["tasks"])
    if paths:
        for value in paths.values():
            require_operational_path(Path(value))
    _, task_section = sections[task_number]
    planned_worker = _parse_worker(task_section)
    planned_file_lease = _parse_file_lease(task_section)
    existing_worker = task["worker"]
    policy_fields = ("kind", "model", "effort")
    if all(
        existing_worker.get(field) == planned_worker[field]
        for field in policy_fields
    ):
        planned_worker["identity"] = existing_worker.get("identity")
    materialized_task = dict(task)
    materialized_task["worker"] = planned_worker
    materialized_task["file_lease"] = planned_file_lease
    content = (
        "# Materialized Beta 1 Functional Task PRD\n\n"
        + global_constraints
        + "\n"
        + task_section
        + _materialized_footer(materialized_task, base_sha)
    )
    path = require_operational_path(Path(task["paths"]["prd"]))
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, name = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.tmp-")
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as output:
            output.write(content)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)
    digest = sha256_file(path)

    def update(snapshot: dict[str, Any]) -> None:
        current = _task(store, snapshot, task_number)
        current["base_sha"] = base_sha
        current["target_sha"] = base_sha
        current["expected_merge_parent"] = base_sha
        current["worker"] = dict(planned_worker)
        current["file_lease"] = list(planned_file_lease)
        current["prd_sha256"] = digest
        current["next_command"] = f"launch --task {task_number}"

    store.mutate(update)
    index = store.read_receipt_index()
    index[str(task_number)] = {"prd": str(path), "sha256": digest}
    store.write_receipt_index(index)
    store.append_event({"event": "materialize", "task": task_number, "prd_sha256": digest})
    return path


def prd_hash_matches(path: Path, digest: str) -> bool:
    return path.is_file() and sha256_file(path) == digest


def record_external_event(
    store: StateStore, task_number: int, identity: str, payload: Mapping[str, Any]
) -> dict[str, Any]:
    def update(state: dict[str, Any]) -> dict[str, Any]:
        task = _task(store, state, task_number)
        task["worker"]["identity"] = identity
        if payload.get("state"):
            _transition(task, str(payload["state"]))
        if payload.get("heartbeat"):
            task["heartbeat"] = payload["heartbeat"]
        return task

    result = store.mutate(update)
    store.append_event({"event": "external", "task": task_number, "identity": identity, "payload": dict(payload)})
    return result


def record_checkpoint(
    store: StateStore,
    task_number: int,
    *,
    head_sha: str,
    pushed_sha: str | None,
    repo: Path | None = None,
) -> dict[str, Any]:
    require_sha(head_sha, "head_sha")
    if pushed_sha is not None:
        require_sha(pushed_sha, "pushed_sha")
    before = store.read_snapshot()
    recorded_task = _task(store, before, task_number)
    worktree = Path(recorded_task["paths"]["worktree"])
    if worktree.exists():
        observation = inspect_worktree(recorded_task)
        if observation["head_sha"] != head_sha:
            raise Blocked(
                f"Task {task_number} checkpoint SHA does not match worktree HEAD"
            )
        branch = _git(worktree, "branch", "--show-current")
        expected_branch = _task_branch(recorded_task)
        if branch != expected_branch:
            raise Blocked(
                f"Task {task_number} worktree branch is {branch}, expected {expected_branch}"
            )
        repository = worktree
    else:
        if repo is None:
            raise Blocked(
                f"Task {task_number} worktree is missing; controller repository required"
            )
        repository = require_operational_path(repo)
        expected_branch = _task_branch(recorded_task)
        try:
            branch_sha = require_sha(
                _git(repository, "rev-parse", f"refs/heads/{expected_branch}"),
                "task branch SHA",
            )
        except Blocked as exc:
            raise Blocked(
                f"Task {task_number} task branch {expected_branch} is unavailable"
            ) from exc
        if branch_sha != head_sha:
            raise Blocked(
                f"Task {task_number} checkpoint does not match task branch {expected_branch}"
            )
    try:
        _git(repository, "cat-file", "-e", f"{head_sha}^{{commit}}")
        commit = _git(repository, "cat-file", "-p", head_sha)
    except Blocked as exc:
        raise Blocked(f"Task {task_number} checkpoint commit is unavailable") from exc
    if "\ngpgsig " not in "\n" + commit:
        raise Blocked(f"Task {task_number} checkpoint is not signed")

    def update(state: dict[str, Any]) -> dict[str, Any]:
        task = _task(store, state, task_number)
        task["head_sha"] = head_sha
        task["pushed_sha"] = pushed_sha
        _transition(task, "pushed" if pushed_sha else "committed")
        task["next_command"] = f"reconcile --task {task_number}"
        return task

    result = store.mutate(update)
    store.append_event({"event": "checkpoint", "task": task_number, "head_sha": head_sha, "pushed_sha": pushed_sha})
    return result


def validate_merge_parent(expected: str | None, actual: str | None) -> None:
    if expected != actual:
        raise Blocked(f"unexpected squash parent: expected {expected}, got {actual}")


def reconcile_task(
    store: StateStore, task_number: int, remote: Mapping[str, Any]
) -> dict[str, Any]:
    def update(state: dict[str, Any]) -> dict[str, Any]:
        task = _task(store, state, task_number)
        merge_sha = remote.get("merge_sha")
        explicit_blocker = task.get("blocker") not in {None, "dependency"}
        if merge_sha:
            require_sha(str(merge_sha), "merge_sha")
            validate_merge_parent(
                task["expected_merge_parent"],
                remote.get("merge_parent", task["expected_merge_parent"]),
            )
        for source, destination in (
            ("head_sha", "head_sha"),
            ("pushed_sha", "pushed_sha"),
            ("merge_sha", "merge_sha"),
        ):
            if remote.get(source) is not None:
                task[destination] = remote[source]
        task["pr_ci_state"] = {
            "pr": remote.get("pr"),
            "url": remote.get("pr_url"),
            "state": remote.get("pr_state"),
            "draft": remote.get("draft"),
            "ci": remote.get("ci"),
        }
        if merge_sha:
            _transition(task, "merged")
            task["next_command"] = "complete"
        elif remote.get("pushed_sha") and not explicit_blocker:
            _transition(task, "pushed")
            task["next_command"] = f"reconcile --task {task_number}"
        return task

    result = store.mutate(update)
    store.append_event({"event": "reconcile", "task": task_number, "remote": dict(remote)})
    return result


def _path_in_lease(path: str, lease: list[str]) -> bool:
    candidate = PurePath(path)
    for raw_rule in lease:
        rule = raw_rule.rstrip("/")
        if not rule:
            continue
        if any(character in rule for character in "*?["):
            if candidate.match(rule):
                return True
        elif path == rule or path.startswith(rule + "/"):
            return True
    return False


def _git(repo: Path, *arguments: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(repo), *arguments],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        raise Blocked(
            f"git {' '.join(arguments)} failed in {repo}: "
            f"{result.stderr.strip() or result.stdout.strip()}"
        )
    return result.stdout.rstrip("\n")


def _github_repository(repo: Path) -> str:
    override = os.environ.get("FLEET_GITHUB_REPOSITORY", "").strip()
    if override:
        return override
    remote = _git(repo, "remote", "get-url", "origin")
    match = re.search(r"github\.com[:/]([^/]+/[^/]+?)(?:\.git)?$", remote)
    if not match:
        raise Refused(f"cannot derive GitHub repository from origin URL: {remote}")
    return match.group(1)


def _ci_state(checks: Any) -> str:
    if not isinstance(checks, list) or not checks:
        return "none"
    failure = {"FAILURE", "CANCELLED", "TIMED_OUT", "ACTION_REQUIRED", "ERROR"}
    success = {"SUCCESS", "NEUTRAL", "SKIPPED"}
    conclusions = {str(item.get("conclusion") or "").upper() for item in checks}
    statuses = {str(item.get("status") or "").upper() for item in checks}
    if conclusions & failure:
        return "failure"
    if any(status not in {"COMPLETED"} for status in statuses) or "" in conclusions:
        return "pending"
    return "success" if conclusions <= success else "pending"


def read_remote_inventory(
    repo: Path, state: Mapping[str, Any], task_number: int | None = None
) -> dict[str, Any]:
    """Read branches, PRs, checks, and merges without changing local or remote refs.

    A single-task reconciliation authenticates only that task's PR, so a
    historical PR on another task cannot prevent its observation.
    """
    repository = require_operational_path(repo)
    target_ref = str(state["target_ref"])
    target_name = target_ref.removeprefix("origin/")
    refs_output = _git(repository, "ls-remote", "--heads", "origin")
    refs: dict[str, str] = {}
    for line in refs_output.splitlines():
        fields = line.split()
        if len(fields) == 2:
            refs[fields[1]] = require_sha(fields[0], "remote branch SHA")

    gh = os.environ.get("FLEET_GH_EXECUTABLE", "").strip() or shutil.which("gh")
    if not gh:
        raise Refused("GitHub CLI is required for read-only PR reconciliation")
    github_repository = _github_repository(repository)
    fields = (
        "number,url,state,isDraft,headRefName,headRefOid,baseRefName,"
        "baseRefOid,mergeCommit,statusCheckRollup"
    )
    recorded_pr: int | None = None
    if task_number is not None:
        pr_ci_state = state["tasks"][str(task_number)].get("pr_ci_state")
        # Malformed or legacy durable PR state is treated as unrecorded so
        # recovery retains the safe inventory-list fallback.
        candidate = pr_ci_state.get("pr") if isinstance(pr_ci_state, Mapping) else None
        if type(candidate) is int and candidate > 0:
            recorded_pr = candidate
    command = [gh, "pr"]
    if recorded_pr is not None:
        command.extend(["view", str(recorded_pr)])
    else:
        command.extend(["list", "--state", "all", "--limit", "200"])
    command.extend(["--repo", github_repository, "--json", fields])
    result = subprocess.run(
        command,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        raise Blocked(
            "read-only GitHub reconciliation failed: "
            + (result.stderr.strip() or result.stdout.strip())
        )
    try:
        response = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise Refused("GitHub PR inventory was not valid JSON") from exc
    if recorded_pr is not None:
        if not isinstance(response, Mapping):
            raise Refused("GitHub PR view must be an object")
        pull_requests = [response]
    else:
        if not isinstance(response, list):
            raise Refused("GitHub PR inventory must be a list")
        pull_requests = response
    prs_by_branch: dict[str, dict[str, Any]] = {}
    for raw in pull_requests:
        if not isinstance(raw, Mapping) or not raw.get("headRefName"):
            continue
        branch = str(raw["headRefName"])
        current = prs_by_branch.get(branch)
        if current is None or int(raw.get("number", 0)) > int(current.get("number", 0)):
            prs_by_branch[branch] = dict(raw)

    tasks: dict[str, dict[str, Any]] = {}
    for key, task in state["tasks"].items():
        if task_number is not None and int(key) != task_number:
            continue
        branch = _task_branch(task)
        pushed_sha = refs.get(f"refs/heads/{branch}")
        pr = prs_by_branch.get(branch)
        remote: dict[str, Any] = {"branch": branch}
        if pushed_sha:
            remote["head_sha"] = pushed_sha
            remote["pushed_sha"] = pushed_sha
        if pr is not None:
            base_ref = str(pr.get("baseRefName") or "")
            if base_ref != target_name:
                raise Blocked(
                    f"PR {pr.get('number')} for {branch} targets {base_ref!r}, "
                    f"not controller integration target {target_name!r}"
                )
            pr_head = pr.get("headRefOid")
            if pr_head:
                remote["head_sha"] = require_sha(str(pr_head), "PR head SHA")
                remote["pushed_sha"] = remote["head_sha"]
            remote.update(
                {
                    "pr": pr.get("number"),
                    "pr_url": pr.get("url"),
                    "pr_state": str(pr.get("state", "")).lower(),
                    "draft": bool(pr.get("isDraft")),
                    "ci": _ci_state(pr.get("statusCheckRollup")),
                }
            )
            merge_commit = pr.get("mergeCommit")
            if str(pr.get("state", "")).upper() == "MERGED" and isinstance(
                merge_commit, Mapping
            ):
                remote["merge_sha"] = require_sha(
                    str(merge_commit.get("oid", "")), "merge SHA"
                )
                remote["merge_parent"] = require_sha(
                    str(pr.get("baseRefOid", "")), "merge parent"
                )
        if len(remote) > 1:
            tasks[str(key)] = remote

    target_sha = refs.get(f"refs/heads/{target_name}")
    if target_sha is None:
        raise Blocked(f"remote target ref is missing: {target_ref}")
    return {"target_sha": target_sha, "tasks": tasks}


def _read_direct_github_pr(repo: Path, pr: int) -> dict[str, Any]:
    gh = os.environ.get("FLEET_GH_EXECUTABLE", "").strip() or shutil.which("gh")
    if not gh:
        raise Refused("GitHub CLI is required for merge-receipt repair")
    fields = (
        "number,url,state,isDraft,headRefName,headRefOid,baseRefName,"
        "baseRefOid,mergeCommit,statusCheckRollup"
    )
    result = subprocess.run(
        [
            gh,
            "pr",
            "view",
            str(pr),
            "--repo",
            _github_repository(repo),
            "--json",
            fields,
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        raise Blocked(
            f"direct GitHub PR {pr} authentication failed: "
            + (result.stderr.strip() or result.stdout.strip())
        )
    try:
        value = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise Refused(f"direct GitHub PR {pr} result was not valid JSON") from exc
    if not isinstance(value, Mapping):
        raise Refused(f"direct GitHub PR {pr} result must be an object")
    return dict(value)


def _load_repair_manifest(path: Path, supplied_hash: str) -> tuple[Path, dict[str, Any]]:
    manifest_path = require_operational_path(path)
    require_hash(supplied_hash, "manifest hash")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(manifest_path, flags)
    except OSError as exc:
        raise Refused(f"repair manifest is not a durable file: {manifest_path}") from exc
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > 1024 * 1024:
            raise Refused("repair manifest must be a regular file no larger than 1 MiB")
        chunks: list[bytes] = []
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            chunks.append(chunk)
        raw_manifest = b"".join(chunks)
    finally:
        os.close(descriptor)
    actual_hash = hashlib.sha256(raw_manifest).hexdigest()
    if actual_hash != supplied_hash:
        raise Refused(
            f"manifest hash mismatch: expected {supplied_hash}, got {actual_hash}"
        )
    try:
        value = json.loads(raw_manifest.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise Refused("repair manifest is not valid JSON") from exc
    if not isinstance(value, Mapping):
        raise Refused("repair manifest must be an object")
    if set(value) != {
        "schema_version",
        "transaction",
        "before",
        "old_target_sha",
        "target_sha",
        "tasks",
    }:
        raise Refused("repair manifest has missing or unsupported top-level fields")
    if type(value["schema_version"]) is not int or value["schema_version"] != 1:
        raise Refused("repair manifest schema_version must be 1")
    if value["transaction"] != "repair-merge-receipts":
        raise Refused("repair manifest transaction must be repair-merge-receipts")
    before = value["before"]
    before_fields = {
        "fleet_state_sha256",
        "fleet_events_sha256",
        "receipt_index_sha256",
    }
    if not isinstance(before, Mapping) or set(before) != before_fields:
        raise Refused("repair manifest before hashes are incomplete")
    for field in sorted(before_fields):
        if not isinstance(before[field], str):
            raise Refused(f"before.{field} must be a string")
        require_hash(before[field], f"before.{field}")
    for field in ("old_target_sha", "target_sha"):
        if not isinstance(value[field], str):
            raise Refused(f"repair manifest {field} must be a string")
    old_target_sha = require_sha(value["old_target_sha"], "old target SHA")
    target_sha = require_sha(value["target_sha"], "target SHA")
    entries = value["tasks"]
    if not isinstance(entries, list) or not entries:
        raise Refused("repair manifest tasks must be a non-empty array")
    entry_fields = {
        "task",
        "old_expected_merge_parent",
        "pr",
        "remote_branch",
        "github_base_sha",
        "head_sha",
        "merge_sha",
    }
    seen_tasks: set[int] = set()
    seen_prs: set[int] = set()
    seen_branches: set[str] = set()
    normalized_entries: list[dict[str, Any]] = []
    for raw in entries:
        if not isinstance(raw, Mapping) or set(raw) != entry_fields:
            raise Refused("repair manifest task entry has missing or unsupported fields")
        number = raw["task"]
        pr = raw["pr"]
        if type(number) is not int or number < 0:
            raise Refused("repair manifest task number must be a non-negative integer")
        if type(pr) is not int or pr <= 0:
            raise Refused("repair manifest PR number must be a positive integer")
        branch = require_remote_branch(raw["remote_branch"])
        entry = dict(raw)
        for field in (
            "old_expected_merge_parent",
            "github_base_sha",
            "head_sha",
            "merge_sha",
        ):
            if not isinstance(raw[field], str):
                raise Refused(f"repair manifest {field} must be a string")
        entry["old_expected_merge_parent"] = require_sha(
            raw["old_expected_merge_parent"], "old expected merge parent"
        )
        entry["github_base_sha"] = require_sha(
            raw["github_base_sha"], "GitHub base SHA"
        )
        entry["head_sha"] = require_sha(raw["head_sha"], "head SHA")
        entry["merge_sha"] = require_sha(raw["merge_sha"], "merge SHA")
        if number in seen_tasks or pr in seen_prs or branch in seen_branches:
            raise Refused("repair manifest task, PR, and remote branch bindings must be unique")
        seen_tasks.add(number)
        seen_prs.add(pr)
        seen_branches.add(branch)
        normalized_entries.append(entry)
    return manifest_path, {
        "schema_version": 1,
        "transaction": "repair-merge-receipts",
        "before": dict(before),
        "old_target_sha": old_target_sha,
        "target_sha": target_sha,
        "tasks": normalized_entries,
    }


def _recorded_pr(task: Mapping[str, Any]) -> int | None:
    value = task.get("pr_ci_state")
    candidate = value.get("pr") if isinstance(value, Mapping) else None
    return candidate if type(candidate) is int and candidate > 0 else None


def _append_event_locked(store: StateStore, event: Mapping[str, Any]) -> dict[str, Any]:
    sequence = event.get("sequence")
    if type(sequence) is not int or sequence < 1:
        raise Refused("repair event must carry its prepared sequence")
    record = dict(event)
    existing = store.events_path.read_bytes() if store.events_path.exists() else b""
    lines = existing.splitlines(keepends=True)
    prior_lines = sequence - 1
    if len(lines) < prior_lines or any(
        not line.endswith(b"\n") for line in lines[:prior_lines]
    ):
        raise Refused("repair event log prefix does not match prepared sequence")
    prefix = b"".join(lines[:prior_lines])
    tail = existing[len(prefix) :]
    encoded = (json.dumps(record, sort_keys=True) + "\n").encode("utf-8")
    if not encoded.startswith(tail):
        raise Refused("repair event log has an unauthenticated partial tail")
    atomic_bytes(store.events_path, prefix + encoded)
    return record


def _validate_repair_binding_uniqueness(
    state: Mapping[str, Any], entries: list[dict[str, Any]]
) -> None:
    repaired = {str(entry["task"]) for entry in entries}
    requested_prs = {int(entry["pr"]): int(entry["task"]) for entry in entries}
    requested_branches = {
        str(entry["remote_branch"]): int(entry["task"]) for entry in entries
    }
    for key, task in state["tasks"].items():
        if key in repaired:
            continue
        pr = _recorded_pr(task)
        if pr in requested_prs:
            raise Refused(
                f"repair PR {pr} is already bound to unrelated Task {key}"
            )
        branch = _task_branch(task)
        if branch in requested_branches:
            raise Refused(
                f"repair branch {branch} is already bound to unrelated Task {key}"
            )


def _verify_repair_replay(
    store: StateStore,
    state: Mapping[str, Any],
    receipt: Mapping[str, Any],
    manifest_path: Path,
    manifest_hash: str,
) -> dict[str, Any]:
    if receipt.get("manifest") != str(manifest_path) or receipt.get(
        "manifest_sha256"
    ) != manifest_hash:
        raise Refused("existing repair receipt does not match this durable manifest")
    if state.get("target_sha") != receipt.get("target_sha"):
        raise Refused("existing repair receipt target does not match controller state")
    changes = receipt.get("tasks")
    if not isinstance(changes, Mapping) or not changes:
        raise Refused("existing repair receipt has no authenticated task bindings")
    for key, raw_change in changes.items():
        if not isinstance(raw_change, Mapping) or not isinstance(
            raw_change.get("new"), Mapping
        ):
            raise Refused("existing repair receipt task binding is malformed")
        task = _task(store, dict(state), int(key))
        new = raw_change["new"]
        if (
            task.get("expected_merge_parent") != new.get("expected_merge_parent")
            or task.get("remote_branch") != new.get("remote_branch")
            or _recorded_pr(task) != new.get("pr")
            or task.get("head_sha") != new.get("head_sha")
            or task.get("pushed_sha") != new.get("pushed_sha")
            or (
                task.get("merge_sha") is not None
                and task.get("merge_sha") != raw_change.get("merge_sha")
            )
        ):
            raise Refused("existing repair receipt does not match current controller state")
    sequence = receipt.get("event_sequence")
    event_found = False
    if type(sequence) is int and store.events_path.is_file():
        for line in store.events_path.read_text(encoding="utf-8").splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if (
                event.get("sequence") == sequence
                and event.get("event") == "repair-merge-receipts"
                and event.get("manifest_sha256") == manifest_hash
                and event.get("tasks") == changes
            ):
                event_found = True
                break
    if not event_found:
        raise Refused("existing repair receipt lacks its append-only repair event")
    return {**dict(receipt), "idempotent": True}


def _authenticate_repair_entry(
    repository: Path,
    store: StateStore,
    state: Mapping[str, Any],
    entry: Mapping[str, Any],
    target_sha: str,
) -> dict[str, Any]:
    number = int(entry["task"])
    task = _task(store, dict(state), number)
    if task.get("expected_merge_parent") != entry["old_expected_merge_parent"]:
        raise Refused(
            f"Task {number} old expected parent does not match controller state"
        )
    pr = int(entry["pr"])
    github = _read_direct_github_pr(repository, pr)
    if type(github.get("number")) is not int or github["number"] != pr:
        raise Blocked(f"Task {number} GitHub PR number identity mismatch")
    if str(github.get("state", "")).upper() != "MERGED" or bool(
        github.get("isDraft")
    ):
        raise Blocked(f"Task {number} GitHub PR is not a merged non-draft PR")
    branch = str(entry["remote_branch"])
    if github.get("headRefName") != branch:
        raise Blocked(f"Task {number} GitHub head branch identity mismatch")
    target_name = str(state["target_ref"]).removeprefix("origin/")
    if github.get("baseRefName") != target_name:
        raise Blocked(f"Task {number} GitHub base branch identity mismatch")
    base_sha = str(entry["github_base_sha"])
    if github.get("baseRefOid") != base_sha:
        raise Blocked(f"Task {number} GitHub base SHA identity mismatch")
    merge_commit = github.get("mergeCommit")
    observed_merge = (
        merge_commit.get("oid") if isinstance(merge_commit, Mapping) else None
    )
    merge_sha = str(entry["merge_sha"])
    if observed_merge != merge_sha:
        raise Blocked(f"Task {number} GitHub merge SHA identity mismatch")
    if _ci_state(github.get("statusCheckRollup")) != "success":
        raise Blocked(f"Task {number} GitHub required checks are not successful")
    head_sha = require_sha(str(github.get("headRefOid", "")), "GitHub head SHA")
    if head_sha != entry["head_sha"]:
        raise Blocked(f"Task {number} GitHub head SHA identity mismatch")
    for field in ("head_sha", "pushed_sha"):
        durable = task.get(field)
        if durable is not None:
            if not isinstance(durable, str):
                raise Refused(f"Task {number} durable {field} must be a SHA or null")
            require_sha(durable, f"Task {number} durable {field}")
    head_ref = f"refs/heads/{branch}"
    pull_ref = f"refs/pull/{pr}/head"
    remote_lines = _git(
        repository, "ls-remote", "origin", head_ref, pull_ref
    ).splitlines()
    remote_refs = {
        fields[1]: require_sha(fields[0], f"Task {number} remote head SHA")
        for line in remote_lines
        if len(fields := line.split()) == 2
    }
    witnesses = {
        ref: remote_refs[ref]
        for ref in (head_ref, pull_ref)
        if ref in remote_refs
    }
    if not witnesses or any(observed != head_sha for observed in witnesses.values()):
        raise Blocked(
            f"Task {number} remote branch/pull head identity mismatch"
        )
    for name, commit in (
        ("GitHub base", base_sha),
        ("merge", merge_sha),
        ("controller target", target_sha),
    ):
        try:
            _git(repository, "cat-file", "-e", f"{commit}^{{commit}}")
        except Blocked as exc:
            raise Blocked(f"Task {number} local {name} commit is unavailable") from exc
    parents = _git(repository, "show", "-s", "--format=%P", merge_sha).split()
    if not parents or parents[0] != base_sha:
        raise Blocked(f"Task {number} local first-parent identity mismatch")
    try:
        _git(
            repository,
            "merge-base",
            "--is-ancestor",
            merge_sha,
            target_sha,
        )
    except Blocked as exc:
        raise Blocked(
            f"Task {number} merge is not an ancestor of the controller target"
        ) from exc
    old = {
        "expected_merge_parent": task.get("expected_merge_parent"),
        "pr": _recorded_pr(task),
        "remote_branch": task.get("remote_branch"),
        "head_sha": task.get("head_sha"),
        "pushed_sha": task.get("pushed_sha"),
    }
    new = {
        "expected_merge_parent": base_sha,
        "pr": pr,
        "remote_branch": branch,
        "head_sha": head_sha,
        "pushed_sha": head_sha,
    }
    return {"old": old, "new": new, "merge_sha": merge_sha}


def _authenticate_repair_target(
    repository: Path,
    state: Mapping[str, Any],
    transaction: Mapping[str, Any],
) -> str:
    old_target_sha = str(transaction["old_target_sha"])
    if state.get("target_sha") != old_target_sha:
        raise Refused("repair old target SHA does not match controller state")
    target_ref = str(state["target_ref"])
    target_name = target_ref.removeprefix("origin/")
    if target_name == target_ref or not target_name:
        raise Refused("controller target_ref must name an origin branch")
    target_sha = str(transaction["target_sha"])
    remote = _git(
        repository,
        "ls-remote",
        "--heads",
        "origin",
        f"refs/heads/{target_name}",
    ).splitlines()
    if len(remote) != 1 or remote[0].split() != [
        target_sha,
        f"refs/heads/{target_name}",
    ]:
        raise Blocked("repair target SHA does not match the remote integration ref")
    try:
        _git(repository, "cat-file", "-e", f"{target_sha}^{{commit}}")
    except Blocked as exc:
        raise Blocked("repair target commit is unavailable locally") from exc
    return target_sha


def repair_transaction_path(store: StateStore, manifest_hash: str) -> Path:
    require_hash(manifest_hash, "manifest hash")
    return store.root / "repair-transactions" / f"{manifest_hash}.json"


def _json_document_bytes(value: Mapping[str, Any]) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")


def _bytes_hash(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _apply_repair_changes(
    store: StateStore,
    state: dict[str, Any],
    changes: Mapping[str, Any],
    target_sha: str,
) -> None:
    for key, raw_change in changes.items():
        if not isinstance(raw_change, Mapping):
            raise Refused("repair journal task change must be an object")
        old = raw_change.get("old")
        new = raw_change.get("new")
        if not isinstance(old, Mapping) or not isinstance(new, Mapping):
            raise Refused("repair journal task change lacks old/new bindings")
        task = _task(store, state, int(key))
        current = {
            "expected_merge_parent": task.get("expected_merge_parent"),
            "pr": _recorded_pr(task),
            "remote_branch": task.get("remote_branch"),
            "head_sha": task.get("head_sha"),
            "pushed_sha": task.get("pushed_sha"),
        }
        if current != old:
            raise Refused(f"Task {key} no longer matches repair journal before binding")
        task["expected_merge_parent"] = new["expected_merge_parent"]
        task["remote_branch"] = new["remote_branch"]
        task["head_sha"] = new["head_sha"]
        task["pushed_sha"] = new["pushed_sha"]
        task["pr_ci_state"] = {"pr": new["pr"]}
    state["target_sha"] = target_sha
    validate_state(state)


def _validate_repair_journal(
    journal: Any,
    manifest_path: Path,
    manifest_hash: str,
    transaction: Mapping[str, Any],
) -> dict[str, Any]:
    fields = {
        "schema_version",
        "phase",
        "manifest",
        "manifest_sha256",
        "before",
        "after",
        "events_before_size",
        "outputs",
        "target_sha",
        "tasks",
        "event",
        "receipt",
    }
    if not isinstance(journal, Mapping) or set(journal) != fields:
        raise Refused("merge-receipt repair journal is malformed")
    if journal.get("schema_version") != 1 or journal.get("phase") not in {
        "prepared",
        "complete",
    }:
        raise Refused("merge-receipt repair journal has invalid lifecycle")
    if journal.get("manifest") != str(manifest_path) or journal.get(
        "manifest_sha256"
    ) != manifest_hash:
        raise Refused("merge-receipt repair journal manifest identity mismatch")
    if journal.get("before") != transaction["before"]:
        raise Refused("merge-receipt repair journal before hashes mismatch")
    if journal.get("target_sha") != transaction["target_sha"]:
        raise Refused("merge-receipt repair journal target SHA mismatch")
    after = journal.get("after")
    after_fields = {
        "fleet_state_sha256",
        "fleet_events_sha256",
        "receipt_index_sha256",
    }
    if not isinstance(after, Mapping) or set(after) != after_fields:
        raise Refused("merge-receipt repair journal after hashes are incomplete")
    for field in after_fields:
        if not isinstance(after[field], str):
            raise Refused(f"repair journal after.{field} must be a string")
        require_hash(after[field], f"repair journal after.{field}")
    if type(journal.get("events_before_size")) is not int or journal[
        "events_before_size"
    ] < 0:
        raise Refused("repair journal events_before_size must be non-negative")
    changes = journal.get("tasks")
    if not isinstance(changes, Mapping):
        raise Refused("merge-receipt repair journal tasks must be an object")
    entries = {str(entry["task"]): entry for entry in transaction["tasks"]}
    if set(changes) != set(entries):
        raise Refused("merge-receipt repair journal task set mismatches manifest")
    for key, entry in entries.items():
        change = changes[key]
        if not isinstance(change, Mapping):
            raise Refused("merge-receipt repair journal task change must be an object")
        old = change.get("old")
        new = change.get("new")
        if not isinstance(old, Mapping) or not isinstance(new, Mapping):
            raise Refused("merge-receipt repair journal task change lacks old/new")
        if old.get("expected_merge_parent") != entry["old_expected_merge_parent"]:
            raise Refused("repair journal old parent mismatches manifest")
        if new != {
            "expected_merge_parent": entry["github_base_sha"],
            "pr": entry["pr"],
            "remote_branch": entry["remote_branch"],
            "head_sha": entry["head_sha"],
            "pushed_sha": entry["head_sha"],
        } or change.get("merge_sha") != entry["merge_sha"]:
            raise Refused("repair journal new binding mismatches manifest")
        if new.get("head_sha") != new.get("pushed_sha"):
            raise Refused("repair journal authenticated head bindings mismatch")
        require_sha(str(new.get("head_sha", "")), "repair journal head SHA")
    event = journal.get("event")
    receipt = journal.get("receipt")
    if not isinstance(event, Mapping) or not isinstance(receipt, Mapping):
        raise Refused("repair journal event and receipt must be objects")
    if (
        type(event.get("sequence")) is not int
        or not isinstance(event.get("timestamp"), str)
        or event.get("event") != "repair-merge-receipts"
        or event.get("manifest") != str(manifest_path)
        or event.get("manifest_sha256") != manifest_hash
        or event.get("before") != transaction["before"]
        or event.get("target_sha") != transaction["target_sha"]
        or event.get("tasks") != changes
    ):
        raise Refused("repair journal append-only event is malformed")
    expected_receipt = {
        "manifest": str(manifest_path),
        "manifest_sha256": manifest_hash,
        "event_sequence": event["sequence"],
        "timestamp": event["timestamp"],
        "target_sha": transaction["target_sha"],
        "tasks": changes,
    }
    if receipt != expected_receipt:
        raise Refused("repair journal receipt does not match its event")
    outputs = journal.get("outputs")
    if not isinstance(outputs, Mapping) or set(outputs) != {
        "snapshot",
        "receipt_index",
    }:
        raise Refused("repair journal exact outputs are incomplete")
    snapshot = outputs["snapshot"]
    receipt_index = outputs["receipt_index"]
    if not isinstance(snapshot, Mapping) or not isinstance(receipt_index, Mapping):
        raise Refused("repair journal exact outputs must be objects")
    validate_state(snapshot)
    if snapshot.get("target_sha") != transaction["target_sha"]:
        raise Refused("repair journal planned snapshot target mismatch")
    if _bytes_hash(_json_document_bytes(snapshot)) != after["fleet_state_sha256"]:
        raise Refused("repair journal planned snapshot hash mismatch")
    for key, change in changes.items():
        task = snapshot["tasks"].get(key)
        if not isinstance(task, Mapping):
            raise Refused("repair journal planned snapshot lacks repaired task")
        new = change["new"]
        if (
            task.get("expected_merge_parent") != new["expected_merge_parent"]
            or task.get("remote_branch") != new["remote_branch"]
            or _recorded_pr(task) != new["pr"]
            or task.get("head_sha") != new["head_sha"]
            or task.get("pushed_sha") != new["pushed_sha"]
        ):
            raise Refused("repair journal planned snapshot identity mismatch")
    if _bytes_hash(_json_document_bytes(receipt_index)) != after[
        "receipt_index_sha256"
    ]:
        raise Refused("repair journal planned receipt-index hash mismatch")
    planned_repairs = receipt_index.get("merge_receipt_repairs", {})
    if not isinstance(planned_repairs, Mapping) or planned_repairs.get(
        manifest_hash
    ) != receipt:
        raise Refused("repair journal planned receipt-index identity mismatch")
    return dict(journal)


def _validate_repair_replay(
    store: StateStore,
    journal: dict[str, Any],
) -> dict[str, Any]:
    before = journal["before"]
    after = journal["after"]
    outputs = journal["outputs"]
    snapshot_output = outputs["snapshot"]
    receipt_index_output = outputs["receipt_index"]

    state_hash = sha256_file(store.snapshot_path)
    if state_hash == before["fleet_state_sha256"]:
        state = store.read_snapshot()
        planned_state = json.loads(json.dumps(state))
        _apply_repair_changes(
            store, planned_state, journal["tasks"], journal["target_sha"]
        )
        if planned_state != snapshot_output:
            raise Refused("repair planned snapshot identity mismatch")
        write_snapshot = True
    elif state_hash == after["fleet_state_sha256"]:
        state = store.read_snapshot()
        if state != snapshot_output:
            raise Refused("journaled repair snapshot identity mismatch")
        write_snapshot = False
    else:
        raise Refused("repair snapshot matches neither journaled before nor after hash")

    events_bytes = store.events_path.read_bytes()
    before_size = journal["events_before_size"]
    if len(events_bytes) < before_size:
        raise Refused("repair event log is shorter than its journaled prefix")
    prefix = events_bytes[:before_size]
    tail = events_bytes[before_size:]
    if _bytes_hash(prefix) != before["fleet_events_sha256"]:
        raise Refused("repair event prefix does not match journaled before hash")
    encoded_event = (
        json.dumps(journal["event"], sort_keys=True) + "\n"
    ).encode("utf-8")
    events_output = prefix + encoded_event
    if _bytes_hash(events_output) != after["fleet_events_sha256"]:
        raise Refused("repair journal planned event hash mismatch")
    if events_bytes == events_output:
        write_event = False
    elif encoded_event.startswith(tail):
        write_event = True
    else:
        raise Refused("repair events match neither journaled before nor after identity")

    index_hash = sha256_file(store.receipt_index_path)
    if index_hash == before["receipt_index_sha256"]:
        index = store.read_receipt_index()
        repairs = index.get("merge_receipt_repairs", {})
        if not isinstance(repairs, Mapping):
            raise Refused("receipt index merge_receipt_repairs must be an object")
        planned_index = dict(index)
        planned_repairs = dict(repairs)
        planned_repairs[journal["manifest_sha256"]] = journal["receipt"]
        planned_index["merge_receipt_repairs"] = planned_repairs
        if planned_index != receipt_index_output:
            raise Refused("repair planned receipt-index identity mismatch")
        write_index = True
    elif index_hash == after["receipt_index_sha256"]:
        index = store.read_receipt_index()
        if index != receipt_index_output:
            raise Refused("journaled repair receipt-index identity mismatch")
        write_index = False
    else:
        raise Refused("repair index matches neither journaled before nor after hash")

    return {
        "snapshot": snapshot_output,
        "events": events_output,
        "receipt_index": receipt_index_output,
        "write_snapshot": write_snapshot,
        "write_event": write_event,
        "write_index": write_index,
    }


def _resume_repair_transaction(
    store: StateStore,
    journal_path: Path,
    journal: dict[str, Any],
) -> dict[str, Any]:
    after = journal["after"]
    replay = _validate_repair_replay(store, journal)
    if replay["write_snapshot"]:
        atomic_json(store.snapshot_path, replay["snapshot"])
        if sha256_file(store.snapshot_path) != after["fleet_state_sha256"]:
            raise Refused("repair snapshot write did not produce journaled hash")
    if replay["write_event"]:
        record = _append_event_locked(store, journal["event"])
        if record != journal["event"]:
            raise Refused("repair event sequence changed during journal replay")
        if sha256_file(store.events_path) != after["fleet_events_sha256"]:
            raise Refused("repair event append did not produce journaled hash")
    if replay["write_index"]:
        atomic_json(store.receipt_index_path, replay["receipt_index"])
        if sha256_file(store.receipt_index_path) != after["receipt_index_sha256"]:
            raise Refused("repair receipt write did not produce journaled hash")

    journal["phase"] = "complete"
    atomic_json(journal_path, journal)
    return {**journal["receipt"], "idempotent": False}


def _prepare_repair_journal(
    store: StateStore,
    state: dict[str, Any],
    index: dict[str, Any],
    transaction: Mapping[str, Any],
    manifest_path: Path,
    manifest_hash: str,
    changes: Mapping[str, Any],
    target_sha: str,
) -> dict[str, Any]:
    _apply_repair_changes(store, state, changes, target_sha)
    state_after_hash = _bytes_hash(_json_document_bytes(state))
    events_before = store.events_path.read_bytes()
    if events_before and not events_before.endswith(b"\n"):
        raise Refused("append-only event log lacks a final newline")
    sequence = len(events_before.splitlines()) + 1
    event = {
        "sequence": sequence,
        "timestamp": utc_now(),
        "event": "repair-merge-receipts",
        "manifest": str(manifest_path),
        "manifest_sha256": manifest_hash,
        "before": transaction["before"],
        "target_sha": target_sha,
        "tasks": changes,
    }
    events_after = events_before + (json.dumps(event, sort_keys=True) + "\n").encode(
        "utf-8"
    )
    receipt = {
        "manifest": str(manifest_path),
        "manifest_sha256": manifest_hash,
        "event_sequence": event["sequence"],
        "timestamp": event["timestamp"],
        "target_sha": target_sha,
        "tasks": changes,
    }
    repairs = index.get("merge_receipt_repairs", {})
    if not isinstance(repairs, Mapping):
        raise Refused("receipt index merge_receipt_repairs must be an object")
    updated_index = dict(index)
    updated_repairs = dict(repairs)
    updated_repairs[manifest_hash] = receipt
    updated_index["merge_receipt_repairs"] = updated_repairs
    return {
        "schema_version": 1,
        "phase": "prepared",
        "manifest": str(manifest_path),
        "manifest_sha256": manifest_hash,
        "before": transaction["before"],
        "target_sha": target_sha,
        "events_before_size": len(events_before),
        "outputs": {
            "snapshot": state,
            "receipt_index": updated_index,
        },
        "after": {
            "fleet_state_sha256": state_after_hash,
            "fleet_events_sha256": _bytes_hash(events_after),
            "receipt_index_sha256": _bytes_hash(_json_document_bytes(updated_index)),
        },
        "tasks": changes,
        "event": event,
        "receipt": receipt,
    }


def repair_merge_receipts(
    store: StateStore,
    repo: Path,
    manifest: Path,
    manifest_sha256: str,
) -> dict[str, Any]:
    """Apply or resume one authenticated merge-receipt binding transaction."""
    repository = require_operational_path(repo)
    manifest_path, transaction = _load_repair_manifest(
        manifest, manifest_sha256
    )
    journal_path = repair_transaction_path(store, manifest_sha256)
    with store._locked():
        store._require_no_pending_repair(allowed_journal=journal_path)
        state = store.read_snapshot()
        index = store.read_receipt_index()
        repairs = index.get("merge_receipt_repairs", {})
        if not isinstance(repairs, Mapping):
            raise Refused("receipt index merge_receipt_repairs must be an object")
        if journal_path.exists():
            try:
                raw_journal = json.loads(journal_path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError) as exc:
                raise Refused(f"malformed merge-receipt repair journal: {journal_path}") from exc
            journal = _validate_repair_journal(
                raw_journal, manifest_path, manifest_sha256, transaction
            )
            if journal["phase"] == "prepared":
                return _resume_repair_transaction(store, journal_path, journal)
            existing = repairs.get(manifest_sha256)
            if not isinstance(existing, Mapping):
                raise Refused("completed repair journal lacks receipt-index entry")
            return _verify_repair_replay(
                store, state, existing, manifest_path, manifest_sha256
            )
        existing = repairs.get(manifest_sha256)
        if existing is not None:
            if not isinstance(existing, Mapping):
                raise Refused("existing repair receipt must be an object")
            return _verify_repair_replay(
                store, state, existing, manifest_path, manifest_sha256
            )

        before_paths = {
            "fleet_state_sha256": store.snapshot_path,
            "fleet_events_sha256": store.events_path,
            "receipt_index_sha256": store.receipt_index_path,
        }
        for field, path in before_paths.items():
            if not path.is_file():
                raise Refused(f"before-state file is missing: {path}")
            actual = sha256_file(path)
            expected = transaction["before"][field]
            if actual != expected:
                raise Refused(
                    f"stale before-state hash for {path.name}: "
                    f"expected {expected}, got {actual}"
                )

        _validate_repair_binding_uniqueness(state, transaction["tasks"])
        target_sha = _authenticate_repair_target(repository, state, transaction)
        changes: dict[str, Any] = {}
        for entry in transaction["tasks"]:
            number = int(entry["task"])
            changes[str(number)] = _authenticate_repair_entry(
                repository, store, state, entry, target_sha
            )
        journal = _prepare_repair_journal(
            store,
            state,
            index,
            transaction,
            manifest_path,
            manifest_sha256,
            changes,
            target_sha,
        )
        atomic_json(journal_path, journal)
        return _resume_repair_transaction(store, journal_path, journal)


def inspect_worktree(task: Mapping[str, Any]) -> dict[str, Any]:
    """Authenticate recoverable local task state without changing the checkout."""
    worktree = require_operational_path(Path(task["paths"]["worktree"]))
    if not (worktree / ".git").exists():
        raise Blocked(f"Task {task['number']} worktree is missing: {worktree}")
    head_sha = require_sha(_git(worktree, "rev-parse", "HEAD"), "worktree HEAD")
    branch = _git(worktree, "branch", "--show-current")
    changed: list[str] = []
    status = _git(worktree, "status", "--porcelain=v1", "--untracked-files=all")
    for line in status.splitlines():
        path = line[3:]
        if " -> " in path:
            path = path.split(" -> ", 1)[1]
        if path == "HANDOFF.md":
            continue
        changed.append(path)
    outside = [path for path in changed if not _path_in_lease(path, list(task["file_lease"]))]
    if outside:
        raise Blocked(
            f"Task {task['number']} has changes outside file lease: {', '.join(outside)}"
        )
    handoff = worktree / "HANDOFF.md"
    observation: dict[str, Any] = {
        "head_sha": head_sha,
        "branch": branch,
        "changed_paths": changed,
        "handoff_sha256": sha256_file(handoff) if handoff.is_file() else None,
        "observed_at": utc_now(),
    }
    for name in ("prd", "report", "review"):
        path = require_operational_path(Path(task["paths"][name]))
        observation[f"{name}_sha256"] = sha256_file(path) if path.is_file() else None
    process = task.get("process")
    observation["process_live"] = worker_is_live(process) if process else False
    return observation


def acquire_merge_lease(store: StateStore, identity: str) -> bool:
    acquired = False

    def update(state: dict[str, Any]) -> None:
        nonlocal acquired
        owner = state["merge_lease"]
        if owner is None or owner == identity:
            state["merge_lease"] = identity
            acquired = True

    store.mutate(update)
    if acquired:
        store.append_event({"event": "merge-lease-acquired", "identity": identity})
    return acquired


def release_merge_lease(store: StateStore, identity: str) -> bool:
    released = False

    def update(state: dict[str, Any]) -> None:
        nonlocal released
        if state["merge_lease"] == identity:
            state["merge_lease"] = None
            released = True

    store.mutate(update)
    if released:
        store.append_event({"event": "merge-lease-released", "identity": identity})
    return released


def process_start_time(pid: int) -> str:
    """Return a kernel start identity; display-oriented ``ps`` is forbidden."""
    stat = Path(f"/proc/{pid}/stat")
    try:
        # field 22 is the kernel tick at which this exact PID instance began.
        fields = stat.read_text(encoding="utf-8").rsplit(") ", 1)[1].split()
        return fields[19]
    except (OSError, IndexError, ValueError):
        pass
    # libproc's BSD-info start fields are kernel values on macOS.  Do not
    # substitute a lossy display string when the native query is unavailable.
    try:
        # proc_bsdinfo is 136 bytes on current macOS; start timeval begins at
        # offsets 120/128.  Decode the native bytes directly to avoid ctypes
        # ABI-padding ambiguity.
        info = ctypes.create_string_buffer(136)
        libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
        if libproc.proc_pidinfo(pid, 3, 0, info, 136) >= 136:
            raw = info.raw
            seconds = int.from_bytes(raw[120:128], sys.byteorder)
            microseconds = int.from_bytes(raw[128:136], sys.byteorder)
            return f"{seconds}:{microseconds}" if seconds else ""
    except (OSError, AttributeError):
        pass
    return ""


def process_command(pid: int) -> str:
    argv = process_kernel_argv(pid)
    return shlex.join(argv) if argv else ""


def process_kernel_executable(pid: int) -> str:
    """Return the kernel executable path, never a lossy process display string."""
    proc_link = Path(f"/proc/{pid}/exe")
    if proc_link.exists():
        try:
            return str(proc_link.resolve())
        except OSError:
            return ""
    try:
        library = ctypes.CDLL("/usr/lib/libproc.dylib")
        buffer = ctypes.create_string_buffer(4096)
        if library.proc_pidpath(pid, buffer, len(buffer)) > 0:
            return os.path.realpath(buffer.value.decode("utf-8"))
    except (OSError, AttributeError, UnicodeDecodeError):
        pass
    return ""


def process_kernel_argv(pid: int) -> list[str]:
    """Read kernel argv boundaries exactly; never fall back to ``ps`` text."""
    proc_args = Path(f"/proc/{pid}/cmdline")
    try:
        raw = proc_args.read_bytes()
        values = raw.split(b"\0")
        if not raw or values[-1] != b"":
            return []
        return [value.decode("utf-8", "surrogateescape") for value in values[:-1]]
    except OSError:
        pass
    try:
        size = ctypes.c_size_t(0)
        libc = ctypes.CDLL("/usr/lib/libSystem.B.dylib")
        # CTL_KERN / KERN_PROCARGS2 is the supported macOS raw argv API.
        mib = (ctypes.c_int * 3)(1, 49, pid)
        if libc.sysctl(mib, 3, None, ctypes.byref(size), None, 0) != 0 or not size.value:
            return []
        data = ctypes.create_string_buffer(size.value)
        if libc.sysctl(mib, 3, data, ctypes.byref(size), None, 0) != 0:
            return []
        return parse_macos_procargs(data.raw[:size.value])
    except (OSError, AttributeError, ValueError):
        return []


def parse_macos_procargs(raw: bytes) -> list[str]:
    """Parse KERN_PROCARGS2 without collapsing empty argv elements."""
    if len(raw) < 5:
        return []
    argc = int.from_bytes(raw[:4], sys.byteorder)
    if argc < 1:
        return []
    cursor = 4
    executable_end = raw.find(b"\0", cursor)
    if executable_end < cursor:
        return []
    cursor = executable_end + 1
    # KERN_PROCARGS2 pads between the executable string and argv[0].  Once the
    # first argument begins, consume exactly argc NUL-terminated values: an
    # empty value after argv[0] is a real argument, not padding.
    while cursor < len(raw) and raw[cursor] == 0:
        cursor += 1
    args: list[str] = []
    for _ in range(argc):
        end = raw.find(b"\0", cursor)
        if end < cursor:
            return []
        args.append(raw[cursor:end].decode("utf-8", "surrogateescape"))
        cursor = end + 1
    return args


def _shell_word(value: str, cursor: int) -> tuple[str, int, int] | None:
    """Read one POSIX-style word while retaining its raw source boundary."""
    while cursor < len(value) and value[cursor].isspace():
        cursor += 1
    if cursor >= len(value):
        return None
    start = cursor
    parts: list[str] = []
    quote: str | None = None
    while cursor < len(value):
        character = value[cursor]
        if quote is not None:
            if character == quote:
                quote = None
            elif character == "\\" and quote == '"':
                cursor += 1
                if cursor >= len(value):
                    return None
                parts.append(value[cursor])
            else:
                parts.append(character)
            cursor += 1
            continue
        if character.isspace():
            break
        if character in ("'", '"'):
            quote = character
        elif character == "\\":
            cursor += 1
            if cursor >= len(value):
                return None
            parts.append(value[cursor])
        else:
            parts.append(character)
        cursor += 1
    if quote is not None:
        return None
    return "".join(parts), start, cursor


def _shebang_invocation(
    executable: Path, *, platform: str | None = None
) -> tuple[list[str], bool]:
    """Return the kernel interpreter argv, preserving the optional-tail boundary.

    Linux passes one optional tail after the interpreter, whereas Darwin passes
    whitespace-separated optional interpreter arguments.  GNU ``env -S`` then
    applies split-string syntax exactly once to the input the relevant kernel
    model provides.  Unsupported Darwin quote/escape forms refuse rather than
    inventing a Linux-like argv vector.
    """
    current_platform = platform or sys.platform
    if current_platform not in {"linux", "darwin"}:
        return [], True
    try:
        with executable.open("rb") as source:
            line = source.readline(4096)
    except OSError:
        return [], False
    if not line.startswith(b"#!"):
        return [], False
    try:
        raw = line[2:].decode("utf-8").rstrip("\r\n")
    except UnicodeDecodeError:
        return [], True
    if current_platform == "darwin":
        if any(character in raw for character in ("'", '"', "\\")):
            return [], True
        values = raw.split()
        if not values:
            return [], True
        interpreter, *arguments = values
        if Path(interpreter).name != "env":
            return values, False
        return _darwin_env_invocation(arguments)
    first = _shell_word(raw, 0)
    if first is None:
        return [], True
    interpreter, _start, cursor = first
    while cursor < len(raw) and raw[cursor].isspace():
        cursor += 1
    tail = raw[cursor:]
    if Path(interpreter).name != "env":
        return ([interpreter, tail] if tail else [interpreter]), False
    return _linux_env_invocation(tail)


def _darwin_env_invocation(arguments: list[str]) -> tuple[list[str], bool]:
    """Model Darwin's already-whitespace-tokenized ``env`` interpreter argv."""
    cursor = 0
    while cursor < len(arguments):
        option = arguments[cursor]
        cursor += 1
        if option in {"-S", "--split-string"}:
            if cursor >= len(arguments):
                return [], True
            payload = arguments[cursor]
            cursor += 1
            try:
                values = shlex.split(payload, posix=True)
            except ValueError:
                return [], True
            values.extend(arguments[cursor:])
            return values, not bool(values)
        if option.startswith("-S") and len(option) > 2:
            try:
                values = shlex.split(option[2:], posix=True)
            except ValueError:
                return [], True
            values.extend(arguments[cursor:])
            return values, not bool(values)
        if option.startswith("--split-string="):
            try:
                values = shlex.split(option[len("--split-string="):], posix=True)
            except ValueError:
                return [], True
            values.extend(arguments[cursor:])
            return values, not bool(values)
        if option == "--":
            values = arguments[cursor:]
            return values, not bool(values)
        if option in {"-i", "--ignore-environment"}:
            continue
        if option in {"-u", "--unset", "-C", "--chdir", "--default-signal", "--ignore-signal", "--block-signal"}:
            if cursor >= len(arguments):
                return [], True
            cursor += 1
            continue
        if option.startswith(("--unset=", "--chdir=", "--default-signal=", "--ignore-signal=", "--block-signal=")):
            continue
        if option.startswith("-"):
            return [], True
        values = [option, *arguments[cursor:]]
        return values, False
    return [], True


def _linux_env_invocation(tail: str) -> tuple[list[str], bool]:
    """Model Linux's one-tail ``env`` interpreter argv without losing quotes."""
    cursor = 0
    while True:
        word = _shell_word(tail, cursor)
        if word is None:
            return [], True
        option, start, cursor = word
        split_offset: int | None = None
        if option in {"-S", "--split-string"}:
            split_offset = cursor
        elif option.startswith("-S") and len(option) > 2:
            split_offset = start + 2
        elif option.startswith("--split-string="):
            split_offset = start + len("--split-string=")
        if split_offset is not None:
            while split_offset < len(tail) and tail[split_offset].isspace():
                split_offset += 1
            if split_offset >= len(tail):
                return [], True
            try:
                values = shlex.split(tail[split_offset:], posix=True)
            except ValueError:
                return [], True
            return values, not bool(values)
        if option == "--":
            try:
                values = shlex.split(tail[cursor:], posix=True)
            except ValueError:
                return [], True
            return values, not bool(values)
        if option in {"-i", "--ignore-environment"}:
            continue
        if option in {"-u", "--unset", "-C", "--chdir", "--default-signal", "--ignore-signal", "--block-signal"}:
            value = _shell_word(tail, cursor)
            if value is None:
                return [], True
            _value, _start, cursor = value
            continue
        if option.startswith(("--unset=", "--chdir=", "--default-signal=", "--ignore-signal=", "--block-signal=")):
            continue
        if option.startswith("-"):
            return [], True
        try:
            values = shlex.split(tail[start:], posix=True)
        except ValueError:
            return [], True
        return values, not bool(values)


def expected_kernel_executable(executable: Path) -> str:
    """Resolve a script interpreter without accepting malformed env syntax."""
    invocation, malformed = _shebang_invocation(executable)
    if not invocation and not malformed:
        return str(executable.resolve())
    if malformed or not invocation:
        return ""
    interpreter = invocation[0]
    # macOS' /bin/sh launcher immediately execs bash; KERN_PROC_PIDPATH
    # correctly reports bash, so account for this OS-supplied indirection.
    if sys.platform == "darwin" and interpreter == "/bin/sh" and Path("/bin/bash").is_file():
        interpreter = "/bin/bash"
    resolved = shutil.which(interpreter) if not interpreter.startswith("/") else interpreter
    return str(Path(resolved).resolve()) if resolved else ""


def expected_kernel_argv(
    executable: Path, argv: list[str], *, platform: str | None = None
) -> list[str]:
    """Match the kernel's shebang expansion on Linux and macOS exactly."""
    invocation, malformed = _shebang_invocation(executable, platform=platform)
    if not invocation and not malformed:
        return list(argv)
    if malformed or not invocation or not argv:
        return []
    return [*invocation, argv[0], *argv[1:]]


def process_identity_matches(
    record: Mapping[str, Any],
    pid: int,
    start_time: str,
    argv: list[str],
    task_number: int | None = None,
) -> bool:
    token = record.get("token")
    return bool(
        record.get("pid") == pid
        and record.get("start_time") == start_time
        and record.get("argv") == argv
        and token
        and any(token in argument for argument in argv)
        and (task_number is None or record.get("task") == task_number)
        and bool(argv)
    )


def worker_is_live(record: Mapping[str, Any]) -> bool:
    pid = record.get("pid")
    if not isinstance(pid, int):
        return False
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    start = process_start_time(pid)
    argv = process_kernel_argv(pid)
    if not start or start != record.get("start_time") or argv != record.get("kernel_argv"):
        return False
    kernel = process_kernel_executable(pid)
    return bool(
        kernel
        and kernel == record.get("kernel_executable")
        and record.get("token")
        and any(record.get("token") in argument for argument in argv)
        and record.get("process_group") == os.getpgid(pid)
    )


def _process_group_exists(group: int) -> bool:
    """Prove group absence only from the kernel's ESRCH response."""
    try:
        os.killpg(group, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError as exc:
        raise Refused(f"cannot inspect process group {group}") from exc


def _group_members(group: int) -> list[int]:
    listing = subprocess.run(["ps", "-ax", "-o", "pid="], capture_output=True, text=True, check=False)
    members: list[int] = []
    for raw in listing.stdout.splitlines():
        try:
            pid = int(raw.strip())
            if os.getpgid(pid) == group:
                members.append(pid)
        except (ValueError, ProcessLookupError):
            continue
    return members


def _wait_for_group_exit(group: int, deadline: float) -> bool:
    while time.monotonic() < deadline:
        try:
            if not _process_group_exists(group):
                return True
        except Refused:
            # Darwin can return EPERM for a very short interval after a
            # successful signal while it reaps the final group member.  It is
            # not absence proof, so retry and require a later ESRCH; a
            # persistent permission failure still raises from the final probe.
            # The launch path is the direct parent of the session leader.
            # Reap only that leader when it is our child; recovery processes
            # receive ChildProcessError and retain the same fail-closed probe.
            try:
                os.waitpid(group, os.WNOHANG)
            except ChildProcessError:
                pass
        time.sleep(0.05)
    return not _process_group_exists(group)


def _terminate_group(group: int) -> None:
    try:
        os.killpg(group, signal.SIGTERM)
    except ProcessLookupError:
        return
    except PermissionError as exc:
        raise Refused(f"cannot terminate process group {group}") from exc
    if _wait_for_group_exit(group, time.monotonic() + 5):
        return
    try:
        os.killpg(group, signal.SIGKILL)
    except ProcessLookupError:
        return
    except PermissionError as exc:
        raise Refused(f"cannot kill process group {group}") from exc
    if not _wait_for_group_exit(group, time.monotonic() + 5):
        raise Refused(f"process group survived termination: {group}")


def _terminate_unrecorded_process(process: subprocess.Popen[Any]) -> None:
    _terminate_group(process.pid)


def _terminate_authenticated_process(
    record: Mapping[str, Any], signum: signal.Signals = signal.SIGTERM
) -> None:
    """Terminate only the process group named by an authenticated record."""
    pid = record.get("pid")
    group = record.get("process_group")
    required = ("start_time", "token", "argv", "kernel_argv", "kernel_executable")
    if (not isinstance(pid, int) or not isinstance(group, int) or group <= 0
            or not all(record.get(field) for field in required)
            or not isinstance(record.get("argv"), list)
            or not isinstance(record.get("kernel_argv"), list)):
        raise Refused("authenticated process record is incomplete; refusing group signal")
    if signum == signal.SIGTERM:
        _terminate_group(group)
        return
    try:
        os.killpg(group, signum)
    except ProcessLookupError:
        return
    except PermissionError as exc:
        raise Refused(f"cannot signal process group {group}") from exc


def launch_intent_path(store: StateStore, task_number: int, segment: int) -> Path:
    return store.root / "processes" / f"{task_number:02d}" / f"launch-intent-{segment}.json"


def _write_launch_intent(path: Path, value: Mapping[str, Any]) -> None:
    atomic_json(path, value)


def launch_handshake_path(intent_path: Path) -> Path:
    return intent_path.with_suffix(".ready.json")


def launch_fence_path(intent_path: Path) -> Path:
    return intent_path.with_suffix(".fence")


def _fence_is_held(path: Path) -> bool:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor = os.open(path, os.O_CREAT | os.O_RDWR, 0o600)
    try:
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return True
        fcntl.flock(descriptor, fcntl.LOCK_UN)
        return False
    finally:
        os.close(descriptor)


def _read_handshake(intent: Mapping[str, Any]) -> Mapping[str, Any] | None:
    path = Path(str(intent.get("handshake", "")))
    if not path.is_file() or path.is_symlink():
        return None
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        raise Blocked(f"malformed launch handshake: {path}")
    if not isinstance(value, Mapping) or not isinstance(value.get("pid"), int):
        raise Blocked(f"malformed launch readiness notification: {path}")
    return value


def _intent_matches_process(intent: Mapping[str, Any], pid: int) -> dict[str, Any] | None:
    start = process_start_time(pid)
    kernel = process_kernel_executable(pid)
    argv = process_kernel_argv(pid)
    expected_kernel = expected_kernel_executable(Path(str(intent["executable"])))
    if not start or not kernel or kernel != expected_kernel or argv != expected_kernel_argv(Path(str(intent["executable"])), list(intent["argv"])):
        return None
    record = {
        "pid": pid,
        "start_time": start,
        "token": intent["token"],
        "task": intent["task"],
        "executable": intent["executable"],
        "model": intent["model"],
        "argv": list(intent["argv"]),
        "kernel_argv": argv,
        "observed_command": process_command(pid),
        "kernel_executable": kernel,
        "process_group": os.getpgid(pid),
        "events": intent["events"],
        "final": intent["final"],
        "segment": intent["segment"],
        "session_id": intent.get("session_id"),
        "offset": 0,
        "launch_count": intent["segment"],
    }
    return record if worker_is_live(record) else None


def _find_intent_children(intent: Mapping[str, Any]) -> list[dict[str, Any]]:
    _read_handshake(intent)  # Notification only; kernel observations prove identity.
    listing = subprocess.run(["ps", "-ax", "-o", "pid="], capture_output=True, text=True, check=False)
    matches: list[dict[str, Any]] = []
    for raw in listing.stdout.splitlines():
        try:
            pid = int(raw.strip())
        except ValueError:
            continue
        record = _intent_matches_process(intent, pid)
        if record is not None:
            matches.append(record)
    return matches


def reconcile_pending_launch_intents(store: StateStore) -> dict[int, dict[str, Any]]:
    """Adopt the one exact child of every prepared intent, never relaunch it.

    This runs before a controller considers a task launchable.  A controller
    killed between ``Popen`` and the process record leaves an intent whose
    token/executable/argv are sufficient to authenticate exactly one detached
    child in a fresh process.
    """
    explicitly_blocked: set[int] = set()
    if store.snapshot_path.exists():
        snapshot = store.read_snapshot()
        explicitly_blocked = {
            int(number)
            for number, task in snapshot["tasks"].items()
            if task.get("blocker") not in {None, "dependency"}
        }
    adopted: dict[int, dict[str, Any]] = {}
    for process_root in sorted((store.root / "processes").glob("[0-9][0-9]")) if (store.root / "processes").is_dir() else []:
        for path in sorted(process_root.glob("launch-intent-*.json")):
            try:
                intent = json.loads(path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError) as exc:
                raise Blocked(f"malformed durable launch intent: {path}") from exc
            if intent.get("lifecycle") not in {"prepared", "spawned"}:
                continue
            required = {"task", "segment", "token", "executable", "argv", "worktree", "prd", "prd_sha256", "model", "events", "final", "prompt", "prompt_sha256"}
            if not required <= intent.keys() or not isinstance(intent["task"], int):
                raise Blocked(f"incomplete durable launch intent: {path}")
            if intent["task"] in explicitly_blocked:
                continue
            matches = _find_intent_children(intent)
            if len(matches) > 1:
                raise Blocked(
                    f"ambiguous live children for durable launch intent: {path} "
                    f"pids={[record['pid'] for record in matches]}"
                )
            if not matches:
                fence_value = intent.get("fence")
                if not isinstance(fence_value, str) or not fence_value:
                    raise Blocked(f"prepared launch intent has no single-flight fence: {path}")
                if _fence_is_held(Path(fence_value)):
                    raise Blocked(f"prepared launch intent fence remains held: {path}")
                # The controller died before fork.  Only an unlocked OS fence,
                # not a timing observation, authorizes a later retry.
                intent["lifecycle"] = "retryable"
                intent["retry_authorized"] = "exclusive-fence-observed-free"
                _write_launch_intent(path, intent)
                store.append_event({"event": "launch-intent-retryable", "task": intent["task"]})
                continue
            record = matches[0]
            record_path = process_root / f"process-{int(intent['segment'])}.json"
            atomic_json(record_path, record)
            intent["lifecycle"] = "recorded"
            intent["record"] = record
            _write_launch_intent(path, intent)
            adopted[int(intent["task"])] = record
            store.append_event({"event": "launch-intent-adopted", "task": intent["task"], "process": record})
    if adopted:
        def update(snapshot: dict[str, Any]) -> None:
            for number, record in adopted.items():
                current = _task(store, snapshot, number)
                current["process"] = record
                current["launch_count"] = max(current["launch_count"], int(record["segment"]))
                _transition(current, "running", detail="adopted durable launch intent")
                current["next_command"] = f"monitor --task {number} --follow"
        store.mutate(update)
    return adopted


def _authenticate_started_process(
    process: subprocess.Popen[Any],
    *,
    executable: Path,
    argv: list[str],
    token: str,
    task_number: int,
    model: str,
    events: Path,
    final: Path,
    segment: int,
    handshake: Mapping[str, Any],
) -> dict[str, Any]:
    pid = process.pid
    start = process_start_time(pid)
    kernel_argv = process_kernel_argv(pid)
    kernel = process_kernel_executable(pid)
    record = {
        "pid": pid,
        "start_time": start,
        "token": token,
        "task": task_number,
        "executable": str(executable),
        "model": model,
        "argv": list(argv),
        "kernel_argv": kernel_argv,
        "observed_command": process_command(pid),
        "kernel_executable": kernel,
        "process_group": os.getpgid(pid) if kernel else None,
        "events": str(events),
        "final": str(final),
        "segment": segment,
        "session_id": None,
        "offset": 0,
        "launch_count": segment,
    }
    authenticated = bool(
        executable.is_file()
        and start
        and kernel
        and kernel == expected_kernel_executable(executable)
        and kernel_argv == expected_kernel_argv(executable, argv)
        and any(token in argument for argument in kernel_argv)
        and record["process_group"] == pid
        and handshake.get("pid") == pid
        and process_identity_matches(record, pid, start, list(argv), task_number)
        and worker_is_live(record)
    )
    if not authenticated:
        _terminate_unrecorded_process(process)
        raise Refused(
            f"Task {task_number} replacement process identity could not be authenticated"
        )
    return record


def _write_launcher_wrapper(path: Path) -> None:
    """Write an exec trampoline whose ready JSON is only a wake-up hint.

    The inherited advisory-lock descriptor survives exec, fencing the earliest
    child state.  Recovery authenticates only the kernel observations.
    """
    path.write_text(
        "import json, os, sys, tempfile\n"
        "p, fd, raw = sys.argv[1:4]\n"
        "argv = json.loads(raw)\n"
        "os.set_inheritable(int(fd), True)\n"
        "d = os.path.dirname(p); f, t = tempfile.mkstemp(dir=d, prefix='.ready-')\n"
        "with os.fdopen(f, 'w', encoding='utf-8') as out:\n"
        " json.dump({'pid': os.getpid()}, out); out.flush(); os.fsync(out.fileno())\n"
        "os.replace(t, p)\n"
        "directory = os.open(d, os.O_RDONLY); os.fsync(directory); os.close(directory)\n"
        "os.execv(argv[0], argv)\n",
        encoding="utf-8",
    )
    path.chmod(0o700)


def launch_worker(
    store: StateStore,
    executable: Path,
    worktree: Path,
    token: str,
    prd: Path,
    *,
    task_number: int,
    model: str,
    segment: int,
    pre_spawn_hook: Callable[[], None] | None = None,
    post_spawn_hook: Callable[[subprocess.Popen[Any]], None] | None = None,
    post_handshake_hook: Callable[[subprocess.Popen[Any]], None] | None = None,
) -> dict[str, Any]:
    executable = executable.expanduser().resolve(strict=False)
    if not executable.is_file():
        raise Refused(f"Codex executable is not a file: {executable}")
    worktree = require_operational_path(worktree)
    prd = require_operational_path(prd)
    if not prd.is_file():
        raise Refused(f"canonical PRD missing: {prd}")
    process_root = store.root / "processes" / f"{task_number:02d}"
    process_root.mkdir(parents=True, exist_ok=True)
    events = process_root / f"events-{segment}.jsonl"
    final = process_root / f"final-{segment}.md"
    prompt = f"Process token {token}. Execute the canonical PRD at {prd}"
    argv = [
        str(executable),
        "--yolo",
        "exec",
        "--disable",
        "fast_mode",
        "--model",
        model,
        "--json",
        "-o",
        str(final),
        "-C",
        str(worktree),
        prompt,
    ]
    intent_path = launch_intent_path(store, task_number, segment)
    handshake_path = launch_handshake_path(intent_path)
    fence_path = launch_fence_path(intent_path)
    intent: dict[str, Any] = {
        "schema_version": 1,
        "lifecycle": "prepared",
        "task": task_number,
        "segment": segment,
        "token": token,
        "executable": str(executable),
        "argv": argv,
        "worktree": str(worktree),
        "prd": str(prd),
        "prd_sha256": sha256_file(prd),
        "prompt": str(prd),
        "prompt_sha256": sha256_file(prd),
        "model": model,
        "events": str(events),
        "final": str(final),
        "session_id": None,
        "mode": "initial",
        "handshake": str(handshake_path),
        "fence": str(fence_path),
    }
    _write_launch_intent(intent_path, intent)
    wrapper = process_root / f"worker-wrapper-{segment}.py"
    _write_launcher_wrapper(wrapper)
    process: subprocess.Popen[Any] | None = None
    record: dict[str, Any] | None = None
    fence_descriptor = os.open(fence_path, os.O_CREAT | os.O_RDWR, 0o600)
    try:
        fcntl.flock(fence_descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError as exc:
        os.close(fence_descriptor)
        raise Blocked(f"single-flight fence already held: {fence_path}") from exc
    if pre_spawn_hook is not None:
        pre_spawn_hook()
    with events.open("a", encoding="utf-8") as output:
        try:
            process = subprocess.Popen(
                [sys.executable, str(wrapper), str(handshake_path), str(fence_descriptor), json.dumps(argv)],
                cwd=worktree, start_new_session=True, pass_fds=(fence_descriptor,),
                stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.STDOUT,
            )
        finally:
            os.close(fence_descriptor)
    try:
        if post_spawn_hook is not None:
            post_spawn_hook(process)
        deadline = time.monotonic() + 5
        handshake: Mapping[str, Any] | None = None
        while time.monotonic() < deadline:
            handshake = _read_handshake(intent)
            if handshake is not None:
                break
            if process.poll() is not None:
                break
            time.sleep(0.01)
        if handshake is None:
            raise Refused(f"Task {task_number} child did not write durable exec handshake")
        kernel_deadline = time.monotonic() + 5
        while time.monotonic() < kernel_deadline:
            if (process_kernel_executable(process.pid) == expected_kernel_executable(executable)
                    and process_kernel_argv(process.pid) == expected_kernel_argv(executable, argv)):
                break
            if process.poll() is not None:
                break
            time.sleep(0.01)
        if (process_kernel_executable(process.pid) != expected_kernel_executable(executable)
                or process_kernel_argv(process.pid) != expected_kernel_argv(executable, argv)):
            raise Refused(f"Task {task_number} child failed exact kernel exec authentication")
        if post_handshake_hook is not None:
            post_handshake_hook(process)
        record = _authenticate_started_process(
            process,
            executable=executable,
            argv=argv,
            token=token,
            task_number=task_number,
            model=model,
            events=events,
            final=final,
            segment=segment,
            handshake=handshake,
        )
        atomic_json(process_root / f"process-{segment}.json", record)
        # The detached worker intentionally outlives this controller only
        # after its durable identity is recorded.  Do not forge Popen's
        # return code before that point: failure cleanup must still be able
        # to reap a just-terminated session leader and prove group ESRCH.
        process.returncode = 0
        intent["lifecycle"] = "recorded"
        intent["record"] = record
        _write_launch_intent(intent_path, intent)
    except Exception as exc:
        if record is not None:
            _terminate_authenticated_process(record)
        elif process is not None:
            _terminate_unrecorded_process(process)
        intent["lifecycle"] = "failed"
        intent["failure"] = str(exc)
        if record is not None:
            intent["record"] = record
        _write_launch_intent(intent_path, intent)
        raise
    store.append_event({"event": "launch", "task": task_number, "process": record})
    return record


def _rendered_prd_file_lease(prd: Path) -> list[str]:
    if not prd.is_file() or prd.is_symlink():
        raise Blocked(f"canonical PRD missing: {prd}")
    content = prd.read_text(encoding="utf-8")
    match = re.search(
        r"(?ms)^### Exact file lease\s*\n(.*?)(?=^### |\Z)", content
    )
    if not match:
        raise Blocked(f"canonical PRD has no exact file lease: {prd}")
    paths: list[str] = []
    saw_none = False
    for raw_line in match.group(1).splitlines():
        line = raw_line.strip()
        if not line:
            continue
        if line == "- none":
            if saw_none or paths:
                raise Blocked(f"canonical PRD has ambiguous file lease: {prd}")
            saw_none = True
            continue
        literal = re.fullmatch(r"- `([^`]+)`", line)
        if not literal:
            raise Blocked(f"canonical PRD has ambiguous file lease: {prd}")
        if saw_none:
            raise Blocked(f"canonical PRD has ambiguous file lease: {prd}")
        paths.append(literal.group(1))
    return sorted(set(paths))


def validate_launch_preflight(task: Mapping[str, Any], state: Mapping[str, Any]) -> None:
    """Refuse a launch unless the task's materialized identity is still exact."""
    worktree = require_operational_path(Path(task["paths"]["worktree"]))
    if not (worktree / ".git").exists():
        raise Blocked(f"Task {task['number']} worktree is missing: {worktree}")
    changed = []
    for line in _git(worktree, "status", "--porcelain=v1", "--untracked-files=all").splitlines():
        path = line[3:]
        if " -> " in path:
            path = path.split(" -> ", 1)[1]
        if path != "HANDOFF.md":
            changed.append(path)
    if changed:
        raise Blocked(
            f"Task {task['number']} worktree is not clean before launch: "
            f"{', '.join(changed)}"
        )
    expected_branch = _task_branch(task)
    branch = _git(worktree, "branch", "--show-current")
    if branch != expected_branch:
        raise Blocked(
            f"Task {task['number']} worktree branch mismatch: expected "
            f"{expected_branch}, got {branch or '<detached>'}"
        )
    head_sha = require_sha(_git(worktree, "rev-parse", "HEAD"), "worktree HEAD")
    expected = require_sha(str(task["expected_merge_parent"]), "expected_merge_parent")
    identities = {
        "worktree HEAD": head_sha,
        "base_sha": require_sha(str(task["base_sha"]), "base_sha"),
        "task target_sha": require_sha(
            str(task["target_sha"]), "task target_sha"
        ),
        "expected_merge_parent": expected,
        "controller target_sha": require_sha(str(state["target_sha"]), "controller target_sha"),
    }
    if len(set(identities.values())) != 1:
        raise Blocked(
            f"Task {task['number']} launch identities diverged: "
            + ", ".join(f"{name}={value}" for name, value in identities.items())
        )
    prd = require_operational_path(Path(task["paths"]["prd"]))
    rendered_lease = _rendered_prd_file_lease(prd)
    state_lease = sorted(set(str(path) for path in task["file_lease"]))
    if rendered_lease != state_lease:
        raise Blocked(
            f"Task {task['number']} PRD file lease does not reconcile with state"
        )


def launch_task(
    store: StateStore, task_number: int, executable: Path
) -> dict[str, Any]:
    reconcile_pending_launch_intents(store)
    state = store.read_snapshot()
    task = _task(store, state, task_number)
    if task["worker"]["kind"] != "CLI":
        raise Blocked(
            f"Task {task_number} is {task['worker']['kind']}-owned and cannot use CLI launch"
        )
    require_dependencies(task, state["tasks"])
    if task["process"] and worker_is_live(task["process"]):
        raise Blocked(f"task {task_number} already has a live worker")
    prd = Path(task["paths"]["prd"])
    if not prd_hash_matches(prd, task["prd_sha256"] or ""):
        raise Blocked(f"Task {task_number} PRD hash does not reconcile")
    validate_launch_preflight(task, state)
    segment = task["launch_count"] + 1
    record = launch_worker(
        store,
        executable,
        Path(task["paths"]["worktree"]),
        uuid.uuid4().hex,
        prd,
        task_number=task_number,
        model=task["worker"]["model"],
        segment=segment,
    )

    def update(snapshot: dict[str, Any]) -> dict[str, Any]:
        current = _task(store, snapshot, task_number)
        current["process"] = record
        current["launch_count"] = segment
        _transition(current, "running")
        current["next_command"] = f"monitor --task {task_number} --follow"
        return current

    return store.mutate(update)


def monitor_task(store: StateStore, task_number: int) -> dict[str, Any]:
    captured: list[dict[str, Any]] = []
    session_id: str | None = None

    def update(state: dict[str, Any]) -> None:
        nonlocal session_id
        task = _task(store, state, task_number)
        record = task.get("process")
        if not record:
            raise Refused(f"Task {task_number} has no process record")
        events = require_operational_path(Path(record["events"]))
        if not events.exists():
            events.parent.mkdir(parents=True, exist_ok=True)
            events.touch()
        with events.open("rb") as stream:
            stream.seek(int(record.get("offset", 0)))
            while True:
                line_start = stream.tell()
                raw_line = stream.readline()
                if not raw_line:
                    break
                if not raw_line.endswith(b"\n"):
                    stream.seek(line_start)
                    break
                try:
                    value = json.loads(raw_line)
                except json.JSONDecodeError:
                    continue
                captured.append(value)
                if value.get("type") == "thread.started":
                    session_id = value.get("thread_id")
            record["offset"] = stream.tell()
        if session_id:
            record["session_id"] = session_id
        else:
            session_id = record.get("session_id")
        task["heartbeat"] = {
            "at": utc_now(),
            "session_id": session_id,
            "live": worker_is_live(record),
        }

    store.mutate(update)
    store.append_event({"event": "monitor", "task": task_number, "session_id": session_id, "new_events": len(captured)})
    return {"session_id": session_id, "events": captured}


def monitor_events(store: StateStore, record: Mapping[str, Any]) -> str | None:
    """Compatibility helper used by older callers; new code uses monitor_task."""
    session = None
    events = Path(str(record["events"]))
    if events.exists():
        for line in events.read_text(encoding="utf-8").splitlines():
            try:
                value = json.loads(line)
            except json.JSONDecodeError:
                continue
            if value.get("type") == "thread.started":
                session = value.get("thread_id")
    store.append_event({"event": "heartbeat", "session_id": session})
    return session


def task_status(store: StateStore, task_number: int | None = None) -> dict[str, Any]:
    state = store.read_snapshot()
    return state if task_number is None else _task(store, state, task_number)


def heartbeat_task(store: StateStore, task_number: int) -> dict[str, Any]:
    heartbeat = {"at": utc_now(), "task": task_number}

    def update(state: dict[str, Any]) -> None:
        _task(store, state, task_number)["heartbeat"] = heartbeat

    store.mutate(update)
    store.append_event({"event": "heartbeat", "task": task_number})
    return heartbeat


def resume_argv(
    record: Mapping[str, Any], executable: Path, token: str, recovery_prompt: Path
) -> list[str]:
    session_id = record.get("session_id")
    if not session_id:
        raise Blocked("recorded session ID required for resume")
    model = record.get("model", "gpt-5.6-terra")
    final = Path(str(record["final"])).with_name(
        f"final-{int(record.get('segment', 1)) + 1}.md"
    )
    return [
        str(executable),
        "--yolo",
        "exec",
        "resume",
        "--disable",
        "fast_mode",
        "--model",
        str(model),
        "--json",
        "-o",
        str(final),
        str(session_id),
        f"Process token {token}. Continue from {recovery_prompt}",
    ]


def _write_recovery_prompt(store: StateStore, task: Mapping[str, Any]) -> Path:
    path = store.root / "processes" / f"{task['number']:02d}" / "recovery-prompt.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    worktree = Path(task["paths"]["worktree"])
    status = subprocess.run(
        ["git", "-C", str(worktree), "status", "--short"],
        capture_output=True,
        text=True,
        check=False,
    ).stdout
    content = (
        f"Canonical PRD: {task['paths']['prd']}\n"
        f"Canonical PRD SHA-256: {task['prd_sha256']}\n"
        "Required recovery action: reread the canonical PRD in full before acting.\n"
        f"Last checkpoint: {task['head_sha']}\n"
        f"State: {task['state']}\n"
        f"Next action: {task['next_command']}\n"
        "Current git status:\n```\n"
        + status
        + "```\n"
    )
    path.write_text(content, encoding="utf-8")
    return path


def resume_task(store: StateStore, task_number: int, executable: Path) -> dict[str, Any]:
    executable = executable.expanduser().resolve(strict=False)
    if not executable.is_file():
        raise Refused(f"Codex executable is not a file: {executable}")
    reconcile_pending_launch_intents(store)
    state = store.read_snapshot()
    task = _task(store, state, task_number)
    record = task.get("process")
    if not record:
        raise Blocked(f"Task {task_number} has no recorded process")
    if worker_is_live(record):
        raise Blocked(f"Task {task_number} worker is still live; watch it")
    if not record.get("session_id"):
        raise Blocked(f"Task {task_number} has no recorded session ID")
    observation = inspect_worktree(task)
    if task.get("head_sha") and observation["head_sha"] != task["head_sha"]:
        raise Blocked(
            f"Task {task_number} worktree HEAD changed; reconcile before resume"
        )
    if task.get("prd_sha256") and observation["prd_sha256"] != task["prd_sha256"]:
        raise Blocked(f"Task {task_number} PRD hash changed; refuse resume")
    prompt = _write_recovery_prompt(store, task)
    token = uuid.uuid4().hex
    argv = resume_argv(record, executable, token, prompt)
    segment = int(record.get("segment", 1)) + 1
    process_root = store.root / "processes" / f"{task_number:02d}"
    events = process_root / f"events-{segment}.jsonl"
    final = Path(argv[argv.index("-o") + 1])
    intent_path = launch_intent_path(store, task_number, segment)
    handshake_path = launch_handshake_path(intent_path)
    fence_path = launch_fence_path(intent_path)
    intent: dict[str, Any] = {
        "schema_version": 1,
        "lifecycle": "prepared",
        "task": task_number,
        "segment": segment,
        "token": token,
        "executable": str(executable),
        "argv": argv,
        "worktree": str(task["paths"]["worktree"]),
        "prd": str(task["paths"]["prd"]),
        "prd_sha256": task["prd_sha256"],
        "prompt": str(prompt),
        "prompt_sha256": sha256_file(prompt),
        "model": task["worker"]["model"],
        "events": str(events),
        "final": str(final),
        "session_id": record["session_id"],
        "mode": "resume",
        "handshake": str(handshake_path),
        "fence": str(fence_path),
    }
    _write_launch_intent(intent_path, intent)
    wrapper = process_root / f"worker-wrapper-{segment}.py"
    _write_launcher_wrapper(wrapper)
    process: subprocess.Popen[Any] | None = None
    resumed: dict[str, Any] | None = None
    fence_descriptor = os.open(fence_path, os.O_CREAT | os.O_RDWR, 0o600)
    try:
        fcntl.flock(fence_descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError as exc:
        os.close(fence_descriptor)
        raise Blocked(f"single-flight fence already held: {fence_path}") from exc
    try:
        with events.open("a", encoding="utf-8") as output:
            try:
                process = subprocess.Popen(
                    [sys.executable, str(wrapper), str(handshake_path), str(fence_descriptor), json.dumps(argv)],
                    cwd=Path(task["paths"]["worktree"]), start_new_session=True,
                    pass_fds=(fence_descriptor,), stdin=subprocess.DEVNULL,
                    stdout=output, stderr=subprocess.STDOUT,
                )
            finally:
                os.close(fence_descriptor)
        deadline = time.monotonic() + 5
        handshake: Mapping[str, Any] | None = None
        while time.monotonic() < deadline:
            handshake = _read_handshake(intent)
            if handshake is not None:
                break
            if process.poll() is not None:
                break
            time.sleep(0.01)
        if handshake is None:
            raise Refused(f"Task {task_number} resumed child did not write durable exec handshake")
        kernel_deadline = time.monotonic() + 5
        while time.monotonic() < kernel_deadline:
            if (process_kernel_executable(process.pid) == expected_kernel_executable(executable)
                    and process_kernel_argv(process.pid) == expected_kernel_argv(executable, argv)):
                break
            if process.poll() is not None:
                break
            time.sleep(0.01)
        if (process_kernel_executable(process.pid) != expected_kernel_executable(executable)
                or process_kernel_argv(process.pid) != expected_kernel_argv(executable, argv)):
            raise Refused(f"Task {task_number} resumed child failed exact kernel process identity authentication")
        resumed = _authenticate_started_process(
            process,
            executable=executable,
            argv=argv,
            token=token,
            task_number=task_number,
            model=task["worker"]["model"],
            events=events,
            final=final,
            segment=segment,
            handshake=handshake,
        )
        resumed["session_id"] = record["session_id"]
        atomic_json(process_root / f"process-{segment}.json", resumed)
        intent["lifecycle"] = "recorded"
        intent["record"] = resumed
        _write_launch_intent(intent_path, intent)
    except Exception as exc:
        if resumed is not None:
            _terminate_authenticated_process(resumed)
        elif process is not None:
            _terminate_unrecorded_process(process)
        intent["lifecycle"] = "failed"
        intent["failure"] = str(exc)
        if resumed is not None:
            intent["record"] = resumed
        _write_launch_intent(intent_path, intent)
        raise

    def update(snapshot: dict[str, Any]) -> dict[str, Any]:
        current = _task(store, snapshot, task_number)
        current["process"] = resumed
        current["launch_count"] = segment
        _transition(current, "running", detail="resumed recorded session")
        current["next_command"] = f"monitor --task {task_number} --follow"
        return current

    result = store.mutate(update)
    store.append_event({"event": "resume", "task": task_number, "process": resumed})
    return result


def stop_task(store: StateStore, task_number: int, signal_name: str) -> dict[str, Any]:
    state = store.read_snapshot()
    task = _task(store, state, task_number)
    record = task.get("process")
    if not record or not worker_is_live(record):
        store.append_event({"event": "stop", "task": task_number, "stopped": False, "reason": "not-live"})
        return {"stopped": False, "reason": "not-live"}
    current_start = process_start_time(record["pid"])
    command = process_command(record["pid"])
    if current_start != record["start_time"] or record["token"] not in command:
        raise Blocked(f"Task {task_number} process identity mismatch; refusing signal")
    try:
        signum = getattr(signal, f"SIG{signal_name.upper()}")
    except AttributeError as exc:
        raise Refused(f"unknown signal {signal_name}") from exc
    _terminate_authenticated_process(record, signum)
    store.append_event({"event": "stop", "task": task_number, "stopped": True, "signal": signal_name.upper()})
    return {"stopped": True, "signal": signal_name.upper()}


def recovery_actions(state: Mapping[str, Any]) -> dict[str, str]:
    actions: dict[str, str] = {}
    for key, task in sorted(state.get("tasks", {}).items(), key=lambda item: int(item[0])):
        task_state = task.get("state")
        if task_state in {"completed", "merged"}:
            continue
        process = task.get("process") or {}
        if task_state == "running" and task.get("worker", {}).get("kind") == "CLI" and process:
            if worker_is_live(process):
                action = f"monitor --task {key} --follow"
            elif process.get("session_id"):
                action = f"resume --task {key}"
            else:
                action = f"reconcile --task {key}"
        elif task_state == "launchable":
            action = str(task.get("next_command") or f"materialize --task {key}")
        else:
            action = f"reconcile --task {key}"
        actions[str(key)] = action
    return actions


def _release_ready_dependencies(store: StateStore) -> None:
    def update(state: dict[str, Any]) -> None:
        for task in state["tasks"].values():
            if task["state"] not in {"blocked", "queued"}:
                continue
            if task.get("blocker") not in {None, "dependency"}:
                continue
            try:
                require_dependencies(task, state["tasks"])
            except Blocked:
                continue
            _transition(task, "launchable", detail="remote dependencies merged")
            task["blocker"] = None
            task["next_command"] = f"materialize --task {task['number']}"

    store.mutate(update)


def recover_controller(
    store: StateStore,
    repo: Path,
) -> dict[str, Any]:
    """Recover using only authenticated read-only Git and GitHub observations."""
    return _recover_controller(store, repo)


def recover_controller_for_test(
    store: StateStore,
    repo: Path,
    remote_inventory: Mapping[str, Any],
) -> dict[str, Any]:
    """Test-only recovery seam; production CLI never exposes supplied observations."""
    return _recover_controller(store, repo, remote_inventory)


def _recover_controller(
    store: StateStore,
    repo: Path,
    remote_inventory: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    repository = require_operational_path(repo)
    reconcile_pending_launch_intents(store)
    initial = store.read_snapshot()
    remote_error: str | None = None
    if remote_inventory is not None:
        inventory = dict(remote_inventory)
    else:
        try:
            inventory = read_remote_inventory(repository, initial)
        except (Blocked, Refused) as exc:
            # Remote unavailability must not prevent recovery from adopting an
            # already-live durable worker.  Surface the failed observation and
            # leave remote-derived task state unchanged.
            remote_error = str(exc)
            inventory = {"target_sha": initial["target_sha"], "tasks": {}}
    remote_target = require_sha(
        str(inventory.get("target_sha", initial["target_sha"])),
        "remote target SHA",
    )
    for key, remote in inventory.get("tasks", {}).items():
        reconcile_task(store, int(key), remote)

    def update_target(state: dict[str, Any]) -> None:
        state["target_sha"] = remote_target

    store.mutate(update_target)
    _release_ready_dependencies(store)
    observations: dict[str, Any] = {}

    def observe(state: dict[str, Any]) -> None:
        for key, task in state["tasks"].items():
            process_root = store.root / "processes" / f"{int(key):02d}"
            if task["state"] not in {"completed", "merged"} and process_root.is_dir():
                try:
                    durable_process = select_latest_process_record(process_root)
                except Refused:
                    durable_process = None
                if durable_process is not None:
                    if durable_process.get("task") != int(key):
                        raise Blocked(
                            f"Task {key} durable process record names task "
                            f"{durable_process.get('task')}"
                        )
                    if task.get("blocker") not in {None, "dependency"}:
                        durable_process = None
                if durable_process is not None:
                    existing = task.get("process")
                    candidate_segment = int(durable_process.get("segment", 0))
                    existing_segment = int(existing.get("segment", 0)) if existing else 0
                    if (
                        existing is None
                        or (
                            candidate_segment > existing_segment
                            and worker_is_live(durable_process)
                        )
                    ):
                        task["process"] = durable_process
                        task["launch_count"] = max(
                            task["launch_count"],
                            int(durable_process.get("launch_count", candidate_segment)),
                        )
                        _transition(task, "running", detail="adopted durable process record")
                        task["next_command"] = (
                            f"monitor --task {key} --follow"
                            if worker_is_live(durable_process)
                            else (
                                f"resume --task {key}"
                                if durable_process.get("session_id")
                                else f"reconcile --task {key}"
                            )
                        )
            worktree = Path(task["paths"]["worktree"])
            if task["state"] in {"completed", "merged"}:
                # Terminal worktrees may intentionally retain post-merge edits.
                # Their historical lease no longer authorizes a recovery check,
                # and terminal state is already authenticated by its receipts.
                continue
            if not worktree.exists():
                continue
            observation = inspect_worktree(task)
            observations[key] = observation
            task["local_observation"] = observation
            task["report_sha256"] = observation["report_sha256"]
            task["review_sha256"] = observation["review_sha256"]
            if task["prd_sha256"] is not None and observation["prd_sha256"] != task["prd_sha256"]:
                raise Blocked(f"Task {key} canonical PRD hash does not reconcile")

    store.mutate(observe)
    state = store.read_snapshot()
    actions = recovery_actions(state)
    result = {
        "actions": actions,
        "idempotent": True,
        "target_sha": state["target_sha"],
        "remote": inventory,
        "remote_error": remote_error,
        "observations": observations,
    }
    store.append_event({"event": "recover", **result})
    return result


def round_receipt_path(root: Path, task_number: int, round_number: int) -> Path:
    root = require_operational_path(root)
    if task_number != 20 or round_number < 1:
        raise Refused("invalid Task 20 receipt")
    return root / "receipts" / "task-20" / f"round-{round_number:03d}.json"


def write_round_receipt(
    root: Path, task_number: int, round_number: int, payload: Mapping[str, Any]
) -> Path:
    path = round_receipt_path(root, task_number, round_number)
    if path.exists():
        raise Refused(f"round receipt already exists: {path}")
    atomic_json(path, payload)
    return path


def select_latest_process_record(process_root: Path) -> dict[str, Any]:
    root = require_operational_path(process_root)
    records = [
        *root.glob("bootstrap-process-*.json"),
        *root.glob("process-*.json"),
    ]
    if not records:
        raise Refused(f"no process records under {root}")
    def segment(path: Path) -> int:
        match = re.search(r"(\d+)\.json$", path.name)
        return int(match.group(1)) if match else -1
    selected = max(records, key=lambda path: (segment(path), path.name))
    return json.loads(selected.read_text(encoding="utf-8"))


def recovery_decision(
    record: Mapping[str, Any], pid: int, start_time: str, argv: list[str]
) -> str:
    if record.get("pid") == pid and process_identity_matches(
        record, pid, start_time, argv, record.get("task")
    ):
        return "watch"
    if record.get("pid") == pid:
        raise Blocked("live PID has different process identity")
    return "resume"


def _fixture_state(root: Path) -> dict[str, Any]:
    def make_task(number: int, state_name: str, kind: str = "CLI") -> dict[str, Any]:
        slug = f"fixture-{number}"
        return {
            "number": number,
            "slug": slug,
            "state": state_name,
            "state_history": [{"state": state_name, "at": utc_now()}],
            "dependencies": [number - 1] if number else [],
            "file_lease": [f"fixture-{number}.txt"],
            "worker": {"kind": kind, "model": "gpt-5.6-terra", "effort": "fast", "identity": None},
            "process": None,
            "launch_count": 0,
            "prd_sha256": None,
            "report_sha256": None,
            "review_sha256": None,
            "base_sha": "a" * 40,
            "head_sha": "b" * 40 if state_name in {"committed", "pushed"} else None,
            "pushed_sha": "b" * 40 if state_name == "pushed" else None,
            "merge_sha": "c" * 40 if state_name in {"completed", "merged"} else None,
            "target_sha": "a" * 40,
            "paths": _task_paths(root, number, slug),
            "heartbeat": None,
            "pr_ci_state": {"pr": 100 + number, "ci": "pending"} if state_name == "pushed" else None,
            "receipt_hashes": {},
            "ruling": None,
            "blocker": "dependency" if state_name == "blocked" else None,
            "expected_merge_parent": "a" * 40,
            "next_command": f"reconcile --task {number}",
        }

    tasks = {
        "0": make_task(0, "running"),
        "1": make_task(1, "committed"),
        "2": make_task(2, "pushed"),
        "3": make_task(3, "running"),
        "4": make_task(4, "running", "Desktop"),
        "5": make_task(5, "blocked"),
    }
    tasks["0"]["process"] = {
        "pid": 99_999_999,
        "start_time": "dead",
        "token": "fixture-old-token",
        "task": 0,
        "executable": "/usr/bin/false",
        "model": "gpt-5.6-terra",
        "argv": ["/usr/bin/false", "fixture-old-token"],
        "observed_command": "/usr/bin/false fixture-old-token",
        "events": str(root / "processes/00/events-1.jsonl"),
        "final": str(root / "processes/00/final-1.md"),
        "segment": 1,
        "session_id": "fixture-old-session",
        "offset": 0,
        "launch_count": 1,
    }
    tasks["0"]["launch_count"] = 1
    tasks["3"]["process"] = {
        "pid": 99_999_999,
        "start_time": "dead",
        "token": "fixture-dead-token",
        "task": 3,
        "executable": "/usr/bin/false",
        "model": "gpt-5.6-terra",
        "argv": ["/usr/bin/false", "fixture-dead-token"],
        "observed_command": "/usr/bin/false fixture-dead-token",
        "events": str(root / "processes/03/events-1.jsonl"),
        "final": str(root / "processes/03/final-1.md"),
        "segment": 1,
        "session_id": "fixture-session",
        "offset": 0,
        "launch_count": 1,
    }
    return {
        "schema_version": 1,
        "plan_sha256": "1" * 64,
        "spec_sha256": "2" * 64,
        "target_ref": "origin/fixture",
        "target_sha": "a" * 40,
        "merge_lease": None,
        "expected_merge_parent": "a" * 40,
        "controller_identity": "fixture-controller",
        "tasks": tasks,
    }


def _launch_fixture_supervisor(process_root: Path, segment: int) -> tuple[dict[str, Any], int]:
    record_path = process_root / f"process-{segment}.json"
    token = f"fixture-token-{segment}-{uuid.uuid4().hex}"
    code = (
        "import json,os,subprocess,sys,time;"
        "p=subprocess.Popen([sys.executable,'-c','import time; time.sleep(30)',sys.argv[2]],start_new_session=True);"
        "time.sleep(.05);"
        "s=subprocess.run(['ps','-p',str(p.pid),'-o','lstart='],capture_output=True,text=True).stdout.strip();"
        "a=[sys.executable,'-c','import time; time.sleep(30)',sys.argv[2]];"
        "c=subprocess.run(['ps','-ww','-p',str(p.pid),'-o','command='],capture_output=True,text=True).stdout.strip();"
        "json.dump({'segment':int(sys.argv[3]),'pid':p.pid,'start_time':s,'token':sys.argv[2],"
        "'task':0,'executable':sys.executable,'model':'gpt-5.6-terra','argv':a,'observed_command':c,"
        "'events':sys.argv[1].replace('process-','events-').replace('.json','.jsonl'),"
        "'final':sys.argv[1].replace('process-','final-'),'session_id':'fixture-session',"
        "'offset':0,'launch_count':int(sys.argv[3])},open(sys.argv[1],'w'))"
    )
    subprocess.run(
        [sys.executable, "-c", code, str(record_path), token, str(segment)],
        check=True,
    )
    record = json.loads(record_path.read_text(encoding="utf-8"))
    record["start_time"] = process_start_time(record["pid"])
    record["kernel_executable"] = process_kernel_executable(record["pid"])
    record["kernel_argv"] = process_kernel_argv(record["pid"])
    record["observed_command"] = process_command(record["pid"])
    record["process_group"] = os.getpgid(record["pid"])
    atomic_json(record_path, record)
    return record, record["pid"]


def run_fixture_rehearsal(root: Path) -> dict[str, Any]:
    root = require_operational_path(root)
    fixture_root = root / "fixture-rehearsal"
    if fixture_root.exists():
        shutil.rmtree(fixture_root)
    fixture_root.mkdir(parents=True)
    store = StateStore(fixture_root)
    state = _fixture_state(fixture_root)
    store.write_snapshot(state)
    store.append_event({"event": "fixture-controller-start"})
    before_states = {key: value["state"] for key, value in state["tasks"].items()}

    process_root = fixture_root / "processes/00"
    process_root.mkdir(parents=True)
    workers: list[int] = []
    try:
        first, first_pid = _launch_fixture_supervisor(process_root, 2)
        second, second_pid = _launch_fixture_supervisor(process_root, 3)
        workers.extend((first_pid, second_pid))
        selected = select_latest_process_record(process_root)
        newest_selected = selected["segment"] == 3
        newest_live = worker_is_live(selected)

        result = subprocess.run(
            [
                sys.executable,
                str(Path(__file__).resolve()),
                "recover",
                "--root",
                str(fixture_root),
                "--repo",
                str(GIT_ROOT),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode:
            raise Refused(
                "fresh-process fixture recovery failed: "
                + (result.stderr.strip() or result.stdout.strip())
            )
        recovered = json.loads(result.stdout)
        after = store.read_snapshot()
        after_states = {key: value["state"] for key, value in after["tasks"].items()}
        actions = recovered["actions"]
        receipt = {
            "before_states": before_states,
            "after_states": after_states,
            "actions": actions,
            "no_duplicate_launch": not (process_root / "process-4.json").exists(),
            "no_duplicate_launch_after_adoption": after["tasks"]["0"]["launch_count"] == 3,
            "fresh_process_crash_window_adoption": (
                after["tasks"]["0"]["process"]["token"] == second["token"]
                and after["tasks"]["0"]["process"]["segment"] == 3
                and actions["0"] == "monitor --task 0 --follow"
            ),
            "newest_supervisor_selected": newest_selected,
            "newest_worker_live_after_supervisor_exit": newest_live,
            "first_worker_pid": first["pid"],
            "second_worker_pid": second["pid"],
        }
        receipt_path = fixture_root / "recovery-receipt.json"
        atomic_json(receipt_path, receipt)
    finally:
        for pid in workers:
            try:
                os.kill(pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
    return {
        "events": str(store.events_path),
        "receipt": str(receipt_path),
        **receipt,
    }


def _git_target_sha(target_ref: str) -> str:
    result = subprocess.run(
        ["git", "rev-parse", target_ref], capture_output=True, text=True, check=False
    )
    if result.returncode:
        raise Refused(result.stderr.strip() or f"cannot resolve {target_ref}")
    return require_sha(result.stdout.strip(), "target_sha")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    names = (
        "init",
        "materialize",
        "event",
        "checkpoint",
        "repair-merge-receipts",
        "refresh-plan-spec",
        "reconcile",
        "recover",
        "launch",
        "monitor",
        "status",
        "heartbeat",
        "resume",
        "stop",
        "fixture",
    )
    for name in names:
        command = commands.add_parser(name)
        command.add_argument("--root", type=Path, required=True)
        if name not in {
            "init",
            "repair-merge-receipts",
            "refresh-plan-spec",
            "recover",
            "status",
            "fixture",
        }:
            command.add_argument("--task", type=int, required=True)
        elif name == "status":
            command.add_argument("--task", type=int)
        if name == "init":
            command.add_argument("--plan", type=Path, required=True)
            command.add_argument("--spec", type=Path, required=True)
            command.add_argument("--target-ref", required=True)
        if name == "materialize":
            command.add_argument("--plan", type=Path, required=True)
            command.add_argument("--base-sha")
        if name == "event":
            command.add_argument("--identity", required=True)
            command.add_argument("--json", required=True)
        if name == "checkpoint":
            command.add_argument("--head", required=True)
            command.add_argument("--pushed")
        if name == "repair-merge-receipts":
            command.add_argument("--manifest", type=Path, required=True)
            command.add_argument("--manifest-sha256", required=True)
        if name == "refresh-plan-spec":
            command.add_argument("--plan", type=Path, required=True)
            command.add_argument("--spec", type=Path, required=True)
            command.add_argument("--old-plan-sha256", required=True)
            command.add_argument("--old-spec-sha256", required=True)
            command.add_argument("--before-state-sha256", required=True)
        if name in {
            "checkpoint",
            "repair-merge-receipts",
            "reconcile",
            "recover",
            "launch",
            "resume",
        }:
            command.add_argument("--repo", type=Path, required=True)
        if name in {"launch", "resume"}:
            command.add_argument("--executable", type=Path, default=Path(shutil.which("codex") or "codex"))
        if name == "monitor":
            command.add_argument("--follow", action="store_true")
        if name == "stop":
            command.add_argument("--signal", default="TERM")
    return parser


def _dispatch(store: StateStore, args: argparse.Namespace) -> Any:
    if args.command == "init":
        target_sha = _git_target_sha(args.target_ref)
        return initialize_controller(
            store, args.plan, args.spec, args.target_ref, target_sha
        )
    if args.command == "materialize":
        state = store.read_snapshot()
        base_sha = args.base_sha or _task(store, state, args.task)["base_sha"]
        return {"prd": str(materialize_prd(store, args.plan, args.task, base_sha))}
    if args.command == "event":
        return record_external_event(store, args.task, args.identity, json.loads(args.json))
    if args.command == "checkpoint":
        return record_checkpoint(
            store,
            args.task,
            head_sha=args.head,
            pushed_sha=args.pushed,
            repo=args.repo,
        )
    if args.command == "repair-merge-receipts":
        return repair_merge_receipts(
            store,
            args.repo,
            args.manifest,
            args.manifest_sha256,
        )
    if args.command == "refresh-plan-spec":
        return refresh_plan_spec(
            store,
            args.plan,
            args.spec,
            old_plan_sha256=args.old_plan_sha256,
            old_spec_sha256=args.old_spec_sha256,
            before_state_sha256=args.before_state_sha256,
        )
    if args.command == "reconcile":
        repository = require_operational_path(args.repo)
        inventory = read_remote_inventory(
            repository, store.read_snapshot(), task_number=args.task
        )
        remote = inventory["tasks"].get(str(args.task), {})
        return reconcile_task(store, args.task, remote)
    if args.command == "recover":
        return recover_controller(store, args.repo)
    if args.command == "launch":
        require_operational_path(args.repo)
        return launch_task(store, args.task, args.executable)
    if args.command == "monitor":
        return monitor_task(store, args.task)
    if args.command == "status":
        return task_status(store, args.task)
    if args.command == "heartbeat":
        return heartbeat_task(store, args.task)
    if args.command == "resume":
        require_operational_path(args.repo)
        return resume_task(store, args.task, args.executable)
    if args.command == "stop":
        return stop_task(store, args.task, args.signal)
    if args.command == "fixture":
        return run_fixture_rehearsal(store.root)
    raise Refused(f"unhandled command {args.command}")


def _dispatch_monitor_follow(store: StateStore, args: argparse.Namespace) -> Any:
    while True:
        with store._locked():
            store._require_no_pending_repair()
            result = monitor_task(store, args.task)
            task = task_status(store, args.task)
            process = task.get("process")
            live = bool(process) and worker_is_live(process)
        if not live:
            return result
        time.sleep(0.5)


def dispatch(args: argparse.Namespace) -> Any:
    store = StateStore(args.root)
    if args.command in {"repair-merge-receipts", "refresh-plan-spec"}:
        return _dispatch(store, args)
    if args.command == "monitor" and args.follow:
        return _dispatch_monitor_follow(store, args)
    with store._locked():
        if args.command != "status":
            store._require_no_pending_repair()
            store._require_no_pending_refresh()
        return _dispatch(store, args)


def main() -> int:
    try:
        result = dispatch(build_parser().parse_args())
        print(json.dumps(result, indent=2, sort_keys=True))
        return 0
    except (Refused, Blocked, json.JSONDecodeError) as exc:
        print(f"REFUSED: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
