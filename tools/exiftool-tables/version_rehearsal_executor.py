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
import shutil
import subprocess
import sys
import tempfile
import time
from typing import Any, Callable, Iterator, Mapping

import version_rehearsal as rehearsal
import version_rehearsal_catalog as catalog_stage
import version_rehearsal_native_oracle as native_oracle

SCHEMA = 1
KIND = "oxidex_exiftool_version_rehearsal_execution"
RESULT_KIND = "oxidex_version_rehearsal_stage_result"
STAGES = ("native", "generate", "build", "read", "write")
REQUIRED_COMMANDS = ("generate", "build", "read")
_SAFE_RELEASE = __import__("re").compile(r"^[0-9]+\.[0-9]+$")
_PLACEHOLDERS = {
    "release", "checkout", "target", "report", "native_source", "native_lib",
    "native_program", "native_perl", "native_probe",
}


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
    return value


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
    _config(config, releases)
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
            or journal.get("config_sha256") != _sha_json(config) or journal.get("promotion") != "forbidden"):
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


def _stage_result(path: Path, release: str, stage: str, native_probe_sha: str | None) -> dict[str, Any]:
    result = _read(_regular(path, f"{stage} result"))
    if (result.get("schema") != SCHEMA or result.get("kind") != RESULT_KIND or result.get("stage") != stage
            or result.get("release") != release or result.get("state") != "passed"
            or not isinstance(result.get("denominator"), int) or result["denominator"] < 1):
        raise Refused(f"{stage} result lacks a passed state or positive denominator")
    if native_probe_sha is not None and (result.get("native_release") != release
                                         or result.get("native_probe_sha256") != native_probe_sha):
        raise Refused(f"{stage} result is not bound to this release's native oracle")
    if native_probe_sha is not None:
        comparison = result.get("comparison")
        if (not isinstance(comparison, dict) or comparison.get("kind") != "oxidex_vs_native"
                or comparison.get("native_release") != release
                or not isinstance(comparison.get("matched"), int) or comparison["matched"] < 0
                or not isinstance(comparison.get("mismatched"), int) or comparison["mismatched"] < 0
                or comparison["matched"] + comparison["mismatched"] != result["denominator"]):
            raise Refused(f"{stage} result lacks attributable OxiDex/native outcomes")
    return result


def _run_record(argv: list[str], *, cwd: Path, env: dict[str, str], run: Callable[..., subprocess.CompletedProcess[str]]) -> dict[str, Any]:
    try:
        result = run(argv, cwd=str(cwd), env=env, text=True, capture_output=True, timeout=3600)
        stdout, stderr = result.stdout or "", result.stderr or ""
        return {"argv": argv, "exit": result.returncode, "stdout_sha256": hashlib.sha256(stdout.encode()).hexdigest(),
                "stderr_sha256": hashlib.sha256(stderr.encode()).hexdigest(), "state": "ok" if result.returncode == 0 else "exit_failed"}
    except subprocess.TimeoutExpired as exc:
        return {"argv": argv, "exit": None, "state": "timeout", "detail": str(exc)}
    except OSError as exc:
        return {"argv": argv, "exit": None, "state": "spawn_failed", "detail": str(exc)}


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
    values = {"release": release, "checkout": str(checkout), "target": str(target), "report": str(output),
              "native_source": str(source), "native_lib": str(lib), "native_program": str(program),
              "native_perl": perl, "native_probe": str(_result_path(run_dir, release, "native"))}
    env = dict(os.environ, CARGO_TARGET_DIR=str(target), OXIDEX_REHEARSAL_RELEASE=release,
               OXIDEX_REHEARSAL_NATIVE_SOURCE=str(source), OXIDEX_REHEARSAL_NATIVE_LIB=str(lib),
               OXIDEX_REHEARSAL_NATIVE_PROGRAM=str(program), OXIDEX_REHEARSAL_NATIVE_PERL=perl,
               OXIDEX_REHEARSAL_NATIVE_PROBE=values["native_probe"], OXIDEX_REHEARSAL_REPORT=str(output))
    record = _run_record(_expand(config["commands"][stage], values), cwd=checkout, env=env, run=run)
    try:
        if record["state"] != "ok":
            raise Refused(f"{stage} command {record['state']}")
        result = _stage_result(output, release, stage,
                               native_probe.get("probe_sha256") if stage in {"read", "write"} else None)
    except Refused as exc:
        journal["releases"][release]["stages"][stage] = "failed"
        journal["releases"][release]["failure"] = {"stage": stage, "detail": str(exc), "command": record}
        journal["phase"], journal["active"] = "failed", None
        _event(journal, "stage_failed", release=release, stage=stage, detail=str(exc))
        _store_journal(run_dir, journal)
        return False
    journal["releases"][release]["stages"][stage] = "passed"
    journal["releases"][release]["reports"][stage] = {"path": str(output.relative_to(run_dir)), "sha256": _sha_json(result), "denominator": result["denominator"], "command": record}
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
    try:
        report = native_oracle.probe_materialized_native(materialization, plan, catalog, capture, resolution,
                                                         archive_cache, source_root, release, config["perls"][release],
                                                         config["native_cases"][release], run=run)
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
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.file = self.path.open("a+")
        try: fcntl.flock(self.file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc: self.file.close(); raise Refused("version rehearsal host lock is already held") from exc
        return self
    def __exit__(self, *_):
        if self.file: fcntl.flock(self.file.fileno(), fcntl.LOCK_UN); self.file.close()


def execute(run_dir: Path, repository: Path, archive_cache: Path, source_root: Path, *,
            run: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run,
            checkout: Callable[[Path, str, Path, Callable[..., subprocess.CompletedProcess[str]]], Path] = _default_checkout) -> dict[str, Any]:
    """Run each selected release once. Failed or interrupted stages are never retried."""
    with _HostLock(run_dir.parent / ".oxidex-version-rehearsal.lock"):
        journal, docs, config = _load_journal(run_dir, archive_cache, source_root)
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
                owned = checkout(repository, plan["repository_commit"], checkout_path, run)
                if owned.resolve() != checkout_path.resolve() or owned.is_symlink() or not owned.is_dir():
                    raise Refused("checkout provider did not return the owned release checkout")
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
                    journal["phase"], journal["active"] = "failed", None
                    journal["releases"][release]["failure"] = {"stage": journal.get("active", {}).get("stage", "checkout"), "detail": str(exc)}
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
    with _HostLock(run_dir.parent / ".oxidex-version-rehearsal.lock"):
        journal, _, _ = _load_journal(run_dir, archive_cache, source_root)
        if journal.get("phase") != "running" or not isinstance(journal.get("active"), dict):
            raise Refused("only a running execution can be recovered as interrupted")
        active = journal["active"]
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
        return 0
    except (Refused, rehearsal.Refused, catalog_stage.Refused, native_oracle.Refused, OSError) as exc:
        print(f"version rehearsal execution refused: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
