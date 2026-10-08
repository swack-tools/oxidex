#!/usr/bin/env python3
"""Read-only replay of three committed Task19 transitions for a frozen candidate."""
from __future__ import annotations

import argparse
from pathlib import Path
import re
import subprocess
import sys

import version_transition_qualification as qualification

ROOT = Path(__file__).resolve().parents[2]
POLICY_SHA256 = "a354c24dfbadae4c1b243b26b706f303bc25326b5dfd255e751f9fadd472dac5"
TOOL_FILES = (
    "tools/exiftool-tables/version_transition_qualification.py",
    "tools/exiftool-tables/version_transition_read_policy.py",
    "tools/exiftool-tables/version_rehearsal_executor.py",
    "tools/exiftool-tables/version_rehearsal_stage_adapter.py",
    "tools/exiftool-tables/version_rehearsal_native_oracle.py",
)
HEX40 = re.compile(r"[0-9a-f]{40}\Z")
HEX64 = re.compile(r"[0-9a-f]{64}\Z")
ROWS = ("same-pin-13.59", "11.78-to-12.64", "12.64-to-11.78")


def refuse(message: str) -> None:
    raise qualification.Refused(message)


def sha(path: Path) -> str:
    if path.is_symlink() or not path.is_file():
        refuse(f"required regular file is unavailable: {path}")
    return qualification._sha_file(path)


def tool_expectations(items: list[str]) -> dict[str, str]:
    values: dict[str, str] = {}
    for item in items:
        name, sep, digest = item.partition("=")
        if not sep or name in values or name not in TOOL_FILES or not HEX64.fullmatch(digest):
            refuse("expected tools must be unique known paths with SHA-256")
        values[name] = digest
    if set(values) != set(TOOL_FILES):
        refuse("all Task19 replay tools require explicit expected SHA-256")
    return values


def binary_expectations(items: list[str]) -> dict[tuple[str, str], dict[str, str]]:
    values: dict[tuple[str, str], dict[str, str]] = {}
    for item in items:
        row, sep, rest = item.partition(":")
        side, sep2, rest = rest.partition(":")
        path, sep3, digest = rest.rpartition(":")
        key = (row, side)
        if (not all((sep, sep2, sep3)) or row not in ROWS
                or side not in qualification.SIDES or key in values
                or not Path(path).is_absolute() or not HEX64.fullmatch(digest)):
            refuse("expected binaries must name row:side:absolute-path:sha256")
        values[key] = {"path": path, "sha256": digest}
    if set(values) != {(row, side) for row in ROWS for side in qualification.SIDES}:
        refuse("all six side binaries require expected identities")
    return values


def verify_results(*, paths: tuple[Path, Path, Path], expected_head: str,
                   expected_tree: str, expected_matrix_sha256: str,
                   expected_policy_sha256: str, expected_tools: dict[str, str],
                   expected_binaries: dict[tuple[str, str], dict[str, str]]) -> dict[str, object]:
    """Replay committed data before comparing it with independent frozen inputs."""
    if (not HEX40.fullmatch(expected_head) or not HEX40.fullmatch(expected_tree)
            or not HEX64.fullmatch(expected_matrix_sha256)
            or expected_policy_sha256 != POLICY_SHA256):
        refuse("frozen source, matrix, or accepted read-policy identity is invalid")
    if set(expected_tools) != set(TOOL_FILES) or any(
            not HEX64.fullmatch(value) or sha(ROOT / name) != value
            for name, value in expected_tools.items()):
        refuse("replay tool hashes differ from frozen expectations")
    if sha(qualification.CANONICAL_MATRIX) != expected_matrix_sha256:
        refuse("local canonical matrix differs from frozen expectation")
    matrix_rows = tuple(row["id"] for row in qualification.load_matrix(
        qualification.CANONICAL_MATRIX, "13.59")["rows"])
    if matrix_rows != ROWS:
        refuse("Task19 matrix does not contain the exact required rows")
    if len({path.resolve() for path in paths}) != 3:
        refuse("final paths must identify three distinct committed runs")
    if set(expected_binaries) != {(row, side) for row in ROWS for side in qualification.SIDES}:
        refuse("six explicit binary identities are required")
    accepted: dict[str, dict[str, object]] = {}
    policy_binding: dict[str, str] | None = None
    for required_row, path in zip(ROWS, paths, strict=True):
        # This replays the final marker, exact manifest, quiescent lease,
        # row receipts, read union, snapshots, and native read pair policy.
        result = qualification.load_committed_result(path)
        rows = result["rows"]
        if len(rows) != 1 or rows[0]["id"] != required_row:
            refuse(f"{required_row} result path does not contain its required row")
        row = rows[0]
        caller = result["caller"]
        if (caller.get("head") != expected_head or caller.get("index_tree") != expected_tree
                or caller.get("pin_version") != "13.59" or caller.get("status") != "clean"):
            refuse("Task19 caller differs from frozen head, tree, or pin")
        if result["matrix"]["sha256"] != expected_matrix_sha256:
            refuse("committed matrix differs from frozen matrix")
        policy = result["read_policy_input"]
        if (policy.get("sha256") != expected_policy_sha256
                or (policy_binding is not None and policy != policy_binding)):
            refuse("read-policy binding differs from accepted shared policy")
        policy_binding = policy
        binaries: dict[str, dict[str, str]] = {}
        sides: dict[str, dict[str, object]] = {}
        releases = {"same-pin-13.59": ("13.59", "13.59"),
                    "11.78-to-12.64": ("11.78", "12.64"),
                    "12.64-to-11.78": ("12.64", "11.78")}[row["id"]]
        for side, release in zip(qualification.SIDES, releases, strict=True):
            entry = row[side]
            instrument = entry["instrument"]
            binary = instrument["binary"]
            expected = expected_binaries[(row["id"], side)]
            if (instrument.get("source_commit") != expected_head
                    or binary.get("path") != expected["path"]
                    or binary.get("sha256") != expected["sha256"]
                    or entry.get("release") != release):
                refuse(f"{row['id']} {side} source, binary, or release differs")
            binaries[side] = expected
            sides[side] = {"release": release, "source_identity": entry["source_identity"],
                           "native_identity": instrument["native_identity"],
                           "read_fixture_manifest_sha256": instrument["read_fixture_manifest_sha256"],
                           "read_report_sha256": entry["read_report_sha256"],
                           "write_report_sha256": entry["write_report_sha256"],
                           "execution_journal_sha256": entry["execution_journal_sha256"]}
        accepted[row["id"]] = {
            "result": {"path": str(path.resolve()), "sha256": sha(path)},
            "run_id": result["run_id"], "binaries": binaries,
            "sides": sides, "read_payload_floors": row["read_payload_floors"],
            "read_policy_pair": row["read_policy_pair"],
        }
    if set(accepted) != set(ROWS):
        refuse("committed results omit a required transition row")
    return {"schema": 1, "kind": "oxidex_task19_committed_replay",
            "status": "verified_read_only", "promotion": "forbidden",
            "candidate": {"head": expected_head, "tree": expected_tree, "pin": "13.59"},
            "matrix_sha256": expected_matrix_sha256,
            "read_policy_input": policy_binding, "tools": expected_tools,
            "rows": {name: accepted[name] for name in ROWS},
            "next_pin": {"selection": "not selected",
                         "todo_sha256": sha(ROOT / "TODO_RELEASE_BETA.md")}}


def git_identity() -> tuple[str, str]:
    try:
        head = subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip()
        tree = subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD^{tree}"], text=True).strip()
    except subprocess.CalledProcessError as exc:
        raise qualification.Refused("cannot resolve local frozen source") from exc
    return head, tree


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    for flag in ("same-pin", "forward", "reverse"):
        parser.add_argument(f"--{flag}-result", required=True, type=Path)
    for flag in ("head", "tree", "matrix-sha256", "read-policy-sha256"):
        parser.add_argument(f"--expected-{flag}", required=True)
    parser.add_argument("--expected-tool", action="append", required=True)
    parser.add_argument("--expected-binary", action="append", required=True)
    parser.add_argument("--next-pin", choices=("not-selected",), required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        if git_identity() != (args.expected_head, args.expected_tree):
            refuse("adapter source is not the exact frozen candidate")
        todo = (ROOT / "TODO_RELEASE_BETA.md").read_text(encoding="utf-8")
        if re.search(r"(?im)^\s*[-*]?\s*(?:intended\s+)?(?:next[- ]pin|ExifTool\s+pin\s+after\s+current)\s*[:=]\s*[0-9]+\.[0-9]+", todo):
            refuse("TODO selects a next pin; a fourth authenticated Task19 row is required")
        tools = tool_expectations(args.expected_tool)
        binaries = binary_expectations(args.expected_binary)
        output = qualification._evidence_location(args.output, "Task19 replay output")
        if output.exists() or output.is_symlink():
            refuse("Task19 replay output already exists")
        value = verify_results(
            paths=(args.same_pin_result, args.forward_result, args.reverse_result),
            expected_head=args.expected_head, expected_tree=args.expected_tree,
            expected_matrix_sha256=args.expected_matrix_sha256,
            expected_policy_sha256=args.expected_read_policy_sha256,
            expected_tools=tools, expected_binaries=binaries)
        qualification._atomic_json(output, value)
    except (qualification.Refused, OSError, KeyError, TypeError, ValueError) as exc:
        print(f"Task19 replay refused: {exc}", file=sys.stderr)
        return 2
    print(str(output))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
