"""Validate OxiDex release receipts structurally and semantically.

The JSON schema files beside the templates document their public shape. This
module deliberately uses only the standard library so release gates can run in
a fresh checkout without installing a schema package. Semantic checks are the
authoritative gate: a structurally plausible ``verified`` receipt still fails
when its identity, evidence, floors, workflows, or authorization are missing.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys
from collections.abc import Sequence
from typing import Any


KINDS = ("parity", "documentation", "finalization")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")


def load_receipt(path: str | pathlib.Path) -> dict[str, Any]:
    payload = json.loads(pathlib.Path(path).read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise ValueError("receipt: expected a JSON object")
    return payload


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

    def nonempty_list(self, path: str) -> list[Any]:
        value = self.value(path)
        if not isinstance(value, list) or not value:
            self.error(path, "expected a non-empty array")
            return []
        return value

    def empty_list(self, path: str) -> None:
        value = self.value(path)
        if value != []:
            self.error(path, "expected an empty array")


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
    checks.string("exiftool_version")
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
    checks.nonempty_list("oracle.probes")
    checks.nonempty_list("corpora")
    checks.nonempty_list("runs")
    for family in ("conformance", "authenticated_reads", "generated_catalog", "write_matrix"):
        checks.equal(f"{family}.status", "verified")
    metric = checks.positive_int("authenticated_reads.metric_c")
    floor = checks.positive_int("authenticated_reads.native_occurrence_floor")
    checks.string("authenticated_reads.native_occurrence_floor_evidence")
    if metric is not None and floor is not None and metric < floor:
        checks.error(
            "authenticated_reads.metric_c",
            f"{metric} is below native occurrence floor {floor}",
        )
    checks.equal("authenticated_reads.published_read_gate.status", "verified")
    checks.equal("authenticated_reads.published_read_gate.lost_reads", 0)
    checks.equal("regressions.status", "verified")
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
    checks.string("parity_receipt.path")
    checks.sha256("parity_receipt.sha256")
    measured = checks.commit("parity_receipt.measured_sha")
    if measured is not None and measured != payload.get("candidate_sha"):
        checks.error("parity_receipt.measured_sha", "must match candidate_sha")
    checks.nonempty_list("claims")
    checks.nonempty_list("pages")
    benchmarks = checks.nonempty_list("benchmarks")
    allowed_dispositions = {"candidate", "historical", "not_applicable"}
    for index, benchmark in enumerate(benchmarks):
        disposition = benchmark.get("disposition") if isinstance(benchmark, dict) else None
        if disposition not in allowed_dispositions:
            checks.error(
                f"benchmarks[{index}].disposition",
                f"expected one of {sorted(allowed_dispositions)!r}",
            )
    checks.equal("local_build.status", "verified")
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
    checks.nonempty_list("visual_review.samples")
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
    checks.string("pages_pipeline.workflow_sha")
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
    if candidate_tree is not None and main_tree is not None and candidate_tree != main_tree:
        checks.error("main_tree", "must match candidate_tree for preserved receipts")
    tag = checks.string("tag")
    if version is not None and tag is not None and tag != f"v{version}":
        checks.error("tag", f"expected 'v{version}'")
    for receipt in ("parity", "documentation"):
        checks.equal(f"receipts.{receipt}.status", "verified")
        checks.string(f"receipts.{receipt}.path")
        checks.sha256(f"receipts.{receipt}.sha256")
    checks.nonempty_list("packaging.targets")
    checks.nonempty_list("version_inventory")
    checks.string("promotion.pr_url")
    checks.equal("promotion.review_decision", "APPROVED")
    checks.equal("promotion.required_checks_status", "success")
    checks.equal("promotion.unresolved_actionable_threads", 0)
    checks.string("promotion.review_threads_evidence")
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
    checks.nonempty_list("gates")
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
    artifacts = checks.nonempty_list("artifacts")
    for index, artifact in enumerate(artifacts):
        if not isinstance(artifact, dict):
            checks.error(f"artifacts[{index}]", "expected an object")
            continue
        value = artifact.get("sha256")
        if not isinstance(value, str) or SHA256_RE.fullmatch(value) is None:
            checks.error(f"artifacts[{index}].sha256", "expected a lowercase 64-character SHA-256")
    checks.equal("macos_verification.status", "verified")
    checks.positive_int("macos_verification.release_run_id")
    for path in (
        "macos_verification.run_artifact_manifest",
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
    checks.equal("macos_verification.gatekeeper_status", "accepted")
    checks.equal("macos_verification.stapler_status", "validated")
    checks.equal("macos_verification.cleanup_status", "verified")
    checks.nonempty_list("macos_verification.evidence")
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
    if template:
        return _validate_template(kind, payload)
    if kind == "parity":
        return _validate_parity(payload, expected_version, expected_sha)
    if kind == "documentation":
        return _validate_documentation(payload, expected_version, expected_sha)
    return _validate_finalization(payload, expected_version, expected_sha)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kind", choices=KINDS, required=True)
    parser.add_argument("--receipt", required=True, type=pathlib.Path)
    parser.add_argument("--version")
    parser.add_argument("--candidate-sha")
    parser.add_argument("--template", action="store_true")
    args = parser.parse_args(argv)
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
