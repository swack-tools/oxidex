"""Validate OxiDex release receipts structurally and semantically.

The JSON schema files beside the templates document their public shape. This
module deliberately uses only the standard library so release gates can run in
a fresh checkout without installing a schema package. Semantic checks are the
authoritative gate: a structurally plausible ``verified`` receipt still fails
when its identity, evidence, floors, workflows, or authorization are missing.
"""

from __future__ import annotations

import argparse
import functools
import hashlib
import json
import pathlib
import re
import subprocess
import sys
from collections.abc import Sequence
from typing import Any


KINDS = ("parity", "documentation", "finalization")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
ROOT = pathlib.Path(__file__).resolve().parents[2]
SCHEMAS = {
    "parity": ROOT / ".claude/skills/exiftool-parity/templates/release-parity-receipt.schema.json",
    "documentation": ROOT / ".claude/skills/oxidex-release-documentation/templates/documentation-release-receipt.schema.json",
    "finalization": ROOT / ".claude/skills/oxidex-release-finalization/templates/release-finalization-receipt.schema.json",
}

TARGET_ASSETS = {
    "aarch64-apple-darwin": ("oxidex-universal-apple-darwin", "oxidex-v{version}.dmg"),
    "x86_64-apple-darwin": ("oxidex-universal-apple-darwin", "oxidex-v{version}.dmg"),
    "x86_64-unknown-linux-musl": ("oxidex-x86_64-unknown-linux-musl",),
    "aarch64-unknown-linux-musl": ("oxidex-aarch64-unknown-linux-musl",),
    "x86_64-pc-windows-gnu": ("oxidex-x86_64-pc-windows-gnu.exe",),
}
REQUIRED_TARGETS = tuple(TARGET_ASSETS)
VERSION_LITERAL_RE = r"(^|[^[:alnum:]_])[vV]?[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?"
VERSION_FIELDS_RE = r"\[package\]|version[[:space:]]*=|VERSION|__version__"
INDEPENDENT_WORKSPACE_PACKAGES = frozenset({"oxidex-tags-shared"})


def load_receipt(path: str | pathlib.Path) -> dict[str, Any]:
    payload = json.loads(pathlib.Path(path).read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise ValueError("receipt: expected a JSON object")
    return payload


def _evidence_path(value: str) -> pathlib.Path:
    path = pathlib.Path(value)
    return path if path.is_absolute() else ROOT / path


def _file_sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


@functools.lru_cache(maxsize=1)
def _workspace_packages() -> dict[str, tuple[str, str]]:
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    payload = json.loads(result.stdout)
    packages: dict[str, tuple[str, str]] = {}
    for package in payload["packages"]:
        manifest = pathlib.Path(package["manifest_path"]).resolve().relative_to(ROOT)
        packages[manifest.as_posix()] = (package["name"], package["version"])
    return packages


def _git_grep_scan(pattern: str) -> dict[str, Any]:
    result = subprocess.run(
        [
            "git", "grep", "-n", "-I", "-E", pattern, "--", ".",
            ":(exclude)tools/ci/testdata/release_receipts/**",
        ],
        cwd=ROOT,
        check=False,
        capture_output=True,
    )
    if result.returncode not in (0, 1):
        raise subprocess.CalledProcessError(result.returncode, result.args, result.stdout, result.stderr)
    lines = result.stdout.splitlines()
    files = {line.split(b":", 1)[0] for line in lines}
    return {
        "sha256": hashlib.sha256(result.stdout).hexdigest(),
        "matching_lines": len(lines),
        "tracked_files": len(files),
    }


@functools.lru_cache(maxsize=1)
def _version_literal_scan() -> dict[str, Any]:
    return _git_grep_scan(VERSION_LITERAL_RE)


@functools.lru_cache(maxsize=1)
def _version_fields_scan() -> dict[str, Any]:
    return _git_grep_scan(VERSION_FIELDS_RE)


def _package_version_line(path: pathlib.Path, version: str) -> int | None:
    in_package = False
    pattern = re.compile(r'^version\s*=\s*"' + re.escape(version) + r'"\s*$')
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        stripped = line.strip()
        if stripped == "[package]":
            in_package = True
            continue
        if in_package and stripped.startswith("["):
            return None
        if in_package and pattern.fullmatch(stripped):
            return number
    return None


def _validate_upstream_receipt(
    checks: "_Checks",
    *,
    prefix: str,
    kind: str,
    expected_version: str | None,
    expected_sha: str | None,
    expected_tree: str | None,
) -> None:
    raw_path = checks.string(f"{prefix}.path")
    declared_hash = checks.sha256(f"{prefix}.sha256")
    if raw_path is None:
        return
    path = _evidence_path(raw_path)
    try:
        actual_hash = _file_sha256(path)
        upstream = load_receipt(path)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        checks.error(f"{prefix}.path", f"cannot load referenced receipt: {exc}")
        return
    if declared_hash is not None and declared_hash != actual_hash:
        checks.error(f"{prefix}.sha256", f"does not match referenced receipt {actual_hash}")
    for error in validate_receipt(
        kind, upstream, expected_version=expected_version, expected_sha=expected_sha
    ):
        checks.error(f"{prefix}.receipt", error)
    identity_field = "oxidex_sha" if kind == "parity" else "candidate_sha"
    tree_field = "oxidex_tree" if kind == "parity" else "candidate_tree"
    if checks.value(f"{prefix}.measured_sha") is not None and upstream.get(
        identity_field
    ) != checks.value(f"{prefix}.measured_sha"):
        checks.error(f"{prefix}.measured_sha", f"does not match receipt {identity_field}")
    if expected_tree is not None and upstream.get(tree_field) != expected_tree:
        checks.error(f"{prefix}.measured_tree", f"receipt {tree_field} must match {expected_tree}")
    if checks.value(f"{prefix}.measured_tree") is not None and upstream.get(
        tree_field
    ) != checks.value(f"{prefix}.measured_tree"):
        checks.error(f"{prefix}.measured_tree", f"does not match receipt {tree_field}")


class _Checks:
    def __init__(self, payload: dict[str, Any]):
        self.payload = payload
        self.errors: list[str] = []

    def value(self, path: str) -> Any:
        value: Any = self.payload
        for part in path.split("."):
            if not isinstance(value, dict) or part not in value:
                return None
            value = value[part]
        return value

    def error(self, path: str, message: str) -> None:
        self.errors.append(f"{path}: {message}")

    def equal(self, path: str, expected: Any) -> None:
        value = self.value(path)
        if value != expected:
            self.error(path, f"expected {expected!r}, got {value!r}")

    def string(self, path: str) -> str | None:
        value = self.value(path)
        if not isinstance(value, str) or not value.strip():
            self.error(path, "expected a non-empty string")
            return None
        return value

    def commit(self, path: str) -> str | None:
        value = self.value(path)
        if not isinstance(value, str) or COMMIT_RE.fullmatch(value) is None:
            self.error(path, "expected a full lowercase 40-character commit SHA")
            return None
        return value

    def sha256(self, path: str) -> str | None:
        value = self.value(path)
        if not isinstance(value, str) or SHA256_RE.fullmatch(value) is None:
            self.error(path, "expected a lowercase 64-character SHA-256")
            return None
        return value

    def positive_int(self, path: str) -> int | None:
        value = self.value(path)
        if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
            self.error(path, "expected an integer greater than zero")
            return None
        return value

    def nonnegative_int(self, path: str) -> int | None:
        value = self.value(path)
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            self.error(path, "expected a non-negative integer")
            return None
        return value

    def object(self, path: str) -> dict[str, Any]:
        value = self.value(path)
        if not isinstance(value, dict):
            self.error(path, "expected an object")
            return {}
        return value

    def nonempty_list(self, path: str) -> list[Any]:
        value = self.value(path)
        if not isinstance(value, list) or not value:
            self.error(path, "expected a non-empty array")
            return []
        return value

    def string_list(self, path: str) -> list[str]:
        values = self.nonempty_list(path)
        for index, value in enumerate(values):
            if not isinstance(value, str) or not value.strip():
                self.error(f"{path}[{index}]", "expected a non-empty string")
        return [value for value in values if isinstance(value, str) and value.strip()]

    def object_list(self, path: str) -> list[dict[str, Any]]:
        values = self.nonempty_list(path)
        result: list[dict[str, Any]] = []
        for index, value in enumerate(values):
            if not isinstance(value, dict):
                self.error(f"{path}[{index}]", "expected an object")
            else:
                result.append(value)
        return result

    def empty_list(self, path: str) -> None:
        value = self.value(path)
        if value != []:
            self.error(path, "expected an empty array")


def _schema_type_matches(value: Any, expected: str) -> bool:
    return {
        "null": value is None,
        "object": isinstance(value, dict),
        "array": isinstance(value, list),
        "string": isinstance(value, str),
        "boolean": isinstance(value, bool),
        "integer": isinstance(value, int) and not isinstance(value, bool),
        "number": isinstance(value, (int, float)) and not isinstance(value, bool),
    }.get(expected, False)


def _schema_errors(
    value: Any, schema: dict[str, Any], root: dict[str, Any], path: str = "receipt"
) -> list[str]:
    """Validate the JSON-Schema subset used by the tracked receipt schemas."""

    if "$ref" in schema:
        reference = schema["$ref"]
        if not isinstance(reference, str) or not reference.startswith("#/"):
            return [f"{path}: unsupported schema reference {reference!r}"]
        target: Any = root
        for part in reference[2:].split("/"):
            target = target.get(part) if isinstance(target, dict) else None
        if not isinstance(target, dict):
            return [f"{path}: unresolved schema reference {reference!r}"]
        return _schema_errors(value, target, root, path)

    errors: list[str] = []
    expected_types = schema.get("type")
    if isinstance(expected_types, str):
        expected_types = [expected_types]
    if isinstance(expected_types, list) and not any(
        isinstance(item, str) and _schema_type_matches(value, item)
        for item in expected_types
    ):
        return [f"{path}: expected schema type {expected_types!r}"]
    if "const" in schema and value != schema["const"]:
        errors.append(f"{path}: expected constant {schema['const']!r}")
    if "enum" in schema and value not in schema["enum"]:
        errors.append(f"{path}: expected one of {schema['enum']!r}")
    if isinstance(value, str):
        if len(value) < schema.get("minLength", 0):
            errors.append(f"{path}: string is too short")
        pattern = schema.get("pattern")
        if isinstance(pattern, str) and re.fullmatch(pattern, value) is None:
            errors.append(f"{path}: does not match {pattern!r}")
    if isinstance(value, int) and not isinstance(value, bool):
        if "minimum" in schema and value < schema["minimum"]:
            errors.append(f"{path}: below minimum {schema['minimum']}")
    if isinstance(value, list):
        if len(value) < schema.get("minItems", 0):
            errors.append(f"{path}: expected at least {schema['minItems']} items")
        if "maxItems" in schema and len(value) > schema["maxItems"]:
            errors.append(f"{path}: expected at most {schema['maxItems']} items")
        if schema.get("uniqueItems"):
            serialized = [json.dumps(item, sort_keys=True) for item in value]
            if len(serialized) != len(set(serialized)):
                errors.append(f"{path}: items must be unique")
        item_schema = schema.get("items")
        if isinstance(item_schema, dict):
            for index, item in enumerate(value):
                errors.extend(_schema_errors(item, item_schema, root, f"{path}[{index}]"))
    if isinstance(value, dict):
        required = schema.get("required", [])
        for key in required:
            if key not in value:
                errors.append(f"{path}.{key}: required by schema")
        properties = schema.get("properties", {})
        if isinstance(properties, dict):
            for key, child_schema in properties.items():
                if key in value and isinstance(child_schema, dict):
                    errors.extend(_schema_errors(value[key], child_schema, root, f"{path}.{key}"))
            if schema.get("additionalProperties") is False:
                for key in value.keys() - properties.keys():
                    errors.append(f"{path}.{key}: additional property is not allowed")
    for child in schema.get("allOf", []):
        if isinstance(child, dict):
            errors.extend(_schema_errors(value, child, root, path))
    condition = schema.get("if")
    consequent = schema.get("then")
    if isinstance(condition, dict) and isinstance(consequent, dict):
        if not _schema_errors(value, condition, root, path):
            errors.extend(_schema_errors(value, consequent, root, path))
    return errors


def validate_schema(kind: str, payload: dict[str, Any]) -> list[str]:
    schema = json.loads(SCHEMAS[kind].read_text(encoding="utf-8"))
    return _schema_errors(payload, schema, schema)


def _validate_template(kind: str, payload: dict[str, Any]) -> list[str]:
    checks = _Checks(payload)
    checks.equal("schema_version", 1)
    if payload.get("status") == "verified":
        checks.error("status", "templates must not ship as verified")
    identity = {
        "parity": "oxidex_sha",
        "documentation": "candidate_sha",
        "finalization": "candidate_sha",
    }[kind]
    if identity not in payload:
        checks.error(identity, "missing template identity field")
    return checks.errors


def _validate_common(
    checks: _Checks,
    *,
    version_path: str,
    sha_path: str,
    expected_version: str | None,
    expected_sha: str | None,
) -> None:
    checks.equal("schema_version", 1)
    checks.equal("status", "verified")
    version = checks.string(version_path)
    sha = checks.commit(sha_path)
    if expected_version is not None and version != expected_version:
        checks.error(version_path, f"does not match requested version {expected_version!r}")
    if expected_sha is not None and sha != expected_sha:
        checks.error(sha_path, f"does not match requested candidate {expected_sha!r}")


def _validate_parity(payload: dict[str, Any], version: str | None, sha: str | None) -> list[str]:
    checks = _Checks(payload)
    _validate_common(
        checks,
        version_path="version",
        sha_path="oxidex_sha",
        expected_version=version,
        expected_sha=sha,
    )
    checks.commit("oxidex_tree")
    checks.equal("source_dirty", False)
    exiftool_version = checks.string("exiftool_version")
    checks.string("binary.path")
    checks.sha256("binary.sha256")
    checks.string("binary.build_proof_path")
    checks.sha256("binary.build_proof_sha256")
    checks.equal("oracle.status", "verified")
    for path in (
        "oracle.perl_path",
        "oracle.perl_version",
        "oracle.tree_path",
        "oracle.library_fingerprint_path",
    ):
        checks.string(path)
    for path in (
        "oracle.perl_sha256",
        "oracle.script_sha256",
        "oracle.library_fingerprint_sha256",
    ):
        checks.sha256(path)
    checks.positive_int("oracle.library_file_count")
    checks.equal("oracle.perl_version", "v5.38.2")
    checks.equal("oracle.exiftool_version", exiftool_version)
    checks.equal("oracle.docx_file_type", "DOCX")
    probes = checks.string_list("oracle.probes")
    for required in ("perl-version", "required-modules", "exiftool-version", "docx"):
        if required not in probes:
            checks.error("oracle.probes", f"missing required probe {required!r}")
    corpora = checks.object_list("corpora")
    for index, corpus in enumerate(corpora):
        for field in ("path",):
            if not isinstance(corpus.get(field), str) or not corpus[field].strip():
                checks.error(f"corpora[{index}].{field}", "expected a non-empty string")
        if not isinstance(corpus.get("manifest_sha256"), str) or SHA256_RE.fullmatch(
            corpus["manifest_sha256"]
        ) is None:
            checks.error(f"corpora[{index}].manifest_sha256", "expected a SHA-256")
        if isinstance(corpus.get("files"), bool) or not isinstance(corpus.get("files"), int) or corpus["files"] <= 0:
            checks.error(f"corpora[{index}].files", "expected an integer greater than zero")
    runs = checks.object_list("runs")
    seen_instruments: set[str] = set()
    for index, run in enumerate(runs):
        instrument = run.get("instrument")
        if not isinstance(instrument, str) or not instrument.strip():
            checks.error(f"runs[{index}].instrument", "expected a non-empty string")
        else:
            seen_instruments.add(instrument)
        if not isinstance(run.get("artifact_sha256"), str) or SHA256_RE.fullmatch(
            run["artifact_sha256"]
        ) is None:
            checks.error(f"runs[{index}].artifact_sha256", "expected a SHA-256")
    for required in ("conformance", "authenticated_reads", "generated_catalog", "write_matrix"):
        if required not in seen_instruments:
            checks.error("runs", f"missing {required!r} run")
    for family in ("conformance", "authenticated_reads", "generated_catalog", "write_matrix"):
        checks.equal(f"{family}.status", "verified")
    checks.string_list("conformance.artifacts")
    checks.positive_int("conformance.files")
    checks.positive_int("conformance.native_scored_denominator")
    checks.string_list("authenticated_reads.receipts")
    checks.string_list("authenticated_reads.verification_runs")
    metric = checks.positive_int("authenticated_reads.metric_c")
    floor = checks.positive_int("authenticated_reads.native_occurrence_floor")
    checks.string("authenticated_reads.native_occurrence_floor_evidence")
    checks.string("authenticated_reads.credited_coordinates_artifact")
    if metric is not None and floor is not None and metric < floor:
        checks.error(
            "authenticated_reads.metric_c",
            f"{metric} is below native occurrence floor {floor}",
        )
    checks.equal("authenticated_reads.published_read_gate.status", "verified")
    checks.equal("authenticated_reads.published_read_gate.lost_reads", 0)
    checks.string_list("generated_catalog.artifacts")
    checks.equal("generated_catalog.ratchet_exit_code", 0)
    checks.string_list("write_matrix.artifacts")
    checks.equal("write_matrix.report_exit_code", 0)
    checks.equal("regressions.status", "verified")
    checks.commit("regressions.base_sha")
    checks.equal("regressions.head_sha", payload.get("oxidex_sha"))
    checks.equal("regressions.new_losses", 0)
    checks.equal("regressions.new_value_differences", 0)
    checks.empty_list("refusals")
    checks.empty_list("unresolved")
    return checks.errors


def _validate_documentation(
    payload: dict[str, Any], version: str | None, sha: str | None
) -> list[str]:
    checks = _Checks(payload)
    _validate_common(
        checks,
        version_path="version",
        sha_path="candidate_sha",
        expected_version=version,
        expected_sha=sha,
    )
    checks.commit("candidate_tree")
    checks.equal("parity_receipt.status", "verified")
    measured = checks.commit("parity_receipt.measured_sha")
    if measured is not None and measured != payload.get("candidate_sha"):
        checks.error("parity_receipt.measured_sha", "must match candidate_sha")
    _validate_upstream_receipt(
        checks,
        prefix="parity_receipt",
        kind="parity",
        expected_version=payload.get("version"),
        expected_sha=payload.get("candidate_sha"),
        expected_tree=payload.get("candidate_tree"),
    )
    claims = checks.object_list("claims")
    for index, claim in enumerate(claims):
        for field in ("claim", "evidence_path"):
            if not isinstance(claim.get(field), str) or not claim[field].strip():
                checks.error(f"claims[{index}].{field}", "expected a non-empty string")
        if claim.get("status") != "verified":
            checks.error(f"claims[{index}].status", "expected 'verified'")
    pages = checks.object_list("pages")
    for index, page in enumerate(pages):
        if not isinstance(page.get("route"), str) or not page["route"].strip():
            checks.error(f"pages[{index}].route", "expected a non-empty string")
        if page.get("disposition") not in {"current", "historical", "excluded"}:
            checks.error(f"pages[{index}].disposition", "expected current, historical, or excluded")
        if page.get("status") != "verified":
            checks.error(f"pages[{index}].status", "expected 'verified'")
    benchmarks = checks.nonempty_list("benchmarks")
    allowed_dispositions = {"candidate", "historical", "not_applicable"}
    for index, benchmark in enumerate(benchmarks):
        disposition = benchmark.get("disposition") if isinstance(benchmark, dict) else None
        if disposition not in allowed_dispositions:
            checks.error(
                f"benchmarks[{index}].disposition",
                f"expected one of {sorted(allowed_dispositions)!r}",
            )
        if disposition == "not_applicable":
            reason = benchmark.get("reason")
            if not isinstance(reason, str) or not reason.strip():
                checks.error(
                    f"benchmarks[{index}].reason",
                    "expected a non-empty reason when disposition is 'not_applicable'",
                )
    checks.equal("local_build.status", "verified")
    checks.equal("local_build.candidate_sha", payload.get("candidate_sha"))
    for path in (
        "local_build.command",
        "local_build.snapshot_path",
        "local_build.dist_path",
        "local_build.input_manifest",
        "local_build.source_inventory",
        "local_build.generated_inventory",
        "local_build.rendered_inventory",
        "local_build.reconciliation",
        "local_build.crawl_evidence",
        "local_build.evidence_path",
    ):
        checks.string(path)
    checks.equal("visual_review.status", "verified")
    checks.string("visual_review.automation_manifest")
    samples = checks.object_list("visual_review.samples")
    for index, sample in enumerate(samples):
        for field in ("route", "viewport", "theme"):
            if not isinstance(sample.get(field), str) or not sample[field].strip():
                checks.error(f"visual_review.samples[{index}].{field}", "expected a non-empty string")
        if sample.get("status") != "verified":
            checks.error(f"visual_review.samples[{index}].status", "expected 'verified'")
    checks.empty_list("visual_review.findings")
    checks.equal("visual_review.human_review.status", "verified")
    for path in (
        "visual_review.human_review.reviewer",
        "visual_review.human_review.reviewed_at",
        "visual_review.human_review.screenshot_manifest",
        "visual_review.human_review.evidence_path",
    ):
        checks.string(path)
    checks.equal("pages_pipeline.status", "verified")
    checks.equal("pages_pipeline.build_type", "workflow")
    checks.equal("pages_pipeline.https_enforced", True)
    workflow_sha = checks.commit("pages_pipeline.workflow_sha")
    if workflow_sha is not None and workflow_sha != payload.get("candidate_sha"):
        checks.error("pages_pipeline.workflow_sha", "must match candidate_sha")
    checks.string("pages_pipeline.evidence_path")
    checks.empty_list("unresolved")
    return checks.errors


def _validate_finalization(
    payload: dict[str, Any], version: str | None, sha: str | None
) -> list[str]:
    checks = _Checks(payload)
    _validate_common(
        checks,
        version_path="version",
        sha_path="candidate_sha",
        expected_version=version,
        expected_sha=sha,
    )
    candidate_tree = checks.commit("candidate_tree")
    main_sha = checks.commit("main_sha")
    main_tree = checks.commit("main_tree")
    checks.string("candidate_cargo_target_dir")
    tag = checks.string("tag")
    if version is not None and tag is not None and tag != f"v{version}":
        checks.error("tag", f"expected 'v{version}'")
    for receipt in ("parity", "documentation"):
        checks.equal(f"receipts.{receipt}.status", "verified")
        measured_sha = checks.commit(f"receipts.{receipt}.measured_sha")
        measured_tree = checks.commit(f"receipts.{receipt}.measured_tree")
        expected_receipt_sha = (
            payload.get("candidate_sha") if candidate_tree == main_tree else main_sha
        )
        if measured_sha is not None and measured_sha != expected_receipt_sha:
            checks.error(
                f"receipts.{receipt}.measured_sha",
                f"expected evidence for {expected_receipt_sha!r}",
            )
        if measured_tree is not None and measured_tree != main_tree:
            checks.error(f"receipts.{receipt}.measured_tree", "must match main_tree")
        _validate_upstream_receipt(
            checks,
            prefix=f"receipts.{receipt}",
            kind=receipt,
            expected_version=payload.get("version"),
            expected_sha=expected_receipt_sha,
            expected_tree=main_tree,
        )
    expected_prerelease = isinstance(payload.get("version"), str) and "-" in payload["version"]
    checks.equal("packaging.prerelease", expected_prerelease)
    checks.equal("packaging.latest", not expected_prerelease)
    targets = checks.string_list("packaging.targets")
    expected_assets = checks.string_list("packaging.expected_assets")
    if len(targets) != len(set(targets)):
        checks.error("packaging.targets", "contains duplicates")
    if sorted(targets) != sorted(REQUIRED_TARGETS):
        checks.error(
            "packaging.targets",
            f"must exactly match the reviewed release matrix: {sorted(REQUIRED_TARGETS)!r}",
        )
    derived_assets = {"SHA256SUMS", f"oxidex-v{payload.get('version')}.sbom.cdx.json"}
    for target in targets:
        patterns = TARGET_ASSETS.get(target)
        if patterns is None:
            checks.error("packaging.targets", f"unsupported release target {target!r}")
            continue
        for pattern in patterns:
            derived_assets.add(pattern.format(version=payload.get("version")))
    if sorted(expected_assets) != sorted(derived_assets):
        checks.error(
            "packaging.expected_assets",
            f"must equal assets derived from targets: {sorted(derived_assets)!r}",
        )
    inventory = checks.object_list("version_inventory")
    current_packages = 0
    inventory_packages: dict[str, dict[str, Any]] = {}
    try:
        workspace_packages = _workspace_packages()
    except (OSError, subprocess.CalledProcessError, json.JSONDecodeError, KeyError, ValueError) as exc:
        checks.error("version_inventory", f"cannot inspect cargo workspace: {exc}")
        workspace_packages = {}
    for index, item in enumerate(inventory):
        for field in ("path", "version", "literal", "kind", "disposition", "reason", "evidence_path"):
            if not isinstance(item.get(field), str) or not item[field].strip():
                checks.error(f"version_inventory[{index}].{field}", "expected a non-empty string")
        if isinstance(item.get("line"), bool) or not isinstance(item.get("line"), int) or item["line"] <= 0:
            checks.error(f"version_inventory[{index}].line", "expected an integer greater than zero")
        if item.get("status") != "verified":
            checks.error(f"version_inventory[{index}].status", "expected 'verified'")
        if item.get("disposition") not in {
            "current", "historical", "independent", "dependency", "excluded"
        }:
            checks.error(f"version_inventory[{index}].disposition", "unsupported disposition")
        if item.get("kind") == "oxidex_package" and item.get("disposition") == "current":
            current_packages += 1
            if item.get("version") != payload.get("version"):
                checks.error(
                    f"version_inventory[{index}].version", "current OxiDex package must match release version"
                )
        raw_path = item.get("path")
        if isinstance(raw_path, str) and raw_path:
            path = _evidence_path(raw_path)
            try:
                relative = path.resolve().relative_to(ROOT).as_posix()
            except (OSError, ValueError):
                checks.error(f"version_inventory[{index}].path", "must resolve inside the release checkout")
                relative = None
            if not path.is_file():
                checks.error(f"version_inventory[{index}].path", "referenced file does not exist")
            if relative is not None and item.get("kind") in {"oxidex_package", "independent_package"}:
                if relative in inventory_packages:
                    checks.error(f"version_inventory[{index}].path", "duplicate workspace manifest")
                inventory_packages[relative] = item
                package = workspace_packages.get(relative)
                if package is None:
                    checks.error(f"version_inventory[{index}].path", "not a cargo workspace manifest")
                else:
                    expected_kind = (
                        "independent_package"
                        if package[0] in INDEPENDENT_WORKSPACE_PACKAGES
                        else "oxidex_package"
                    )
                    expected_disposition = "current" if expected_kind == "oxidex_package" else "independent"
                    if item.get("kind") != expected_kind:
                        checks.error(f"version_inventory[{index}].kind", f"expected {expected_kind!r} from cargo metadata")
                    if item.get("disposition") != expected_disposition:
                        checks.error(f"version_inventory[{index}].disposition", f"expected {expected_disposition!r}")
                    if item.get("version") != package[1]:
                        checks.error(
                            f"version_inventory[{index}].version",
                            f"does not match cargo metadata version {package[1]!r}",
                        )
                    expected_line = _package_version_line(path, package[1]) if path.is_file() else None
                    if item.get("line") != expected_line:
                        checks.error(f"version_inventory[{index}].line", f"expected [package] version declaration at line {expected_line!r}")
                    if item.get("literal") != package[1]:
                        checks.error(f"version_inventory[{index}].literal", f"expected exact package version {package[1]!r}")
                    if item.get("evidence_path") != raw_path:
                        checks.error(f"version_inventory[{index}].evidence_path", "workspace evidence must be the same manifest")
        evidence_path = item.get("evidence_path")
        if isinstance(evidence_path, str) and evidence_path and not _evidence_path(evidence_path).is_file():
            checks.error(f"version_inventory[{index}].evidence_path", "referenced evidence does not exist")
    if current_packages == 0:
        checks.error("version_inventory", "missing a current oxidex_package entry")
    if set(inventory_packages) != set(workspace_packages):
        missing = sorted(set(workspace_packages) - set(inventory_packages))
        extra = sorted(set(inventory_packages) - set(workspace_packages))
        checks.error(
            "version_inventory",
            f"workspace manifest coverage mismatch; missing={missing!r}, extra={extra!r}",
        )
    checks.equal("version_reconciliation.status", "verified")
    checks.string("version_reconciliation.reviewer")
    checks.string("version_reconciliation.reviewed_at")
    reconciliation_path = checks.string("version_reconciliation.path")
    reconciliation_sha = checks.sha256("version_reconciliation.sha256")
    try:
        scan = _version_literal_scan()
        fields_scan = _version_fields_scan()
    except (OSError, subprocess.CalledProcessError) as exc:
        checks.error("version_reconciliation", f"cannot scan tracked version literals: {exc}")
        scan = {}
        fields_scan = {}
    for field in ("scan_sha256",):
        checks.equal(f"version_reconciliation.{field}", scan.get("sha256"))
    for field in ("tracked_files", "matching_lines"):
        checks.equal(f"version_reconciliation.{field}", scan.get(field))
    checks.equal("version_reconciliation.fields_scan_sha256", fields_scan.get("sha256"))
    for field in ("tracked_files", "matching_lines"):
        checks.equal(
            f"version_reconciliation.fields_{field}", fields_scan.get(field)
        )
    checks.equal("version_reconciliation.reconciled_files", scan.get("tracked_files"))
    checks.equal("version_reconciliation.reconciled_lines", scan.get("matching_lines"))
    checks.equal(
        "version_reconciliation.reconciled_field_files", fields_scan.get("tracked_files")
    )
    checks.equal(
        "version_reconciliation.reconciled_field_lines", fields_scan.get("matching_lines")
    )
    checks.empty_list("version_reconciliation.unresolved")
    if reconciliation_path is not None:
        path = _evidence_path(reconciliation_path)
        try:
            actual_sha = _file_sha256(path)
            record = load_receipt(path)
        except (OSError, ValueError, json.JSONDecodeError) as exc:
            checks.error("version_reconciliation.path", f"cannot load reconciliation: {exc}")
        else:
            if reconciliation_sha is not None and reconciliation_sha != actual_sha:
                checks.error("version_reconciliation.sha256", f"does not match referenced reconciliation {actual_sha}")
            for field in (
                "status", "scan_sha256", "tracked_files", "matching_lines",
                "reconciled_files", "reconciled_lines", "unresolved",
                "fields_scan_sha256", "fields_tracked_files", "fields_matching_lines",
                "reconciled_field_files", "reconciled_field_lines",
                "reviewer", "reviewed_at",
            ):
                if record.get(field) != checks.value(f"version_reconciliation.{field}"):
                    checks.error(f"version_reconciliation.{field}", "does not match referenced reconciliation")
    checks.string("promotion.pr_url")
    checks.equal("promotion.review_decision", "APPROVED")
    checks.equal("promotion.required_checks_status", "success")
    checks.equal("promotion.unresolved_actionable_threads", 0)
    checks.string("promotion.review_threads_evidence")
    checks.sha256("promotion.review_threads_sha256")
    checks.string("promotion.pr_state_evidence")
    checks.sha256("promotion.pr_state_sha256")
    checks.string("promotion.required_checks_evidence")
    checks.sha256("promotion.required_checks_sha256")
    for path in (
        "authorization.authorized_by",
        "authorization.authorized_at",
        "authorization.evidence_path",
    ):
        checks.string(path)
    checks.equal("authorization.version", payload.get("version"))
    checks.equal("authorization.tag", tag)
    checks.equal("authorization.main_sha", main_sha)
    checks.equal("tag_verification.status", "verified")
    checks.equal("tag_verification.tag", tag)
    checks.equal("tag_verification.target_sha", main_sha)
    checks.equal("tag_verification.signature", "good")
    checks.string("tag_verification.evidence_path")
    gates = checks.object_list("gates")
    for index, gate in enumerate(gates):
        for field in ("name", "evidence_path"):
            if not isinstance(gate.get(field), str) or not gate[field].strip():
                checks.error(f"gates[{index}].{field}", "expected a non-empty string")
        if gate.get("status") != "verified":
            checks.error(f"gates[{index}].status", "expected 'verified'")
    workflows = checks.nonempty_list("workflows")
    seen: set[str] = set()
    for index, workflow in enumerate(workflows):
        if not isinstance(workflow, dict):
            checks.error(f"workflows[{index}]", "expected an object")
            continue
        name = workflow.get("name")
        if isinstance(name, str):
            seen.add(name)
        for field, expected in (
            ("status", "completed"),
            ("conclusion", "success"),
            ("head_branch", tag),
            ("head_sha", main_sha),
            ("event", "push"),
        ):
            if workflow.get(field) != expected:
                checks.error(f"workflows[{index}].{field}", f"expected {expected!r}")
        if not isinstance(workflow.get("run_id"), int) or workflow["run_id"] <= 0:
            checks.error(f"workflows[{index}].run_id", "expected a positive integer")
        if not isinstance(workflow.get("evidence_path"), str) or not workflow["evidence_path"]:
            checks.error(f"workflows[{index}].evidence_path", "expected a non-empty string")
    for required in ("Release", "Docker"):
        if required not in seen:
            checks.error("workflows", f"missing successful {required} workflow")
    release_run_id = next(
        (workflow.get("run_id") for workflow in workflows if isinstance(workflow, dict) and workflow.get("name") == "Release"),
        None,
    )
    artifacts = checks.object_list("artifacts")
    artifact_names: list[str] = []
    artifacts_by_name: dict[str, dict[str, Any]] = {}
    for index, artifact in enumerate(artifacts):
        name = artifact.get("name")
        if not isinstance(name, str) or not name.strip():
            checks.error(f"artifacts[{index}].name", "expected a non-empty string")
        else:
            artifact_names.append(name)
            artifacts_by_name[name] = artifact
        value = artifact.get("sha256")
        if not isinstance(value, str) or SHA256_RE.fullmatch(value) is None:
            checks.error(f"artifacts[{index}].sha256", "expected a lowercase 64-character SHA-256")
        if isinstance(artifact.get("size"), bool) or not isinstance(artifact.get("size"), int) or artifact["size"] <= 0:
            checks.error(f"artifacts[{index}].size", "expected an integer greater than zero")
        if artifact.get("source_run_id") != release_run_id:
            checks.error(f"artifacts[{index}].source_run_id", "must match Release workflow run_id")
    if sorted(artifact_names) != sorted(expected_assets):
        checks.error("artifacts", "names must exactly match packaging.expected_assets")
    checks.equal("macos_verification.status", "verified")
    mac_release_run = checks.positive_int("macos_verification.release_run_id")
    if mac_release_run is not None and mac_release_run != release_run_id:
        checks.error("macos_verification.release_run_id", "must match Release workflow run_id")
    for path in (
        "macos_verification.run_artifact_manifest",
        "macos_verification.raw_binary_artifact",
        "macos_verification.dmg_artifact",
        "macos_verification.expected_developer_id",
        "macos_verification.expected_team_identifier",
        "macos_verification.observed_developer_id",
        "macos_verification.observed_team_identifier",
        "macos_verification.reported_version",
    ):
        checks.string(path)
    checks.equal(
        "macos_verification.observed_developer_id",
        checks.value("macos_verification.expected_developer_id"),
    )
    checks.equal(
        "macos_verification.observed_team_identifier",
        checks.value("macos_verification.expected_team_identifier"),
    )
    for path in (
        "macos_verification.raw_binary_sha256",
        "macos_verification.dmg_sha256",
        "macos_verification.dmg_payload_sha256",
    ):
        checks.sha256(path)
    checks.equal("macos_verification.payload_match", True)
    checks.equal(
        "macos_verification.dmg_payload_sha256",
        checks.value("macos_verification.raw_binary_sha256"),
    )
    if isinstance(payload.get("version"), str):
        checks.equal("macos_verification.reported_version", f"oxidex {payload['version']}")
    checks.equal("macos_verification.gatekeeper_status", "accepted")
    checks.equal("macos_verification.stapler_status", "validated")
    checks.equal("macos_verification.cleanup_status", "verified")
    checks.string_list("macos_verification.evidence")
    checks.equal("macos_verification.raw_binary_artifact", "oxidex-universal-apple-darwin")
    checks.equal(
        "macos_verification.dmg_artifact", f"oxidex-v{payload.get('version')}.dmg"
    )
    for field, hash_field in (
        ("raw_binary_artifact", "raw_binary_sha256"),
        ("dmg_artifact", "dmg_sha256"),
    ):
        name = checks.value(f"macos_verification.{field}")
        artifact = artifacts_by_name.get(name) if isinstance(name, str) else None
        if artifact is None:
            checks.error(f"macos_verification.{field}", "must name a published artifact")
            continue
        if artifact.get("sha256") != checks.value(f"macos_verification.{hash_field}"):
            checks.error(f"macos_verification.{field}", f"published artifact hash must match {hash_field}")
        if artifact.get("source_run_id") != release_run_id:
            checks.error(f"macos_verification.{field}", "artifact must come from Release workflow run")
    return checks.errors


def validate_receipt(
    kind: str,
    payload: dict[str, Any],
    expected_version: str | None = None,
    expected_sha: str | None = None,
    template: bool = False,
) -> list[str]:
    if kind not in KINDS:
        return [f"kind: expected one of {KINDS!r}"]
    if not isinstance(payload, dict):
        return ["receipt: expected a JSON object"]
    schema_errors = validate_schema(kind, payload)
    if template:
        return schema_errors + _validate_template(kind, payload)
    if kind == "parity":
        return schema_errors + _validate_parity(payload, expected_version, expected_sha)
    if kind == "documentation":
        return schema_errors + _validate_documentation(payload, expected_version, expected_sha)
    return schema_errors + _validate_finalization(payload, expected_version, expected_sha)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kind", choices=KINDS, required=True)
    parser.add_argument("--receipt", required=True, type=pathlib.Path)
    parser.add_argument("--version")
    parser.add_argument("--candidate-sha")
    parser.add_argument("--template", action="store_true")
    args = parser.parse_args(argv)
    if not args.template and (not args.version or not args.candidate_sha):
        print(
            "receipt: --version and --candidate-sha are required outside --template mode",
            file=sys.stderr,
        )
        return 2
    try:
        payload = load_receipt(args.receipt)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"receipt: {exc}", file=sys.stderr)
        return 2
    errors = validate_receipt(
        args.kind,
        payload,
        expected_version=args.version,
        expected_sha=args.candidate_sha,
        template=args.template,
    )
    for error in errors:
        print(error, file=sys.stderr)
    return 2 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
