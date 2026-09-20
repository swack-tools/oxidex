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


def _load(path: pathlib.Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise PrGateError(f"{label}: {exc}") from exc
    if not isinstance(value, dict):
        raise PrGateError(f"{label}: expected a JSON object")
    return value


def _sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate(
    pr_state_path: pathlib.Path,
    review_threads_path: pathlib.Path,
    expected_head: str,
) -> dict[str, Any]:
    if COMMIT_RE.fullmatch(expected_head) is None:
        raise PrGateError("expected_head: expected a full lowercase commit SHA")
    state = _load(pr_state_path, "pr-state")
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
    }


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pr-state", type=pathlib.Path, required=True)
    parser.add_argument("--review-threads", type=pathlib.Path, required=True)
    parser.add_argument("--expected-head", required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args(argv)
    try:
        result = validate(args.pr_state, args.review_threads, args.expected_head)
    except PrGateError as exc:
        print(f"release PR gate refused: {exc}", file=sys.stderr)
        return 1
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
