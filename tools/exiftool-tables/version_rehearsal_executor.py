#!/usr/bin/env python3
"""Execute an already-selected two-version rehearsal without promotion.

The planner selects releases; this module never does.  It binds one immutable
plan/materialization to a durable execution journal, creates one owned OxiDex
checkout and target directory for every selected native release, probes that
release with its own explicit native interpreter/library, and accepts a stage
only when its command writes a structured result with a non-zero denominator.
It deliberately has no promotion action.
"""
from __future__ import annotations

import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
from typing import Any, Callable, Mapping

import version_rehearsal as rehearsal
import version_rehearsal_catalog as catalog_stage
import version_rehearsal_native_oracle as native_oracle
import artifacts

SCHEMA = 1
KIND = "oxidex_exiftool_version_rehearsal_execution"
RESULT_KIND = "oxidex_version_rehearsal_stage_result"
STAGES = ("native", "generate", "build", "read", "write")
REQUIRED_COMMANDS = ("generate", "build", "read")
_SAFE_RELEASE = __import__("re").compile(r"^[0-9]+\.[0-9]+$")
COMMAND_TIMEOUT_SECONDS = 3600
_PLACEHOLDERS = {
    "release", "checkout", "target", "report", "native_source", "native_lib",
    "native_program", "native_perl", "native_probe", "native_probe_sha256", "source_commit",
    "write_fixture_manifest",
}
_WRITE_FIXTURE_KIND = "oxidex_version_rehearsal_write_fixture_manifest"


class Refused(ValueError):
    """The requested execution cannot be attributed safely."""


def _sha_json(value: Any) -> str:
    return rehearsal.sha256_json(value)


def _atomic(path: Path, value: dict[str, Any]) -> None:
    rehearsal.atomic_json(path, value)


def _read(path: Path) -> dict[str, Any]:
    return rehearsal.read_json(path)


def _selected(plan: Mapping[str, Any]) -> list[str]:
    releases = sorted({side["release"] for pair in plan["pairs"] for side in (pair["old"], pair["new"])},
                      key=rehearsal.release_key)
    if len(releases) < 2 or any(_SAFE_RELEASE.fullmatch(row) is None for row in releases):
        raise Refused("plan must select at least two numeric releases")
    return releases


def _safe_name(release: str) -> str:
    if _SAFE_RELEASE.fullmatch(release) is None:
        raise Refused("unsafe release name")
    return "release-" + release.replace(".", "-")


def _sha_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _write_fixture_binding(value: Any) -> dict[str, Any]:
    """Capture a write fixture manifest and every requested JPEG identity.

    The path alone cannot be immutable execution input: both the manifest and
    its listed source fixtures must still equal this snapshot at execution.
    """
    if not isinstance(value, str) or not os.path.isabs(value) or "\x00" in value:
        raise Refused("write fixture manifest path is malformed")
    manifest = _regular(Path(value), "write fixture manifest")
    try:
        document = json.loads(manifest.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise Refused("write fixture manifest is unreadable") from exc
    if (not isinstance(document, dict) or document.get("schema") != 1
            or document.get("kind") != _WRITE_FIXTURE_KIND
            or not isinstance(document.get("fixtures"), list) or not document["fixtures"]):
        raise Refused("write fixture manifest schema is unsupported")
    fixtures = []
    for item in document["fixtures"]:
        if (not isinstance(item, dict) or set(item) != {"path", "sha256", "bytes"}
                or not isinstance(item["path"], str) or not os.path.isabs(item["path"])
                or not isinstance(item["sha256"], str) or __import__("re").fullmatch(r"[0-9a-f]{64}", item["sha256"]) is None
                or type(item["bytes"]) is not int or item["bytes"] < 1):
            raise Refused("write fixture manifest item is malformed")
        fixture = _regular(Path(item["path"]), "write fixture")
        if _sha_file(fixture) != item["sha256"] or fixture.stat().st_size != item["bytes"]:
            raise Refused("write fixture differs from immutable manifest")
        if fixture.read_bytes()[:2] != b"\xff\xd8":
            raise Refused("write fixture manifest contains a non-JPEG fixture")
        fixtures.append({"path": str(fixture), "sha256": item["sha256"], "bytes": item["bytes"]})
    if len({(item["path"], item["sha256"]) for item in fixtures}) != len(fixtures):
        raise Refused("write fixture manifest has duplicate fixture identity")
    return {"path": str(manifest), "sha256": _sha_file(manifest), "bytes": manifest.stat().st_size,
            "fixtures": fixtures}


def _config(value: Any, releases: list[str]) -> dict[str, Any]:
    if not isinstance(value, dict) or value.get("schema") != SCHEMA:
        raise Refused("execution config schema is unsupported")
    commands = value.get("commands")
    if not isinstance(commands, dict) or any(stage not in commands for stage in REQUIRED_COMMANDS):
        raise Refused("generate, build, and read commands are required")
    for stage, spec in commands.items():
        if stage not in {"generate", "build", "read", "write"}:
            raise Refused("execution config has an unknown command stage")
        if not isinstance(spec, dict) or not isinstance(spec.get("argv"), list) or not spec["argv"]:
            raise Refused(f"{stage} command requires a nonempty argv array")
        for item in spec["argv"]:
            if not isinstance(item, str) or not item or "\x00" in item or "\n" in item:
                raise Refused(f"{stage} command argv is unsafe")
            try:
                fields = [part[1] for part in __import__("string").Formatter().parse(item) if part[1] is not None]
            except ValueError as exc:
                raise Refused(f"{stage} command has malformed placeholder") from exc
            if any(field not in _PLACEHOLDERS for field in fields):
                raise Refused(f"{stage} command has an unsupported placeholder")
    perls = value.get("perls")
    cases = value.get("native_cases")
    if not isinstance(perls, dict) or not isinstance(cases, dict) or set(perls) != set(releases) or set(cases) != set(releases):
        raise Refused("config must bind an explicit Perl and native cases to every selected release")
    if any(not isinstance(perls[row], str) or not perls[row] for row in releases):
        raise Refused("native Perl binding is malformed")
    if any(not isinstance(cases[row], list) or not cases[row] for row in releases):
        raise Refused("native cases must be a nonempty list for every selected release")
    lock = value.get("host_lock")
    if not isinstance(lock, str) or not os.path.isabs(lock) or "\x00" in lock:
        raise Refused("config must bind one absolute shared host lock path")
    source_commit = value.get("execution_source_commit")
    if not isinstance(source_commit, str) or rehearsal.GIT_OID_RE.fullmatch(source_commit) is None:
        raise Refused("config must bind an immutable execution source commit")
    has_write = "write" in commands
    manifests, bindings = value.get("write_fixture_manifests"), value.get("write_fixture_bindings")
    if not has_write:
        if manifests is not None or bindings is not None:
            raise Refused("write fixture bindings require a configured write command")
        return value
    if not isinstance(manifests, dict) or set(manifests) != set(releases):
        raise Refused("write command requires one immutable fixture manifest per selected release")
    current = {release: _write_fixture_binding(manifests[release]) for release in releases}
    if bindings is not None and bindings != current:
        raise Refused("write fixture manifest or requested JPEG scope changed after initialization")
    normalized = dict(value)
    normalized["write_fixture_bindings"] = current
    return normalized


def _input_documents(run_dir: Path) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any]]:
    return tuple(_read(run_dir / "inputs" / name) for name in (
        "capture.json", "catalog.json", "plan.json", "resolution.json", "materialization.json"
    ))  # type: ignore[return-value]


def _verify_inputs(run_dir: Path, archive_cache: Path, source_root: Path) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any]]:
    capture, catalog, plan, resolution, materialization = _input_documents(run_dir)
    catalog_stage.verify_capture_binding(capture, catalog)
    rehearsal.verify_plan(plan, catalog)
    catalog_stage.verify_source_resolution(resolution, plan, catalog, capture)
    catalog_stage.verify_source_materialization(materialization, plan, catalog, capture, resolution,
                                                archive_cache, source_root)
    return capture, catalog, plan, resolution, materialization


def _journal_payload(plan: dict[str, Any], materialization: dict[str, Any], config: dict[str, Any]) -> dict[str, Any]:
    releases = _selected(plan)
    return {
        "schema": SCHEMA,
        "kind": KIND,
        "plan_sha256": plan["plan_sha256"],
        "materialization_sha256": materialization["materialization_sha256"],
        "config_sha256": _sha_json(config),
        "execution_source_commit": config["execution_source_commit"],
        "host_lock": config["host_lock"],
        "promotion": "forbidden",
        "phase": "planned",
        "active": None,
        "events": [{"event": "execution_initialized", "at_unix": time.time()}],
        "releases": {
            release: {
                "state": "pending",
                "stages": {stage: "pending" for stage in STAGES},
                "reports": {},
                "failure": None,
            }
            for release in releases
        },
        "pairs": [
            {"pair_index": pair["pair_index"], "old_release": pair["old"]["release"],
             "new_release": pair["new"]["release"], "state": "unexercised",
             "reason": "per-version native comparisons do not establish a native-to-native delta"}
            for pair in plan["pairs"]
        ],
        "scope": {
            "selected_releases": releases,
            "untested_eligible_releases": plan["untested_eligible_releases"],
            "write_acceptance": "pending_or_unsupported",
            "parity": "unproven_until_each_release_has_passed_read_and_write_reports",
        },
    }


def initialize_run(run_dir: Path, capture: dict[str, Any], catalog: dict[str, Any], plan: dict[str, Any],
                   resolution: dict[str, Any], materialization: dict[str, Any], config: dict[str, Any]) -> dict[str, Any]:
    """Copy immutable inputs once and create an all-pending non-promoting journal."""
    catalog_stage.verify_capture_binding(capture, catalog)
    rehearsal.verify_plan(plan, catalog)
    catalog_stage.verify_source_resolution(resolution, plan, catalog, capture)
    releases = _selected(plan)
    config = _config(config, releases)
    if run_dir.exists() or run_dir.is_symlink():
        raise Refused("execution run directory already exists")
    run_dir.mkdir(parents=True)
    inputs = run_dir / "inputs"
    inputs.mkdir()
    for name, value in (("capture.json", capture), ("catalog.json", catalog), ("plan.json", plan),
                        ("resolution.json", resolution), ("materialization.json", materialization),
                        ("config.json", config)):
        _atomic(inputs / name, value)
    journal = _journal_payload(plan, materialization, config)
    _atomic(run_dir / "execution-status.json", journal)
    return journal


def _load_journal(run_dir: Path, archive_cache: Path, source_root: Path) -> tuple[dict[str, Any], tuple[dict[str, Any], ...], dict[str, Any]]:
    docs = _verify_inputs(run_dir, archive_cache, source_root)
    config = _read(run_dir / "inputs" / "config.json")
    plan, materialization = docs[2], docs[4]
    releases = _selected(plan)
    _config(config, releases)
    journal = _read(run_dir / "execution-status.json")
    if (journal.get("schema") != SCHEMA or journal.get("kind") != KIND
            or journal.get("plan_sha256") != plan["plan_sha256"]
            or journal.get("materialization_sha256") != materialization["materialization_sha256"]
            or journal.get("config_sha256") != _sha_json(config)
            or journal.get("execution_source_commit") != config["execution_source_commit"]
            or journal.get("host_lock") != config["host_lock"] or journal.get("promotion") != "forbidden"):
        raise Refused("execution journal is not bound to immutable inputs")
    if journal.get("phase") not in {"planned", "running", "failed", "interrupted", "complete"}:
        raise Refused("execution journal phase is unsupported")
    if not isinstance(journal.get("releases"), dict) or set(journal["releases"]) != set(releases):
        raise Refused("execution journal releases differ from selected plan")
    return journal, docs, config


def _store_journal(run_dir: Path, journal: dict[str, Any]) -> None:
    _atomic(run_dir / "execution-status.json", journal)


def _event(journal: dict[str, Any], event: str, **fields: Any) -> None:
    journal["events"].append({"event": event, "at_unix": time.time(), **fields})


def _result_path(run_dir: Path, release: str, stage: str) -> Path:
    return run_dir / "stage-results" / _safe_name(release) / f"{stage}.json"


def _regular(path: Path, label: str) -> Path:
    if path.is_symlink() or not path.is_file():
        raise Refused(f"{label} must be a regular file")
    return path.resolve()


def _source_tree(checkout: Path) -> dict[str, Any]:
    """Hash the owned checkout, including untracked source entries.

    Build products are outside the source proof.  `.git`, Python bytecode and
    the isolated Cargo target are implementation metadata/cache, never source.
    """
    files: dict[str, list[str]] = {}
    for directory, dirs, names in os.walk(checkout, followlinks=False):
        root = Path(directory)
        dirs[:] = sorted(name for name in dirs if name not in {".git", "target", "__pycache__"})
        links = [name for name in dirs if (root / name).is_symlink()]
        dirs[:] = [name for name in dirs if name not in links]
        for name in sorted(name for name in names + links if name != ".git"):
            path = root / name
            relative = path.relative_to(checkout).as_posix()
            if path.is_symlink():
                files[relative] = ["symlink", os.readlink(path)]
            elif path.is_file():
                digest = hashlib.sha256(path.read_bytes()).hexdigest()
                files[relative] = ["file", digest]
            else:
                raise Refused("owned checkout contains unsupported source entry")
    return {"files": files, "sha256": _sha_json(files)}


def _verify_source_transition(before: Mapping[str, Any], after: Mapping[str, Any], allowed: set[str]) -> None:
    if not isinstance(before.get("files"), dict) or not isinstance(after.get("files"), dict):
        raise Refused("source tree snapshot is malformed")
    changed = {name for name in before["files"].keys() | after["files"].keys()
               if before["files"].get(name) != after["files"].get(name)}
    unexpected = sorted(changed - allowed)
    if unexpected:
        raise Refused("stage changed non-generated source entries: " + ", ".join(unexpected))


def _require_source_proof(result: Mapping[str, Any], checkout: Path, source_commit: str,
                          snapshot: Mapping[str, Any]) -> None:
    if result.get("source_commit") != source_commit or result.get("source_tree_sha256") != snapshot.get("sha256"):
        raise Refused("stage result is not bound to the immutable execution source tree")


def _require_generated_artifacts(result: Mapping[str, Any], checkout: Path) -> list[dict[str, Any]]:
    rows = result.get("generated_artifacts")
    expected = [item.path for item in artifacts.ARTIFACTS]
    if not isinstance(rows, list) or len(rows) != len(expected):
        raise Refused("stage result lacks complete sanctioned generated artifact proof")
    found: list[str] = []
    for row in rows:
        if (not isinstance(row, dict) or not isinstance(row.get("path"), str) or row["path"] not in expected
                or not isinstance(row.get("sha256"), str) or __import__("re").fullmatch(r"[0-9a-f]{64}", row["sha256"]) is None
                or type(row.get("bytes")) is not int or row["bytes"] < 0):
            raise Refused("generated artifact proof is malformed")
        path = _regular(checkout / row["path"], "generated artifact")
        if hashlib.sha256(path.read_bytes()).hexdigest() != row["sha256"] or path.stat().st_size != row["bytes"]:
            raise Refused("generated artifact no longer matches stage proof")
        found.append(row["path"])
    if found != expected:
        raise Refused("generated artifact proof differs from sanctioned manifest")
    return rows


def _require_raw_report(result: Mapping[str, Any]) -> None:
    row = result.get("raw_report")
    if not isinstance(row, dict) or not isinstance(row.get("path"), str) or not isinstance(row.get("sha256"), str):
        raise Refused("stage result lacks durable raw command report")
    path = _regular(Path(row["path"]), "raw command report")
    if hashlib.sha256(path.read_bytes()).hexdigest() != row["sha256"]:
        raise Refused("raw command report no longer matches stage proof")


def _require_binary_proof(result: Mapping[str, Any], target: Path, field: str = "binary") -> dict[str, Any]:
    row = result.get(field)
    if (not isinstance(row, dict) or not isinstance(row.get("path"), str) or not isinstance(row.get("sha256"), str)
            or __import__("re").fullmatch(r"[0-9a-f]{64}", row["sha256"]) is None
            or type(row.get("bytes")) is not int or row["bytes"] < 0):
        raise Refused("stage result lacks binary identity proof")
    binary = _regular(Path(row["path"]), "built OxiDex executable")
    if not binary.is_relative_to(target.resolve()) or hashlib.sha256(binary.read_bytes()).hexdigest() != row["sha256"] or binary.stat().st_size != row["bytes"]:
        raise Refused("built OxiDex executable no longer matches stage proof")
    return row


def _require_fixture_proof(result: Mapping[str, Any]) -> None:
    row = result.get("fixtures")
    if (not isinstance(row, dict) or not isinstance(row.get("manifest"), str) or not isinstance(row.get("manifest_sha256"), str)
            or __import__("re").fullmatch(r"[0-9a-f]{64}", row["manifest_sha256"]) is None or not isinstance(row.get("entries"), list) or not row["entries"]):
        raise Refused("read result lacks immutable fixture proof")
    manifest = _regular(Path(row["manifest"]), "fixture manifest")
    if hashlib.sha256(manifest.read_bytes()).hexdigest() != row["manifest_sha256"]:
        raise Refused("fixture manifest changed after comparison")
    for item in row["entries"]:
        if (not isinstance(item, dict) or not isinstance(item.get("source"), str) or not isinstance(item.get("sha256"), str)
                or __import__("re").fullmatch(r"[0-9a-f]{64}", item["sha256"]) is None
                or type(item.get("bytes")) is not int or item["bytes"] < 0
                or not isinstance(item.get("corpus_path"), str) or not isinstance(item.get("corpus_sha256"), str)
                or __import__("re").fullmatch(r"[0-9a-f]{64}", item["corpus_sha256"]) is None
                or type(item.get("corpus_bytes")) is not int or item["corpus_bytes"] < 0):
            raise Refused("fixture identity proof is malformed")
        fixture = _regular(Path(item["source"]), "fixture")
        if hashlib.sha256(fixture.read_bytes()).hexdigest() != item["sha256"] or fixture.stat().st_size != item["bytes"]:
            raise Refused("fixture changed after comparison")
        staged = _regular(Path(item["corpus_path"]), "staged fixture")
        if (hashlib.sha256(staged.read_bytes()).hexdigest() != item["corpus_sha256"]
                or staged.stat().st_size != item["corpus_bytes"]
                or item["corpus_sha256"] != item["sha256"] or item["corpus_bytes"] != item["bytes"]):
            raise Refused("staged fixture changed after comparison")


def _require_native_identity(result: Mapping[str, Any], release: str, native: tuple[Path, Path, Path], perl: str) -> None:
    source, lib, _program = native
    pm = _regular(lib / "Image" / "ExifTool.pm", "selected native ExifTool library")
    executable = _regular(Path(perl), "selected native Perl")
    expected = {"release": release, "perl": {"path": str(executable), "sha256": hashlib.sha256(executable.read_bytes()).hexdigest()},
                "source": {"path": str(source.resolve())}, "lib": {"path": str(lib.resolve()), "exiftool_pm_sha256": hashlib.sha256(pm.read_bytes()).hexdigest()}}
    if result.get("native_identity") != expected:
        raise Refused("stage result is not bound to the selected native identity")


def _stage_result(path: Path, release: str, stage: str, native_probe_sha: str | None,
                  checkout: Path | None = None, source_commit: str | None = None,
                  source_tree: Mapping[str, Any] | None = None, target: Path | None = None,
                  native: tuple[Path, Path, Path] | None = None, perl: str | None = None) -> dict[str, Any]:
    result = _read(_regular(path, f"{stage} result"))
    if (result.get("schema") != SCHEMA or result.get("kind") != RESULT_KIND or result.get("stage") != stage
            or result.get("release") != release or result.get("state") != "passed"
            or type(result.get("denominator")) is not int or result["denominator"] < 1):
        raise Refused(f"{stage} result lacks a passed state or positive denominator")
    if native_probe_sha is not None and (result.get("native_release") != release
                                         or result.get("native_probe_sha256") != native_probe_sha):
        raise Refused(f"{stage} result is not bound to this release's native oracle")
    if native_probe_sha is not None:
        comparison = result.get("comparison")
        if (not isinstance(comparison, dict) or comparison.get("kind") != "oxidex_vs_native"
                or comparison.get("native_release") != release
                or type(comparison.get("matched")) is not int or comparison["matched"] < 0
                or type(comparison.get("mismatched")) is not int or comparison["mismatched"] != 0
                or comparison["matched"] + comparison["mismatched"] != result["denominator"]):
            raise Refused(f"{stage} result lacks attributable OxiDex/native outcomes")
    if checkout is not None and source_commit is not None and source_tree is not None:
        _require_source_proof(result, checkout, source_commit, source_tree)
        _require_generated_artifacts(result, checkout)
        _require_raw_report(result)
        if native is None or perl is None:
            raise Refused("stage result native binding was not supplied")
        _require_native_identity(result, release, native, perl)
        if stage in {"build", "read"}:
            if target is None:
                raise Refused("stage result binary target was not supplied")
            _require_binary_proof(result, target)
        if stage in {"build", "write"}:
            if target is None:
                raise Refused("stage result writer binary target was not supplied")
            _require_binary_proof(result, target, "writer_binary")
        if stage in {"read", "write"}:
            _require_fixture_proof(result)
        if stage == "write":
            mode = result.get("write_mode")
            if (not isinstance(mode, dict) or mode.get("kind") != "selected-release-live-native"
                    or mode.get("release") != release
                    or not isinstance(mode.get("ledger_sha256"), str)
                    or __import__("re").fullmatch(r"[0-9a-f]{64}", mode["ledger_sha256"]) is None
                    or not isinstance(mode.get("rules_sha256"), str)
                    or __import__("re").fullmatch(r"[0-9a-f]{64}", mode["rules_sha256"]) is None):
                raise Refused("write result lacks selected-release matrix mode proof")
    return result


def _run_record(argv: list[str], *, cwd: Path, env: dict[str, str], run: Callable[..., subprocess.CompletedProcess[str]],
                started: Callable[[int, int], None] | None = None) -> dict[str, Any]:
    """Run one bounded command and retain its actual output for the journal log.

    The production branch deliberately leaves inherited descriptors open. The
    host-lock descriptor is inheritable, so a supervisor dying while this child
    is live cannot let another rehearsal acquire the shared lock prematurely.
    """
    try:
        if run is subprocess.run:
            child = subprocess.Popen(argv, cwd=str(cwd), env=env, text=True, errors="replace",
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                     start_new_session=True, close_fds=False)
            if started is not None:
                started(child.pid, child.pid)
            try:
                stdout, stderr = child.communicate(timeout=COMMAND_TIMEOUT_SECONDS)
            except subprocess.TimeoutExpired as exc:
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    stdout, stderr = child.communicate(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    stdout, stderr = child.communicate()
                return {"argv": argv, "exit": None, "stdout": stdout or "", "stderr": (stderr or "") + str(exc), "state": "timeout", "pid": child.pid, "pgid": child.pid}
            result = subprocess.CompletedProcess(argv, child.returncode, stdout, stderr)
            process_identity = {"pid": child.pid, "pgid": child.pid}
        else:
            result = run(argv, cwd=str(cwd), env=env, text=True, capture_output=True, timeout=COMMAND_TIMEOUT_SECONDS,
                         start_new_session=True, close_fds=False)
            process_identity = {}
        stdout, stderr = result.stdout or "", result.stderr or ""
        return {"argv": argv, "exit": result.returncode, "stdout": stdout, "stderr": stderr,
                "state": "ok" if result.returncode == 0 else "exit_failed", **process_identity}
    except subprocess.TimeoutExpired as exc:
        return {"argv": argv, "exit": None, "stdout": "", "stderr": str(exc), "state": "timeout"}
    except OSError as exc:
        return {"argv": argv, "exit": None, "stdout": "", "stderr": str(exc), "state": "spawn_failed"}


def _command_log(run_dir: Path, release: str, stage: str, record: dict[str, Any]) -> dict[str, Any]:
    path = run_dir / "command-logs" / _safe_name(release) / f"{stage}.json"
    if path.exists() or path.is_symlink():
        raise Refused("command log already exists")
    path.parent.mkdir(parents=True, exist_ok=True)
    _atomic(path, record)
    return {"path": str(path.relative_to(run_dir)), "sha256": _sha_json(record), "state": record["state"], "exit": record["exit"]}


def _expand(spec: Mapping[str, Any], values: Mapping[str, str]) -> list[str]:
    try:
        return [item.format(**values) for item in spec["argv"]]
    except (KeyError, ValueError) as exc:
        raise Refused("configured stage command cannot be expanded") from exc


def _default_checkout(repository: Path, commit: str, destination: Path,
                      run: Callable[..., subprocess.CompletedProcess[str]]) -> Path:
    if destination.exists() or destination.is_symlink():
        raise Refused("owned checkout destination already exists")
    check = _run_record(["git", "-C", str(repository), "rev-parse", "--verify", commit + "^{commit}"],
                        cwd=repository, env=dict(os.environ), run=run)
    if check["state"] != "ok":
        raise Refused("repository does not contain the plan commit")
    made = _run_record(["git", "-C", str(repository), "worktree", "add", "--detach", str(destination), commit],
                       cwd=repository, env=dict(os.environ), run=run)
    if made["state"] != "ok" or not destination.is_dir() or destination.is_symlink():
        raise Refused("cannot create owned isolated checkout")
    return destination.resolve()


def _native_identity(materialization: Mapping[str, Any], source_root: Path, release: str) -> tuple[Path, Path, Path]:
    row = next((item for item in materialization["selected_releases"] if item["release"] == release), None)
    if not isinstance(row, dict) or row.get("state") != "materialized":
        raise Refused("selected native release is not materialized")
    source = (source_root / row["source_directory"]).resolve()
    return source, source / "lib", source / "exiftool"


def _verify_checkout_head(checkout: Path, expected_commit: str,
                          run: Callable[..., subprocess.CompletedProcess[str]]) -> None:
    try:
        result = run(["git", "-C", str(checkout), "rev-parse", "HEAD"], cwd=str(checkout), env=dict(os.environ),
                     text=True, capture_output=True, timeout=30, start_new_session=True, close_fds=False)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise Refused("cannot verify owned checkout HEAD") from exc
    if result.returncode != 0 or (result.stdout or "").strip() != expected_commit:
        raise Refused("owned checkout HEAD differs from immutable execution source commit")


def _run_stage(run_dir: Path, journal: dict[str, Any], release: str, stage: str, checkout: Path, target: Path,
               native: tuple[Path, Path, Path], perl: str, native_probe: dict[str, Any], config: dict[str, Any],
               run: Callable[..., subprocess.CompletedProcess[str]]) -> bool:
    state = journal["releases"][release]["stages"][stage]
    if state == "passed" or state == "unsupported":
        return True
    if state != "pending":
        raise Refused(f"{release} {stage} is not safely runnable after interruption or failure")
    if stage == "write" and stage not in config["commands"]:
        journal["releases"][release]["stages"][stage] = "unsupported"
        journal["releases"][release]["reports"][stage] = {"reason": "no generated write acceptance command configured"}
        _event(journal, "stage_unsupported", release=release, stage=stage)
        _store_journal(run_dir, journal)
        return True
    output = _result_path(run_dir, release, stage)
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists() or output.is_symlink():
        raise Refused("stage output already exists before its journal stage begins")
    journal["phase"] = "running"
    journal["active"] = {"release": release, "stage": stage}
    journal["releases"][release]["stages"][stage] = "running"
    _event(journal, "stage_started", release=release, stage=stage)
    _store_journal(run_dir, journal)
    source, lib, program = native
    before_source = _source_tree(checkout)
    saved_source = journal["releases"][release].get("source_tree")
    if saved_source is not None:
        _verify_source_transition(saved_source, before_source, set())
    _verify_checkout_head(checkout, config["execution_source_commit"], run)
    values = {"release": release, "checkout": str(checkout), "target": str(target), "report": str(output),
              "native_source": str(source), "native_lib": str(lib), "native_program": str(program),
              "native_perl": perl, "native_probe": str(_result_path(run_dir, release, "native")),
              "native_probe_sha256": str(native_probe.get("probe_sha256", "")),
              "write_fixture_manifest": str(config.get("write_fixture_bindings", {}).get(release, {}).get("path", "")),
              "source_commit": config["execution_source_commit"]}
    env = dict(os.environ, CARGO_TARGET_DIR=str(target), OXIDEX_REHEARSAL_RELEASE=release,
               OXIDEX_REHEARSAL_NATIVE_SOURCE=str(source), OXIDEX_REHEARSAL_NATIVE_LIB=str(lib),
               OXIDEX_REHEARSAL_NATIVE_PROGRAM=str(program), OXIDEX_REHEARSAL_NATIVE_PERL=perl,
               OXIDEX_REHEARSAL_NATIVE_PROBE=values["native_probe"], OXIDEX_REHEARSAL_REPORT=str(output),
               OXIDEX_REHEARSAL_NATIVE_PROBE_SHA256=values["native_probe_sha256"],
               OXIDEX_REHEARSAL_CHECKOUT=str(checkout), OXIDEX_REHEARSAL_SOURCE_COMMIT=config["execution_source_commit"],
               OXIDEX_REHEARSAL_WRITE_FIXTURE_MANIFEST=values["write_fixture_manifest"])
    def started(pid: int, pgid: int) -> None:
        journal["active"]["child"] = {"pid": pid, "pgid": pgid}
        _store_journal(run_dir, journal)
    record = _run_record(_expand(config["commands"][stage], values), cwd=checkout, env=env, run=run, started=started)
    command_log = _command_log(run_dir, release, stage, record)
    try:
        if record["state"] != "ok":
            raise Refused(f"{stage} command {record['state']}")
        after_source = _source_tree(checkout)
        allowed = {".exiftool-version", *(item.path for item in artifacts.ARTIFACTS)} if stage == "generate" else set()
        _verify_source_transition(before_source, after_source, allowed)
        _verify_checkout_head(checkout, config["execution_source_commit"], run)
        result = _stage_result(output, release, stage,
                               native_probe.get("probe_sha256") if stage in {"read", "write"} else None,
                               checkout, config["execution_source_commit"], after_source, target, native, perl)
        if stage == "read":
            build_report = _read(run_dir / journal["releases"][release]["reports"]["build"]["path"])
            if result.get("binary") != build_report.get("binary"):
                raise Refused("read result did not use the proven build binary")
        if stage == "write":
            build_report = _read(run_dir / journal["releases"][release]["reports"]["build"]["path"])
            if result.get("writer_binary") != build_report.get("writer_binary"):
                raise Refused("write result did not use the proven writer driver")
            fixture_binding = config["write_fixture_bindings"][release]
            fixture_report = result.get("fixtures", {})
            if (not isinstance(fixture_report, dict)
                    or fixture_report.get("manifest") != fixture_binding["path"]
                    or fixture_report.get("manifest_sha256") != fixture_binding["sha256"]):
                raise Refused("write result did not use the immutable selected JPEG fixture manifest")
    except Refused as exc:
        journal["releases"][release]["stages"][stage] = "failed"
        journal["releases"][release]["failure"] = {"stage": stage, "detail": str(exc), "command": command_log}
        journal["phase"], journal["active"] = "failed", None
        _event(journal, "stage_failed", release=release, stage=stage, detail=str(exc))
        _store_journal(run_dir, journal)
        return False
    journal["releases"][release]["stages"][stage] = "passed"
    journal["releases"][release]["reports"][stage] = {"path": str(output.relative_to(run_dir)), "sha256": _sha_json(result), "denominator": result["denominator"], "command": command_log}
    journal["releases"][release]["source_tree"] = after_source
    journal["active"] = None
    _event(journal, "stage_passed", release=release, stage=stage, denominator=result["denominator"])
    _store_journal(run_dir, journal)
    return True


def _run_native(run_dir: Path, journal: dict[str, Any], release: str, docs: tuple[dict[str, Any], ...], config: dict[str, Any],
                archive_cache: Path, source_root: Path, run: Callable[..., subprocess.CompletedProcess[str]]) -> dict[str, Any] | None:
    state = journal["releases"][release]["stages"]["native"]
    if state == "passed":
        return _read(run_dir / journal["releases"][release]["reports"]["native"]["path"])
    if state != "pending":
        raise Refused("native stage is not safely runnable")
    output = _result_path(run_dir, release, "native")
    output.parent.mkdir(parents=True, exist_ok=True)
    journal["phase"], journal["active"] = "running", {"release": release, "stage": "native"}
    journal["releases"][release]["stages"]["native"] = "running"
    _event(journal, "stage_started", release=release, stage="native")
    _store_journal(run_dir, journal)
    capture, catalog, plan, resolution, materialization = docs
    def native_run(argv: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        if run is not subprocess.run:
            return run(argv, **kwargs)
        timeout = kwargs.pop("timeout", None)
        kwargs.pop("capture_output", None)
        child = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 start_new_session=True, close_fds=False, **kwargs)
        journal["active"]["child"] = {"pid": child.pid, "pgid": child.pid}
        _store_journal(run_dir, journal)
        try:
            stdout, stderr = child.communicate(timeout=timeout)
        except subprocess.TimeoutExpired as exc:
            os.killpg(child.pid, signal.SIGTERM)
            stdout, stderr = child.communicate()
            exc.output, exc.stderr = stdout, stderr
            raise
        return subprocess.CompletedProcess(argv, child.returncode, stdout, stderr)
    try:
        report = native_oracle.probe_materialized_native(materialization, plan, catalog, capture, resolution,
                                                         archive_cache, source_root, release, config["perls"][release],
                                                         config["native_cases"][release], run=native_run)
        native_oracle.write_probe_report(output, report)
        if report.get("state") != "ready" or not report.get("cases"):
            raise Refused("native oracle did not provide ready cases")
    except (Refused, native_oracle.Refused, catalog_stage.Refused, rehearsal.Refused, OSError) as exc:
        journal["releases"][release]["stages"]["native"] = "failed"
        journal["releases"][release]["failure"] = {"stage": "native", "detail": str(exc)}
        journal["phase"], journal["active"] = "failed", None
        _event(journal, "stage_failed", release=release, stage="native", detail=str(exc))
        _store_journal(run_dir, journal)
        return None
    journal["releases"][release]["stages"]["native"] = "passed"
    journal["releases"][release]["reports"]["native"] = {"path": str(output.relative_to(run_dir)), "sha256": _sha_json(report), "denominator": len(report["cases"])}
    journal["active"] = None
    _event(journal, "stage_passed", release=release, stage="native", denominator=len(report["cases"]))
    _store_journal(run_dir, journal)
    return report


class _HostLock:
    def __init__(self, path: Path): self.path, self.file = path, None
    def __enter__(self):
        if self.path.is_symlink():
            raise Refused("version rehearsal host lock must not be a symbolic link")
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.file = self.path.open("a+")
        os.set_inheritable(self.file.fileno(), True)
        try: fcntl.flock(self.file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc: self.file.close(); raise Refused("version rehearsal host lock is already held") from exc
        return self
    def __exit__(self, *_):
        if self.file: fcntl.flock(self.file.fileno(), fcntl.LOCK_UN); self.file.close()


def execute(run_dir: Path, repository: Path, archive_cache: Path, source_root: Path, *,
            run: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run,
            checkout: Callable[[Path, str, Path, Callable[..., subprocess.CompletedProcess[str]]], Path] = _default_checkout) -> dict[str, Any]:
    """Run each selected release once. Failed or interrupted stages are never retried."""
    journal, docs, config = _load_journal(run_dir, archive_cache, source_root)
    with _HostLock(Path(config["host_lock"])):
        if journal["phase"] == "running":
            raise Refused("execution is interrupted; recover it before any later run")
        if journal["phase"] in {"failed", "interrupted", "complete"}:
            raise Refused("execution journal is terminal and cannot re-run selected releases")
        plan, materialization = docs[2], docs[4]
        repository = repository.resolve()
        if repository.is_symlink() or not (repository / ".git").exists():
            raise Refused("repository must be an existing Git checkout")
        for release in _selected(plan):
            checkout_path = run_dir / "checkouts" / _safe_name(release)
            target = run_dir / "targets" / _safe_name(release)
            try:
                journal["phase"], journal["active"] = "running", {"release": release, "stage": "checkout"}
                _event(journal, "checkout_started", release=release)
                _store_journal(run_dir, journal)
                owned = checkout(repository, config["execution_source_commit"], checkout_path, run)
                if owned.resolve() != checkout_path.resolve() or owned.is_symlink() or not owned.is_dir():
                    raise Refused("checkout provider did not return the owned release checkout")
                _verify_checkout_head(owned, config["execution_source_commit"], run)
                journal["releases"][release]["source_tree"] = _source_tree(owned)
                target.mkdir(parents=True, exist_ok=True)
                journal["active"] = None
                _event(journal, "checkout_completed", release=release)
                _store_journal(run_dir, journal)
                native = _run_native(run_dir, journal, release, docs, config, archive_cache, source_root, run)
                if native is None: return journal
                for stage in ("generate", "build", "read", "write"):
                    if not _run_stage(run_dir, journal, release, stage, owned, target, _native_identity(materialization, source_root, release),
                                      config["perls"][release], native, config, run): return journal
                statuses = journal["releases"][release]["stages"]
                journal["releases"][release]["state"] = "passed_with_write_gap" if statuses["write"] == "unsupported" else "passed"
                _event(journal, "release_completed", release=release, state=journal["releases"][release]["state"])
                _store_journal(run_dir, journal)
            except (Refused, OSError) as exc:
                if journal.get("active") is not None:
                    active = journal["active"]
                    stage = active.get("stage", "checkout") if isinstance(active, dict) else "checkout"
                    if stage in STAGES:
                        journal["releases"][release]["stages"][stage] = "failed"
                    journal["phase"], journal["active"] = "failed", None
                    journal["releases"][release]["state"] = "failed"
                    journal["releases"][release]["failure"] = {"stage": stage, "detail": str(exc)}
                    _event(journal, "release_failed", release=release, detail=str(exc))
                    _store_journal(run_dir, journal)
                raise
        journal["phase"] = "complete"
        journal["scope"]["write_acceptance"] = "unsupported_for_one_or_more_releases" if any(
            row["stages"]["write"] == "unsupported" for row in journal["releases"].values()) else "passed_per_release"
        journal["scope"]["parity"] = "unproven_without_all_per-release_read_and_write_acceptance" if any(
            row["stages"]["write"] != "passed" for row in journal["releases"].values()) else "per-version-read-write-rehearsed; no-promotion"
        _event(journal, "execution_complete", promotion="forbidden")
        _store_journal(run_dir, journal)
        return journal


def recover(run_dir: Path, archive_cache: Path, source_root: Path) -> dict[str, Any]:
    """Record interruption without guessing whether an active command completed."""
    journal, _, config = _load_journal(run_dir, archive_cache, source_root)
    with _HostLock(Path(config["host_lock"])):
        if journal.get("phase") != "running" or not isinstance(journal.get("active"), dict):
            raise Refused("only a running execution can be recovered as interrupted")
        active = journal["active"]
        child = active.get("child")
        if isinstance(child, dict) and type(child.get("pid")) is int and child["pid"] > 0:
            try:
                os.kill(child["pid"], 0)
            except ProcessLookupError:
                pass
            except PermissionError:
                raise Refused("active child process cannot be inspected; refusing interruption recovery")
            else:
                raise Refused("active child process is still live; refusing interruption recovery")
        if active.get("stage") in STAGES:
            journal["releases"][active["release"]]["stages"][active["stage"]] = "interrupted"
        journal["phase"], journal["active"] = "interrupted", None
        _event(journal, "interrupted_recovered", previous_active=active)
        _store_journal(run_dir, journal)
        return journal


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    init = sub.add_parser("init", help="bind existing selected inputs; does no checkout, build, or promotion")
    for name in ("capture", "catalog", "plan", "resolution", "materialization", "config"):
        init.add_argument("--" + name, required=True)
    init.add_argument("--run-dir", required=True)
    execute_p = sub.add_parser("execute", help="run immutable per-release native/read/write rehearsal")
    for name in ("run-dir", "repository", "archive-cache", "source-root"):
        execute_p.add_argument("--" + name, required=True)
    recover_p = sub.add_parser("recover", help="mark an active execution interrupted; never retries it")
    for name in ("run-dir", "archive-cache", "source-root"):
        recover_p.add_argument("--" + name, required=True)
    args = parser.parse_args(argv)
    try:
        if args.command == "init":
            result = initialize_run(Path(args.run_dir), *(_read(Path(getattr(args, name))) for name in (
                "capture", "catalog", "plan", "resolution", "materialization", "config")))
        elif args.command == "execute":
            result = execute(Path(args.run_dir), Path(args.repository), Path(args.archive_cache), Path(args.source_root))
        else:
            result = recover(Path(args.run_dir), Path(args.archive_cache), Path(args.source_root))
        print(json.dumps({"phase": result["phase"], "promotion": result["promotion"], "parity": result["scope"]["parity"]}, sort_keys=True))
        return 2 if args.command == "execute" and result["phase"] != "complete" else 0
    except (Refused, rehearsal.Refused, catalog_stage.Refused, native_oracle.Refused, OSError) as exc:
        print(f"version rehearsal execution refused: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
