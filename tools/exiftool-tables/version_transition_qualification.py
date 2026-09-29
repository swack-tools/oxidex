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
import version_transition_read_policy as read_policy

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
CANONICAL_MATRIX = REPOSITORY_ROOT / "tools" / "exiftool-tables" / "version_transition_matrix.json"
INPUT_NAMES = ("capture", "catalog", "plan", "resolution", "materialization")
SIDES = ("before", "after")


class Refused(ValueError):
    """The requested qualification cannot produce attributable evidence."""


class OutcomeUnknown(Refused):
    """A final marker exists, but publication could not be confirmed or refused."""


class LeaseRetained(Refused):
    """The lease is deliberately still held: an owned child was not proven gone."""

    def __init__(self, message: str, survivors: list[dict[str, Any]]):
        super().__init__(message)
        self.survivors = survivors


def _sha_file(file_path: Path) -> str:
    digest = hashlib.sha256()
    with file_path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _fsync_directory(directory: Path) -> None:
    descriptor = os.open(directory, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def _make_parent(file_path: Path) -> list[Path]:
    """Create the receipt directory; return directories whose entries changed."""
    missing = [parent for parent in (file_path.parent, *file_path.parent.parents) if not parent.exists()]
    file_path.parent.mkdir(parents=True, exist_ok=True)
    return [file_path.parent, *(directory.parent for directory in missing)]


def _atomic_json(file_path: Path, value: Mapping[str, Any]) -> None:
    if file_path.exists() or file_path.is_symlink():
        raise Refused(f"receipt already exists: {file_path}")
    changed = _make_parent(file_path)
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
    # Persist the new directory entries too, not only the file contents.
    for directory in dict.fromkeys(changed):
        _fsync_directory(directory)


def _append_jsonl(file_path: Path, value: Mapping[str, Any]) -> None:
    if file_path.is_symlink():
        raise Refused(f"JSONL receipt must not be a symbolic link: {file_path}")
    changed = _make_parent(file_path) if not file_path.exists() else []
    with file_path.open("a", encoding="utf-8") as stream:
        stream.write(json.dumps(value, sort_keys=True) + "\n")
        stream.flush()
        os.fsync(stream.fileno())
    for directory in dict.fromkeys(changed):
        _fsync_directory(directory)


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


def _evidence_location(value: Any, label: str) -> Path:
    """A durable evidence path beneath the ops root (OXIDEX_OPS_DIR)."""
    if not isinstance(value, (str, Path)) or not Path(value).is_absolute():
        raise Refused(f"{label} must be an absolute path beneath the ops root: {value}")
    try:
        resolved = ops_paths.durable_root(Path(value), label)
    except ValueError as exc:
        raise Refused(str(exc)) from exc
    root = ops_paths.ops_root()
    if not resolved.is_relative_to(root):
        raise Refused(f"{label} must be beneath the ops root {root}: {value}")
    return resolved


def materialize_matrix(matrix: dict[str, Any], *, output_root: Path, target_root: Path,
                       run_id: str) -> dict[str, Any]:
    if RUN_ID.fullmatch(run_id) is None:
        raise Refused("run ID is malformed")
    # Output and every input bundle beneath it are evidence under the ops root.
    # The Cargo target root is the documented separate exception.
    output_root = _evidence_location(output_root, "qualification output")
    try:
        target_root = ops_paths.durable_root(target_root, "OXIDEX_TARGET_ROOT")
    except ValueError as exc:
        raise Refused(str(exc)) from exc
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
    archive_cache = _evidence_location(locations["archive_cache"], "verified archive cache")
    source_root = _evidence_location(locations["source_root"], "verified source root")
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


def _freeze_read_union(before: Mapping[str, Any], after: Mapping[str, Any]) -> dict[str, Any]:
    """Freeze both original selections before either generated side can run."""
    try:
        return read_policy.freeze_union(
            _regular_receipt(Path(before["read_manifest"])).read_bytes(),
            _regular_receipt(Path(after["read_manifest"])).read_bytes(),
        )
    except (read_policy.ReadPolicyRefused, OSError) as exc:
        raise Refused(f"transition read union is invalid: {exc}") from exc


def _read_policy_input(path: Path, expected_rows: set[str]) -> tuple[dict[str, Any], dict[str, str]]:
    path = _evidence_location(path, "Task19 read-policy input")
    document = _read_object(path, "Task19 read-policy input")
    floors = document.get("rows")
    if (set(document) != {"schema", "kind", "policy", "rows"}
            or document.get("schema") != 1
            or document.get("kind") != "oxidex_task19_read_policy_input"
            or document.get("policy") != read_policy.PAIR_SCHEMA
            or not isinstance(floors, dict) or set(floors) != expected_rows
            or any(not isinstance(value, dict) or set(value) != set(SIDES)
                   or any(type(value[side]) is not int or value[side] <= 0 for side in SIDES)
                   for value in floors.values())):
        raise Refused("Task19 read-policy floors or policy are not the frozen matrix selection")
    return document, {"path": str(path), "sha256": _sha_file(path)}


def _verify_read_policy_input(binding: Mapping[str, Any], expected_rows: set[str]) -> dict[str, Any]:
    if not isinstance(binding, dict) or not isinstance(binding.get("path"), str):
        raise Refused("Task19 read-policy input binding is missing")
    document, current = _read_policy_input(Path(binding["path"]), expected_rows)
    if current != binding:
        raise Refused("Task19 read-policy input changed after preflight")
    return document


def _materialize_read_union(union: Mapping[str, Any], before: Mapping[str, Any],
                            after: Mapping[str, Any], row_output: Path) -> dict[str, Any]:
    """Use the same selected carrier bytes and order on both release sides."""
    row_output = row_output.resolve()
    if _freeze_read_union(before, after) != union:
        raise Refused("original transition read selections changed before union materialization")
    fixtures = union.get("fixtures")
    if not isinstance(fixtures, list) or not fixtures:
        raise Refused("transition read union has no carriers")
    rows = []
    sidecar = []
    for fixture in fixtures:
        source = fixture["sources"]["before"] or fixture["sources"]["after"]
        path = _regular_receipt(Path(source))
        digest = _sha_file(path)
        if digest != fixture["sha256"] or path.stat().st_size != fixture["bytes"]:
            raise Refused(f"transition read union carrier changed: {path}")
        rows.append({"path": str(path), "sha256": digest, "bytes": fixture["bytes"]})
        sidecar.append({"logical_name": fixture["logical_name"], "sha256": digest,
                        "bytes": fixture["bytes"], "sources": fixture["sources"],
                        "chosen_source": str(path)})
    manifest_path = row_output / "read-union-manifest.json"
    sidecar_path = row_output / "read-union-sidecar.json"
    _atomic_json(manifest_path, {"schema": 1,
                "kind": "oxidex_version_rehearsal_fixture_manifest", "fixtures": rows})
    _atomic_json(sidecar_path, {"schema": 1, "kind": "task19-read-union-sidecar/v1",
                "union_sha256": union["union_sha256"], "carriers": sidecar})
    binding = executor._fixture_binding(
        str(manifest_path), kind="oxidex_version_rehearsal_fixture_manifest", jpeg_only=False)
    return {"union": union, "manifest": binding,
            "original_manifests": {"before": before["read_binding"],
                                   "after": after["read_binding"]},
            "sidecar": {"path": str(sidecar_path), "sha256": _sha_file(sidecar_path)}}


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
        "test": {"argv": [sys.executable, adapter, "test", *common]},
        "read": {"argv": [sys.executable, adapter, "read", *common,
                            "--fixture-manifest", "{read_fixture_manifest}",
                            "--native-probe-sha256", "{native_probe_sha256}"]},
        "write": {"argv": [sys.executable, adapter, "write", *common,
                             "--fixture-manifest", "{write_fixture_manifest}",
                             "--native-probe-sha256", "{native_probe_sha256}"]},
    }


def _side_config(*, release: str, source_commit: str, perl: Path, read_manifest: Path,
                 write_manifest: Path, native_cases: list[Any], lease: Path,
                 target: Path, verified_input_bundle: Path,
                 read_policy_input: Mapping[str, str] | None = None) -> dict[str, Any]:
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
        "verified_input_bundle": str(verified_input_bundle),
        "read_policy_input": dict(read_policy_input) if read_policy_input is not None else None,
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
    release_row = journal.get("releases", {}).get(release, {})
    if (journal.get("phase") != "complete"
            or journal.get("scope", {}).get("write_acceptance") != "passed_per_release"
            or journal.get("scope", {}).get("read_acceptance") != "pending_pair_policy"
            or release_row.get("state") != "measured_pending_pair_policy"
            or release_row.get("stages", {}).get("read") != "measured"):
        raise Refused(f"{release} did not complete mandatory native read/write qualification")
    generate = _report_for(run_dir, journal, release, "generate")
    read = _report_for(run_dir, journal, release, "read")
    if (read.get("state") != "measured"
            or release_row.get("reports", {}).get("read", {}).get("acceptance") != "pending_pair_policy"):
        raise Refused(f"{release} read is not an authenticated pending pair measurement")
    write = _report_for(run_dir, journal, release, "write")
    classification = read.get("classification_counts")
    if (not isinstance(classification, dict)
            or any(type(classification.get(name)) is not int or classification[name] < 0
                   for name in ("matched", "value_diff", "missing", "renames", "extra"))):
        raise Refused("read proof lacks its exact nonnegative classification counts")
    native = _report_for(run_dir, journal, release, "native")
    version, docx = native.get("version"), native.get("docx_capability")
    perl_capability = native.get("perl_capability")
    if (native.get("state") != "ready" or native.get("probe_sha256") != read.get("native_probe_sha256")
            or not isinstance(version, dict) or version.get("state") != "ok"
            or str(version.get("stdout", "")).strip() != release
            or not isinstance(docx, dict) or docx.get("state") != "ok"
            or str(docx.get("stdout", "")).strip() != "DOCX"
            or not isinstance(perl_capability, dict) or perl_capability.get("available") is not True):
        raise Refused(f"{release} native capability probe is not the ready probe used by read")
    checkout = run_dir / "checkouts" / executor._safe_name(release)
    # The build first: the release tests are then checked against its compiler.
    build_environment = _build_environment_receipt(_report_for(run_dir, journal, release, "build"), release,
                                                   checkout)
    release_tests = _release_test_receipt(run_dir, journal, release)
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
            "capability_probe": {"state": "ready", "version": release, "docx_filetype": "DOCX",
                                 "perl_modules_available": True},
        },
        "release_tests": release_tests,
        "build_environment": build_environment,
        "generated_artifacts": generate.get("generated_artifacts"),
        "classification_counts": classification,
        "generated_refusals": refusals,
        "read_report_sha256": rehearsal.sha256_json(read),
        "raw_maps": read.get("raw_maps"),
        "write_report_sha256": rehearsal.sha256_json(write),
        "execution_journal_sha256": _sha_file(run_dir / "execution-status.json"),
    }


def _release_test_receipt(run_dir: Path, journal: Mapping[str, Any], release: str) -> dict[str, Any]:
    """Require the regenerated checkout's own suite: exact commands, zero failures."""
    if journal.get("scope", {}).get("release_tests") != "passed_per_release":
        raise Refused(f"{release} release test suite did not pass for this side")
    report = _report_for(run_dir, journal, release, "test")
    suite = report.get("test_suite")
    keys = ("passed", "failed", "ignored", "measured", "filtered_out", "targets")

    def counts(value: Any) -> bool:
        return isinstance(value, dict) and all(type(value.get(key)) is int and value[key] >= 0 for key in keys)

    commands = suite.get("commands") if isinstance(suite, dict) else None
    totals = suite.get("totals") if isinstance(suite, dict) else None
    log = suite.get("log") if isinstance(suite, dict) else None
    expected = [list(argv) for argv in stage_adapter.TEST_COMMANDS]
    if (report.get("state") != "passed" or not isinstance(commands, list) or not counts(totals)
            or not all(counts(row) for row in commands)
            or [row.get("argv") for row in commands] != expected
            or any(row.get("exit") != 0 or type(row.get("duration_seconds")) is not float for row in commands)
            or any(totals[key] != sum(row[key] for row in commands) for key in keys)
            or totals["failed"] != 0 or totals["passed"] < 1
            or report.get("denominator") != totals["passed"]
            or not isinstance(log, dict) or log != report.get("raw_report")
            or not isinstance(log.get("path"), str) or not isinstance(suite.get("target_directory"), str)):
        raise Refused(f"{release} release test suite is not a counted zero-failure run of the required commands")
    try:
        log_sha = _sha_file(_regular_receipt(Path(log["path"])))
    except (OSError, Refused) as exc:
        raise Refused(f"{release} release test suite log is unavailable") from exc
    if log_sha != log.get("sha256"):
        raise Refused(f"{release} release test suite log differs from its report")
    oracle = _release_test_oracle(report, release)
    corpus = _release_test_corpus(suite, release)
    # The suite compiled its own target: it must have proved, with its own
    # environment, the same pinned rustc that built the qualified binaries.
    compiler = suite.get("compiler")
    checkout = run_dir / "checkouts" / executor._safe_name(release)
    try:
        stage_adapter.validate_pinned_toolchain(compiler, checkout)
        build_toolchain = _report_for(run_dir, journal, release, "build")["build_environment"]["toolchain"]
        if compiler["toolchain"] != build_toolchain:
            raise stage_adapter.Refused("the suite's rustc/cargo differ from the build's")
    except (stage_adapter.Refused, OSError, KeyError, TypeError) as exc:
        raise Refused(f"{release} release test suite did not run on the checkout's pinned toolchain: {exc}") from exc
    cargo_config = suite.get("cargo_config")
    if (not isinstance(cargo_config, dict) or cargo_config.get("outside_checkout") != []
            or not isinstance(cargo_config.get("checked"), list) or not cargo_config["checked"]):
        raise Refused(f"{release} release test suite may have read cargo configuration outside the checkout")
    return {"commands": expected, "exits": [row["exit"] for row in commands],
            **{key: totals[key] for key in keys},
            "duration_seconds": round(sum(row["duration_seconds"] for row in commands), 3),
            "target_directory": suite["target_directory"], "log": log, "exiftool_oracle": oracle,
            "fixture_corpus": corpus, "compiler": compiler}


def _fixture_corpus_authority() -> dict[str, Any]:
    """This host's bootstrap-verified combined corpus, read independently of any receipt."""
    import importlib.util
    script = REPOSITORY_ROOT / "tools" / "release" / "bootstrap_oracle.py"
    spec = importlib.util.spec_from_file_location("oxidex_qualification_bootstrap_oracle", script)
    if spec is None or spec.loader is None:
        raise Refused("oracle bootstrap cannot be loaded")
    bootstrap = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(bootstrap)
    root = ops_paths.ops_root()
    corpus = Path(bootstrap.corpus_path(root))
    manifest = corpus.parent / "combined-samples.manifest"
    storage = _read_object(Path(bootstrap.manifest_path(root)), "oracle storage manifest")
    manifest_sha = _sha_file(manifest)
    tree_sha = bootstrap.LOCK.get("corpus_tree_sha256")
    artifacts_ = storage.get("artifacts") if isinstance(storage.get("artifacts"), dict) else {}
    if (artifacts_.get("corpus_manifest") != {"kind": "file", "path": str(manifest), "sha256": manifest_sha}
            or artifacts_.get("corpus_tree") != {"kind": "tree", "path": str(corpus), "sha256": tree_sha}):
        raise Refused("this host's combined corpus manifest is not bootstrap-verified")
    count = sum(1 for line in manifest.read_text(encoding="utf-8").splitlines() if line)
    return {"ops_root": str(root), "bootstrap_pin": bootstrap.VERSION, "corpus": str(corpus),
            "corpus_tree_sha256": tree_sha,
            "manifest": {"path": str(manifest), "sha256": manifest_sha, "file_count": count}}


def _release_test_corpus(suite: Mapping[str, Any], release: str) -> dict[str, Any]:
    """The suite's combined samples must be this host's verified corpus, unchanged across the run."""
    corpus = suite.get("fixture_corpus")
    try:
        authority = _fixture_corpus_authority()
    except (OSError, ValueError) as exc:
        raise Refused(f"{release} release test suite fixture corpus cannot be verified on this host") from exc
    if (not isinstance(corpus, dict)
            or any(corpus.get(key) != authority[key]
                   for key in ("ops_root", "bootstrap_pin", "corpus", "corpus_tree_sha256", "manifest"))
            or corpus.get("version_independent") is not True
            or corpus.get("verified_before_run") is not True or corpus.get("verified_after_run") is not True
            or corpus.get("link") != str(Path(suite["exiftool_oracle"]["cache_dir"]) / "combined-samples")):
        raise Refused(f"{release} release test suite fixture corpus is not this host's verified combined corpus")
    return corpus


def _release_test_oracle(report: Mapping[str, Any], release: str) -> dict[str, Any]:
    """The suite must have been graded by this side's selected, capable ExifTool.

    The report's native identity is the executor-verified selected release
    (tree, Perl and library). The recorded oracle probe and the suite's
    allowlisted environment must both resolve to exactly that tree.
    """
    suite = report["test_suite"]
    oracle, env, native = suite.get("exiftool_oracle"), suite.get("environment"), report.get("native_identity")
    allowed = set(stage_adapter.TEST_ENVIRONMENT_PASSTHROUGH) | set(stage_adapter.TEST_ENVIRONMENT_SET)
    try:
        cache = oracle["cache_dir"]
        shim = str(Path(cache) / "bin")
        valid = (
            isinstance(native, dict) and isinstance(env, dict)
            and oracle["version"] == release and oracle["docx_filetype"] == "DOCX"
            and oracle["perl_modules_available"] is True
            and oracle["tree"] == str(Path(cache) / "exiftool")
            and oracle["tree_realpath"] == native["source"]["path"]
            and oracle["lib"]["exiftool_pm_sha256"] == native["lib"]["exiftool_pm_sha256"]
            and oracle["perl"] == native["perl"]
            and set(env) <= allowed and set(stage_adapter.TEST_ENVIRONMENT_SET) <= set(env)
            and env["EXIFTOOL_CACHE_DIR"] == cache and env["EXIFTOOL_PERL"] == native["perl"]["path"]
            and env["OXIDEX_RELEASE_REQUIRE_PINNED_FIXTURES"] == "1"
            and env["CARGO_TARGET_DIR"] == suite["target_directory"]
            and isinstance(env.get("PATH"), str) and env["PATH"].split(os.pathsep)[0] == shim
        )
    except (KeyError, TypeError) as exc:
        raise Refused(f"{release} release test suite lacks its ExifTool oracle proof") from exc
    if not valid:
        raise Refused(f"{release} release test suite was not graded by the selected capable ExifTool "
                      "under the allowlisted environment")
    return oracle


def _build_environment_receipt(build: Mapping[str, Any], release: str, checkout: Path) -> dict[str, Any]:
    """The qualified binaries must come from the allowlisted build environment,
    compiled by the rustc release that checkout's own rust-toolchain.toml pins.

    PATH is allowlisted, and a rustc ahead of rustup's proxies on it ignores
    the pin silently, so "some identified rustc" is not enough: the recorded
    ``rustc -vV``/``cargo -V`` must be the pinned release, the pin recorded at
    build time must still be the checkout's file, and both executables must
    embed that rustc's commit (std's /rustc/<commit>/ paths)."""
    recorded = build.get("build_environment")
    allowed = set(stage_adapter.BUILD_ENVIRONMENT_PASSTHROUGH) | set(stage_adapter.BUILD_ENVIRONMENT_SET)
    try:
        env, toolchain, cargo_config = recorded["environment"], recorded["toolchain"], recorded["cargo_config"]
        binary = Path(build["binary"]["path"])
        valid = (
            isinstance(env, dict) and set(env) <= allowed and set(stage_adapter.BUILD_ENVIRONMENT_SET) <= set(env)
            and isinstance(toolchain, dict) and set(toolchain) == {"rustc", "cargo"}
            and toolchain["rustc"].startswith("rustc ") and toolchain["cargo"].startswith("cargo ")
            and isinstance(cargo_config, dict) and cargo_config.get("outside_checkout") == []
            and isinstance(cargo_config.get("checked"), list) and cargo_config["checked"]
            and binary.is_relative_to(Path(env["CARGO_TARGET_DIR"]))
        )
        recorded_pin, compiled_by = recorded["toolchain_pin"], recorded["compiled_by"]
    except (KeyError, TypeError, AttributeError) as exc:
        raise Refused(f"{release} build environment is not recorded") from exc
    if not valid:
        raise Refused(f"{release} build environment is not the allowlisted, identified toolchain build")
    try:
        stage_adapter.validate_pinned_toolchain(
            {"toolchain": toolchain, "toolchain_pin": recorded_pin, "rustc_path": recorded.get("rustc_path"),
             "pin_rustc": recorded.get("pin_rustc")},
            checkout)
        stage_adapter.check_binary_compilers(toolchain, compiled_by)
    except (stage_adapter.Refused, OSError) as exc:
        raise Refused(f"{release} build environment is not the checkout's pinned toolchain: {exc}") from exc
    return recorded


def _regular_receipt(path: Path) -> Path:
    if path.is_symlink() or not path.is_file():
        raise Refused(f"receipt must be an existing regular file: {path}")
    return path


def _replay_read_union(row: Mapping[str, Any], row_output: Path) -> dict[str, Any]:
    row_output = row_output.resolve()
    saved = row.get("read_union")
    if not isinstance(saved, dict):
        raise Refused("committed transition lacks its frozen read union")
    originals = saved.get("original_manifests")
    if not isinstance(originals, dict) or set(originals) != set(SIDES):
        raise Refused("committed transition lacks both original fixture selections")
    try:
        bindings = {side: executor._fixture_binding(
            originals[side]["path"], kind="oxidex_version_rehearsal_fixture_manifest",
            jpeg_only=False) for side in SIDES}
        if bindings != originals:
            raise Refused("original transition fixture selection changed")
        union = read_policy.freeze_union(
            _regular_receipt(Path(originals["before"]["path"])).read_bytes(),
            _regular_receipt(Path(originals["after"]["path"])).read_bytes())
    except (executor.Refused, read_policy.ReadPolicyRefused, KeyError, TypeError) as exc:
        raise Refused(f"original transition read union cannot be replayed: {exc}") from exc
    if union != saved.get("union"):
        raise Refused("committed transition read union differs from frozen selections")
    manifest = saved.get("manifest")
    sidecar = saved.get("sidecar")
    if (not isinstance(manifest, dict) or not isinstance(sidecar, dict)
            or manifest.get("path") != str(row_output / "read-union-manifest.json")
            or sidecar.get("path") != str(row_output / "read-union-sidecar.json")):
        raise Refused("committed transition read union path changed")
    try:
        current = executor._fixture_binding(
            manifest["path"], kind="oxidex_version_rehearsal_fixture_manifest",
            jpeg_only=False)
        detail = _read_object(Path(sidecar["path"]), "read union sidecar")
    except (executor.Refused, KeyError, TypeError) as exc:
        raise Refused(f"committed transition read union is unavailable: {exc}") from exc
    if current != manifest or _sha_file(Path(sidecar["path"])) != sidecar.get("sha256"):
        raise Refused("committed transition read union receipt changed")
    fixtures = union["fixtures"]
    expected_rows = []
    expected_sidecar = []
    for fixture in fixtures:
        source = fixture["sources"]["before"] or fixture["sources"]["after"]
        expected_rows.append({"path": str(Path(source).resolve()), "sha256": fixture["sha256"],
                              "bytes": fixture["bytes"]})
        expected_sidecar.append({"logical_name": fixture["logical_name"],
                                 "sha256": fixture["sha256"], "bytes": fixture["bytes"],
                                 "sources": fixture["sources"], "chosen_source": source})
    if (current["fixtures"] != expected_rows
            or detail != {"schema": 1, "kind": "task19-read-union-sidecar/v1",
                          "union_sha256": union["union_sha256"],
                          "carriers": expected_sidecar}):
        raise Refused("committed transition read union mapping changed")
    return saved


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


def _read_policy_ledgers(read_union: Mapping[str, Any], read: Mapping[str, Any]) -> list[dict[str, Any]]:
    fixture_rows = read.get("fixtures", {}).get("entries")
    raw_binding = read.get("raw_maps")
    if (not isinstance(fixture_rows, list) or not isinstance(raw_binding, dict)
            or not isinstance(raw_binding.get("path"), str)):
        raise Refused("measured read lacks fixture or authenticated raw-map binding")
    raw_path = _regular_receipt(Path(raw_binding["path"]))
    if _sha_file(raw_path) != raw_binding.get("sha256"):
        raise Refused("measured read raw-map receipt changed")
    capture = _read_object(raw_path, "authenticated read raw maps")
    raw_rows = capture.get("rows")
    carriers = read_union["union"]["fixtures"]
    if not isinstance(raw_rows, list) or len(raw_rows) != len(carriers) or len(fixture_rows) != len(carriers):
        raise Refused("authenticated raw maps omit a frozen union carrier")
    indexed = {item.get("fixture", {}).get("corpus_path"): item for item in raw_rows
               if isinstance(item, dict)}
    if len(indexed) != len(carriers) or None in indexed:
        raise Refused("authenticated raw maps repeat a staged carrier")
    ledgers = []
    for carrier, fixture in zip(carriers, fixture_rows, strict=True):
        staged = fixture.get("corpus_path")
        chosen = carrier["sources"]["before"] or carrier["sources"]["after"]
        if (not isinstance(staged, str) or staged not in indexed
                or fixture.get("source") != chosen
                or fixture.get("sha256") != carrier["sha256"]
                or fixture.get("bytes") != carrier["bytes"]):
            raise Refused("authenticated raw map does not bind frozen union fixture")
        raw = indexed[staged]
        if (raw.get("fixture", {}).get("sha256") != carrier["sha256"]
                or raw.get("fixture", {}).get("source") != str(Path(chosen).resolve())):
            raise Refused("authenticated raw map source differs from frozen union")
        try:
            ledgers.append(read_policy.occurrence_ledger(
                carrier, raw["oracle_raw_map"], raw["candidate_raw_map"],
                raw["transcript_row"], native_status=raw["native_status"]))
        except (read_policy.ReadPolicyRefused, KeyError, TypeError) as exc:
            raise Refused(f"authenticated occurrence ledger refused: {exc}") from exc
    return ledgers


def _read_policy_classifications(before: list[dict[str, Any]], after: list[dict[str, Any]],
                                 prior_artifacts: dict[str, str],
                                 later_artifacts: dict[str, str]) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    native_evidence = []
    for old, new in zip(before, after, strict=True):
        fixture = old["fixture"]
        if fixture != new["fixture"]:
            raise Refused("paired occurrence ledgers differ in frozen carrier order")
        old_native = read_policy._keyed_values(old["native"], old["native_status"])
        new_native = read_policy._keyed_values(new["native"], new["native_status"])
        new_classes = read_policy._class_index(new)
        for oracle_key in sorted(old_native.keys() | new_native.keys()):
            prior, later = old_native.get(oracle_key), new_native.get(oracle_key)
            if prior == later:
                continue
            reason = "native-version"
            if prior is None and later is not None:
                native_row = next(row for row in new["native"] if row["oracle_key"] == oracle_key)
                member_class = new_classes[(native_row["group"], native_row["name"], later)]
                if (member_class["matched_count"] < member_class["native_count"]
                        and read_policy._exact_missing_key(new, native_row) is not None):
                    reason = "native-new-unread"
            native_evidence.append({"logical_name": fixture["logical_name"],
                                    "fixture_sha256": fixture["sha256"],
                                    "oracle_key": oracle_key, "before": prior,
                                    "after": later, "reason": reason})
    artifact_evidence = [{"path": path, "before": prior_artifacts.get(path),
                          "after": later_artifacts.get(path), "reason": "generated-artifact"}
                         for path in sorted(prior_artifacts.keys() | later_artifacts.keys())
                         if prior_artifacts.get(path) != later_artifacts.get(path)]
    return native_evidence, artifact_evidence


def _read_policy_pair_body(row_output: Path, read_union: Mapping[str, Any],
                           sides: Mapping[str, Any], floors: Mapping[str, Any]) -> dict[str, Any]:
    ledgers = {}
    reports = {}
    for side in SIDES:
        entry = sides.get(side)
        if not isinstance(entry, dict) or not isinstance(entry.get("release"), str):
            raise Refused("transition read pair lacks side identity")
        run_dir = row_output / side
        journal = _read_object(run_dir / "execution-status.json", "read side journal")
        read = _report_for(run_dir, journal, entry["release"], "read")
        if (entry.get("execution_journal_sha256") != _sha_file(run_dir / "execution-status.json")
                or entry.get("read_report_sha256") != rehearsal.sha256_json(read)
                or entry.get("raw_maps") != read.get("raw_maps")):
            raise Refused("transition read pair differs from authenticated side receipt")
        ledgers[side] = _read_policy_ledgers(read_union, read)
        reports[side] = {"read_report_sha256": entry["read_report_sha256"],
                         "raw_maps": read["raw_maps"]}
    prior_artifacts = {item["path"]: item["sha256"] for item in sides["before"]["generated_artifacts"]}
    later_artifacts = {item["path"]: item["sha256"] for item in sides["after"]["generated_artifacts"]}
    native_evidence, artifact_evidence = _read_policy_classifications(
        ledgers["before"], ledgers["after"], prior_artifacts, later_artifacts)
    mode = ("same-pin" if sides["before"]["release"] == sides["after"]["release"]
            else "historical")
    try:
        proof = read_policy.replay_pair(
            mode=mode, union=read_union["union"], before_ledgers=ledgers["before"],
            after_ledgers=ledgers["after"], payload_floors=dict(floors),
            before_artifacts=prior_artifacts, after_artifacts=later_artifacts,
            native_change_evidence=native_evidence, artifact_change_evidence=artifact_evidence)
    except read_policy.ReadPolicyRefused as exc:
        raise Refused(f"transition read pair refused: {exc}") from exc
    return {"schema": 1, "kind": "oxidex_task19_authenticated_read_pair",
            "read_union_sha256": read_union["union"]["union_sha256"],
            "side_reports": reports, "ledgers": ledgers,
            "native_change_evidence": native_evidence,
            "artifact_change_evidence": artifact_evidence,
            "payload_floors": dict(floors), "proof": proof}


def _save_read_policy_pair(row_output: Path, read_union: Mapping[str, Any],
                           sides: Mapping[str, Any], floors: Mapping[str, Any]) -> dict[str, str]:
    body = _read_policy_pair_body(row_output, read_union, sides, floors)
    path = row_output / "read-policy-pair.json"
    _atomic_json(path, body)
    return {"path": str(path.resolve()), "sha256": _sha_file(path)}


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
        # Guards every liveness record; release waits for an in-flight one and
        # then forbids more, so none can follow the lock release.
        self._heartbeat_lock = threading.RLock()
        self._closing = False
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
            if self._closing or self.file is None or self.file.closed:
                raise Refused("transition lease is not held; no liveness record may follow release")
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
        with self._heartbeat_lock:
            self._closing = True
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
        survivors: list[dict[str, Any]] = []
        if self.file is not None:
            stream, self.file = self.file, None
            try:
                # Children inherit this open file description. LOCK_UN would
                # release it for a still-live child too, so unlock only after
                # every owned child is proven gone; otherwise keep it held.
                survivors = executor.release_or_retain(stream, self._host_lock_capability)
                if survivors:
                    release_status = "retained-unproven-child"
                else:
                    confirmed = True
            except OSError as release_error:
                release_status = "release-failed"
                receipt_failures.append(f"lock release: {release_error}")
            finally:
                if not survivors:
                    try:
                        stream.close()
                    except OSError as close_error:
                        release_status = "release-failed"
                        receipt_failures.append(f"lock close: {close_error}")
        if not confirmed and not survivors:
            receipt_failures.append("lock release was not confirmed")
        try:
            _atomic_json(self.release_receipt, {
                "run_id": self.run_id, "owner": self.owner, "lock_path": str(self.lease),
                "lock_realpath": str(self.lease.resolve()), "released_at": None if survivors else time.time(),
                "release_status": release_status, "terminal_status": terminal,
                "release_reason": "qualification-terminal", "flock_release_confirmed": confirmed,
                "surviving_children": survivors,
                "receipt_failures": receipt_failures,
                "qualification_outcome": "pending",
            })
        except BaseException as receipt_error:
            receipt_failures.append(f"release receipt: {receipt_error}")
        if survivors:
            retained = LeaseRetained(
                "transition lease " + executor.retained_lock_message(self.lease, survivors), survivors,
            )
            if receipt_failures:
                retained.add_note("transition lease receipt failures: " + "; ".join(receipt_failures))
            if exc is not None:
                retained.add_note(f"original exception: {exc!r}")
            raise retained
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
                with self.lease._heartbeat_lock:
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
                        host_lock_fd: executor._HeldHostLock | None = None) -> bool:
    journal_path = run_dir / "execution-status.json"
    if not journal_path.is_file() or journal_path.is_symlink():
        return False
    journal = _read_object(journal_path, "execution journal")
    # A running journal is recoverable mid-stage (active object) and between
    # stages (active null); both must be marked terminal, never left running.
    if journal.get("phase") == "running" and (journal.get("active") is None or isinstance(journal.get("active"), dict)):
        executor.recover(run_dir, archive_cache, source_root, host_lock_fd=host_lock_fd)
        return True
    return False


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
    final_marker = receipt_root / "qualification-result.json"
    if final_marker.exists() or final_marker.is_symlink():
        raise Refused(f"stale receipt reuse is forbidden: {final_marker}")


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


def _replay_committed_read_snapshot(row: Mapping[str, Any], side: str, root: Path) -> None:
    entry = row.get(side)
    if not isinstance(entry, dict) or not isinstance(entry.get("release"), str):
        raise Refused("committed transition side lacks release identity")
    release = entry["release"]
    run_dir = root / row["id"] / side
    journal_path = run_dir / "execution-status.json"
    if entry.get("execution_journal_sha256") != _sha_file(_regular_receipt(journal_path)):
        raise Refused("committed transition execution journal changed")
    journal = _read_object(journal_path, "committed execution journal")
    release_state = journal.get("releases", {}).get(release, {})
    if (journal.get("phase") != "complete"
            or journal.get("scope", {}).get("read_acceptance") != "pending_pair_policy"
            or journal.get("scope", {}).get("write_acceptance") != "passed_per_release"
            or release_state.get("state") != "measured_pending_pair_policy"
            or release_state.get("stages", {}).get("read") != "measured"):
        raise Refused("committed read was not measured pending an authenticated pair verdict")
    config = _read_object(run_dir / "inputs" / "config.json", "committed execution config")
    instrument_row = entry.get("instrument")
    if (journal.get("config_sha256") != rehearsal.sha256_json(config)
            or not isinstance(instrument_row, dict)
            or config.get("execution_source_commit") != instrument_row.get("source_commit")):
        raise Refused("committed transition config differs from read source identity")
    if config.get("read_policy_input") != row.get("read_policy_input"):
        raise Refused("committed transition side differs from frozen read-policy input")
    bundle_path = config.get("verified_input_bundle")
    if not isinstance(bundle_path, str):
        raise Refused("committed transition lacks its verified input bundle")
    locations = _read_object(Path(bundle_path) / "locations.json", "verified input locations")
    if (locations.get("kind") != "oxidex_version_transition_input_locations"
            or not isinstance(locations.get("archive_cache"), str)
            or not isinstance(locations.get("source_root"), str)):
        raise Refused("committed transition input locations are malformed")
    try:
        executor._verify_inputs(run_dir, Path(locations["archive_cache"]),
                                Path(locations["source_root"]))
    except executor.Refused as error:
        raise Refused(f"committed transition materialized native source changed: {error}") from error
    targets = config.get("target_directories")
    if not isinstance(targets, dict) or not isinstance(targets.get(release), str):
        raise Refused("committed transition lacks owned measurement target")
    checkout = run_dir / "checkouts" / executor._safe_name(release)
    generation = _report_for(run_dir, journal, release, "generate")
    build = _report_for(run_dir, journal, release, "build")
    read = _report_for(run_dir, journal, release, "read")
    read_report_path = run_dir / journal["releases"][release]["reports"]["read"]["path"]
    if (read.get("state") != "measured"
            or release_state.get("reports", {}).get("read", {}).get("acceptance") != "pending_pair_policy"):
        raise Refused("committed read report does not remain pending pair policy")
    native_report = _report_for(run_dir, journal, release, "native")
    union = _replay_read_union(row, root / row["id"])
    fixture_entries = read.get("fixtures", {}).get("entries")
    carriers = union["union"]["fixtures"]
    if (read.get("fixtures", {}).get("manifest") != union["manifest"]["path"]
            or read.get("fixtures", {}).get("manifest_sha256") != union["manifest"]["sha256"]
            or not isinstance(fixture_entries, list) or len(fixture_entries) != len(carriers)):
        raise Refused("committed read differs from the common frozen fixture union")
    for entry_row, carrier in zip(fixture_entries, carriers, strict=True):
        chosen = carrier["sources"]["before"] or carrier["sources"]["after"]
        if (entry_row.get("source") != chosen
                or entry_row.get("sha256") != carrier["sha256"]
                or entry_row.get("bytes") != carrier["bytes"]):
            raise Refused("committed read carrier differs from frozen union mapping")
    if entry.get("read_report_sha256") != rehearsal.sha256_json(read):
        raise Refused("committed transition read report changed")
    try:
        source_tree = executor._source_tree(checkout)
        target = Path(targets[release])
        native_identity = read.get("native_identity")
        if (not isinstance(native_identity, dict)
                or instrument_row.get("native_identity") != native_identity
                or build.get("native_identity") != native_identity
                or native_report.get("probe_sha256") != read.get("native_probe_sha256")
                or instrument_row.get("native_probe_sha256") != read.get("native_probe_sha256")):
            raise executor.Refused("read, build and side native identities differ")
        native_source = native_identity.get("source")
        native_lib = native_identity.get("lib")
        native_perl = native_identity.get("perl")
        if (not isinstance(native_source, dict) or not isinstance(native_lib, dict)
                or not isinstance(native_perl, dict)
                or any(not isinstance(part.get("path"), str)
                       for part in (native_source, native_lib, native_perl))):
            raise executor.Refused("committed native identity is malformed")
        native_tuple = (Path(native_source["path"]), Path(native_lib["path"]),
                        Path(native_source["path"]) / "exiftool")
        perl_path = native_perl["path"]
        fixture_row = read.get("fixtures")
        if (not isinstance(fixture_row, dict)
                or instrument_row.get("read_fixture_manifest") != fixture_row.get("manifest")
                or instrument_row.get("read_fixture_manifest_sha256") != fixture_row.get("manifest_sha256")
                or instrument_row.get("read_fixture_count") != len(fixture_row.get("entries", []))):
            raise executor.Refused("committed fixture scope differs from side receipt")
        for stage_result in (build, read):
            executor._require_source_proof(stage_result, checkout, config["execution_source_commit"], source_tree)
            executor._require_generated_artifacts(stage_result, checkout)
            executor._require_binary_proof(stage_result, target)
            executor._require_native_identity(stage_result, release, native_tuple, perl_path)
        if read.get("binary") != build.get("binary"):
            raise executor.Refused("read and build binary identities differ")
        if instrument_row.get("binary") != read.get("binary"):
            raise executor.Refused("side receipt binary differs from authenticated read")
        executor._require_fixture_proof(read)
        executor._require_read_measurement_snapshot(
            read, generation, checkout, target,
            config["execution_source_commit"], source_tree)
        executor._require_read_counts(read)
        executor._require_read_raw_maps(read, read_report_path, native_tuple, perl_path)
    except (executor.Refused, KeyError) as error:
        raise Refused(f"committed transition read measurement replay refused: {error}") from error


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
    matrix = final.get("matrix")
    if (not isinstance(matrix, dict) or set(matrix) != {"path", "sha256"}
            or not isinstance(matrix["path"], str) or Path(matrix["path"]).name != CANONICAL_MATRIX.name
            or not isinstance(matrix["sha256"], str) or re.fullmatch(r"[0-9a-f]{64}", matrix["sha256"]) is None
            or not Path(matrix["path"]).is_file() or _sha_file(Path(matrix["path"])) != matrix["sha256"]):
        raise Refused("qualification final marker does not bind the canonical transition matrix")
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
            or release.get("surviving_children") != []
            or release.get("receipt_failures") != []):
        raise Refused("qualification final marker has invalid cleanup or deadline evidence")
    rows = final.get("rows")
    policy_binding = final.get("read_policy_input")
    caller = final.get("caller")
    if not isinstance(caller, dict) or not isinstance(caller.get("pin_version"), str):
        raise Refused("qualification final marker lacks caller pin identity")
    expected_policy_rows = {item["id"] for item in load_matrix(
        Path(matrix["path"]), caller["pin_version"])["rows"]}
    policy_document = _verify_read_policy_input(policy_binding, expected_policy_rows)
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
        if (row.get("read_policy_input") != policy_binding
                or row.get("read_payload_floors") != policy_document["rows"].get(row["id"])):
            raise Refused("qualification row differs from predeclared read-policy floors")
        _replay_read_union(row, root / row["id"])
        for side in SIDES:
            _replay_committed_read_snapshot(row, side, root)
        saved_pair = row.get("read_policy_pair")
        expected_pair = root / row["id"] / "read-policy-pair.json"
        if (not isinstance(saved_pair, dict)
                or saved_pair.get("path") != str(expected_pair.resolve())
                or saved_pair.get("sha256") != _sha_file(_regular_receipt(expected_pair))):
            raise Refused("committed transition read-pair receipt changed")
        observed_pair = _read_object(expected_pair, "committed transition read pair")
        replayed_pair = _read_policy_pair_body(
            root / row["id"], row["read_union"],
            {side: row[side] for side in SIDES}, row["read_payload_floors"])
        if observed_pair != replayed_pair:
            raise Refused("committed transition read-pair policy replay differs")
    return final


def run_qualification(*, matrix_path: Path, repository: Path, output_root: Path,
                      target_root: Path, lease_path: Path, run_id: str,
                      owner_receipt: Path, heartbeat_receipt: Path,
                      expiry_receipt: Path, release_receipt: Path,
                      handoff_receipt: Path, only: str | None = None,
                      read_policy_input: Path | None = None,
                      execute: Callable[..., dict[str, Any]] = executor.execute) -> dict[str, Any]:
    _validate_receipt_contract(
        output_root=output_root, run_id=run_id, lease_path=lease_path,
        owner_receipt=owner_receipt, heartbeat_receipt=heartbeat_receipt,
        expiry_receipt=expiry_receipt, release_receipt=release_receipt,
        handoff_receipt=handoff_receipt,
    )
    # The caller is this entry point's own checkout, so the qualification,
    # executor and inventory code that runs is the code whose HEAD is recorded.
    if repository.resolve() != REPOSITORY_ROOT.resolve():
        raise Refused(f"caller repository must be the checkout containing this entry point "
                      f"({REPOSITORY_ROOT}), not {repository}")
    if matrix_path.resolve() != CANONICAL_MATRIX.resolve():
        raise Refused(f"only the canonical transition matrix {CANONICAL_MATRIX} may be qualified, "
                      f"not {matrix_path}")
    matrix_binding = {"path": str(CANONICAL_MATRIX), "sha256": _sha_file(CANONICAL_MATRIX)}
    caller = snapshot_caller(repository)
    # The owned checkout is a worktree, but its later signed measurement clone
    # does not inherit repository-local SSH verification configuration. Refuse
    # missing trust before the costly generation/build stages begin.
    try:
        stage_adapter.clean_snapshot._signature_trust(repository)
    except stage_adapter.clean_snapshot.Refused as error:
        raise Refused(str(error)) from error
    pinned = caller["pin_version"]
    matrix = materialize_matrix(load_matrix(CANONICAL_MATRIX, pinned), output_root=output_root,
                                target_root=target_root, run_id=run_id)
    rows = [row for row in matrix["rows"] if only is None or row["id"] == only]
    if not rows:
        raise Refused(f"matrix row is not selected: {only}")
    if read_policy_input is None:
        raise Refused("Task19 read-policy input must be frozen before transition execution")
    policy_document, policy_binding = _read_policy_input(
        read_policy_input, {row["id"] for row in matrix["rows"]})
    source_commit = caller["head"]
    frozen_inputs: dict[str, dict[str, dict[str, Any]]] = {}
    for row in rows:
        frozen_inputs[row["id"]] = {}
        for side in SIDES:
            frozen = _freeze_side_inputs(row, side)
            if frozen["identity"]["documents"]["plan"].get("repository_commit") != source_commit:
                raise Refused("verified transition plan is not bound to the caller execution source commit")
            frozen_inputs[row["id"]][side] = frozen
    frozen_unions = {row["id"]: _freeze_read_union(frozen_inputs[row["id"]]["before"],
                                                    frozen_inputs[row["id"]]["after"])
                     for row in rows}
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
                read_union = _materialize_read_union(
                    frozen_unions[row["id"]], frozen_inputs[row["id"]]["before"],
                    frozen_inputs[row["id"]]["after"], row_output)
                sides: dict[str, dict[str, Any]] = {}
                for side, release in zip(SIDES, (row["before_version"], row["after_version"]), strict=True):
                    cadence.position(row["id"], side)
                    host_lease.heartbeat("side-start", row["id"], side)
                    frozen = frozen_inputs[row["id"]][side]
                    _verify_frozen_side(frozen)
                    identity = frozen["identity"]
                    read_manifest = Path(read_union["manifest"]["path"])
                    write_manifest = Path(frozen["write_manifest"])
                    native_cases = frozen["native_cases"]
                    side_run = row_output / side
                    side_target = row_target / side
                    documents = identity["documents"]
                    config = _side_config(
                        release=release, source_commit=source_commit, perl=perl,
                        read_manifest=read_manifest, write_manifest=write_manifest,
                        native_cases=native_cases, lease=lease_path, target=side_target,
                        verified_input_bundle=Path(identity["bundle"]),
                        read_policy_input=policy_binding,
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
                            _verify_read_policy_input(
                                policy_binding, {item["id"] for item in matrix["rows"]})
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
                            if getattr(execution_error, "_oxidex_owned_child_cleanup", None) == "incomplete":
                                raise executor.Refused(
                                    "owned child cleanup is incomplete; refusing durable interruption recovery",
                                )
                            recovered = _recover_if_running(
                                side_run, Path(identity["archive_cache"]), Path(identity["source_root"]),
                                host_lock_fd=host_lease.host_lock_capability,
                            )
                        except BaseException as recovery_error:
                            if isinstance(execution_error, KeyboardInterrupt):
                                setattr(execution_error, "_oxidex_durable_recovery", "incomplete")
                            if hasattr(execution_error, "add_note"):
                                execution_error.add_note(f"durable interruption recovery failed: {recovery_error}")
                        else:
                            if isinstance(execution_error, KeyboardInterrupt):
                                setattr(
                                    execution_error, "_oxidex_durable_recovery",
                                    "recovered" if recovered else "not-required",
                                )
                        raise
                    _verify_frozen_side(frozen)
                    _verify_read_policy_input(policy_binding,
                                              {item["id"] for item in matrix["rows"]})
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
                read_pair = _save_read_policy_pair(
                    row_output, read_union, sides, policy_document["rows"][row["id"]])
                host_lease.guard()
                cadence.check()
                for frozen in frozen_inputs[row["id"]].values():
                    _verify_frozen_side(frozen)
                result = {"id": row["id"], "before": sides["before"], "after": sides["after"],
                          "read_union": read_union,
                          "read_policy_input": policy_binding,
                          "read_payload_floors": policy_document["rows"][row["id"]],
                          "read_policy_pair": read_pair,
                          "artifact_delta": delta, "caller_restored": True,
                          "promotion": "forbidden", "qualification_outcome": "pending"}
                _atomic_json(row_output / "transition-result.json", result)
                results.append(result)
                verify_caller(caller)
            for row_inputs in frozen_inputs.values():
                for frozen in row_inputs.values():
                    _verify_frozen_side(frozen)
            _verify_read_policy_input(policy_binding,
                                      {item["id"] for item in matrix["rows"]})
            host_lease.guard()
            cadence.check()
            final = {
                "schema": SCHEMA, "kind": RESULT_KIND, "run_id": run_id,
                "promotion": "forbidden", "status": "tooling-executed-nonpromoting",
                "caller": caller, "matrix": matrix_binding, "rows": results, "caller_restored": True,
                "read_policy_input": policy_binding,
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
    if _sha_file(CANONICAL_MATRIX) != matrix_binding["sha256"]:
        raise Refused("canonical transition matrix changed during qualification")
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
    parser.add_argument("--read-policy-input")
    return parser


def _instrument_header(result: Mapping[str, Any]) -> str:
    """Attribute every reported comparison to its validated build and oracle."""
    caller = result["caller"]
    lines = ["=== instrument: version_transition_qualification.py ===",
             f"caller:  {caller['head']}  pin {caller['pin_version']}  tree {caller['status']}",
             f"matrix:  {result['matrix']['path']} sha256={result['matrix']['sha256']}"]
    for row in result["rows"]:
        for side in SIDES:
            entry = row[side]
            proof = entry["instrument"]
            native = proof["native_identity"]
            binary = proof["binary"]
            capability = proof["capability_probe"]
            tests = entry["release_tests"]
            label = f"{row['id']}/{side}"
            lines.extend((
                f"{label}: OxiDex {binary['path']} sha256={binary['sha256']} source={proof['source_commit']}",
                f"{label}: ExifTool {native['release']} source={native['source']['path']} "
                f"lib_sha256={native['lib']['exiftool_pm_sha256']} perl={native['perl']['path']} "
                f"probe_sha256={proof['native_probe_sha256']}",
                f"{label}: capability {capability['state']} -ver={capability['version']} "
                f"OOXML.docx={capability['docx_filetype']} perl-modules="
                f"{'available' if capability['perl_modules_available'] else 'missing'}",
                f"{label}: tests passed={tests['passed']} failed={tests['failed']} "
                f"ignored={tests['ignored']} targets={tests['targets']} log={tests['log']['path']}",
                f"{label}: tests graded by ExifTool {tests['exiftool_oracle']['version']} "
                f"tree={tests['exiftool_oracle']['tree_realpath']} "
                f"OOXML.docx={tests['exiftool_oracle']['docx_filetype']}",
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
            read_policy_input=Path(args.read_policy_input) if args.read_policy_input else None,
        )
    except LeaseRetained as exc:
        print(f"version transition qualification stopped fail-closed: {exc}", file=sys.stderr)
        return 5
    except KeyboardInterrupt as interruption:
        notes = list(getattr(interruption, "__notes__", []))
        recovery = getattr(interruption, "_oxidex_durable_recovery", None)
        if recovery == "recovered":
            message = "version transition qualification interrupted after durable recovery"
        elif recovery == "not-required":
            message = "version transition qualification interrupted; no running executor journal required recovery"
        else:
            message = ("version transition qualification interrupted; durable recovery incomplete or "
                       "unverified; inspect the durable execution journal and recovery receipts before retrying")
        print(message, file=sys.stderr)
        for note in notes:
            print(f"recovery detail: {note}", file=sys.stderr)
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
