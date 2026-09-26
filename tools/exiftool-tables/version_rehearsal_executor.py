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
from contextlib import contextmanager
import ctypes
import fcntl
import hashlib
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import threading
import time
from typing import Any, Callable, Mapping

import version_rehearsal as rehearsal
import version_rehearsal_catalog as catalog_stage
import version_rehearsal_native_oracle as native_oracle
import artifacts

SCHEMA = 1
KIND = "oxidex_exiftool_version_rehearsal_execution"
RESULT_KIND = "oxidex_version_rehearsal_stage_result"
STAGES = ("native", "generate", "build", "test", "read", "write")
OPTIONAL_COMMANDS = ("test", "write")
REQUIRED_COMMANDS = ("generate", "build", "read")
_SAFE_RELEASE = __import__("re").compile(r"^[0-9]+\.[0-9]+$")
COMMAND_TIMEOUT_SECONDS = 3600
_TERMINATION_GRACE_SECONDS = 2
_PLACEHOLDERS = {
    "release", "checkout", "target", "report", "native_source", "native_lib",
    "native_program", "native_perl", "native_probe", "native_probe_sha256", "source_commit",
    "write_fixture_manifest",
    "read_fixture_manifest",
}
_WRITE_FIXTURE_KIND = "oxidex_version_rehearsal_write_fixture_manifest"
_READ_FIXTURE_KIND = "oxidex_version_rehearsal_fixture_manifest"


class Refused(ValueError):
    """The requested execution cannot be attributed safely."""


class LockRetained(Refused):
    """A host lock was deliberately left held because an owned child may be live."""

    def __init__(self, message: str, survivors: list[dict[str, Any]]):
        super().__init__(message)
        self.survivors = survivors


class OwnedChildCleanupIncomplete(OSError):
    """A spawned worker may still own the shared lease; preserve active state."""

    def __init__(self, detail: str):
        super().__init__(detail)
        self._oxidex_owned_child_cleanup = "incomplete"


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


def _fixture_binding(value: Any, *, kind: str, jpeg_only: bool) -> dict[str, Any]:
    """Capture a fixture manifest and every requested source identity.

    The path alone cannot be immutable execution input: both the manifest and
    its listed source fixtures must still equal this snapshot at execution.
    """
    label = "write fixture" if jpeg_only else "read fixture"
    if not isinstance(value, str) or not os.path.isabs(value) or "\x00" in value:
        raise Refused(f"{label} manifest path is malformed")
    manifest = _regular(Path(value), f"{label} manifest")
    try:
        document = json.loads(manifest.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise Refused(f"{label} manifest is unreadable") from exc
    if (not isinstance(document, dict) or document.get("schema") != 1
            or document.get("kind") != kind
            or not isinstance(document.get("fixtures"), list) or not document["fixtures"]):
        raise Refused(f"{label} manifest schema is unsupported")
    fixtures = []
    for item in document["fixtures"]:
        if (not isinstance(item, dict) or set(item) != {"path", "sha256", "bytes"}
                or not isinstance(item["path"], str) or not os.path.isabs(item["path"])
                or not isinstance(item["sha256"], str) or __import__("re").fullmatch(r"[0-9a-f]{64}", item["sha256"]) is None
                or type(item["bytes"]) is not int or item["bytes"] < 1):
            raise Refused(f"{label} manifest item is malformed")
        fixture = _regular(Path(item["path"]), label)
        if _sha_file(fixture) != item["sha256"] or fixture.stat().st_size != item["bytes"]:
            raise Refused(f"{label} differs from immutable manifest")
        if jpeg_only and fixture.read_bytes()[:2] != b"\xff\xd8":
            raise Refused("write fixture manifest contains a non-JPEG fixture")
        fixtures.append({"path": str(fixture), "sha256": item["sha256"], "bytes": item["bytes"]})
    if len({(item["path"], item["sha256"]) for item in fixtures}) != len(fixtures):
        raise Refused(f"{label} manifest has duplicate fixture identity")
    return {"path": str(manifest), "sha256": _sha_file(manifest), "bytes": manifest.stat().st_size,
            "fixtures": fixtures}


def _write_fixture_binding(value: Any) -> dict[str, Any]:
    return _fixture_binding(value, kind=_WRITE_FIXTURE_KIND, jpeg_only=True)


def _config(value: Any, releases: list[str], *, allow_legacy_recovery: bool = False) -> dict[str, Any]:
    if not isinstance(value, dict) or value.get("schema") != SCHEMA:
        raise Refused("execution config schema is unsupported")
    commands = value.get("commands")
    if not isinstance(commands, dict) or any(stage not in commands for stage in REQUIRED_COMMANDS):
        raise Refused("generate, build, and read commands are required")
    for stage, spec in commands.items():
        if stage not in {"generate", "build", "test", "read", "write"}:
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
    execution_releases = value.get("execution_releases", releases)
    if (not isinstance(execution_releases, list) or not execution_releases
            or len(set(execution_releases)) != len(execution_releases)
            or any(release not in releases for release in execution_releases)):
        raise Refused("execution releases must be a unique nonempty subset of the verified plan")
    perls = value.get("perls")
    cases = value.get("native_cases")
    if (not isinstance(perls, dict) or not isinstance(cases, dict)
            or set(perls) != set(execution_releases) or set(cases) != set(execution_releases)):
        raise Refused("config must bind an explicit Perl and native cases to every execution release")
    if any(not isinstance(perls[row], str) or not perls[row] for row in execution_releases):
        raise Refused("native Perl binding is malformed")
    if any(not isinstance(cases[row], list) or not cases[row] for row in execution_releases):
        raise Refused("native cases must be a nonempty list for every selected release")
    lock = value.get("host_lock")
    if not isinstance(lock, str) or not os.path.isabs(lock) or "\x00" in lock:
        raise Refused("config must bind one absolute shared host lock path")
    source_commit = value.get("execution_source_commit")
    if not isinstance(source_commit, str) or rehearsal.GIT_OID_RE.fullmatch(source_commit) is None:
        raise Refused("config must bind an immutable execution source commit")
    targets = value.get("target_directories")
    if targets is not None:
        if (not isinstance(targets, dict) or set(targets) != set(execution_releases)
                or any(not isinstance(targets[release], str) or not os.path.isabs(targets[release])
                       or "\x00" in targets[release] for release in execution_releases)
                or len({str(Path(targets[release]).resolve()) for release in execution_releases}) != len(execution_releases)):
            raise Refused("target directories must uniquely bind every execution release")
    read_manifests = value.get("read_fixture_manifests")
    saved_reads = value.get("read_fixture_bindings")
    normalized = dict(value)
    legacy_reads_absent = read_manifests is None and saved_reads is None
    if legacy_reads_absent and allow_legacy_recovery:
        pass
    else:
        if not isinstance(read_manifests, dict) or set(read_manifests) != set(execution_releases):
            raise Refused("read command requires one immutable fixture manifest per execution release")
        current_reads = {
            release: _fixture_binding(read_manifests[release], kind=_READ_FIXTURE_KIND, jpeg_only=False)
            for release in execution_releases
        }
        if saved_reads is not None and saved_reads != current_reads:
            raise Refused("read fixture manifest or requested scope changed after initialization")
        normalized["read_fixture_bindings"] = current_reads
    has_write = "write" in commands
    manifests, bindings = value.get("write_fixture_manifests"), value.get("write_fixture_bindings")
    if not has_write:
        if manifests is not None or bindings is not None:
            raise Refused("write fixture bindings require a configured write command")
        return normalized
    if not isinstance(manifests, dict) or set(manifests) != set(execution_releases):
        raise Refused("write command requires one immutable fixture manifest per selected release")
    current = {release: _write_fixture_binding(manifests[release]) for release in execution_releases}
    if bindings is not None and bindings != current:
        raise Refused("write fixture manifest or requested JPEG scope changed after initialization")
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
    releases = config.get("execution_releases", _selected(plan))
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
            "release_tests": "pending_or_unsupported",
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


def _load_journal(run_dir: Path, archive_cache: Path, source_root: Path, *,
                  allow_legacy_recovery: bool = False) -> tuple[dict[str, Any], tuple[dict[str, Any], ...], dict[str, Any]]:
    docs = _verify_inputs(run_dir, archive_cache, source_root)
    config = _read(run_dir / "inputs" / "config.json")
    plan, materialization = docs[2], docs[4]
    selected_releases = _selected(plan)
    _config(config, selected_releases, allow_legacy_recovery=allow_legacy_recovery)
    releases = config.get("execution_releases", selected_releases)
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


def _verify_source_transition(before: Mapping[str, Any], after: Mapping[str, Any], allowed: set[str],
                              *, split_tables: Path | None = None) -> None:
    """`split_tables`: the checkout whose regenerated split table directories
    may gain or lose module files (a release adds or drops ExifTool modules),
    provided its hubs then name exactly the files present."""
    if not isinstance(before.get("files"), dict) or not isinstance(after.get("files"), dict):
        raise Refused("source tree snapshot is malformed")
    changed = {name for name in before["files"].keys() | after["files"].keys()
               if before["files"].get(name) != after["files"].get(name)}
    if split_tables is not None:
        errors = artifacts.family_errors(split_tables)
        if errors:
            raise Refused("regenerated split tables are inconsistent: " + "; ".join(errors))
        changed = {name for name in changed if not artifacts.is_family_member(name)}
    unexpected = sorted(changed - allowed)
    if unexpected:
        raise Refused("stage changed non-generated source entries: " + ", ".join(unexpected))


def _require_source_proof(result: Mapping[str, Any], checkout: Path, source_commit: str,
                          snapshot: Mapping[str, Any]) -> None:
    if result.get("source_commit") != source_commit or result.get("source_tree_sha256") != snapshot.get("sha256"):
        raise Refused("stage result is not bound to the immutable execution source tree")


def _require_generated_artifacts(result: Mapping[str, Any], checkout: Path) -> list[dict[str, Any]]:
    rows = result.get("generated_artifacts")
    expected = [item.path for item in artifacts.inventory(checkout)]
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


def _require_fixture_proof(result: Mapping[str, Any]) -> list[dict[str, Any]]:
    row = result.get("fixtures")
    if (not isinstance(row, dict) or not isinstance(row.get("manifest"), str) or not isinstance(row.get("manifest_sha256"), str)
            or __import__("re").fullmatch(r"[0-9a-f]{64}", row["manifest_sha256"]) is None or not isinstance(row.get("entries"), list) or not row["entries"]):
        raise Refused("read result lacks immutable fixture proof")
    manifest = _regular(Path(row["manifest"]), "fixture manifest")
    if hashlib.sha256(manifest.read_bytes()).hexdigest() != row["manifest_sha256"]:
        raise Refused("fixture manifest changed after comparison")
    scope: list[dict[str, Any]] = []
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
        scope.append({"path": str(fixture), "sha256": item["sha256"], "bytes": item["bytes"]})
    return scope


def _require_native_identity(result: Mapping[str, Any], release: str, native: tuple[Path, Path, Path], perl: str) -> None:
    source, lib, _program = native
    pm = _regular(lib / "Image" / "ExifTool.pm", "selected native ExifTool library")
    executable = _regular(Path(perl), "selected native Perl")
    expected = {"release": release, "perl": {"path": str(executable), "sha256": hashlib.sha256(executable.read_bytes()).hexdigest()},
                "source": {"path": str(source.resolve())}, "lib": {"path": str(lib.resolve()), "exiftool_pm_sha256": hashlib.sha256(pm.read_bytes()).hexdigest()}}
    if result.get("native_identity") != expected:
        raise Refused("stage result is not bound to the selected native identity")


def _require_test_suite_proof(result: Mapping[str, Any]) -> None:
    """A release test stage passes only with counted, zero-failure results."""
    suite = result.get("test_suite")
    keys = ("passed", "failed", "ignored", "measured", "filtered_out", "targets")
    totals = suite.get("totals") if isinstance(suite, dict) else None
    commands = suite.get("commands") if isinstance(suite, dict) else None
    if (not isinstance(totals, dict) or not isinstance(commands, list) or not commands
            or any(type(totals.get(key)) is not int or totals[key] < 0 for key in keys)
            or totals["failed"] != 0 or totals["passed"] < 1 or totals["targets"] < 1
            or result.get("denominator") != totals["passed"] + totals["failed"]
            or suite.get("log") != result.get("raw_report")
            or any(not isinstance(row, dict) or row.get("exit") != 0
                   or any(type(row.get(key)) is not int or row[key] < 0 for key in keys)
                   for row in commands)
            or any(totals[key] != sum(row[key] for row in commands) for key in keys)):
        raise Refused("test result lacks a counted zero-failure release test suite")
    oracle = suite.get("exiftool_oracle")
    native = result.get("native_identity")
    try:
        same_native = (oracle["tree_realpath"] == native["source"]["path"]
                       and oracle["lib"]["exiftool_pm_sha256"] == native["lib"]["exiftool_pm_sha256"]
                       and oracle["perl"] == native["perl"])
    except (KeyError, TypeError):
        same_native = False
    if (not isinstance(oracle, dict) or oracle.get("version") != result.get("release")
            or oracle.get("docx_filetype") != "DOCX" or oracle.get("perl_modules_available") is not True
            or not same_native):
        raise Refused("test result was not graded by the selected release's capable ExifTool")
    corpus = suite.get("fixture_corpus")
    manifest = corpus.get("manifest") if isinstance(corpus, dict) else None
    if (not isinstance(manifest, dict) or type(manifest.get("file_count")) is not int or manifest["file_count"] < 1
            or not isinstance(manifest.get("sha256"), str)
            or __import__("re").fullmatch(r"[0-9a-f]{64}", manifest["sha256"]) is None
            or corpus.get("verified_before_run") is not True or corpus.get("verified_after_run") is not True):
        raise Refused("test result lacks a verified fixture corpus held unchanged across the run")


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
        if stage == "test":
            _require_test_suite_proof(result)
        if stage == "write":
            mode = result.get("write_mode")
            if (not isinstance(mode, dict) or mode.get("kind") != "selected-release-live-native"
                    or mode.get("release") != release
                    or not isinstance(mode.get("ledger_sha256"), str)
                    or __import__("re").fullmatch(r"[0-9a-f]{64}", mode["ledger_sha256"]) is None
                    or not isinstance(mode.get("rules_sha256"), str)
                    or __import__("re").fullmatch(r"[0-9a-f]{64}", mode["rules_sha256"]) is None):
                raise Refused("write result lacks selected-release matrix mode proof")
            ledger = _regular(checkout / "tools/exiftool-tables/tiff_scalar_final_ledger.json",
                              "generated final-stage ledger")
            rules = _regular(checkout / "src/writers/generated_tiff_scalar_final_rules.rs",
                             "generated final-stage rules")
            if (mode["ledger_sha256"] != _sha_file(ledger)
                    or mode["rules_sha256"] != _sha_file(rules)):
                raise Refused("write result matrix mode differs from generated source operands")
    return result


def _procfs_process_row(path: Path) -> tuple[str, int, int, int] | None:
    try:
        raw = path.read_text(encoding="utf-8")
    except OSError:
        return None
    close = raw.rfind(")")
    fields = raw[close + 2:].split() if close >= 0 else []
    if len(fields) < 20 or len(fields[0]) != 1:
        return None
    try:
        return fields[0], int(fields[1]), int(fields[2]), int(fields[19])
    except ValueError:
        return None


def _procfs_children(proc_root: Path) -> dict[int, list[int]]:
    try:
        entries = list(proc_root.iterdir())
    except OSError as exc:
        raise OSError("cannot enumerate procfs descendants") from exc
    children: dict[int, list[int]] = {}
    for entry in entries:
        if not entry.name.isdigit():
            continue
        row = _procfs_process_row(entry / "stat")
        if row is None:
            try:
                entry.stat()
            except FileNotFoundError:
                continue
            except OSError as exc:
                raise OSError("cannot verify procfs descendant enumeration") from exc
            raise OSError("cannot verify procfs descendant enumeration")
        children.setdefault(row[1], []).append(int(entry.name))
    return children


def _descendants(pid: int, *, proc_root: Path = Path("/proc")) -> list[int]:
    """Return descendants or fail when a complete enumeration is unavailable."""
    children: dict[int, list[int]] = {}
    if sys.platform == "linux":
        pending = [pid]
        while pending:
            parent = pending.pop()
            try:
                raw = (proc_root / str(parent) / "task" / str(parent) / "children").read_text(
                    encoding="ascii",
                )
            except OSError:
                children = _procfs_children(proc_root)
                break
            children[parent] = [int(value) for value in raw.split() if value.isdigit() and int(value) > 0]
            pending.extend(children[parent])
    else:
        try:
            result = subprocess.run(["ps", "-axo", "pid=,ppid="], text=True, capture_output=True,
                                    timeout=2, check=False)
        except (OSError, subprocess.TimeoutExpired) as exc:
            raise OSError("cannot enumerate descendants") from exc
        if result.returncode != 0 or not result.stdout.strip():
            raise OSError("cannot verify descendant enumeration")
        for row in result.stdout.splitlines():
            fields = row.split()
            if len(fields) == 2 and all(value.isdigit() for value in fields):
                children.setdefault(int(fields[1]), []).append(int(fields[0]))
    pending, found = [pid], []
    while pending:
        parent = pending.pop()
        for child in children.get(parent, []):
            if child not in found:
                found.append(child)
                pending.append(child)
    return found


class _OwnedChildren:
    """Every child spawned with inherited host-lock descriptors, until proven gone.

    Children run with ``close_fds=False`` so a live child keeps the host lock's
    open file description referenced. flock(2) is per description: an explicit
    LOCK_UN by the owner would release it for the child too, while closing only
    the owner's descriptor would not. A lock owner therefore unlocks only once
    each registered child has been reaped and its process group is empty.
    """

    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.children: dict[int, subprocess.Popen[Any]] = {}
        self.spawns_in_flight = 0


_OWNED = _OwnedChildren()
# Lock streams deliberately neither unlocked nor closed because an owned
# child could not be proven gone; see release_retained_locks().
_RETAINED_LOCKS: list[Any] = []


def _spawn(argv: list[str], **kwargs: Any) -> subprocess.Popen[Any]:
    """Popen that registers the child before any interruption can lose its PID."""
    owned = _OWNED
    with owned.lock:
        owned.spawns_in_flight += 1
    try:
        child = subprocess.Popen(argv, **kwargs)
    except OSError:
        with owned.lock:
            owned.spawns_in_flight -= 1
        raise
    # Any other interruption above deliberately leaves the in-flight count
    # raised: a child may exist whose PID was never observed, so every later
    # lock release in this process must fail closed.
    with owned.lock:
        owned.children[child.pid] = child
        owned.spawns_in_flight -= 1
    return child


# Process-group membership is read from the kernel, never from `ps` columns
# (a `ps` column silently wrong on one platform is AGENTS.md incident #9).
# Only macOS offers a trustworthy view here. On Linux /proc/<pid>/stat shows
# only a main thread's state (a worker thread sharing the lock descriptor can
# outlive a `Z` main thread) and a /proc walk is not a snapshot, so every
# other platform stays fail closed: a group that still answers is unproven.
_DARWIN_PROC_PGRP_ONLY = 2          # proc_listpids(PROC_PGRP_ONLY, pgid, ...)
_DARWIN_KERN_PROC = (1, 14)         # CTL_KERN, KERN_PROC
_DARWIN_KERN_PROC_PID, _DARWIN_KERN_PROC_PGRP = 1, 2
_DARWIN_KINFO_PROC_SIZE = 648       # sizeof(struct kinfo_proc), 64-bit
_DARWIN_P_STAT_OFFSET, _DARWIN_P_PID_OFFSET = 36, 40  # extern_proc.p_stat / p_pid
_DARWIN_SRUN, _DARWIN_SZOMB = 2, 5
_DARWIN_LIBRARIES: tuple[Any, Any] | None = None


def _darwin_libraries() -> tuple[Any, Any]:
    """libc and libproc with explicit prototypes (no default int conversions)."""
    global _DARWIN_LIBRARIES
    if _DARWIN_LIBRARIES is None:
        import ctypes
        import ctypes.util
        libc = ctypes.CDLL(None, use_errno=True)
        libc.sysctl.argtypes = [ctypes.POINTER(ctypes.c_int), ctypes.c_uint, ctypes.c_void_p,
                                ctypes.POINTER(ctypes.c_size_t), ctypes.c_void_p, ctypes.c_size_t]
        libc.sysctl.restype = ctypes.c_int
        library = ctypes.util.find_library("proc")
        if library is None:
            raise OSError("libproc is unavailable")
        libproc = ctypes.CDLL(library, use_errno=True)
        libproc.proc_listpids.argtypes = [ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_int]
        libproc.proc_listpids.restype = ctypes.c_int
        _DARWIN_LIBRARIES = (libc, libproc)
    return _DARWIN_LIBRARIES


def _darwin_pgrp_listpids(pgid: int) -> list[int] | None:
    """libproc's view of a group's members; None when it cannot be read."""
    try:
        import ctypes
        _libc, libproc = _darwin_libraries()
        needed = libproc.proc_listpids(_DARWIN_PROC_PGRP_ONLY, pgid, None, 0)
        if needed < 0:
            return None
        buffer = (ctypes.c_int * (needed // ctypes.sizeof(ctypes.c_int) + 64))()
        filled = libproc.proc_listpids(_DARWIN_PROC_PGRP_ONLY, pgid, buffer, ctypes.sizeof(buffer))
        if filled < 0 or filled >= ctypes.sizeof(buffer):
            return None  # an error, or possibly truncated: never guess
        return [pid for pid in buffer[: filled // ctypes.sizeof(ctypes.c_int)] if pid > 0]
    except (OSError, AttributeError, ctypes.ArgumentError):
        return None


def _darwin_kinfo(mib: list[int]) -> list[tuple[int, int]] | None:
    """(pid, p_stat) for each kinfo_proc a KERN_PROC sysctl returns."""
    try:
        import ctypes
        libc, _libproc = _darwin_libraries()
        names = (ctypes.c_int * len(mib))(*mib)
        size = ctypes.c_size_t(0)
        if libc.sysctl(names, len(mib), None, ctypes.byref(size), None, 0) != 0:
            return None
        size = ctypes.c_size_t(size.value + _DARWIN_KINFO_PROC_SIZE * 16)
        buffer = ctypes.create_string_buffer(size.value)
        if libc.sysctl(names, len(mib), buffer, ctypes.byref(size), None, 0) != 0:
            return None
    except (OSError, AttributeError, ctypes.ArgumentError):
        return None
    raw = buffer.raw[: size.value]
    if len(raw) % _DARWIN_KINFO_PROC_SIZE:
        return None
    return [(int.from_bytes(raw[start + _DARWIN_P_PID_OFFSET:start + _DARWIN_P_PID_OFFSET + 4], sys.byteorder,
                            signed=True), raw[start + _DARWIN_P_STAT_OFFSET])
            for start in range(0, len(raw), _DARWIN_KINFO_PROC_SIZE)]


def _darwin_group_members(pgid: int) -> list[tuple[int, str]] | None:
    """Members only when libproc and sysctl agree and the struct layout checks."""
    # Validate the kinfo_proc layout against this running process first.
    me = os.getpid()
    if _darwin_kinfo([*_DARWIN_KERN_PROC, _DARWIN_KERN_PROC_PID, me]) != [(me, _DARWIN_SRUN)]:
        return None
    listed = _darwin_pgrp_listpids(pgid)
    kinfo = _darwin_kinfo([*_DARWIN_KERN_PROC, _DARWIN_KERN_PROC_PGRP, pgid])
    if listed is None or kinfo is None or sorted(listed) != sorted(pid for pid, _stat in kinfo):
        return None
    return [(pid, "zombie" if stat == _DARWIN_SZOMB else "live") for pid, stat in kinfo]


def _group_member_states(pgid: int) -> list[tuple[int, str]] | None:
    """Every member of a process group as (pid, zombie|live); None if unknowable here.

    macOS only. Every other platform returns None, which keeps the group
    unproven and the host lock held.
    """
    if sys.platform == "darwin":
        return _darwin_group_members(pgid)
    return None


def _zombie_only_group(pgid: int) -> bool:
    """True only when every member is verified to be a zombie, twice over.

    A zombie has already closed every descriptor, so a group holding only
    zombies cannot keep the host lock's file description referenced. The
    enumeration is repeated and must return the identical all-zombie member
    set, closing the window in which a member forks a live child into the
    group between the two kernel views. Any enumeration failure, an empty
    answer, a live member, a changed membership or a non-macOS platform
    leaves the group unproven.
    """
    first = _group_member_states(pgid)
    if not first or any(state != "zombie" for _pid, state in first):
        return False
    second = _group_member_states(pgid)
    return second is not None and sorted(second) == sorted(first)


def _inherited_ownership_state(child: subprocess.Popen[Any]) -> str | None:
    """None unless a process may still hold the child's inherited descriptors.

    Children created by ``_spawn_with_deferred_sigint`` carry an ownership
    probe: an inherited pipe writer that every descendant keeps alongside the
    host-lock descriptor. Until its reader sees EOF some process, possibly a
    descendant that left the child's process group, still holds them, and an
    explicit unlock would release the lock for that process too.
    """
    if (not hasattr(child, "_oxidex_ownership_read_fd")
            and not getattr(child, "_oxidex_ownership_released", False)):
        return None  # no probe was issued for this child
    try:
        if _ownership_probe_live(child):
            return "exited; a descendant still holds its inherited descriptors"
    except OSError as exc:
        return f"exited; inherited descriptor ownership cannot be verified: {exc}"
    return None


def _unproven_state(child: subprocess.Popen[Any]) -> str | None:
    """None only when the child is reaped and its whole process group is gone.

    A group that still answers ``killpg(pgid, 0)`` (or refuses it with EPERM,
    as macOS does for a zombie-only group) counts as gone only when every
    member is verified to be a zombie. A child with an ownership probe also
    stays unproven while any process still holds its inherited descriptors.
    """
    try:
        if child.poll() is None:
            return "running"
    except OSError as exc:
        return f"exit status cannot be observed: {exc}"
    inherited = _inherited_ownership_state(child)
    if inherited is not None:
        return inherited
    try:
        os.killpg(child.pid, 0)
    except ProcessLookupError:
        return None
    except OSError as exc:
        if _zombie_only_group(child.pid):
            return None
        return f"exited; process group {child.pid} cannot be inspected: {exc}"
    if _zombie_only_group(child.pid):
        return None
    return f"exited; process group {child.pid} still has live members"


def _settle(child: subprocess.Popen[Any]) -> None:
    if child.poll() is not None:
        _close_lineage_status(child)
    owned = _OWNED
    with owned.lock:
        if owned.children.get(child.pid) is child and _unproven_state(child) is None:
            del owned.children[child.pid]


def unproven_children() -> list[dict[str, Any]]:
    """Owned children not proven gone; proven ones are pruned."""
    owned = _OWNED
    survivors: list[dict[str, Any]] = []
    with owned.lock:
        for pid, child in list(owned.children.items()):
            state = _unproven_state(child)
            if state is None:
                del owned.children[pid]
            else:
                survivors.append({"pid": pid, "pgid": pid, "state": state})
        if owned.spawns_in_flight:
            survivors.append({"pid": None, "pgid": None,
                              "state": "spawn interrupted before its PID was recorded"})
    return survivors


def describe_survivors(survivors: list[dict[str, Any]]) -> str:
    return ", ".join(
        f"PID {item['pid']} (process group {item['pgid']}, {item['state']})" if item["pid"] is not None
        else f"PID unknown ({item['state']})"
        for item in survivors
    )


def release_or_retain(stream: Any, capability: "_HeldHostLock | None") -> list[dict[str, Any]]:
    """Unlock only when every owned child is proven gone; otherwise retain.

    Retention means no LOCK_UN and no close: the stream is parked for the rest
    of this process so the lock stays held even if a surviving child has
    closed its inherited copy. Returns the survivors (empty after release).
    """
    if capability is not None:
        capability.deactivate()
    survivors = unproven_children()
    # A reaped child whose group still answers (macOS returns EPERM while the
    # group's last members are zombies awaiting reaping by init) gets a short,
    # bounded chance to be proven gone. A running or unknown child gets none.
    deadline = time.monotonic() + _TERMINATION_GRACE_SECONDS
    while (survivors and time.monotonic() < deadline
           and all(item["pid"] is not None and item["state"].startswith("exited") for item in survivors)):
        time.sleep(0.02)
        survivors = unproven_children()
    if survivors:
        _RETAINED_LOCKS.append(stream)
        return survivors
    fcntl.flock(stream.fileno(), fcntl.LOCK_UN)
    return []


def release_retained_locks() -> list[dict[str, Any]]:
    """Release retained locks once every owned child is proven gone."""
    survivors = unproven_children()
    if survivors:
        return survivors
    while _RETAINED_LOCKS:
        stream = _RETAINED_LOCKS.pop()
        try:
            fcntl.flock(stream.fileno(), fcntl.LOCK_UN)
        finally:
            stream.close()
    return []


def retained_lock_message(path: Path, survivors: list[dict[str, Any]]) -> str:
    return (f"{path} is intentionally still held (no unlock, descriptor retained) because owned "
            f"child process(es) could not be proven gone: {describe_survivors(survivors)}. The lock is "
            "released only when they exit; inspect or terminate them, then recover the execution "
            "journal before retrying")


def _tracked_run(argv: list[str], *, timeout: float | None = None, capture_output: bool = False,
                 started: Callable[[int, int], None] | None = None,
                 **kwargs: Any) -> subprocess.CompletedProcess[Any]:
    """subprocess.run for an owned child with interrupt-safe creation and verified cleanup.

    The child stays registered in ``_OWNED`` until it is proven gone. Cleanup
    that cannot be verified raises ``OwnedChildCleanupIncomplete`` so callers
    keep the child's journal identity and the lock owner retains the lock.
    """
    if capture_output:
        kwargs["stdout"] = kwargs["stderr"] = subprocess.PIPE
    owner: list[subprocess.Popen[Any] | None] = [None]
    try:
        child = _spawn_with_deferred_sigint(owner, argv, **kwargs)
        if started is not None:
            try:
                started(child.pid, child.pid)
            except BaseException as persistence:
                _cleanup_owned_child_after_interrupt(child, persistence)
                if getattr(persistence, "_oxidex_owned_child_cleanup", None) != "verified":
                    if isinstance(persistence, (KeyboardInterrupt, SystemExit)):
                        raise
                    raise OwnedChildCleanupIncomplete(
                        "owned child cleanup remains incomplete after child journal failure",
                    ) from persistence
                raise
        try:
            stdout, stderr = child.communicate(timeout=timeout)
        except subprocess.TimeoutExpired as exc:
            try:
                stdout, stderr = _bounded_timeout_cleanup(child)
            except (KeyboardInterrupt, SystemExit):
                raise
            except BaseException as cleanup:
                stdout, stderr = _emergency_cleanup_after_timeout_failure(child, cleanup)
                if hasattr(exc, "add_note"):
                    exc.add_note(f"owned child timeout cleanup warning: {cleanup}")
            exc.output, exc.stderr = stdout, stderr
            raise
        escaped = _finish_lineage(child)
        _require_ownership_release(child, "successful command completion")
        if escaped:
            raise OSError("owned command left descendants running after it exited; the lineage "
                          "boundary killed them: " + ", ".join(str(pid) for pid in escaped))
        return subprocess.CompletedProcess(argv, child.returncode, stdout, stderr)
    except (KeyboardInterrupt, SystemExit) as interruption:
        if (owner[0] is not None
                and getattr(interruption, "_oxidex_owned_child_cleanup", None) is None):
            _cleanup_owned_child_after_interrupt(owner[0], interruption)
        raise
    finally:
        if owner[0] is not None:
            _settle(owner[0])


def _signal_pid(pid: int, signal_value: signal.Signals) -> None:
    try:
        os.kill(pid, signal_value)
    except ProcessLookupError:
        pass


def _text_output(value: str | bytes | None) -> str:
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return value or ""


def _procfs_stat(path: Path) -> tuple[str, int] | None:
    try:
        raw = path.read_text(encoding="utf-8")
    except OSError:
        return None
    close = raw.rfind(")")
    fields = raw[close + 2:].split() if close >= 0 else []
    if len(fields) < 3 or len(fields[0]) != 1:
        return None
    try:
        return fields[0], int(fields[2])
    except ValueError:
        return None


_PROCFS_DEAD_STATES = frozenset({"Z", "X", "x"})


def _procfs_all_tasks_dead(pid: int, proc_root: Path) -> bool:
    """True only when every task of a zombie-looking process is itself dead.

    procfs reports the thread-group leader's state; a leader that exited
    before its other threads reads ``Z`` while a live thread still holds the
    process's descriptors. Anything unreadable counts as live.
    """
    try:
        tasks = [entry for entry in (proc_root / str(pid) / "task").iterdir() if entry.name.isdigit()]
    except OSError:
        return False
    if not tasks:
        return False
    for entry in tasks:
        row = _procfs_stat(entry / "stat")
        if row is None:
            try:
                entry.stat()
            except FileNotFoundError:
                continue  # that task finished while we looked
            except OSError:
                return False
            return False
        if row[0] not in _PROCFS_DEAD_STATES:
            return False
    return True


def _pid_live(pid: int, *, proc_root: Path = Path("/proc")) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    if sys.platform.startswith("linux"):
        row = _procfs_stat(proc_root / str(pid) / "stat")
        if row is None:
            return True
        return row[0] not in _PROCFS_DEAD_STATES or not _procfs_all_tasks_dead(pid, proc_root)
    return True


class _DarwinProcBSDInfo(ctypes.Structure):
    """Darwin's public proc_bsdinfo ABI from <sys/proc_info.h>."""

    _fields_ = [
        ("pbi_flags", ctypes.c_uint32),
        ("pbi_status", ctypes.c_uint32),
        ("pbi_xstatus", ctypes.c_uint32),
        ("pbi_pid", ctypes.c_uint32),
        ("pbi_ppid", ctypes.c_uint32),
        ("pbi_uid", ctypes.c_uint32),
        ("pbi_gid", ctypes.c_uint32),
        ("pbi_ruid", ctypes.c_uint32),
        ("pbi_rgid", ctypes.c_uint32),
        ("pbi_svuid", ctypes.c_uint32),
        ("pbi_svgid", ctypes.c_uint32),
        ("rfu_1", ctypes.c_uint32),
        ("pbi_comm", ctypes.c_char * 16),
        ("pbi_name", ctypes.c_char * 32),
        ("pbi_nfiles", ctypes.c_uint32),
        ("pbi_pgid", ctypes.c_uint32),
        ("pbi_pjobc", ctypes.c_uint32),
        ("e_tdev", ctypes.c_uint32),
        ("e_tpgid", ctypes.c_uint32),
        ("pbi_nice", ctypes.c_int32),
        ("pbi_start_tvsec", ctypes.c_uint64),
        ("pbi_start_tvusec", ctypes.c_uint64),
    ]


def _darwin_start_time(pid: int) -> tuple[int, int]:
    """Read the kernel process start timeval; never substitute coarse `ps` text."""
    try:
        libproc = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
        proc_pidinfo = libproc.proc_pidinfo
    except (OSError, AttributeError) as exc:
        raise OSError(f"cannot load precise Darwin process identity for pid {pid}") from exc
    proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64,
                             ctypes.c_void_p, ctypes.c_int]
    proc_pidinfo.restype = ctypes.c_int
    info = _DarwinProcBSDInfo()
    size = ctypes.sizeof(info)
    received = proc_pidinfo(pid, 3, 0, ctypes.byref(info), size)
    if received != size or info.pbi_pid != pid:
        # proc_pidinfo has no BSD info for a zombie, and signal zero to one
        # reparented to launchd is refused with EPERM. A zombie has exited
        # (and holds no descriptors); its PID cannot be reused until reaped.
        if _darwin_kinfo([*_DARWIN_KERN_PROC, _DARWIN_KERN_PROC_PID, pid]) == [(pid, _DARWIN_SZOMB)]:
            raise ProcessLookupError(pid)
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            raise
        except PermissionError:
            pass
        raise OSError(f"cannot obtain precise Darwin process identity for pid {pid}")
    seconds, microseconds = int(info.pbi_start_tvsec), int(info.pbi_start_tvusec)
    if seconds <= 0 or not 0 <= microseconds < 1_000_000:
        raise OSError(f"Darwin process identity is malformed for pid {pid}")
    return seconds, microseconds


def _process_identity(pid: int, *, proc_root: Path = Path("/proc")) -> str:
    """Return a stable identity token, distinguishing exit from unreadability."""
    if sys.platform.startswith("linux"):
        row = _procfs_process_row(proc_root / str(pid) / "stat")
        if row is not None:
            return f"procfs-start:{row[3]}"
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            raise
        raise OSError(f"cannot verify process identity for pid {pid}")
    if sys.platform == "darwin":
        seconds, microseconds = _darwin_start_time(pid)
        return f"darwin-start:{seconds}:{microseconds}"
    try:
        result = subprocess.run(
            ["ps", "-o", "lstart=", "-p", str(pid)], text=True,
            capture_output=True, timeout=2, check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise OSError(f"cannot verify process identity for pid {pid}") from exc
    token = result.stdout.strip()
    if result.returncode != 0 or not token:
        if not _pid_live(pid):
            raise ProcessLookupError(pid)
        raise OSError(f"cannot verify process identity for pid {pid}")
    return f"ps-start:{token}"


def _procfs_group_states(pgid: int, proc_root: Path) -> list[str] | None:
    try:
        entries = list(proc_root.iterdir())
    except OSError:
        return None
    states = []
    for entry in entries:
        if not entry.name.isdigit():
            continue
        row = _procfs_stat(entry / "stat")
        if row is None:
            try:
                entry.stat()
            except FileNotFoundError:
                continue
            except OSError:
                return None
            return None
        state, group = row
        if group == pgid:
            if state in _PROCFS_DEAD_STATES and not _procfs_all_tasks_dead(int(entry.name), proc_root):
                state = "live-thread"
            states.append(state)
    return states


def _group_live(pgid: int, *, proc_root: Path = Path("/proc")) -> bool:
    try:
        os.killpg(pgid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        # macOS refuses signal-zero to a group whose remaining members are
        # zombies awaiting reaping; only the kernel member view may say so.
        # Anything else, including a membership that changed between the two
        # kernel views while launchd reaped, still counts as live.
        return not _zombie_only_group(pgid)
    if sys.platform.startswith("linux"):
        states = _procfs_group_states(pgid, proc_root)
        if not states:
            return True
        return any(state not in {"Z", "X", "x"} for state in states)
    return not _zombie_only_group(pgid)


def _refresh_owned_descendants(child: subprocess.Popen[str]) -> dict[int, str]:
    descendants = dict(getattr(child, "_oxidex_owned_descendants", {}))
    for pid in _descendants(child.pid):
        if pid not in descendants:
            descendants[pid] = _process_identity(pid)
    setattr(child, "_oxidex_owned_descendants", descendants)
    return descendants


def _live_owned_descendants(child: subprocess.Popen[str]) -> list[int]:
    live = []
    for pid, identity in dict(getattr(child, "_oxidex_owned_descendants", {})).items():
        try:
            current = _process_identity(pid)
        except ProcessLookupError:
            continue
        if current == identity and _pid_live(pid):
            live.append(pid)
    return sorted(live)


def _mark_catchable_termination_unverifiable(child: subprocess.Popen[str]) -> None:
    """Remember that an owned handler had a chance to fork beyond our proofs."""
    setattr(child, "_oxidex_catchable_termination_unverifiable", True)


def _catchable_termination_unverifiable(child: subprocess.Popen[str]) -> bool:
    return getattr(child, "_oxidex_catchable_termination_unverifiable", False) is True


def _signal_owned_descendants(child: subprocess.Popen[str], signal_value: signal.Signals) -> None:
    identities = dict(getattr(child, "_oxidex_owned_descendants", {}))
    for pid, identity in sorted(identities.items(), reverse=True):
        try:
            current = _process_identity(pid)
        except ProcessLookupError:
            continue
        if current == identity:
            if signal_value == signal.SIGTERM and _pid_live(pid):
                _mark_catchable_termination_unverifiable(child)
            _signal_pid(pid, signal_value)


def _group_member_pids(pgid: int, *, proc_root: Path = Path("/proc")) -> list[int] | None:
    """Current members of a process group, or None when they cannot be listed."""
    if sys.platform.startswith("linux"):
        try:
            entries = list(proc_root.iterdir())
        except OSError:
            return None
        members = []
        for entry in entries:
            if not entry.name.isdigit():
                continue
            row = _procfs_stat(entry / "stat")
            if row is None:
                try:
                    entry.stat()
                except FileNotFoundError:
                    continue
                except OSError:
                    return None
                return None
            if row[1] == pgid:
                members.append(int(entry.name))
        return members
    if sys.platform == "darwin":
        members = _darwin_group_members(pgid)
        return None if members is None else [pid for pid, _state in members]
    return None


def _signal_owned_group(child: subprocess.Popen[str], signal_value: signal.Signals) -> None:
    """Signal the child's process group without ever hitting a reused PGID.

    While the leader is unreaped its PID, and so the PGID, cannot be reused,
    and ``killpg`` is safe. Once it is reaped the number may name an unrelated
    group, so only members whose recorded identity still matches are
    signalled; anything unverifiable is left for the caller's liveness check
    to report as incomplete.
    """
    if signal_value == signal.SIGTERM and _group_live(child.pid):
        _mark_catchable_termination_unverifiable(child)
    if child.returncode is not None:
        owned = dict(getattr(child, "_oxidex_owned_descendants", {}))
        for pid in _group_member_pids(child.pid) or []:
            identity = owned.get(pid)
            if identity is None:
                continue
            try:
                if not _pid_live(pid):
                    continue  # a zombie awaiting its reaper needs no signal
                if _process_identity(pid) == identity:
                    _signal_pid(pid, signal_value)
            except OSError:
                pass  # gone or unverifiable: the caller's liveness check decides
        return
    try:
        os.killpg(child.pid, signal_value)
    except ProcessLookupError:
        pass
    except PermissionError:
        # macOS refuses to signal a group whose members are zombies (they need
        # no signal). Whatever the reason, the caller's liveness check decides
        # whether the group is gone; an unsignalled live member fails closed.
        pass


def _wait_owned_descendants(child: subprocess.Popen[str]) -> list[int]:
    deadline = time.monotonic() + _TERMINATION_GRACE_SECONDS
    survivors = _live_owned_descendants(child)
    while survivors and time.monotonic() < deadline:
        time.sleep(0.02)
        survivors = _live_owned_descendants(child)
    return survivors


def _readable(descriptor: int, timeout: float) -> bool:
    """Whether a descriptor is readable (or at EOF) within ``timeout`` seconds.

    ``select.select`` refuses descriptors at or above ``FD_SETSIZE`` with a
    ValueError; ``poll`` has no such ceiling, and any failure is an OSError
    that callers already treat as unverifiable.
    """
    poller = select.poll()
    poller.register(descriptor, select.POLLIN | select.POLLHUP | select.POLLERR)
    try:
        events = poller.poll(max(0, int(timeout * 1000)))
    except (OSError, ValueError) as exc:
        raise OSError(f"cannot poll descriptor {descriptor}: {exc}") from exc
    if any(mask & select.POLLNVAL for _fd, mask in events):
        raise OSError(f"descriptor {descriptor} is not open")
    return bool(events)


def _ownership_probe_live(child: subprocess.Popen[str]) -> bool:
    """Report whether any spawned process still inherits the ownership writer."""
    descriptor = getattr(child, "_oxidex_ownership_read_fd", None)
    if type(descriptor) is not int or descriptor < 0:
        if getattr(child, "_oxidex_ownership_released", False) is True:
            return False
        raise OSError("owned process lifetime verification is unavailable")
    try:
        if not _readable(descriptor, 0):
            return True
        payload = os.read(descriptor, 1)
    except OSError as exc:
        raise OSError("owned process lifetime verification failed") from exc
    if payload:
        raise OSError("owned process lifetime probe received unexpected data")
    os.close(descriptor)
    setattr(child, "_oxidex_ownership_read_fd", -1)
    setattr(child, "_oxidex_ownership_released", True)
    return False


def _wait_for_ownership_release(child: subprocess.Popen[str]) -> bool:
    deadline = time.monotonic() + _TERMINATION_GRACE_SECONDS
    inherited = _ownership_probe_live(child)
    while inherited and time.monotonic() < deadline:
        time.sleep(0.02)
        inherited = _ownership_probe_live(child)
    return inherited


def _require_ownership_release(child: subprocess.Popen[str], operation: str) -> None:
    try:
        inherited = _wait_for_ownership_release(child)
    except OSError as exc:
        raise OwnedChildCleanupIncomplete(
            f"owned process lifetime ownership cannot be verified after {operation}",
        ) from exc
    if inherited:
        raise OwnedChildCleanupIncomplete(
            f"owned process lifetime ownership is still inherited after {operation}",
        )
    if _catchable_termination_unverifiable(child):
        raise OwnedChildCleanupIncomplete(
            f"owned process lifetime cannot be verified after catchable termination during {operation}",
        )


def _close_ownership_probe(child: subprocess.Popen[str]) -> None:
    descriptor = getattr(child, "_oxidex_ownership_read_fd", None)
    if type(descriptor) is int and descriptor >= 0:
        os.close(descriptor)
        setattr(child, "_oxidex_ownership_read_fd", -1)


def _bounded_timeout_cleanup(child: subprocess.Popen[str]) -> tuple[str, str]:
    """Non-catchably stop and verify the complete owned process lifetime."""
    _request_lineage_sweep(child)
    _refresh_owned_descendants(child)
    _signal_owned_descendants(child, signal.SIGKILL)
    _signal_owned_group(child, signal.SIGKILL)
    try:
        stdout, stderr = child.communicate(timeout=_TERMINATION_GRACE_SECONDS)
    except subprocess.TimeoutExpired as exc:
        stdout, stderr = exc.output, exc.stderr
        _signal_owned_group(child, signal.SIGKILL)
        try:
            stdout, stderr = child.communicate(timeout=_TERMINATION_GRACE_SECONDS)
        except subprocess.TimeoutExpired as retry:
            stdout, stderr = retry.output, retry.stderr
    if child.poll() is None:
        raise OSError("owned direct child is still live after bounded cleanup")
    deadline = time.monotonic() + _TERMINATION_GRACE_SECONDS
    while _group_live(child.pid) and time.monotonic() < deadline:
        _signal_owned_group(child, signal.SIGKILL)
        time.sleep(0.02)
    if _group_live(child.pid):
        raise OSError("owned process group is still live after bounded cleanup")
    _refresh_owned_descendants(child)
    _signal_owned_descendants(child, signal.SIGKILL)
    survivors = _wait_owned_descendants(child)
    if survivors:
        raise OSError("owned descendant processes are still live after bounded cleanup: "
                      + ", ".join(str(pid) for pid in survivors))
    _require_ownership_release(child, "bounded cleanup")
    return _text_output(stdout), _text_output(stderr)


def _emergency_reap_group(child: subprocess.Popen[str]) -> tuple[str, str]:
    """Kill and reap an owned group after ordinary cleanup itself faults."""
    enumeration_error = None
    try:
        _request_lineage_sweep(child)
    except OSError:
        pass
    try:
        _refresh_owned_descendants(child)
        _signal_owned_descendants(child, signal.SIGKILL)
    except BaseException as exc:
        enumeration_error = exc
    try:
        _signal_owned_group(child, signal.SIGKILL)
    except OSError:
        pass
    try:
        stdout, stderr = child.communicate(timeout=_TERMINATION_GRACE_SECONDS)
    except subprocess.TimeoutExpired as exc:
        stdout, stderr = exc.output, exc.stderr
    except OSError:
        stdout, stderr = "", ""
    survivors = _wait_owned_descendants(child)
    if survivors:
        raise OSError("owned descendant processes are still live after emergency cleanup: "
                      + ", ".join(str(pid) for pid in survivors))
    if enumeration_error is not None:
        raise OSError("owned descendant enumeration remained unverified during emergency cleanup") from enumeration_error
    _require_ownership_release(child, "emergency cleanup")
    return _text_output(stdout), _text_output(stderr)


def _cleanup_owned_child_after_interrupt(child: subprocess.Popen[str], interruption: BaseException) -> None:
    """Bound cleanup of the live Popen we own while preserving the original failure."""
    cleanup_failures = []
    emergency_needed = False
    cleanup_incomplete = False
    try:
        _bounded_timeout_cleanup(child)
    except BaseException as cleanup:
        cleanup_failures.append(f"bounded owned-child cleanup failed: {cleanup}")
        emergency_needed = True
    try:
        _refresh_owned_descendants(child)
        if (child.poll() is None or _group_live(child.pid) or _live_owned_descendants(child)
                or _ownership_probe_live(child)):
            cleanup_failures.append("owned child process group is still live after bounded cleanup")
            emergency_needed = True
        if _catchable_termination_unverifiable(child):
            cleanup_failures.append(
                "owned child cleanup cannot be verified after catchable termination",
            )
            emergency_needed = True
    except BaseException as inspection:
        cleanup_failures.append(f"owned child cleanup could not be verified: {inspection}")
        emergency_needed = True
    if emergency_needed:
        try:
            _emergency_reap_group(child)
        except BaseException as emergency:
            cleanup_failures.append(f"emergency owned-child cleanup failed: {emergency}")
        try:
            _refresh_owned_descendants(child)
            if (child.poll() is None or _group_live(child.pid) or _live_owned_descendants(child)
                    or _ownership_probe_live(child)):
                cleanup_failures.append("owned child process group is still live after emergency cleanup")
                cleanup_incomplete = True
            if _catchable_termination_unverifiable(child):
                cleanup_failures.append(
                    "owned child cleanup remains unverifiable after catchable termination",
                )
                cleanup_incomplete = True
        except BaseException as inspection:
            cleanup_failures.append(f"emergency owned-child cleanup could not be verified: {inspection}")
            cleanup_incomplete = True
    setattr(
        interruption,
        "_oxidex_owned_child_cleanup",
        "incomplete" if cleanup_incomplete else "verified",
    )
    for failure in cleanup_failures:
        if hasattr(interruption, "add_note"):
            interruption.add_note(failure)


def _emergency_cleanup_after_timeout_failure(
    child: subprocess.Popen[str], cleanup: BaseException,
) -> tuple[str, str]:
    """Emergency-clean a timed-out child or fail without clearing its journal identity."""
    failures = [f"bounded owned-child cleanup failed: {cleanup}"]
    try:
        stdout, stderr = _emergency_reap_group(child)
    except BaseException as emergency:
        failures.append(f"emergency owned-child cleanup failed: {emergency}")
        stdout, stderr = "", ""
    try:
        _refresh_owned_descendants(child)
        incomplete = (child.poll() is None or _group_live(child.pid) or bool(_live_owned_descendants(child))
                      or _ownership_probe_live(child) or _catchable_termination_unverifiable(child))
    except BaseException as inspection:
        failures.append(f"owned child cleanup could not be verified: {inspection}")
        incomplete = True
    if incomplete:
        raise OwnedChildCleanupIncomplete(
            "owned child cleanup remains incomplete after timeout: " + "; ".join(failures),
        ) from cleanup
    return stdout, stderr


# Linux lineage boundary. An owned command runs under a tiny supervisor that
# is a child subreaper (PR_SET_CHILD_SUBREAPER): every descendant that the
# command orphans -- including one that starts a new session and closes all
# inherited descriptors -- is reparented to the supervisor instead of init, so
# it cannot leave the command's lineage. When the command exits (or the
# executor requests cleanup with SIGUSR1) the supervisor SIGKILLs and reaps
# its children until waitpid reports ECHILD, the kernel's atomic proof that no
# descendant remains, and reports the command's status and any live escaped
# descendant on a private status pipe. Children of the supervisor cannot be
# PID-reused until it reaps them, so its signals are identity-safe. Darwin has
# no subreaper, so there a descendant that detaches and closes every
# inherited descriptor stays outside the ownership proof (a documented
# residual); the ownership probe still covers every descriptor holder.
_LINEAGE_SUPERVISED = sys.platform.startswith("linux")
_LINUX_LINEAGE_SUPERVISOR = r"""
import ctypes, json, os, signal, sys, time
status_fd = int(sys.argv[1])
argv = sys.argv[2:]

def report(**fields):
    # A lost reader must never stop the sweep: reporting is best effort, and
    # the executor treats a missing report as unverified.
    try:
        os.write(status_fd, (json.dumps(fields, sort_keys=True) + "\n").encode())
    except OSError:
        pass

class Sweep(Exception):
    pass

def request_sweep(_signum, _frame):
    raise Sweep()

def children():
    me = os.getpid()
    found = set()
    try:
        for task in os.listdir(f"/proc/{me}/task"):
            with open(f"/proc/{me}/task/{task}/children", encoding="ascii") as listing:
                found.update(int(value) for value in listing.read().split())
        return found
    except OSError:
        pass
    # Kernels without CONFIG_PROC_CHILDREN: scan every /proc/<pid>/stat for
    # our PID as parent. An entry that vanishes mid-scan exited; any other
    # unreadable entry makes the answer unverifiable.
    found = set()
    for name in os.listdir("/proc"):
        if not name.isdigit():
            continue
        try:
            with open(f"/proc/{name}/stat", encoding="utf-8") as stat:
                raw = stat.read()
        except FileNotFoundError:
            continue
        fields = raw[raw.rfind(")") + 2:].split()
        if len(fields) < 2:
            raise OSError(f"unparseable /proc/{name}/stat")
        if int(fields[1]) == me:
            found.add(int(name))
    return found

def state(pid):
    try:
        with open(f"/proc/{pid}/stat", encoding="utf-8") as stat:
            raw = stat.read()
        return raw[raw.rfind(")") + 2:].split()[0]
    except (OSError, IndexError):
        return "?"

try:
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.prctl(36, 1, 0, 0, 0) != 0:  # PR_SET_CHILD_SUBREAPER
        raise OSError(ctypes.get_errno(), "prctl(PR_SET_CHILD_SUBREAPER) failed")
    signal.signal(signal.SIGUSR1, request_sweep)
    failure_read, failure_write = os.pipe()  # close-on-exec in the command
    primary = os.fork()
    if primary == 0:
        try:
            os.close(status_fd)
            os.close(failure_read)
            signal.signal(signal.SIGUSR1, signal.SIG_DFL)
            os.execvp(argv[0], argv)
        except OSError as exc:
            os.write(failure_write, f"{exc.errno}:{exc.strerror}".encode())
        os._exit(127)
    os.close(failure_write)
    failure = b""
    while True:
        chunk = os.read(failure_read, 4096)
        if not chunk:
            break
        failure += chunk
    os.close(failure_read)
    if failure:
        os.waitpid(primary, 0)
        number, _, message = failure.decode(errors="replace").partition(":")
        report(phase="exec", errno=int(number), error=message)
        os._exit(0)
    report(phase="exec", ok=True)
    status = None
    requested = False
    try:
        while status is None:
            pid, raw = os.waitpid(-1, 0)
            if pid == primary:
                status = raw
    except Sweep:
        requested = True
    signal.signal(signal.SIGUSR1, signal.SIG_IGN)
    escaped = set()
    verified = False
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        for pid in children():
            if pid != primary and state(pid) not in ("Z", "X", "x"):
                escaped.add(pid)
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        try:
            while True:
                pid, raw = os.waitpid(-1, os.WNOHANG)
                if pid == 0:
                    break
                if pid == primary and status is None:
                    status = raw
        except ChildProcessError:
            verified = True
            break
        time.sleep(0.01)
    report(phase="exit", verified=verified, requested=requested, escaped=sorted(escaped),
           returncode=None if status is None else os.waitstatus_to_exitcode(status))
except BaseException as exc:
    try:
        report(phase="error", error=repr(exc))
    except BaseException:
        pass
os._exit(0)
"""


def _lineage_reports(child: subprocess.Popen[Any], *, timeout: float, phase: str) -> list[dict[str, Any]]:
    """Supervisor status lines read so far, waiting (bounded) for ``phase``.

    Stops at the first complete line reporting ``phase``, at EOF, or when
    ``timeout`` elapses; the caller decides what a missing phase means.
    """
    descriptor = getattr(child, "_oxidex_lineage_status_fd", -1)
    buffered: bytes = getattr(child, "_oxidex_lineage_buffer", b"")

    def parsed() -> list[dict[str, Any]]:
        try:
            return [json.loads(line) for line in buffered.split(b"\n")[:-1] if line.strip()]
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise OSError("owned command lineage report is malformed") from exc

    deadline = time.monotonic() + timeout
    while type(descriptor) is int and descriptor >= 0:
        if any(row.get("phase") == phase for row in parsed()):
            break
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            break
        if not _readable(descriptor, remaining):
            break
        chunk = os.read(descriptor, 65536)
        if not chunk:
            _close_lineage_status(child)
            break
        buffered += chunk
        setattr(child, "_oxidex_lineage_buffer", buffered)
    return parsed()


def _close_lineage_status(child: subprocess.Popen[Any]) -> None:
    descriptor = getattr(child, "_oxidex_lineage_status_fd", -1)
    if type(descriptor) is int and descriptor >= 0:
        os.close(descriptor)
        setattr(child, "_oxidex_lineage_status_fd", -1)


def _await_supervised_exec(child: subprocess.Popen[Any]) -> None:
    """Replicate Popen's exec-failure semantics for a supervised command.

    If the exec report cannot be read, the command may already be running
    while its handle is unpublished: the supervisor is told to sweep its
    lineage (SIGUSR1; before its handler exists that simply kills it before
    it forks) and the still-unreaped group is killed as a fallback.
    """
    try:
        reports = _lineage_reports(child, timeout=30, phase="exec")
        failure: BaseException | None = None
    except BaseException as exc:
        reports, failure = [], exc
    exec_report = next((row for row in reports if row.get("phase") == "exec"), None)
    if failure is None and exec_report is not None and exec_report.get("ok") is True:
        return
    if failure is not None or exec_report is None:
        try:
            os.kill(child.pid, signal.SIGUSR1)
        except ProcessLookupError:
            pass
    try:
        child.wait(timeout=_TERMINATION_GRACE_SECONDS * 3)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        child.wait()
    _close_lineage_status(child)
    for stream in (child.stdin, child.stdout, child.stderr):
        if stream is not None:
            stream.close()
    if failure is not None:
        raise OSError(f"owned command start could not be confirmed: {failure}") from failure
    if exec_report is not None and type(exec_report.get("errno")) is int:
        number = exec_report["errno"]
        raise OSError(number, exec_report.get("error") or os.strerror(number))
    raise OSError(f"owned command supervisor did not start the command: {reports}")


def _finish_lineage(child: subprocess.Popen[Any], *, timeout: float = _TERMINATION_GRACE_SECONDS) -> list[int]:
    """After a supervised command exits, adopt its status and escaped descendants.

    Returns the live descendants the supervisor found and killed. Raises
    ``OwnedChildCleanupIncomplete`` when the supervisor could not prove that
    no descendant remained. An unsupervised child returns an empty list.
    """
    if not hasattr(child, "_oxidex_lineage_status_fd"):
        return []
    if getattr(child, "_oxidex_lineage_finished", None) is not None:
        return child._oxidex_lineage_finished
    try:
        reports = _lineage_reports(child, timeout=timeout, phase="exit")
    except OSError as exc:
        raise OwnedChildCleanupIncomplete(f"owned command lineage cannot be verified: {exc}") from exc
    finished = next((row for row in reports if row.get("phase") == "exit"), None)
    if (finished is None or finished.get("verified") is not True
            or not isinstance(finished.get("escaped"), list)
            or any(type(pid) is not int for pid in finished["escaped"])):
        raise OwnedChildCleanupIncomplete(f"owned command lineage was not proven empty: {reports}")
    if type(finished.get("returncode")) is int:
        child.returncode = finished["returncode"]
    setattr(child, "_oxidex_lineage_finished", finished["escaped"])
    return finished["escaped"]


def _request_lineage_sweep(child: subprocess.Popen[Any]) -> None:
    """Ask a live supervisor to kill and reap its whole lineage, then exit."""
    if not hasattr(child, "_oxidex_lineage_status_fd") or child.poll() is not None:
        return
    try:
        os.kill(child.pid, signal.SIGUSR1)
    except ProcessLookupError:
        return
    deadline = time.monotonic() + _TERMINATION_GRACE_SECONDS * 3
    while child.poll() is None and time.monotonic() < deadline:
        time.sleep(0.02)


def _spawn_with_deferred_sigint(
    owner: list[subprocess.Popen[str] | None], argv: list[str], **kwargs: Any,
) -> subprocess.Popen[str]:
    """Publish the child handle before a process-wide SIGINT can interrupt Python."""
    if threading.current_thread() is not threading.main_thread():
        raise OSError("owned child creation requires the main thread")
    pending = False
    pending_frame = None
    previous_handler = signal.getsignal(signal.SIGINT)

    def defer_sigint(_signum: int, frame: Any) -> None:
        nonlocal pending, pending_frame
        pending = True
        pending_frame = frame

    read_fd = write_fd = status_read_fd = status_write_fd = -1
    child: subprocess.Popen[str] | None = None
    spawn_failure: BaseException | None = None
    signal.signal(signal.SIGINT, defer_sigint)
    try:
        if kwargs.get("close_fds") is not False:
            raise OSError("owned child creation requires inherited file descriptors")
        read_fd, write_fd = os.pipe()
        os.set_inheritable(write_fd, True)
        spawn_argv = list(argv)
        if _LINEAGE_SUPERVISED:
            status_read_fd, status_write_fd = os.pipe()
            os.set_inheritable(status_write_fd, True)
            spawn_argv = [sys.executable, "-I", "-c", _LINUX_LINEAGE_SUPERVISOR,
                          str(status_write_fd), *argv]
        created = _spawn(spawn_argv, **kwargs)
        setattr(created, "_oxidex_ownership_read_fd", read_fd)
        os.close(write_fd)
        write_fd = -1
        if status_read_fd >= 0:
            setattr(created, "_oxidex_lineage_status_fd", status_read_fd)
            status_read_fd = -1
            os.close(status_write_fd)
            status_write_fd = -1
            try:
                _await_supervised_exec(created)
            except OSError:
                # Nothing was exec'd: once the supervisor's inherited writer
                # is gone the child is provably finished; otherwise it stays
                # registered and every later lock release fails closed.
                read_fd = -1  # now owned by the probe attribute
                try:
                    _wait_for_ownership_release(created)
                except OSError:
                    pass
                _settle(created)
                raise
        child = created
        owner[0] = child
    except BaseException as failure:
        spawn_failure = failure
    finally:
        for descriptor in (write_fd, status_write_fd, status_read_fd):
            if descriptor >= 0:
                os.close(descriptor)
        if owner[0] is None and read_fd >= 0:
            os.close(read_fd)
        signal.signal(signal.SIGINT, previous_handler)
    if pending:
        handler = signal.default_int_handler if previous_handler == signal.SIG_DFL else previous_handler
        if previous_handler != signal.SIG_IGN and callable(handler):
            try:
                handler(signal.SIGINT, pending_frame)
            except BaseException as outcome:
                if child is not None:
                    _cleanup_owned_child_after_interrupt(child, outcome)
                raise
        elif previous_handler != signal.SIG_IGN:
            raise KeyboardInterrupt("SIGINT deferred until owned child creation completed")
    if spawn_failure is not None:
        raise spawn_failure
    if child is None:
        raise OSError("owned child creation did not return a process")
    return child


def _run_record(argv: list[str], *, cwd: Path, env: dict[str, str], run: Callable[..., subprocess.CompletedProcess[str]],
                started: Callable[[int, int], None] | None = None) -> dict[str, Any]:
    """Run one bounded command and retain its actual output for the journal log.

    The production branch deliberately leaves inherited descriptors open. The
    host-lock descriptor is inheritable, so a supervisor dying while this child
    is live cannot let another rehearsal acquire the shared lock prematurely.
    The child stays registered in ``_OWNED`` until it is proven gone, so the
    lock owner never explicitly unlocks while it may still be live.
    """
    if run is subprocess.run:
        owner: list[subprocess.Popen[str] | None] = [None]
        try:
            child = _spawn_with_deferred_sigint(
                owner, argv, cwd=str(cwd), env=env, text=True, errors="replace",
                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                start_new_session=True, close_fds=False,
            )
            if started is not None:
                started(child.pid, child.pid)
            try:
                stdout, stderr = child.communicate(timeout=COMMAND_TIMEOUT_SECONDS)
            except subprocess.TimeoutExpired as exc:
                partial_stdout, partial_stderr = _text_output(exc.output), _text_output(exc.stderr)
                try:
                    stdout, stderr = _bounded_timeout_cleanup(child)
                    cleanup_error = None
                except OSError as cleanup:
                    # The timeout is already an established execution fact.
                    # Preserve its process identity and surface cleanup failure
                    # rather than misclassifying it as a failed spawn.
                    stdout, stderr = _emergency_cleanup_after_timeout_failure(child, cleanup)
                    cleanup_error = str(cleanup)
                    if not stdout:
                        stdout = partial_stdout
                    if not stderr:
                        stderr = partial_stderr
                _settle(child)
                record = {"argv": argv, "exit": None, "stdout": stdout,
                          "stderr": stderr + str(exc), "state": "timeout",
                          "pid": child.pid, "pgid": child.pid}
                if cleanup_error is not None:
                    record.update(cleanup_error=cleanup_error, cleanup_operation="timeout_cleanup")
                return record
            escaped = _finish_lineage(child)
            _settle(child)
            result = subprocess.CompletedProcess(argv, child.returncode, stdout, stderr)
            process_identity = {"pid": child.pid, "pgid": child.pid}
            _require_ownership_release(child, "successful command completion")
            if escaped:
                # The lineage boundary killed descendants that outlived the
                # command; its output cannot be accepted as complete work.
                return {"argv": argv, "exit": child.returncode, "stdout": stdout or "",
                        "stderr": stderr or "", "state": "escaped_descendants",
                        "escaped_descendants": escaped, **process_identity}
        except (KeyboardInterrupt, SystemExit) as interruption:
            if (owner[0] is not None
                    and getattr(interruption, "_oxidex_owned_child_cleanup", None) is None):
                _cleanup_owned_child_after_interrupt(owner[0], interruption)
            raise
        except OwnedChildCleanupIncomplete:
            raise
        except OSError as exc:
            if owner[0] is None:
                return {"argv": argv, "exit": None, "stdout": "", "stderr": str(exc),
                        "state": "spawn_failed", "operation": "spawn"}
            child = owner[0]
            try:
                stdout, stderr = _bounded_timeout_cleanup(child)
                cleanup_error = None
            except OSError as cleanup:
                stdout, stderr = _emergency_cleanup_after_timeout_failure(child, cleanup)
                cleanup_error = str(cleanup)
            _settle(child)
            record = {"argv": argv, "exit": None, "stdout": stdout, "stderr": stderr + str(exc),
                      "state": "execution_failed", "operation": "post_spawn", "pid": child.pid, "pgid": child.pid}
            if cleanup_error is not None:
                record.update(cleanup_error=cleanup_error, cleanup_operation="post_spawn_cleanup")
            return record
    else:
        try:
            result = run(argv, cwd=str(cwd), env=env, text=True, capture_output=True, timeout=COMMAND_TIMEOUT_SECONDS,
                         start_new_session=True, close_fds=False)
            process_identity = {}
        except subprocess.TimeoutExpired as exc:
            return {"argv": argv, "exit": None, "stdout": "", "stderr": str(exc), "state": "timeout"}
        except OSError as exc:
            return {"argv": argv, "exit": None, "stdout": "", "stderr": str(exc),
                    "state": "execution_failed", "operation": "runner"}
    try:
        stdout, stderr = result.stdout or "", result.stderr or ""
        return {"argv": argv, "exit": result.returncode, "stdout": stdout, "stderr": stderr,
                "state": "ok" if result.returncode == 0 else "exit_failed", **process_identity}
    except OSError as exc:
        return {"argv": argv, "exit": None, "stdout": "", "stderr": str(exc),
                "state": "execution_failed", "operation": "result"}


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
    runner = _tracked_run if run is subprocess.run else run
    try:
        result = runner(["git", "-C", str(checkout), "rev-parse", "HEAD"], cwd=str(checkout), env=dict(os.environ),
                        text=True, capture_output=True, timeout=30, start_new_session=True, close_fds=False)
    except OwnedChildCleanupIncomplete:
        raise  # an unverified owned child is not an ordinary HEAD refusal
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise Refused("cannot verify owned checkout HEAD") from exc
    if result.returncode != 0 or (result.stdout or "").strip() != expected_commit:
        raise Refused("owned checkout HEAD differs from immutable execution source commit")


def _run_stage(run_dir: Path, journal: dict[str, Any], release: str, stage: str, checkout: Path, target: Path,
               native: tuple[Path, Path, Path], perl: str, native_probe: dict[str, Any], config: dict[str, Any],
               run: Callable[..., subprocess.CompletedProcess[str]],
               stage_guard: Callable[[str, str, str], None] | None = None) -> bool:
    # Journals initialized before the release-test stage existed lack its slot.
    state = journal["releases"][release]["stages"].setdefault(stage, "pending")
    if state == "passed" or state == "unsupported":
        return True
    if state != "pending":
        raise Refused(f"{release} {stage} is not safely runnable after interruption or failure")
    if stage in OPTIONAL_COMMANDS and stage not in config["commands"]:
        journal["releases"][release]["stages"][stage] = "unsupported"
        journal["releases"][release]["reports"][stage] = {"reason": f"no {stage} command configured"}
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
              "read_fixture_manifest": str(config["read_fixture_bindings"][release]["path"]),
              "write_fixture_manifest": str(config.get("write_fixture_bindings", {}).get(release, {}).get("path", "")),
              "source_commit": config["execution_source_commit"]}
    env = dict(os.environ, CARGO_TARGET_DIR=str(target), OXIDEX_REHEARSAL_RELEASE=release,
               OXIDEX_REHEARSAL_NATIVE_SOURCE=str(source), OXIDEX_REHEARSAL_NATIVE_LIB=str(lib),
               OXIDEX_REHEARSAL_NATIVE_PROGRAM=str(program), OXIDEX_REHEARSAL_NATIVE_PERL=perl,
               OXIDEX_REHEARSAL_NATIVE_PROBE=values["native_probe"], OXIDEX_REHEARSAL_REPORT=str(output),
               OXIDEX_REHEARSAL_NATIVE_PROBE_SHA256=values["native_probe_sha256"],
               OXIDEX_REHEARSAL_CHECKOUT=str(checkout), OXIDEX_REHEARSAL_SOURCE_COMMIT=config["execution_source_commit"],
               OXIDEX_REHEARSAL_READ_FIXTURE_MANIFEST=values["read_fixture_manifest"],
               OXIDEX_REHEARSAL_WRITE_FIXTURE_MANIFEST=values["write_fixture_manifest"])
    def started(pid: int, pgid: int) -> None:
        journal["active"]["child"] = {"pid": pid, "pgid": pgid}
        _store_journal(run_dir, journal)
    try:
        if stage_guard is not None:
            stage_guard(release, stage, "before")
        record = _run_record(_expand(config["commands"][stage], values), cwd=checkout, env=env, run=run, started=started)
        command_log = _command_log(run_dir, release, stage, record)
        if record["state"] != "ok":
            raise Refused(f"{stage} command {record['state']}")
        after_source = _source_tree(checkout)
        allowed = {".exiftool-version", *(item.path for item in artifacts.inventory(checkout))} if stage == "generate" else set()
        _verify_source_transition(before_source, after_source, allowed,
                                  split_tables=checkout if stage == "generate" else None)
        _verify_checkout_head(checkout, config["execution_source_commit"], run)
        result = _stage_result(output, release, stage,
                               native_probe.get("probe_sha256") if stage in {"read", "write"} else None,
                               checkout, config["execution_source_commit"], after_source, target, native, perl)
        if stage == "read":
            build_report = _read(run_dir / journal["releases"][release]["reports"]["build"]["path"])
            if result.get("binary") != build_report.get("binary"):
                raise Refused("read result did not use the proven build binary")
            fixture_binding = config["read_fixture_bindings"][release]
            fixture_report = result.get("fixtures", {})
            if (not isinstance(fixture_report, dict)
                    or fixture_report.get("manifest") != fixture_binding["path"]
                    or fixture_report.get("manifest_sha256") != fixture_binding["sha256"]
                    or _require_fixture_proof(result) != fixture_binding["fixtures"]):
                raise Refused("read result did not cover the exact immutable selected fixture scope")
        if stage == "write":
            build_report = _read(run_dir / journal["releases"][release]["reports"]["build"]["path"])
            if result.get("writer_binary") != build_report.get("writer_binary"):
                raise Refused("write result did not use the proven writer driver")
            fixture_binding = config["write_fixture_bindings"][release]
            fixture_report = result.get("fixtures", {})
            if (not isinstance(fixture_report, dict)
                    or fixture_report.get("manifest") != fixture_binding["path"]
                    or fixture_report.get("manifest_sha256") != fixture_binding["sha256"]
                    or _require_fixture_proof(result) != fixture_binding["fixtures"]):
                raise Refused("write result did not cover the exact immutable selected JPEG fixture scope")
        if stage_guard is not None:
            stage_guard(release, stage, "after")
    except OwnedChildCleanupIncomplete:
        _store_journal(run_dir, journal)
        raise
    except (Refused, OSError) as exc:
        journal["releases"][release]["stages"][stage] = "failed"
        journal["releases"][release]["state"] = "failed"
        failure = {"stage": stage, "detail": str(exc)}
        if "command_log" in locals():
            failure["command"] = command_log
        journal["releases"][release]["failure"] = failure
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
                archive_cache: Path, source_root: Path, run: Callable[..., subprocess.CompletedProcess[str]],
                stage_guard: Callable[[str, str, str], None] | None = None) -> dict[str, Any] | None:
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
    incomplete_cleanup: list[OwnedChildCleanupIncomplete] = []
    def native_run(argv: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        if run is not subprocess.run:
            return run(argv, **kwargs)
        if incomplete_cleanup:
            # A surviving child owns the journal's active identity: start
            # nothing else and never overwrite it.
            raise incomplete_cleanup[0]
        kwargs.pop("capture_output", None)
        def started(pid: int, pgid: int) -> None:
            journal["active"]["child"] = {"pid": pid, "pgid": pgid}
            _store_journal(run_dir, journal)
        try:
            return _tracked_run(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                start_new_session=True, close_fds=False, started=started, **kwargs)
        except OwnedChildCleanupIncomplete as incomplete:
            # The oracle folds runner OSErrors into per-command records, so
            # remember this one: the stage must keep its active child.
            incomplete_cleanup.append(incomplete)
            raise
    try:
        if stage_guard is not None:
            stage_guard(release, "native", "before")
        try:
            report = native_oracle.probe_materialized_native(
                materialization, plan, catalog, capture, resolution, archive_cache, source_root, release,
                config["perls"][release], config["native_cases"][release], run=native_run)
        except (KeyboardInterrupt, SystemExit):
            raise  # an interruption carries its own cleanup verdict
        except BaseException:
            if incomplete_cleanup:
                raise incomplete_cleanup[0]
            raise
        if incomplete_cleanup:
            raise incomplete_cleanup[0]
        native_oracle.write_probe_report(output, report)
        if report.get("state") != "ready" or not report.get("cases"):
            raise Refused("native oracle did not provide ready cases")
        if stage_guard is not None:
            stage_guard(release, "native", "after")
    except OwnedChildCleanupIncomplete:
        _store_journal(run_dir, journal)
        raise
    except (Refused, native_oracle.Refused, catalog_stage.Refused, rehearsal.Refused, OSError,
            subprocess.TimeoutExpired) as exc:
        journal["releases"][release]["stages"]["native"] = "failed"
        journal["releases"][release]["state"] = "failed"
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


class _HeldHostLock:
    """Borrow an owner-held flock without ever acquiring an arbitrary raw FD."""

    def __init__(self, path: Path, stream: Any):
        raise Refused("host lock capability must be issued by lock acquisition")

    @classmethod
    def acquire(cls, path: Path, stream: Any) -> "_HeldHostLock":
        """Issue a capability only after this stream acquires the configured flock."""
        path = path.absolute()
        if stream.closed or path.is_symlink() or not path.is_file():
            raise Refused("host lock is not an open configured regular file")
        try:
            held = os.fstat(stream.fileno())
            current = os.stat(path, follow_symlinks=False)
        except OSError as exc:
            raise Refused("host lock is not an open configured regular file") from exc
        if (not __import__("stat").S_ISREG(current.st_mode)
                or (held.st_dev, held.st_ino) != (current.st_dev, current.st_ino)):
            raise Refused("host lock stream differs from configured lease")
        fcntl.flock(stream.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        try:
            current = os.stat(path, follow_symlinks=False)
            if (path.is_symlink() or not __import__("stat").S_ISREG(current.st_mode)
                    or (held.st_dev, held.st_ino) != (current.st_dev, current.st_ino)):
                raise Refused("host lock changed during acquisition")
        except BaseException:
            fcntl.flock(stream.fileno(), fcntl.LOCK_UN)
            raise
        self = object.__new__(cls)
        self.path = path.absolute()
        self.stream = stream
        self.owner_pid = os.getpid()
        self.active = True
        self._borrow_lock = threading.Lock()
        return self

    @contextmanager
    def borrow(self, configured: Path):
        configured = configured.absolute()
        with self._borrow_lock:
            if not self.active or self.owner_pid != os.getpid() or self.stream.closed:
                raise Refused("external host lock is not held by a live owner")
            if configured.is_symlink() or not configured.is_file() or configured != self.path:
                raise Refused("external host lock differs from configured lease")
            try:
                held = os.fstat(self.stream.fileno())
                current = os.stat(configured, follow_symlinks=False)
            except OSError as exc:
                raise Refused("external host lock is not held by a live owner") from exc
            if (not __import__("stat").S_ISREG(current.st_mode)
                    or (held.st_dev, held.st_ino) != (current.st_dev, current.st_ino)):
                raise Refused("external host lock differs from configured lease")
            yield

    def deactivate(self) -> None:
        # An owner cannot release its flock until every in-process borrower
        # has left the critical section.
        with self._borrow_lock:
            self.active = False


class _HostLock:
    def __init__(self, path: Path):
        self.path, self.file, self.capability = path, None, None
    def __enter__(self):
        if self.path.is_symlink():
            raise Refused("version rehearsal host lock must not be a symbolic link")
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.file = self.path.open("a+")
        os.set_inheritable(self.file.fileno(), True)
        try:
            self.capability = _HeldHostLock.acquire(self.path, self.file)
        except BlockingIOError as exc:
            self.file.close()
            raise Refused("version rehearsal host lock is already held") from exc
        except BaseException:
            self.file.close()
            raise
        return self
    def __exit__(self, *_):
        if self.file:
            stream, self.file = self.file, None
            survivors = release_or_retain(stream, self.capability)
            if survivors:
                raise LockRetained("version rehearsal host lock "
                                   + retained_lock_message(self.path, survivors), survivors)
            stream.close()


def _external_host_lock(config: Mapping[str, Any], descriptor: _HeldHostLock | None):
    if descriptor is None:
        return _HostLock(Path(config["host_lock"]))
    if not isinstance(descriptor, _HeldHostLock):
        raise Refused("external host lock is not held by a live owner")
    return descriptor.borrow(Path(config["host_lock"]))


def execute(run_dir: Path, repository: Path, archive_cache: Path, source_root: Path, *,
            run: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run,
            checkout: Callable[[Path, str, Path, Callable[..., subprocess.CompletedProcess[str]]], Path] = _default_checkout,
            host_lock_fd: _HeldHostLock | None = None,
            stage_guard: Callable[[str, str, str], None] | None = None) -> dict[str, Any]:
    """Run each selected release once. Failed or interrupted stages are never retried."""
    journal, docs, config = _load_journal(run_dir, archive_cache, source_root)
    lock = _external_host_lock(config, host_lock_fd)
    with lock:
        if journal["phase"] == "running":
            raise Refused("execution is interrupted; recover it before any later run")
        if journal["phase"] in {"failed", "interrupted", "complete"}:
            raise Refused("execution journal is terminal and cannot re-run selected releases")
        plan, materialization = docs[2], docs[4]
        repository = repository.resolve()
        if repository.is_symlink() or not (repository / ".git").exists():
            raise Refused("repository must be an existing Git checkout")
        for release in journal["releases"]:
            checkout_path = run_dir / "checkouts" / _safe_name(release)
            configured_targets = config.get("target_directories")
            target = (Path(configured_targets[release]) if configured_targets is not None
                      else run_dir / "targets" / _safe_name(release))
            try:
                journal["phase"], journal["active"] = "running", {"release": release, "stage": "checkout"}
                _event(journal, "checkout_started", release=release)
                _store_journal(run_dir, journal)
                if stage_guard is not None:
                    stage_guard(release, "checkout", "before")
                owned = checkout(repository, config["execution_source_commit"], checkout_path, run)
                if owned.resolve() != checkout_path.resolve() or owned.is_symlink() or not owned.is_dir():
                    raise Refused("checkout provider did not return the owned release checkout")
                _verify_checkout_head(owned, config["execution_source_commit"], run)
                journal["releases"][release]["source_tree"] = _source_tree(owned)
                if target.exists() or target.is_symlink():
                    raise Refused("execution target already exists; stale target reuse is forbidden")
                target.mkdir(parents=True)
                if stage_guard is not None:
                    stage_guard(release, "checkout", "after")
                journal["active"] = None
                _event(journal, "checkout_completed", release=release)
                _store_journal(run_dir, journal)
                native = _run_native(run_dir, journal, release, docs, config, archive_cache, source_root, run,
                                     stage_guard)
                if native is None: return journal
                for stage in ("generate", "build", "test", "read", "write"):
                    if not _run_stage(run_dir, journal, release, stage, owned, target, _native_identity(materialization, source_root, release),
                                      config["perls"][release], native, config, run, stage_guard): return journal
                statuses = journal["releases"][release]["stages"]
                journal["releases"][release]["state"] = "passed_with_write_gap" if statuses["write"] == "unsupported" else "passed"
                _event(journal, "release_completed", release=release, state=journal["releases"][release]["state"])
                _store_journal(run_dir, journal)
            except OwnedChildCleanupIncomplete:
                _store_journal(run_dir, journal)
                raise
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
        journal["scope"]["release_tests"] = "unsupported_for_one_or_more_releases" if any(
            row["stages"].get("test") != "passed" for row in journal["releases"].values()) else "passed_per_release"
        journal["scope"]["parity"] = "unproven_without_all_per-release_read_and_write_acceptance" if any(
            row["stages"]["write"] != "passed" for row in journal["releases"].values()) else "per-version-read-write-rehearsed; no-promotion"
        _event(journal, "execution_complete", promotion="forbidden")
        _store_journal(run_dir, journal)
        return journal


def recover(run_dir: Path, archive_cache: Path, source_root: Path, *,
            host_lock_fd: _HeldHostLock | None = None) -> dict[str, Any]:
    """Record interruption without guessing whether an active command completed."""
    journal, _, config = _load_journal(
        run_dir, archive_cache, source_root, allow_legacy_recovery=True,
    )
    with _external_host_lock(config, host_lock_fd):
        if journal.get("phase") != "running":
            raise Refused("only a running execution can be recovered as interrupted")
        if journal.get("active") is None:
            # Interrupted between stages (after checkout_completed or
            # stage_passed, before the next stage started): nothing was in
            # flight and no child was spawned, so only the phase changes.
            journal["phase"] = "interrupted"
            _event(journal, "interrupted_between_stages")
            _store_journal(run_dir, journal)
            return journal
        if not isinstance(journal.get("active"), dict):
            raise Refused("running execution journal has a malformed active stage")
        active = journal["active"]
        child = active.get("child")
        if isinstance(child, dict):
            if type(child.get("pid")) is int and child["pid"] > 0:
                try:
                    process_live = _pid_live(child["pid"])
                except PermissionError:
                    raise Refused("active child process cannot be inspected; refusing interruption recovery")
                if process_live:
                    raise Refused("active child process is still live; refusing interruption recovery")
            if type(child.get("pgid")) is int and child["pgid"] > 0:
                try:
                    group_live = _group_live(child["pgid"])
                except PermissionError:
                    raise Refused("active child process group cannot be inspected; refusing interruption recovery")
                if group_live:
                    raise Refused("active child process group is still live; refusing interruption recovery")
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
