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


class PrGateError(RuntimeError):
    """The reviewed-promotion evidence is incomplete or contradictory."""


def _load(path: pathlib.Path, label: str) -> Any:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise PrGateError(f"{label}: {exc}") from exc
    return value


def _sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate(
    pr_state_path: pathlib.Path,
    review_threads_path: pathlib.Path,
    required_checks_path: pathlib.Path,
    expected_head: str,
) -> dict[str, Any]:
    if COMMIT_RE.fullmatch(expected_head) is None:
        raise PrGateError("expected_head: expected a full lowercase commit SHA")
    state = _load(pr_state_path, "pr-state")
    if not isinstance(state, dict):
        raise PrGateError("pr-state: expected a JSON object")
    for field, expected in (
        ("baseRefName", "main"),
        ("headRefOid", expected_head),
        ("reviewDecision", "APPROVED"),
    ):
        if state.get(field) != expected:
            raise PrGateError(
                f"pr-state.{field}: expected {expected!r}, got {state.get(field)!r}"
            )

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
        "review_decision": "APPROVED",
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
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args(argv)
    try:
        result = validate(
            args.pr_state, args.review_threads, args.required_checks, args.expected_head
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
