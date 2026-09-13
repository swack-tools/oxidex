#!/usr/bin/env python3
"""Probe one materialized ExifTool release without grading OxiDex.

This is the native-oracle capability/identity stage for a future version
rehearsal runner.  It does not regenerate, build, compare, or promote.  Every
native invocation has an explicit Perl, ``-I<materialized-lib>``, and program
path; it never resolves a bare ``exiftool``.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from typing import Any, Callable

import version_rehearsal as rehearsal
import version_rehearsal_catalog as catalog_stage

SCHEMA = 1
KIND = "oxidex_exiftool_version_rehearsal_native_capability"


class Refused(ValueError):
    pass


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _regular(path: Path, label: str) -> Path:
    path = path.resolve()
    if path.is_symlink() or not path.is_file():
        raise Refused(f"{label} must be an existing regular file")
    return path


def _argv_strings(value: Any, label: str) -> list[str]:
    if not isinstance(value, list) or any(not isinstance(arg, str) or not arg or "\x00" in arg for arg in value):
        raise Refused(f"{label} arguments must be nonempty strings")
    # Fixture paths are supplied only by this module after a private copy.  A
    # case must not smuggle a second input or write outside its disposable copy.
    if any(Path(arg).is_absolute() for arg in value):
        raise Refused(f"{label} arguments must not contain filesystem paths")
    return value


def _case(case: Any) -> dict[str, Any]:
    if not isinstance(case, dict) or not isinstance(case.get("name"), str) or not case["name"]:
        raise Refused("capability case requires a nonempty name")
    fixture = case.get("fixture")
    if not isinstance(fixture, (str, Path)):
        raise Refused(f"{case['name']}: fixture is required")
    result = {"name": case["name"], "fixture": Path(fixture)}
    for phase in ("read", "write"):
        spec = case.get(phase)
        if not isinstance(spec, dict) or spec.get("expectation") not in {"success", "native_unsupported"}:
            raise Refused(f"{case['name']}: {phase} requires success or native_unsupported expectation")
        result[phase] = {"args": _argv_strings(spec.get("args"), f"{case['name']} {phase}"), "expectation": spec["expectation"]}
    return result


def _run(argv: list[str], run: Callable[..., subprocess.CompletedProcess[str]]) -> dict[str, Any]:
    completed = run(argv, capture_output=True, text=True, errors="replace")
    stdout, stderr = completed.stdout or "", completed.stderr or ""
    return {
        "command": argv,
        "exit": completed.returncode,
        "stdout": stdout,
        "stderr": stderr,
        "stdout_sha256": hashlib.sha256(stdout.encode()).hexdigest(),
        "stderr_sha256": hashlib.sha256(stderr.encode()).hexdigest(),
    }


def _capability(perl: Path, run: Callable[..., subprocess.CompletedProcess[str]]) -> dict[str, Any]:
    # Share the existing oracle's deliberately meaningful module requirement,
    # but do not call resolve(): this stage grades a selected historical tree,
    # not this checkout's current pin.
    required = ("Archive::Zip",)
    modules = []
    for module in required:
        result = _run([str(perl), f"-M{module}", "-e", "1"], run)
        modules.append({"module": module, **result})
    return {"required_modules": list(required), "modules": modules,
            "available": all(row["exit"] == 0 for row in modules)}


def write_probe_report(path: Path, report: dict[str, Any]) -> None:
    """Persist one immutable probe result; never overwrite a prior attempt."""
    if path.exists() or path.is_symlink():
        raise Refused("native capability output already exists")
    payload = {key: value for key, value in report.items() if key != "probe_sha256"}
    if report.get("probe_sha256") != catalog_stage.sha256_json(payload):
        raise Refused("native capability report identity is malformed")
    rehearsal.atomic_json(path, report)


def probe_materialized_native(
    materialization: dict[str, Any], plan: dict[str, Any], catalog: dict[str, Any], capture: dict[str, Any],
    resolution: dict[str, Any], archive_cache: Path, source_root: Path, release: str, perl: str | Path,
    cases: list[dict[str, Any]], *, run: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run,
) -> dict[str, Any]:
    """Return immutable invocation evidence for one selected native source.

    The caller supplies deliberately small read/write capability cases.  Write
    probes operate only on a temporary copy of each fixture.  A native format
    that the caller declares unsupported is a valid observed result; a missing
    Perl capability is distinct and leaves every capability case failed.
    """
    catalog_stage.verify_source_materialization(
        materialization, plan, catalog, capture, resolution, archive_cache, source_root
    )
    if not isinstance(release, str):
        raise Refused("release must be a selected release string")
    row = next((item for item in materialization["selected_releases"] if item["release"] == release), None)
    if row is None:
        raise Refused("native capability release is not materialized by this plan")
    expected_version = release
    source = (source_root / row["source_directory"]).resolve()
    if source.is_symlink() or not source.is_dir():
        raise Refused("verified materialized source directory is unavailable")
    perl_path = _regular(Path(perl), "Perl interpreter")
    program = _regular(source / "exiftool", "materialized ExifTool program")
    lib = (source / "lib").resolve()
    if lib.is_symlink() or not lib.is_dir():
        raise Refused("materialized ExifTool lib directory is unavailable")
    parsed_cases = [_case(case) for case in cases]
    if not parsed_cases or len({case["name"] for case in parsed_cases}) != len(parsed_cases):
        raise Refused("at least one uniquely named capability case is required")
    prefix = [str(perl_path), f"-I{lib}", str(program)]
    version = _run([*prefix, "-ver"], run)
    capability = _capability(perl_path, run)
    identity = {
        "release": release, "expected_version": expected_version,
        "materialization_sha256": materialization["materialization_sha256"],
        "source_directory": row["source_directory"], "source_tree_sha256": row["tree"]["tree_sha256"],
        "perl": {"path": str(perl_path), "sha256": _sha256(perl_path)},
        "lib": {"path": str(lib)},
        "program": {"path": str(program), "sha256": _sha256(program)},
    }
    state = "ready" if version["exit"] == 0 and version["stdout"].strip() == expected_version and capability["available"] else "failed"
    report_cases: list[dict[str, Any]] = []
    if state == "ready":
      with tempfile.TemporaryDirectory(prefix="oxidex-native-capability-") as work:
          workspace = Path(work)
          for case in parsed_cases:
              fixture = _regular(case["fixture"], f"{case['name']} fixture")
              private = workspace / f"{case['name']}-{fixture.name}"
              shutil.copyfile(fixture, private)
              record: dict[str, Any] = {"name": case["name"], "fixture": {"path": str(fixture.resolve()), "sha256": _sha256(fixture), "bytes": fixture.stat().st_size}, "disposable_copy_sha256_before": _sha256(private)}
              for phase in ("read", "write"):
                  invocation = _run([*prefix, *case[phase]["args"], str(private)], run)
                  observed = "success" if invocation["exit"] == 0 else "failed"
                  if case[phase]["expectation"] == "native_unsupported" and invocation["exit"] == 0:
                      observed = "native_unsupported"
                  invocation.update({"expectation": case[phase]["expectation"], "observed": observed})
                  record[phase] = invocation
                  if observed == "failed":
                      state = "failed"
              record["disposable_copy_sha256_after"] = _sha256(private)
              report_cases.append(record)
    payload = {"schema": SCHEMA, "kind": KIND, "identity": identity, "version": version,
               "perl_capability": capability, "cases": report_cases,
               "state": state,
               "execution": {"native_read": "probed" if state == "ready" else "failed", "native_write": "probed" if state == "ready" else "failed", "conformance": "unrun", "limit": "capability evidence is not OxiDex/native conformance"}}
    return {**payload, "probe_sha256": catalog_stage.sha256_json(payload)}
