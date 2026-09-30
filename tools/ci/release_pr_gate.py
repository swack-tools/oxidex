#!/usr/bin/env python3
"""Validate persisted GitHub PR state and review-thread evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import sys
from collections.abc import Sequence
from typing import Any


COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
FINDING_SIGNAL_RE = re.compile(r"\[P[0-3]\]|\bP[0-3]\b")


class PrGateError(RuntimeError):
    """The reviewed-promotion evidence is incomplete or contradictory."""


def _load(path: pathlib.Path, label: str) -> Any:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise PrGateError(f"{label}: {exc}") from exc
    return value


def _sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _bound_file(record: dict[str, Any], label: str) -> tuple[pathlib.Path, Any]:
    raw_path, digest = record.get(f"{label}_path"), record.get(f"{label}_sha256")
    if (
        not isinstance(raw_path, str)
        or not raw_path
        or not isinstance(digest, str)
        or re.fullmatch(r"[0-9a-f]{64}", digest) is None
    ):
        raise PrGateError(f"local-review.{label}: missing path or SHA-256")
    path = pathlib.Path(raw_path)
    if not path.is_absolute():
        raise PrGateError(f"local-review.{label}: expected an absolute evidence path")
    try:
        actual = _sha256(path)
    except OSError as exc:
        raise PrGateError(f"local-review.{label}: {exc}") from exc
    if actual != digest:
        raise PrGateError(f"local-review.{label}: SHA-256 mismatch")
    if label == "review_result":
        try:
            return path, path.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as exc:
            raise PrGateError(f"local-review.{label}: {exc}") from exc
    return path, _load(path, f"local-review.{label}")


def _local_review(
    path: pathlib.Path, expected_head: str, expected_base: str, expected_tree: str
) -> dict[str, Any]:
    record = _load(path, "local-review")
    if not isinstance(record, dict):
        raise PrGateError("local-review: expected a JSON object")
    for field, expected in (
        ("schema_version", 1), ("status", "accepted"),
        ("head", expected_head), ("base", expected_base),
        ("tree", expected_tree),
        ("result_disposition", "no_unresolved_actionable_findings"),
        ("unresolved_actionable_findings", 0),
    ):
        if record.get(field) != expected:
            raise PrGateError(
                f"local-review.{field}: expected {expected!r}, got {record.get(field)!r}"
            )
    for field in ("reviewer", "reviewed_at"):
        if not isinstance(record.get(field), str) or not record[field].strip():
            raise PrGateError(f"local-review.{field}: expected a nonempty assessor and review time")
    _, receipt = _bound_file(record, "review_receipt")
    if not isinstance(receipt, dict):
        raise PrGateError("local-review.review_receipt: expected an object")
    if receipt.get("role") not in ("review", "acceptance"):
        raise PrGateError("local-review.review_receipt.role: expected review or acceptance")
    for field, expected in (
        ("status", "completed"), ("exit_code", 0), ("base", expected_base),
        ("head", expected_head), ("tree", expected_tree), ("dirty", ""),
    ):
        if receipt.get(field) != expected:
            raise PrGateError(f"local-review.review_receipt.{field}: expected {expected!r}")
    after = receipt.get("after")
    if not isinstance(after, dict) or any(
        after.get(field) != expected
        for field, expected in (("head", expected_head), ("tree", expected_tree), ("dirty", ""))
    ):
        raise PrGateError("local-review.review_receipt.after: stale or dirty review")
    result_path, result = _bound_file(record, "review_result")
    command = receipt.get("command")
    if not isinstance(command, list) or "--output-last-message" not in command:
        raise PrGateError("local-review.review_receipt.command: result output is not bound")
    output_index = command.index("--output-last-message") + 1
    if output_index >= len(command) or command[output_index] != str(result_path):
        raise PrGateError("local-review.review_receipt.command: result path mismatch")
    if len(command) < 3 or command[-3:] != ["review", "--base", expected_base]:
        raise PrGateError("local-review.review_receipt.command: expected review of exact base")
    if not isinstance(result, str) or not result.strip():
        raise PrGateError("local-review.review_result: empty result")
    _, findings = _bound_file(record, "findings")
    if not isinstance(findings, dict):
        raise PrGateError("local-review.findings: expected an object")
    for field, expected in (("status", "reviewed"), ("head", expected_head),
                            ("base", expected_base), ("tree", expected_tree),
                            ("unresolved_actionable_findings", 0),
                            ("review_result_sha256", record["review_result_sha256"])):
        if findings.get(field) != expected:
            raise PrGateError(f"local-review.findings.{field}: expected {expected!r}")
    items = findings.get("findings")
    if not isinstance(items, list) or any(
        not isinstance(item, dict)
        or item.get("disposition") not in {"resolved", "not_actionable"}
        or not isinstance(item.get("review_line"), str)
        or not isinstance(item.get("reason"), str)
        or not item["reason"].strip()
        for item in items
    ):
        raise PrGateError("local-review.findings.findings: unresolved or malformed finding")
    raw_lines = sorted(line for line in result.splitlines() if FINDING_SIGNAL_RE.search(line))
    disposed_lines = sorted(item["review_line"] for item in items)
    if raw_lines != disposed_lines:
        raise PrGateError("local-review.findings.findings: raw review findings are not all accounted for")
    _, authorization = _bound_file(record, "authorization")
    if (
        not isinstance(authorization, dict)
        or authorization.get("schema_version") != 1
        or authorization.get("decision") != "approved"
        or authorization.get("scope") != "local_review_fallback"
        or authorization.get("assessment") != "affirmative_in_scope"
        or not isinstance(authorization.get("authorized_by"), str)
        or not authorization["authorized_by"].strip()
        or not isinstance(authorization.get("authorized_at"), str)
        or not authorization["authorized_at"].strip()
        or not isinstance(authorization.get("assessed_by"), str)
        or not authorization["assessed_by"].strip()
        or not isinstance(authorization.get("assessed_at"), str)
        or not authorization["assessed_at"].strip()
        or authorization.get("assessed_instruction_sha256")
        != authorization.get("original_instruction_sha256")
    ):
        raise PrGateError("local-review.authorization: missing consistent affirmative source")
    _, source = _bound_file(authorization, "original_instruction")
    if (
        not isinstance(source, dict)
        or not isinstance(source.get("authorization"), str)
        or not source["authorization"].strip()
    ):
        raise PrGateError("local-review.authorization.source: missing original user instruction")
    return {
        "local_review_path": str(path.resolve()),
        "local_review_sha256": _sha256(path),
        **{
            f"{label}_{field}": record[f"{label}_{field}"]
            for label in ("review_receipt", "review_result", "findings", "authorization")
            for field in ("path", "sha256")
        },
    }


def validate(
    pr_state_path: pathlib.Path,
    review_threads_path: pathlib.Path,
    required_checks_path: pathlib.Path,
    expected_head: str,
    local_review_path: pathlib.Path | None = None,
    expected_base: str | None = None,
    expected_tree: str | None = None,
) -> dict[str, Any]:
    if COMMIT_RE.fullmatch(expected_head) is None:
        raise PrGateError("expected_head: expected a full lowercase commit SHA")
    state = _load(pr_state_path, "pr-state")
    if not isinstance(state, dict):
        raise PrGateError("pr-state: expected a JSON object")
    for field, expected in (
        ("baseRefName", "main"),
        ("headRefOid", expected_head),
    ):
        if state.get(field) != expected:
            raise PrGateError(
                f"pr-state.{field}: expected {expected!r}, got {state.get(field)!r}"
            )
    if "reviewDecision" not in state:
        raise PrGateError("pr-state.reviewDecision: missing captured GitHub decision")
    decision = state["reviewDecision"]
    if local_review_path is None:
        if decision != "APPROVED":
            raise PrGateError(f"pr-state.reviewDecision: expected 'APPROVED', got {decision!r}")
        local_evidence: dict[str, Any] = {}
        review_basis = "github_approved"
    else:
        if decision not in (None, "", "REVIEW_REQUIRED"):
            raise PrGateError(
                f"pr-state.reviewDecision: local fallback cannot override {decision!r}"
            )
        if expected_base is None or COMMIT_RE.fullmatch(expected_base) is None:
            raise PrGateError("expected_base: expected a full lowercase commit SHA")
        if expected_tree is None or COMMIT_RE.fullmatch(expected_tree) is None:
            raise PrGateError("expected_tree: expected a full lowercase tree SHA")
        if state.get("baseRefOid") != expected_base:
            raise PrGateError("pr-state.baseRefOid: stale base")
        local_evidence = _local_review(
            local_review_path, expected_head, expected_base, expected_tree
        )
        review_basis = "local_review_fallback"

    payload = _load(review_threads_path, "review-threads")
    if not isinstance(payload, dict):
        raise PrGateError("review-threads: expected a JSON object")
    try:
        threads = payload["data"]["repository"]["pullRequest"]["reviewThreads"]
        has_next = threads["pageInfo"]["hasNextPage"]
        nodes = threads["nodes"]
    except (KeyError, TypeError) as exc:
        raise PrGateError(f"review-threads: malformed response at {exc}") from exc
    if has_next is not False:
        raise PrGateError("review-threads.pagination: response is incomplete")
    if not isinstance(nodes, list):
        raise PrGateError("review-threads.nodes: expected an array")
    unresolved = 0
    for index, node in enumerate(nodes):
        if not isinstance(node, dict):
            raise PrGateError(f"review-threads.nodes[{index}]: expected an object")
        if not isinstance(node.get("isResolved"), bool) or not isinstance(
            node.get("isOutdated"), bool
        ):
            raise PrGateError(
                f"review-threads.nodes[{index}]: missing boolean resolution state"
            )
        if not node["isResolved"] and not node["isOutdated"]:
            unresolved += 1
    if unresolved:
        raise PrGateError(f"review-threads.unresolved: found {unresolved} actionable thread(s)")

    required_checks = _load(required_checks_path, "required-checks")
    if not isinstance(required_checks, list) or not required_checks:
        raise PrGateError("required-checks: expected a non-empty JSON array")
    for index, check in enumerate(required_checks):
        if not isinstance(check, dict):
            raise PrGateError(f"required-checks[{index}]: expected an object")
        if not isinstance(check.get("name"), str) or not check["name"].strip():
            raise PrGateError(f"required-checks[{index}].name: expected a non-empty string")
        if check.get("state") != "SUCCESS":
            raise PrGateError(
                f"required-checks[{index}].state: expected 'SUCCESS', got {check.get('state')!r}"
            )
        if not isinstance(check.get("link"), str) or not check["link"].strip():
            raise PrGateError(f"required-checks[{index}].link: expected a non-empty string")
    return {
        "schema_version": 1,
        "status": "verified",
        "expected_head": expected_head,
        "base_ref": "main",
        "review_decision": decision,
        "review_basis": review_basis,
        **(
            {"base_sha": expected_base, "head_tree": expected_tree}
            if local_review_path is not None else {}
        ),
        **local_evidence,
        "unresolved_actionable_threads": 0,
        "pr_state_path": str(pr_state_path.resolve()),
        "pr_state_sha256": _sha256(pr_state_path),
        "review_threads_path": str(review_threads_path.resolve()),
        "review_threads_sha256": _sha256(review_threads_path),
        "required_checks_path": str(required_checks_path.resolve()),
        "required_checks_sha256": _sha256(required_checks_path),
        "required_checks_count": len(required_checks),
        "required_checks_status": "success",
    }


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pr-state", type=pathlib.Path, required=True)
    parser.add_argument("--review-threads", type=pathlib.Path, required=True)
    parser.add_argument("--required-checks", type=pathlib.Path, required=True)
    parser.add_argument("--expected-head", required=True)
    parser.add_argument("--local-review", type=pathlib.Path)
    parser.add_argument("--expected-base")
    parser.add_argument("--expected-tree")
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args(argv)
    try:
        result = validate(
            args.pr_state, args.review_threads, args.required_checks, args.expected_head,
            args.local_review, args.expected_base, args.expected_tree,
        )
    except PrGateError as exc:
        print(f"release PR gate refused: {exc}", file=sys.stderr)
        return 1
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
