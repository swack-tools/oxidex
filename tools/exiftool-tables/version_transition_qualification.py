#!/usr/bin/env python3
"""Non-promoting entry point for reversible ExifTool transition qualification.

The checked matrix contains resolvers, not invented historical receipts.  A
row becomes runnable only after its catalog, source-resolution,
materialization, fixture, and native-case inputs exist and pass the existing
rehearsal verifiers.  Every generated checkout and Cargo target is isolated;
the caller is inspected before and after every side and is never promoted.
"""
from __future__ import annotations

import argparse
import copy
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import threading
import time
from typing import Any, Callable, Mapping

HERE = Path(__file__).resolve().parent
REPOSITORY_ROOT = HERE.parents[1]
SCRIPTS = REPOSITORY_ROOT / "scripts"
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import artifacts
import ops_paths
import version_rehearsal as rehearsal
import version_rehearsal_catalog as catalog_stage
import version_rehearsal_executor as executor
import version_rehearsal_native_oracle as native_oracle
import version_rehearsal_stage_adapter as stage_adapter

SCHEMA = 1
KIND = "oxidex_exiftool_version_transition_matrix"
RESULT_KIND = "oxidex_exiftool_version_transition_qualification"
RELEASE = re.compile(r"^[0-9]+\.[0-9]+$")
RUN_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$")
EXPECTED_PERL_VERSION = "v5.38.2"
EXPECTED_PERL_SHA256 = "e78cfd5a061c7e0f4ee7d0cc40a8878186d9bd321f930609ad5bdaec78410959"
FIXED_SOURCE_COMMITS = {
    "11.78": "ca8685788f5763c547349f239764bd19cf1952da",
    "12.64": "d35e9e26e0a8b443dae307f55d0a4a067d311a16",
}
INPUT_NAMES = ("capture", "catalog", "plan", "resolution", "materialization")
SIDES = ("before", "after")


class Refused(ValueError):
    """The requested qualification cannot produce attributable evidence."""


class OutcomeUnknown(Refused):
    """A final marker exists, but publication could not be confirmed or refused."""


def _sha_file(file_path: Path) -> str:
    digest = hashlib.sha256()
    with file_path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _atomic_json(file_path: Path, value: Mapping[str, Any]) -> None:
    if file_path.exists() or file_path.is_symlink():
        raise Refused(f"receipt already exists: {file_path}")
    file_path.parent.mkdir(parents=True, exist_ok=True)
    temporary = file_path.with_name(f".{file_path.name}.{os.getpid()}.tmp")
    try:
        with temporary.open("x", encoding="utf-8") as stream:
            json.dump(value, stream, indent=2, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, file_path)
    finally:
        temporary.unlink(missing_ok=True)


def _append_jsonl(file_path: Path, value: Mapping[str, Any]) -> None:
    if file_path.is_symlink():
        raise Refused(f"JSONL receipt must not be a symbolic link: {file_path}")
    file_path.parent.mkdir(parents=True, exist_ok=True)
    with file_path.open("a", encoding="utf-8") as stream:
        stream.write(json.dumps(value, sort_keys=True) + "\n")
        stream.flush()
        os.fsync(stream.fileno())


def _read_object(file_path: Path, label: str) -> dict[str, Any]:
    if file_path.is_symlink() or not file_path.is_file():
        raise Refused(f"{label} must be an existing regular file: {file_path}")
    try:
        value = json.loads(file_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise Refused(f"{label} is not readable JSON: {file_path}") from exc
    if not isinstance(value, dict):
        raise Refused(f"{label} must contain a JSON object")
    return value


def _read_array(file_path: Path, label: str) -> list[Any]:
    if file_path.is_symlink() or not file_path.is_file():
        raise Refused(f"{label} must be an existing regular file: {file_path}")
    try:
        value = json.loads(file_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise Refused(f"{label} is not readable JSON: {file_path}") from exc
    if not isinstance(value, list) or not value:
        raise Refused(f"{label} must contain a nonempty JSON array")
    return value


def _format_strings(value: Any, replacements: Mapping[str, str]) -> Any:
    if isinstance(value, str):
        class KeepUnknown(dict[str, str]):
            def __missing__(self, key: str) -> str:
                return "{" + key + "}"
        try:
            return value.format_map(KeepUnknown(replacements))
        except ValueError as exc:
            raise Refused(f"matrix contains an unsupported template: {value}") from exc
    if isinstance(value, list):
        return [_format_strings(item, replacements) for item in value]
    if isinstance(value, dict):
        return {key: _format_strings(item, replacements) for key, item in value.items()}
    return value


def load_matrix(matrix_path: Path, pinned_version: str) -> dict[str, Any]:
    """Load the checked shape and resolve only the repository pin token."""
    if RELEASE.fullmatch(pinned_version) is None:
        raise Refused("the caller pin is not a numeric ExifTool release")
    raw = _read_object(matrix_path, "transition matrix")
    value = _format_strings(raw, {"pinned_version": pinned_version})
    if (value.get("schema") != SCHEMA or value.get("kind") != KIND
            or value.get("source_identity_contract") != "verified-catalog-resolution-materialization"
            or not isinstance(value.get("rows"), list) or len(value["rows"]) != 3):
        raise Refused("transition matrix schema is unsupported")
    expected = [f"same-pin-{pinned_version}", "11.78-to-12.64", "12.64-to-11.78"]
    if [row.get("id") if isinstance(row, dict) else None for row in value["rows"]] != expected:
        raise Refused("transition matrix must contain the exact ordered Task19 rows")
    target_templates: list[str] = []
    output_templates: list[str] = []
    contracts = {
        f"same-pin-{pinned_version}": (pinned_version, pinned_version, "identical"),
        "11.78-to-12.64": ("11.78", "12.64", "manifest-delta"),
        "12.64-to-11.78": ("12.64", "11.78", "manifest-delta-with-removals"),
    }
    for row in value["rows"]:
        _validate_row(row, contracts[row["id"]])
        target_templates.append(row["target_directory"])
        output_templates.append(row["durable_output_directory"])
    if len(set(target_templates)) != 3 or len(set(output_templates)) != 3:
        raise Refused("every transition row requires distinct target and durable output directories")
    return value


def _validate_row(row: Mapping[str, Any], contract: tuple[str, str, str]) -> None:
    before, after = row.get("before_version"), row.get("after_version")
    if not isinstance(before, str) or not isinstance(after, str) or RELEASE.fullmatch(before) is None or RELEASE.fullmatch(after) is None:
        raise Refused("matrix row versions are malformed")
    if (before, after) != contract[:2]:
        raise Refused("matrix row label or release pair differs from the Task19 contract")
    identities, fixtures = row.get("immutable_source_identities"), row.get("fixtures")
    if not isinstance(identities, dict) or set(identities) != set(SIDES):
        raise Refused("matrix row must resolve both immutable source identities")
    if not isinstance(fixtures, dict) or set(fixtures) != set(SIDES):
        raise Refused("matrix row must bind read, write, and native fixtures on both sides")
    for side, release in zip(SIDES, (before, after), strict=True):
        identity = identities[side]
        if (not isinstance(identity, dict) or identity.get("expected_release") != release
                or identity.get("resolver") != "verified-input-bundle"
                or not isinstance(identity.get("input_bundle"), str)):
            raise Refused("matrix source identity resolver is incomplete")
        if (row.get("id") in {"11.78-to-12.64", "12.64-to-11.78"}
                and identity.get("expected_peeled_commit") != FIXED_SOURCE_COMMITS[release]):
            raise Refused("matrix fixed source identity differs from the checked Task19 contract")
        fixture = fixtures[side]
        if (not isinstance(fixture, dict)
                or set(fixture) != {"read_manifest", "write_manifest", "native_cases"}
                or any(not isinstance(fixture[name], str) for name in fixture)):
            raise Refused("matrix fixture configuration is incomplete")
    artifact = row.get("artifact_manifest")
    if (not isinstance(artifact, dict) or artifact.get("resolver") != "live-generated-inventory"
            or artifact.get("source") != "tools/exiftool-tables/artifacts.py"
            or artifact.get("comparison") not in {"identical", "manifest-delta", "manifest-delta-with-removals"}):
        raise Refused("matrix artifact-manifest contract is incomplete")
    if (before, after, artifact["comparison"]) != contract:
        raise Refused("matrix row label, release pair, or comparison policy differs from the Task19 contract")
    if (row.get("fresh_generation") != "both-sides" or row.get("native_read") != "mandatory"
            or row.get("native_write_readback") != "mandatory" or row.get("promotion") != "forbidden"
            or not isinstance(row.get("target_directory"), str)
            or not isinstance(row.get("durable_output_directory"), str)):
        raise Refused("matrix row weakens mandatory transition controls")


def materialize_matrix(matrix: dict[str, Any], *, output_root: Path, target_root: Path,
                       run_id: str) -> dict[str, Any]:
    if RUN_ID.fullmatch(run_id) is None:
        raise Refused("run ID is malformed")
    output_root = ops_paths.durable_root(output_root, "qualification output")
    target_root = ops_paths.durable_root(target_root, "OXIDEX_TARGET_ROOT")
    value = _format_strings(matrix, {
        "output_root": str(output_root), "target_root": str(target_root), "run_id": run_id,
    })
    targets, outputs = [], []
    for row in value["rows"]:
        target = Path(row["target_directory"])
        output = Path(row["durable_output_directory"])
        if not target.is_absolute() or not output.is_absolute():
            raise Refused("materialized target and output directories must be absolute")
        if not target.resolve().is_relative_to(target_root) or not output.resolve().is_relative_to(output_root):
            raise Refused("materialized row paths escape their configured durable roots")
        targets.append(target.resolve())
        outputs.append(output.resolve())
    if len(set(targets)) != 3 or len(set(outputs)) != 3:
        raise Refused("materialized row paths are not isolated")
    return value


def _git(repository: Path, arguments: list[str], *,
         run: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run) -> str:
    result = run(["git", "-C", str(repository), *arguments], cwd=str(repository),
                 text=True, capture_output=True, timeout=30)
    if result.returncode != 0:
        raise Refused(f"cannot inspect caller with git {' '.join(arguments)}")
    return result.stdout


def snapshot_caller(repository: Path, *,
                    run: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run) -> dict[str, Any]:
    repository = repository.resolve()
    if repository.is_symlink() or not (repository / ".git").exists():
        raise Refused("caller repository must be an existing non-symlink Git checkout")
    pin = repository / ".exiftool-version"
    if pin.is_symlink() or not pin.is_file():
        raise Refused("caller pin must be a regular file")
    raw_status = _git(repository, ["status", "--porcelain=v1", "--untracked-files=all"], run=run)
    if raw_status:
        raise Refused("caller repository must be clean before transition qualification")
    rows = []
    for item in artifacts.inventory(repository):
        generated = repository / item.path
        if generated.is_symlink() or not generated.is_file():
            raise Refused(f"caller generated artifact is unavailable: {item.path}")
        rows.append({"path": item.path, "sha256": _sha_file(generated), "bytes": generated.stat().st_size})
    return {
        "repository": str(repository),
        "head": _git(repository, ["rev-parse", "HEAD"], run=run).strip(),
        "branch": _git(repository, ["branch", "--show-current"], run=run).strip(),
        "index_tree": _git(repository, ["write-tree"], run=run).strip(),
        "pin_hex": pin.read_bytes().hex(),
        "pin_version": pin.read_text(encoding="utf-8").strip(),
        "artifacts": rows,
        "status": "clean",
    }


def verify_caller(snapshot: Mapping[str, Any], *,
                  run: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run) -> None:
    repository = Path(str(snapshot["repository"]))
    current = snapshot_caller(repository, run=run)
    if current != snapshot:
        raise Refused("caller HEAD, branch, index, pin, artifacts, or cleanliness changed during qualification")
    if _git(repository, ["diff", "--exit-code"], run=run) != "":
        raise Refused("caller tracked diff is not empty after qualification")


def _bundle_documents(bundle: Path) -> tuple[dict[str, Any], ...]:
    if bundle.is_symlink() or not bundle.is_dir():
        raise Refused(f"verified input bundle is absent: {bundle}")
    return tuple(_read_object(bundle / f"{name}.json", f"{name} input") for name in INPUT_NAMES)


def resolve_source_identity(identity: Mapping[str, Any], bundle: Path) -> dict[str, Any]:
    """Use the existing catalog/materialization chain to resolve real hashes."""
    capture, catalog, plan, resolution, materialization = _bundle_documents(bundle)
    catalog_stage.verify_capture_binding(capture, catalog)
    rehearsal.verify_plan(plan, catalog)
    catalog_stage.verify_source_resolution(resolution, plan, catalog, capture)
    locations = _read_object(bundle / "locations.json", "verified input locations")
    if (locations.get("schema") != 1
            or locations.get("kind") != "oxidex_version_transition_input_locations"
            or not isinstance(locations.get("archive_cache"), str)
            or not isinstance(locations.get("source_root"), str)):
        raise Refused("verified input locations are incomplete")
    archive_cache = Path(locations["archive_cache"])
    source_root = Path(locations["source_root"])
    if not archive_cache.is_absolute() or not source_root.is_absolute():
        raise Refused("verified archive and source roots must be absolute")
    catalog_stage.verify_source_materialization(
        materialization, plan, catalog, capture, resolution, archive_cache, source_root,
    )
    release = identity.get("expected_release")
    row = next((item for item in materialization["selected_releases"]
                if isinstance(item, dict) and item.get("release") == release), None)
    plan_side = next((side for pair in plan["pairs"] for side in (pair["old"], pair["new"])
                      if side.get("release") == release), None)
    if not isinstance(row, dict) or not isinstance(plan_side, dict):
        raise Refused(f"verified input bundle does not select release {release}")
    expected_commit = identity.get("expected_peeled_commit")
    if expected_commit is not None and plan_side.get("peeled_commit") != expected_commit:
        raise Refused(f"verified source identity for {release} differs from the checked expectation")
    source_directory = row.get("source_directory")
    tree = row.get("tree")
    if (not isinstance(source_directory, str) or not isinstance(tree, dict)
            or not isinstance(tree.get("tree_sha256"), str)):
        raise Refused("materialized source identity is incomplete")
    return {
        "release": release,
        "tag_object": plan_side.get("tag_object"),
        "peeled_commit": plan_side.get("peeled_commit"),
        "source_directory": source_directory,
        "source_tree_sha256": tree["tree_sha256"],
        "materialization_sha256": materialization.get("materialization_sha256"),
        "bundle": str(bundle),
        "archive_cache": str(archive_cache),
        "source_root": str(source_root),
        "documents": dict(zip(INPUT_NAMES, (capture, catalog, plan, resolution, materialization), strict=True)),
    }


def _file_binding(file_path: Path, label: str) -> dict[str, Any]:
    if file_path.is_symlink() or not file_path.is_file():
        raise Refused(f"{label} must be an existing regular file: {file_path}")
    resolved = file_path.resolve()
    return {"path": str(resolved), "sha256": _sha_file(resolved), "bytes": resolved.stat().st_size}


def _native_fixture_bindings(cases: list[Any]) -> list[dict[str, Any]]:
    bindings: list[dict[str, Any]] = []
    names: set[str] = set()
    for case in cases:
        try:
            parsed = native_oracle._case(case)
        except native_oracle.Refused as exc:
            raise Refused(f"native case is invalid: {exc}") from exc
        if parsed["name"] in names:
            raise Refused("native case names must be unique")
        names.add(parsed["name"])
        bindings.append({
            "name": parsed["name"],
            **_file_binding(parsed["fixture"], f"{parsed['name']} native fixture"),
        })
    if not bindings:
        raise Refused("at least one native case fixture is required")
    return bindings


def _freeze_side_inputs(row: Mapping[str, Any], side: str) -> dict[str, Any]:
    identity_config = row["immutable_source_identities"][side]
    bundle = Path(identity_config["input_bundle"])
    identity = resolve_source_identity(identity_config, bundle)
    fixtures = row["fixtures"][side]
    read_manifest = Path(fixtures["read_manifest"])
    write_manifest = Path(fixtures["write_manifest"])
    cases_path = Path(fixtures["native_cases"])
    cases = _read_array(cases_path, "native cases")
    return {
        "identity_config": dict(identity_config),
        "bundle": str(bundle),
        "identity": copy.deepcopy(identity),
        "read_manifest": str(read_manifest),
        "read_binding": executor._fixture_binding(
            str(read_manifest), kind="oxidex_version_rehearsal_fixture_manifest", jpeg_only=False,
        ),
        "write_manifest": str(write_manifest),
        "write_binding": executor._write_fixture_binding(str(write_manifest)),
        "native_cases_path": str(cases_path),
        "native_cases_binding": _file_binding(cases_path, "native cases"),
        "native_cases": copy.deepcopy(cases),
        "native_fixture_bindings": _native_fixture_bindings(cases),
    }


def _verify_frozen_side(frozen: Mapping[str, Any]) -> None:
    try:
        identity = resolve_source_identity(frozen["identity_config"], Path(frozen["bundle"]))
        if identity != frozen["identity"]:
            raise Refused("verified source input changed after qualification preflight")
        read_binding = executor._fixture_binding(
            frozen["read_manifest"], kind="oxidex_version_rehearsal_fixture_manifest", jpeg_only=False,
        )
        if read_binding != frozen["read_binding"]:
            raise Refused("read fixture input changed after qualification preflight")
        if executor._write_fixture_binding(frozen["write_manifest"]) != frozen["write_binding"]:
            raise Refused("write fixture input changed after qualification preflight")
        cases_path = Path(frozen["native_cases_path"])
        if (_file_binding(cases_path, "native cases") != frozen["native_cases_binding"]
                or _read_array(cases_path, "native cases") != frozen["native_cases"]):
            raise Refused("native-case input changed after qualification preflight")
        try:
            native_fixture_bindings = _native_fixture_bindings(frozen["native_cases"])
        except Refused as exc:
            raise Refused("native fixture input changed after qualification preflight") from exc
        if native_fixture_bindings != frozen["native_fixture_bindings"]:
            raise Refused("native fixture input changed after qualification preflight")
    except executor.Refused as exc:
        raise Refused(f"selected input changed after qualification preflight: {exc}") from exc


def _perl(ops_root: Path) -> Path:
    perl = ops_root / "toolchains/perl-5.38.2/prefix/bin/perl5.38.2"
    if perl.is_symlink() or not perl.is_file() or not os.access(perl, os.X_OK):
        raise Refused("pinned Perl 5.38.2 executable is absent")
    if _sha_file(perl) != EXPECTED_PERL_SHA256:
        raise Refused("pinned Perl 5.38.2 executable hash differs from the Task19 contract")
    environment = dict(os.environ)
    for name in ("PERL5LIB", "PERLLIB", "PERL5OPT", "PERL_MM_OPT", "PERL_MB_OPT", "PERL_LOCAL_LIB_ROOT"):
        environment.pop(name, None)
    probe = subprocess.run([str(perl), "-e", "print $^V"], capture_output=True, text=True,
                           timeout=20, env=environment)
    if probe.returncode != 0 or probe.stdout.strip() != EXPECTED_PERL_VERSION:
        raise Refused("pinned Perl executable does not report exact v5.38.2")
    return perl.resolve()


def _commands() -> dict[str, Any]:
    adapter = "{checkout}/tools/exiftool-tables/version_rehearsal_stage_adapter.py"
    common = ["--checkout", "{checkout}", "--target", "{target}", "--report", "{report}",
              "--release", "{release}", "--source-commit", "{source_commit}",
              "--native-source", "{native_source}", "--native-lib", "{native_lib}",
              "--native-perl", "{native_perl}"]
    return {
        "generate": {"argv": [sys.executable, adapter, "generate", *common]},
        "build": {"argv": [sys.executable, adapter, "build", *common]},
        "read": {"argv": [sys.executable, adapter, "read", *common,
                            "--fixture-manifest", "{read_fixture_manifest}",
                            "--native-probe-sha256", "{native_probe_sha256}"]},
        "write": {"argv": [sys.executable, adapter, "write", *common,
                             "--fixture-manifest", "{write_fixture_manifest}",
                             "--native-probe-sha256", "{native_probe_sha256}"]},
    }


def _side_config(*, release: str, source_commit: str, perl: Path, read_manifest: Path,
                 write_manifest: Path, native_cases: list[Any], lease: Path,
                 target: Path) -> dict[str, Any]:
    return {
        "schema": executor.SCHEMA,
        "commands": _commands(),
        "host_lock": str(lease),
        "execution_source_commit": source_commit,
        "execution_releases": [release],
        "perls": {release: str(perl)},
        "native_cases": {release: native_cases},
        "read_fixture_manifests": {release: str(read_manifest)},
        "write_fixture_manifests": {release: str(write_manifest)},
        "target_directories": {release: str(target)},
    }


def _report_for(run_dir: Path, journal: Mapping[str, Any], release: str, stage: str) -> dict[str, Any]:
    report = journal["releases"][release]["reports"].get(stage)
    if (not isinstance(report, dict) or not isinstance(report.get("path"), str)
            or not isinstance(report.get("sha256"), str)
            or re.fullmatch(r"[0-9a-f]{64}", report["sha256"]) is None):
        raise Refused(f"{release} lacks a durable {stage} report")
    relative = Path(report["path"])
    if relative.is_absolute() or ".." in relative.parts:
        raise Refused(f"{release} {stage} report path escapes its durable run")
    value = _read_object(run_dir / relative, f"{release} {stage} report")
    if rehearsal.sha256_json(value) != report["sha256"]:
        raise Refused(f"{release} {stage} report differs from its execution journal digest")
    return value


def _side_receipt(run_dir: Path, journal: Mapping[str, Any], release: str,
                  identity: Mapping[str, Any]) -> dict[str, Any]:
    if journal.get("phase") != "complete" or journal.get("scope", {}).get("write_acceptance") != "passed_per_release":
        raise Refused(f"{release} did not complete mandatory native read/write qualification")
    generate = _report_for(run_dir, journal, release, "generate")
    read = _report_for(run_dir, journal, release, "read")
    write = _report_for(run_dir, journal, release, "write")
    classification = read.get("classification_counts")
    if (not isinstance(classification, dict) or classification.get("extra") != 0
            or any(type(classification.get(name)) is not int or classification[name] < 0
                   for name in ("matched", "value_diff", "missing", "renames", "extra"))):
        raise Refused("read proof lacks the explicit zero-EXTRA hand-behavior retention control")
    checkout = run_dir / "checkouts" / executor._safe_name(release)
    refusals = stage_adapter.generated_refusal_counts(checkout)
    if not isinstance(refusals.get("total"), int) or refusals["total"] < 0:
        raise Refused("generated refusal accounting is unavailable")
    return {
        "release": release,
        "source_identity": {key: identity[key] for key in (
            "release", "tag_object", "peeled_commit", "source_directory",
            "source_tree_sha256", "materialization_sha256")},
        "instrument": {
            "source_commit": read["source_commit"],
            "binary": read["binary"],
            "native_identity": read["native_identity"],
            "native_probe_sha256": read["native_probe_sha256"],
            "read_fixture_manifest": read["fixtures"]["manifest"],
            "read_fixture_manifest_sha256": read["fixtures"]["manifest_sha256"],
            "read_fixture_count": len(read["fixtures"]["entries"]),
        },
        "generated_artifacts": generate.get("generated_artifacts"),
        "classification_counts": classification,
        "generated_refusals": refusals,
        "read_report_sha256": rehearsal.sha256_json(read),
        "write_report_sha256": rehearsal.sha256_json(write),
        "execution_journal_sha256": _sha_file(run_dir / "execution-status.json"),
    }


def _compare_sides(row: Mapping[str, Any], before: Mapping[str, Any], after: Mapping[str, Any]) -> dict[str, Any]:
    before_rows = {item["path"]: item["sha256"] for item in before["generated_artifacts"]}
    after_rows = {item["path"]: item["sha256"] for item in after["generated_artifacts"]}
    added = sorted(set(after_rows) - set(before_rows))
    removed = sorted(set(before_rows) - set(after_rows))
    changed = sorted(name for name in set(before_rows) & set(after_rows) if before_rows[name] != after_rows[name])
    comparison = row["artifact_manifest"]["comparison"]
    if comparison == "identical" and (added or removed or changed or before_rows != after_rows):
        raise Refused("same-pin fresh generations produced different artifact manifests")
    if comparison == "manifest-delta" and not (added or removed or changed):
        raise Refused("forward transition produced no attributable artifact manifest delta")
    if comparison == "manifest-delta-with-removals" and not removed:
        raise Refused("reverse transition did not account for any removed artifact by manifest delta")
    return {"policy": comparison, "added": added, "removed": removed, "changed": changed}


class TransitionLease:
    """One nonblocking host lease with durable owner/heartbeat/expiry/release receipts."""

    def __init__(self, *, lease: Path, run_id: str, owner_receipt: Path,
                 heartbeat_receipt: Path, expiry_receipt: Path, release_receipt: Path,
                 expires_seconds: int = 6 * 60 * 60):
        self.lease = lease
        self.run_id = run_id
        self.owner_receipt = owner_receipt
        self.heartbeat_receipt = heartbeat_receipt
        self.expiry_receipt = expiry_receipt
        self.release_receipt = release_receipt
        self.expires_seconds = expires_seconds
        self.file: Any = None
        self.owner = f"{os.environ.get('USER', 'unknown')}@{socket.gethostname()}"
        self.acquired_at = 0.0
        self.expires_at = 0.0
        self.sequence = 0
        self.terminal_status = "aborted"
        self._heartbeat_lock = threading.Lock()
        self._host_lock_capability: executor._HeldHostLock | None = None

    def __enter__(self) -> "TransitionLease":
        if self.lease.is_symlink() or not self.lease.is_file():
            raise Refused("transition lease must be an existing regular non-symlink file")
        self.file = self.lease.open("r+")
        try:
            os.set_inheritable(self.file.fileno(), True)
            self._host_lock_capability = executor._HeldHostLock.acquire(self.lease, self.file)
        except BlockingIOError as exc:
            self.file.seek(0)
            observed_owner = self.file.read().strip() or "owner metadata unavailable"
            self.file.close()
            raise Refused(f"transition lease is held: {observed_owner}") from exc
        except BaseException:
            self.file.close()
            raise
        try:
            self.acquired_at = time.time()
            self.expires_at = self.acquired_at + self.expires_seconds
            real = str(self.lease.resolve())
            owner_record = {
                "run_id": self.run_id, "owner": self.owner, "pid": os.getpid(), "pgid": os.getpgrp(),
                "acquired_at": self.acquired_at, "lock_path": str(self.lease), "lock_realpath": real,
                "lease_mode": "nonblocking-exclusive", "lease_expires_at": self.expires_at,
                "expiry_policy": "stop-before-next-stage-cleanup-journal-release",
                "qualification_outcome": "pending",
            }
            self.file.seek(0)
            self.file.truncate()
            self.file.write(json.dumps(owner_record, sort_keys=True) + "\n")
            self.file.flush()
            os.fsync(self.file.fileno())
            _atomic_json(self.owner_receipt, owner_record)
            self.heartbeat("acquired", None, None)
            return self
        except BaseException as acquire_error:
            try:
                self._host_lock_capability.deactivate()
                fcntl.flock(self.file.fileno(), fcntl.LOCK_UN)
            finally:
                self.file.close()
                self.file = None
            try:
                _atomic_json(self.release_receipt, {
                    "run_id": self.run_id, "owner": self.owner, "lock_path": str(self.lease),
                    "lock_realpath": str(self.lease.resolve()), "released_at": time.time(),
                    "release_status": "released-after-acquire-failure", "terminal_status": "failed",
                    "release_reason": "lease-acquisition-receipt-failure",
                    "flock_release_confirmed": True, "receipt_failures": [str(acquire_error)],
                    "qualification_outcome": "pending",
                })
            except BaseException as receipt_error:
                if hasattr(acquire_error, "add_note"):
                    acquire_error.add_note(f"transition lease acquisition release receipt failed: {receipt_error}")
            raise

    def heartbeat(self, event: str, row: str | None, stage: str | None) -> None:
        with self._heartbeat_lock:
            if time.time() >= self.expires_at:
                raise Refused("transition lease expired before the next stage")
            self.sequence += 1
            _append_jsonl(self.heartbeat_receipt, {
                "run_id": self.run_id, "owner": self.owner, "lock_path": str(self.lease),
                "sequence": self.sequence, "timestamp": time.time(), "event": event,
                "row": row, "stage": stage,
                "qualification_outcome": "pending",
            })

    def guard(self) -> None:
        if self.file is None or self.file.closed:
            raise Refused("transition lease is not held")
        if time.time() >= self.expires_at:
            raise Refused("transition lease expired before the next stage")

    @property
    def fileno(self) -> int:
        if self.file is None or self.file.closed:
            raise Refused("transition lease descriptor is not held")
        return self.file.fileno()

    @property
    def host_lock_capability(self) -> executor._HeldHostLock:
        # Expiry forbids another stage, but interruption recovery must still
        # borrow the lock while this owner holds it to journal the active stage.
        if self.file is None or self.file.closed:
            raise Refused("transition lease is not held")
        if self._host_lock_capability is None:
            raise Refused("transition lease has no owned host lock")
        return self._host_lock_capability

    def finish(self, terminal_status: str) -> None:
        self.terminal_status = terminal_status

    def __exit__(self, exc_type: Any, exc: Any, traceback: Any) -> None:
        expired = time.time() >= self.expires_at
        terminal = "failed" if exc_type is not None or expired else self.terminal_status
        expiry_status = "expired" if expired else (
            "aborted" if terminal != "body-validated" else "not-expired"
        )
        receipt_failures: list[str] = []
        try:
            _atomic_json(self.expiry_receipt, {
                "run_id": self.run_id, "owner": self.owner, "lock_path": str(self.lease),
                "lock_realpath": str(self.lease.resolve()), "lease_expires_at": self.expires_at,
                "observed_at": time.time(), "expired_at": time.time() if expired else None,
                "expiry_status": expiry_status, "terminal_status": terminal,
                "qualification_outcome": "pending",
            })
        except BaseException as receipt_error:
            receipt_failures.append(f"expiry receipt: {receipt_error}")
        release_status = "released"
        confirmed = False
        if self.file is not None:
            try:
                self._host_lock_capability.deactivate()
                fcntl.flock(self.file.fileno(), fcntl.LOCK_UN)
                confirmed = True
            except OSError as release_error:
                release_status = "release-failed"
                receipt_failures.append(f"lock release: {release_error}")
            finally:
                try:
                    self.file.close()
                except OSError as close_error:
                    release_status = "release-failed"
                    receipt_failures.append(f"lock close: {close_error}")
                finally:
                    self.file = None
        if not confirmed:
            receipt_failures.append("lock release was not confirmed")
        try:
            _atomic_json(self.release_receipt, {
                "run_id": self.run_id, "owner": self.owner, "lock_path": str(self.lease),
                "lock_realpath": str(self.lease.resolve()), "released_at": time.time(),
                "release_status": release_status, "terminal_status": terminal,
                "release_reason": "qualification-terminal", "flock_release_confirmed": confirmed,
                "receipt_failures": receipt_failures,
                "qualification_outcome": "pending",
            })
        except BaseException as receipt_error:
            receipt_failures.append(f"release receipt: {receipt_error}")
        # Operational receipts never assert qualification success, so a late
        # expiry needs only to refuse the single final outcome commit. No
        # fallible successful-to-failed rewrite is required here.
        if time.time() >= self.expires_at:
            receipt_failures.insert(0, "transition lease expired during final cleanup")
        if receipt_failures:
            detail = "; ".join(receipt_failures)
            if exc is not None:
                if hasattr(exc, "add_note"):
                    exc.add_note(f"transition lease cleanup warning: {detail}")
                return None
            raise Refused(f"transition lease cleanup failed: {detail}")


class ReceiptCadence:
    """Persist lease and handoff liveness while a long executor stage runs."""

    def __init__(self, lease: TransitionLease, handoff_receipt: Path, interval_seconds: int = 600):
        self.lease = lease
        self.handoff_receipt = handoff_receipt
        self.interval_seconds = interval_seconds
        self.row: str | None = None
        self.stage: str | None = None
        self.stop = threading.Event()
        self.failure: BaseException | None = None
        self.thread = threading.Thread(target=self._run, name="transition-receipt-cadence", daemon=True)

    def __enter__(self) -> "ReceiptCadence":
        self.thread.start()
        return self

    def position(self, row: str | None, stage: str | None) -> None:
        self.row, self.stage = row, stage
        self.check()

    def check(self) -> None:
        if self.failure is not None:
            raise Refused(f"periodic lease/handoff receipt failed: {self.failure}") from self.failure

    def _run(self) -> None:
        while not self.stop.wait(self.interval_seconds):
            try:
                self.lease.heartbeat("periodic", self.row, self.stage)
                _append_jsonl(self.handoff_receipt, {
                    "run_id": self.lease.run_id, "owner": self.lease.owner,
                    "timestamp": time.time(), "state": "periodic",
                    "row": self.row, "stage": self.stage, "lease": str(self.lease.lease),
                    "qualification_outcome": "pending",
                })
            except BaseException as exc:
                self.failure = exc
                self.stop.set()

    def __exit__(self, exc_type: Any, exc: Any, traceback: Any) -> None:
        self.stop.set()
        self.thread.join(timeout=5)
        if self.thread.is_alive():
            failure = Refused("receipt cadence remains running after shutdown")
            if exc is not None:
                if hasattr(exc, "add_note"):
                    exc.add_note(str(failure))
                return None
            raise failure
        if exc_type is None:
            self.check()


def _recover_if_running(run_dir: Path, archive_cache: Path, source_root: Path, *,
                        host_lock_fd: executor._HeldHostLock | None = None) -> None:
    journal_path = run_dir / "execution-status.json"
    if not journal_path.is_file() or journal_path.is_symlink():
        return
    journal = _read_object(journal_path, "execution journal")
    if journal.get("phase") == "running" and isinstance(journal.get("active"), dict):
        executor.recover(run_dir, archive_cache, source_root, host_lock_fd=host_lock_fd)


def _validate_receipt_contract(*, output_root: Path, run_id: str, lease_path: Path,
                               owner_receipt: Path, heartbeat_receipt: Path,
                               expiry_receipt: Path, release_receipt: Path,
                               handoff_receipt: Path) -> None:
    output = output_root.resolve()
    if lease_path.name != "transition.host.lock" or lease_path.resolve().parent != output:
        raise Refused("lease must be the shared transition.host.lock directly beneath the output root")
    receipt_root = output / run_id
    expected = {
        owner_receipt: "lease-owner.json",
        heartbeat_receipt: "lease-heartbeat.jsonl",
        expiry_receipt: "lease-expiry.json",
        release_receipt: "lease-release.json",
        handoff_receipt: "handoff.jsonl",
    }
    if len(expected) != 5:
        raise Refused("lease and handoff receipt paths must be distinct")
    for receipt, basename in expected.items():
        if receipt.name != basename or receipt.resolve().parent != receipt_root:
            raise Refused(f"{basename} must be emitted beneath the run output directory")
        if receipt.exists() or receipt.is_symlink():
            raise Refused(f"stale receipt reuse is forbidden: {receipt}")


def _bind_receipt(path: Path) -> dict[str, str]:
    if path.is_symlink() or not path.is_file():
        raise Refused(f"required receipt is not a regular file: {path}")
    return {"path": str(path.resolve()), "sha256": _sha_file(path)}


def _receipt_manifest(*, owner_receipt: Path, heartbeat_receipt: Path,
                      expiry_receipt: Path, release_receipt: Path,
                      handoff_receipt: Path, row_results: list[Path]) -> dict[str, Any]:
    return {
        "owner_receipt": _bind_receipt(owner_receipt),
        "heartbeat_receipt": _bind_receipt(heartbeat_receipt),
        "expiry_receipt": _bind_receipt(expiry_receipt),
        "release_receipt": _bind_receipt(release_receipt),
        "handoff_receipt": _bind_receipt(handoff_receipt),
        "row_results": [_bind_receipt(path) for path in row_results],
    }


def load_committed_result(final_path: Path) -> dict[str, Any]:
    """Accept qualification only through its final marker and exact pending inputs."""
    final = _read_object(final_path, "qualification result")
    run_id = final.get("run_id")
    if (final_path.name != "qualification-result.json" or not isinstance(run_id, str)
            or final_path.parent.name != run_id or not RUN_ID.fullmatch(run_id)
            or final.get("schema") != SCHEMA or final.get("kind") != RESULT_KIND
            or final.get("status") != "tooling-executed-nonpromoting"
            or final.get("promotion") != "forbidden"
            or final.get("caller_restored") is not True):
        raise Refused("qualification final marker has invalid identity or outcome")
    root = final_path.parent
    manifest = final.get("receipt_manifest")
    names = {
        "owner_receipt": "lease-owner.json",
        "heartbeat_receipt": "lease-heartbeat.jsonl",
        "expiry_receipt": "lease-expiry.json",
        "release_receipt": "lease-release.json",
        "handoff_receipt": "handoff.jsonl",
    }
    if not isinstance(manifest, dict) or set(manifest) != {*names, "row_results"}:
        raise Refused("qualification final marker lacks the exact receipt manifest")
    for name, basename in names.items():
        path = root / basename
        if manifest[name] != _bind_receipt(path):
            raise Refused(f"qualification receipt digest or path mismatch: {name}")
        if basename.endswith(".jsonl"):
            try:
                records = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]
            except (OSError, json.JSONDecodeError) as exc:
                raise Refused(f"qualification receipt is unreadable: {name}") from exc
            if not records or any(not isinstance(record, dict) or
                                  record.get("run_id") != run_id or
                                  record.get("qualification_outcome") != "pending"
                                  for record in records):
                raise Refused(f"qualification receipt is not pending for this run: {name}")
        else:
            record = _read_object(path, name)
            if record.get("run_id") != run_id or record.get("qualification_outcome") != "pending":
                raise Refused(f"qualification receipt is not pending for this run: {name}")
    owner = _read_object(root / names["owner_receipt"], "lease owner")
    expiry = _read_object(root / names["expiry_receipt"], "lease expiry")
    release = _read_object(root / names["release_receipt"], "lease release")
    observed = final.get("deadline_observed_at")
    deadline = final.get("lease_expires_at")
    if (not isinstance(observed, (int, float)) or not isinstance(deadline, (int, float))
            or not math.isfinite(observed) or not math.isfinite(deadline)
            or observed >= deadline or owner.get("lease_expires_at") != deadline
            or expiry.get("lease_expires_at") != deadline or expiry.get("expiry_status") != "not-expired"
            or expiry.get("terminal_status") != "body-validated"
            or release.get("release_status") != "released"
            or release.get("terminal_status") != "body-validated"
            or release.get("flock_release_confirmed") is not True
            or release.get("receipt_failures") != []):
        raise Refused("qualification final marker has invalid cleanup or deadline evidence")
    rows = final.get("rows")
    bound_rows = manifest["row_results"]
    if (not isinstance(rows, list) or not rows or not isinstance(bound_rows, list)
            or len(rows) != len(bound_rows)):
        raise Refused("qualification final marker has invalid row receipts")
    seen_rows: set[str] = set()
    for row, bound in zip(rows, bound_rows, strict=True):
        if not isinstance(row, dict) or not isinstance(row.get("id"), str):
            raise Refused("qualification final marker has invalid row identity")
        if row["id"] in seen_rows:
            raise Refused("qualification final marker repeats a row identity")
        seen_rows.add(row["id"])
        row_path = root / row["id"] / "transition-result.json"
        if row_path.resolve().parent.parent != root.resolve() or bound != _bind_receipt(row_path):
            raise Refused("qualification row receipt digest or path mismatch")
        if (_read_object(row_path, "row result") != row
                or row.get("qualification_outcome") != "pending"
                or row.get("caller_restored") is not True
                or row.get("promotion") != "forbidden"):
            raise Refused("qualification row receipt is not pending or differs from final")
    return final


def run_qualification(*, matrix_path: Path, repository: Path, output_root: Path,
                      target_root: Path, lease_path: Path, run_id: str,
                      owner_receipt: Path, heartbeat_receipt: Path,
                      expiry_receipt: Path, release_receipt: Path,
                      handoff_receipt: Path, only: str | None = None,
                      execute: Callable[..., dict[str, Any]] = executor.execute) -> dict[str, Any]:
    _validate_receipt_contract(
        output_root=output_root, run_id=run_id, lease_path=lease_path,
        owner_receipt=owner_receipt, heartbeat_receipt=heartbeat_receipt,
        expiry_receipt=expiry_receipt, release_receipt=release_receipt,
        handoff_receipt=handoff_receipt,
    )
    caller = snapshot_caller(repository)
    pinned = caller["pin_version"]
    matrix = materialize_matrix(load_matrix(matrix_path, pinned), output_root=output_root,
                                target_root=target_root, run_id=run_id)
    rows = [row for row in matrix["rows"] if only is None or row["id"] == only]
    if not rows:
        raise Refused(f"matrix row is not selected: {only}")
    source_commit = caller["head"]
    frozen_inputs: dict[str, dict[str, dict[str, Any]]] = {}
    for row in rows:
        frozen_inputs[row["id"]] = {}
        for side in SIDES:
            frozen = _freeze_side_inputs(row, side)
            if frozen["identity"]["documents"]["plan"].get("repository_commit") != source_commit:
                raise Refused("verified transition plan is not bound to the caller execution source commit")
            frozen_inputs[row["id"]][side] = frozen
    perl = _perl(ops_paths.ops_root())
    results: list[dict[str, Any]] = []
    final: dict[str, Any] | None = None
    final_path = output_root / run_id / "qualification-result.json"
    with TransitionLease(lease=lease_path, run_id=run_id, owner_receipt=owner_receipt,
                         heartbeat_receipt=heartbeat_receipt, expiry_receipt=expiry_receipt,
                         release_receipt=release_receipt) as host_lease:
        _append_jsonl(handoff_receipt, {"run_id": run_id, "timestamp": time.time(),
                                        "state": "preflight", "rows": [row["id"] for row in rows],
                                        "lease": str(lease_path),
                                        "qualification_outcome": "pending"})
        cadence = ReceiptCadence(host_lease, handoff_receipt)
        cadence.__enter__()
        try:
            for row in rows:
                row_output = Path(row["durable_output_directory"])
                row_target = Path(row["target_directory"])
                if row_output.exists() or row_output.is_symlink() or row_target.exists() or row_target.is_symlink():
                    raise Refused("row output or target already exists; stale reuse is forbidden")
                row_output.mkdir(parents=True)
                sides: dict[str, dict[str, Any]] = {}
                for side, release in zip(SIDES, (row["before_version"], row["after_version"]), strict=True):
                    cadence.position(row["id"], side)
                    host_lease.heartbeat("side-start", row["id"], side)
                    frozen = frozen_inputs[row["id"]][side]
                    _verify_frozen_side(frozen)
                    identity = frozen["identity"]
                    read_manifest = Path(frozen["read_manifest"])
                    write_manifest = Path(frozen["write_manifest"])
                    native_cases = frozen["native_cases"]
                    side_run = row_output / side
                    side_target = row_target / side
                    documents = identity["documents"]
                    config = _side_config(
                        release=release, source_commit=source_commit, perl=perl,
                        read_manifest=read_manifest, write_manifest=write_manifest,
                        native_cases=native_cases, lease=lease_path, target=side_target,
                    )
                    executor.initialize_run(
                        side_run, documents["capture"], documents["catalog"], documents["plan"],
                        documents["resolution"], documents["materialization"], config,
                    )
                    def stage_guard(_release: str, _stage: str, _boundary: str,
                                    *, selected=frozen) -> None:
                        try:
                            host_lease.guard()
                            cadence.check()
                            _verify_frozen_side(selected)
                            host_lease.guard()
                            cadence.check()
                        except (Refused, OSError, ValueError) as exc:
                            raise executor.Refused(str(exc)) from exc
                    try:
                        journal = execute(
                            side_run, repository, Path(identity["archive_cache"]), Path(identity["source_root"]),
                            host_lock_fd=host_lease.host_lock_capability, stage_guard=stage_guard,
                        )
                        cadence.check()
                    except BaseException as execution_error:
                        try:
                            _recover_if_running(
                                side_run, Path(identity["archive_cache"]), Path(identity["source_root"]),
                                host_lock_fd=host_lease.host_lock_capability,
                            )
                        except BaseException as recovery_error:
                            if hasattr(execution_error, "add_note"):
                                execution_error.add_note(f"durable interruption recovery failed: {recovery_error}")
                        raise
                    _verify_frozen_side(frozen)
                    sides[side] = _side_receipt(side_run, journal, release, identity)
                    verify_caller(caller)
                    host_lease.heartbeat("side-complete", row["id"], side)
                    _append_jsonl(handoff_receipt, {
                        "run_id": run_id, "timestamp": time.time(), "state": "side-complete",
                        "row": row["id"], "stage": side, "lease": str(lease_path),
                        "execution_journal": str(side_run / "execution-status.json"),
                        "qualification_outcome": "pending",
                    })
                delta = _compare_sides(row, sides["before"], sides["after"])
                host_lease.guard()
                cadence.check()
                for frozen in frozen_inputs[row["id"]].values():
                    _verify_frozen_side(frozen)
                result = {"id": row["id"], "before": sides["before"], "after": sides["after"],
                          "artifact_delta": delta, "caller_restored": True,
                          "promotion": "forbidden", "qualification_outcome": "pending"}
                _atomic_json(row_output / "transition-result.json", result)
                results.append(result)
                verify_caller(caller)
            for row_inputs in frozen_inputs.values():
                for frozen in row_inputs.values():
                    _verify_frozen_side(frozen)
            host_lease.guard()
            cadence.check()
            final = {
                "schema": SCHEMA, "kind": RESULT_KIND, "run_id": run_id,
                "promotion": "forbidden", "status": "tooling-executed-nonpromoting",
                "caller": caller, "rows": results, "caller_restored": True,
            }
            _append_jsonl(handoff_receipt, {"run_id": run_id, "timestamp": time.time(),
                                            "state": "final-validation-complete",
                                            "lease": str(lease_path),
                                            "qualification_outcome": "pending"})
            host_lease.heartbeat("final-validation-complete", None, "final")
            verify_caller(caller)
            host_lease.finish("body-validated")
        finally:
            active_exception = sys.exc_info()
            try:
                verify_caller(caller)
            finally:
                cadence.__exit__(*active_exception)
    if final is None:
        raise Refused("qualification ended without a validated final result")
    verify_caller(caller)
    final["receipt_manifest"] = _receipt_manifest(
        owner_receipt=owner_receipt, heartbeat_receipt=heartbeat_receipt,
        expiry_receipt=expiry_receipt, release_receipt=release_receipt,
        handoff_receipt=handoff_receipt,
        row_results=[Path(row["durable_output_directory"]) / "transition-result.json"
                     for row in rows],
    )
    # This is the sole eligibility decision after cadence shutdown, lock
    # release/close and all prerequisite receipt I/O. Publication follows it;
    # arbitrary later filesystem latency is not promised to fit the lease.
    final["deadline_observed_at"] = time.time()
    final["lease_expires_at"] = host_lease.expires_at
    if final["deadline_observed_at"] >= host_lease.expires_at:
        raise Refused("transition lease expired before final outcome commit")
    try:
        _atomic_json(final_path, final)
    except BaseException as publication_error:
        # The replacement may have succeeded before a subsequent I/O/reporting
        # error surfaced. Inspect the exact marker rather than guessing that a
        # committed result was refused or that an absent result succeeded.
        if final_path.exists() or final_path.is_symlink():
            try:
                committed = load_committed_result(final_path)
            except (Refused, OSError, ValueError) as inspection_error:
                raise OutcomeUnknown(
                    f"final publication outcome uncertain: {inspection_error}"
                ) from publication_error
            if committed == final:
                return committed
            raise OutcomeUnknown("final publication outcome uncertain: marker differs") from publication_error
        raise
    try:
        committed = load_committed_result(final_path)
    except (Refused, OSError, ValueError) as inspection_error:
        # The publisher returned after replacing the marker. A later read or
        # validation failure cannot be called an uncommitted refusal.
        raise OutcomeUnknown(
            f"postpublication final marker validation is uncertain: {inspection_error}"
        ) from inspection_error
    if committed != final:
        raise OutcomeUnknown("postpublication final marker differs from this invocation")
    return committed


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--matrix", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--target-root", default=str(ops_paths.target_root()))
    parser.add_argument("--lease", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--owner-receipt", required=True)
    parser.add_argument("--heartbeat-receipt", required=True)
    parser.add_argument("--expiry-receipt", required=True)
    parser.add_argument("--release-receipt", required=True)
    parser.add_argument("--handoff-receipt", required=True)
    parser.add_argument("--only")
    return parser


def _instrument_header(result: Mapping[str, Any]) -> str:
    """Attribute every reported comparison to its validated build and oracle."""
    caller = result["caller"]
    lines = ["=== instrument: version_transition_qualification.py ===",
             f"caller:  {caller['head']}  pin {caller['pin_version']}"]
    for row in result["rows"]:
        for side in SIDES:
            entry = row[side]
            proof = entry["instrument"]
            native = proof["native_identity"]
            binary = proof["binary"]
            label = f"{row['id']}/{side}"
            lines.extend((
                f"{label}: OxiDex {binary['path']} sha256={binary['sha256']} source={proof['source_commit']}",
                f"{label}: ExifTool {native['release']} source={native['source']['path']} "
                f"lib_sha256={native['lib']['exiftool_pm_sha256']} perl={native['perl']['path']} "
                f"probe_sha256={proof['native_probe_sha256']}",
                f"{label}: corpus {proof['read_fixture_manifest']} "
                f"manifest_sha256={proof['read_fixture_manifest_sha256']} "
                f"files={proof['read_fixture_count']}",
            ))
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        result = run_qualification(
            matrix_path=Path(args.matrix), repository=Path(args.repository),
            output_root=Path(args.output), target_root=Path(args.target_root),
            lease_path=Path(args.lease), run_id=args.run_id,
            owner_receipt=Path(args.owner_receipt), heartbeat_receipt=Path(args.heartbeat_receipt),
            expiry_receipt=Path(args.expiry_receipt), release_receipt=Path(args.release_receipt),
            handoff_receipt=Path(args.handoff_receipt), only=args.only,
        )
    except KeyboardInterrupt:
        print("version transition qualification interrupted; inspect the durable execution "
              "journal and recovery receipts before retrying", file=sys.stderr)
        return 130
    except OutcomeUnknown as exc:
        print(f"version transition qualification outcome unknown: {exc}", file=sys.stderr)
        return 4
    except (Refused, executor.Refused, rehearsal.Refused, catalog_stage.Refused,
            native_oracle.Refused, stage_adapter.Refused, OSError, ValueError) as exc:
        print(f"version transition qualification refused: {exc}", file=sys.stderr)
        return 2
    # `run_qualification` returns only after validating the committed marker.
    # Reporting failure is distinct from an uncommitted/refused qualification.
    try:
        print(_instrument_header(result))
        print(json.dumps({"run_id": result["run_id"], "status": result["status"],
                          "promotion": result["promotion"]}, sort_keys=True))
    except (OSError, KeyError, TypeError, ValueError) as exc:
        print(f"version transition qualification committed for {result['run_id']}, "
              f"but stdout reporting failed: {exc}", file=sys.stderr)
        return 3
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
