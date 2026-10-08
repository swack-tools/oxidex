#!/usr/bin/env python3
"""Read-only replay of three committed Task19 transitions for a frozen candidate."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys

import version_transition_qualification as qualification

executor = qualification.executor
stage_adapter = qualification.stage_adapter

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
# One explicit selection grammar preserves the original aliases and Markdown forms.
NEXT_PIN_LABEL = re.compile(
    r"(?:next[- ]pin|intended next[- ]pin|intended ExifTool pin|ExifTool pin after current"
    r"|next ExifTool release|intended ExifTool release)",
    re.IGNORECASE)
NEXT_PIN_FIELD = re.compile(
    rf"^(?:{NEXT_PIN_LABEL.pattern})\s*:\s*(not selected|[0-9]+\.[0-9]+)$",
    re.IGNORECASE)
SELECTION_HINT = re.compile(
    r"(?:intended\s+next|next|intended|planned|upcoming|target)\s+"
    r"(?:ExifTool\s+(?:pin|release|version)|(?:pin|version))\b",
    re.IGNORECASE)
UNKNOWN_SELECTION_FIELD = re.compile(
    r"^[A-Za-z][A-Za-z -]*(?:pin|ExifTool\s+(?:release|version)|version)\s*:",
    re.IGNORECASE)
PIN_WORD = re.compile(r"\bpin\b", re.IGNORECASE)
CURRENT_PIN_PROSE = re.compile(
    r"^(?:(?:for|the)\s+)?current pin(?:\s+is)?\s+[0-9]+\.[0-9]+(?:[,;]\s*[^0-9]*)?$",
    re.IGNORECASE)
CURRENT_PIN_FIELD = re.compile(
    r"^current (?:pin|ExifTool (?:release|version))\s*:\s*13\.59$",
    re.IGNORECASE)
CURRENT_PIN_FIELD = re.compile(
    r"^current (?:pin|ExifTool (?:release|version))\s*:\s*13\.59$",
    re.IGNORECASE)
# Do not mine `2.0` out of an OxiDex `v2.0.0-beta.1` release label.
RELEASE_IN_LINE = re.compile(r"(?<![0-9A-Za-z.])v?[0-9]+\.[0-9]+(?![0-9.])")


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



def _git(*arguments: str) -> str:
    try:
        return subprocess.check_output(
            ["git", "-C", str(ROOT), *arguments], text=True, stderr=subprocess.PIPE,
            timeout=30).strip()
    except (OSError, subprocess.SubprocessError) as exc:
        raise qualification.Refused("cannot inspect exact owned source") from exc


def _git_blob(name: str) -> bytes:
    try:
        return subprocess.check_output(
            ["git", "-C", str(ROOT), "show", f"HEAD:{name}"],
            stderr=subprocess.PIPE, timeout=30)
    except (OSError, subprocess.SubprocessError) as exc:
        raise qualification.Refused(f"cannot read committed {name}") from exc


def source_snapshot(expected_head: str, expected_tree: str) -> dict[str, str]:
    """Require actual clean bytes and index at the frozen signed source."""
    if _git("status", "--porcelain=v1", "--untracked-files=all"):
        refuse("adapter source has tracked or untracked changes")
    head, tree, index = _git("rev-parse", "HEAD"), _git("rev-parse", "HEAD^{tree}"), _git("write-tree")
    if (head, tree, index) != (expected_head, expected_tree, expected_tree):
        refuse("adapter HEAD, tree, or index differs from frozen candidate")
    pin = ROOT / ".exiftool-version"
    todo = ROOT / "TODO_RELEASE_BETA.md"
    if pin.read_bytes() != b"13.59\n":
        refuse("actual caller pin is not 13.59")
    if pin.read_bytes() != _git_blob(".exiftool-version"):
        refuse("actual caller pin differs from committed source")
    if todo.read_bytes() != _git_blob("TODO_RELEASE_BETA.md"):
        refuse("actual TODO differs from committed source")
    return {"head": head, "tree": tree, "pin_sha256": sha(pin), "todo_sha256": sha(todo)}


def next_pin_selection(todo: str) -> str:
    """Parse explicit next selectors; leave only the bound current pin descriptive."""
    selected: list[str] = []
    for line in todo.splitlines():
        # Only Markdown presentation is stripped. The selection language stays
        # one explicit grammar, so a new alias cannot silently mean "absent".
        normalized = re.sub(r"^\s*(?:[-*]\s*)?(?:\[[ xX]\]\s*)?", "", line)
        normalized = normalized.replace("**", "").strip()
        match = NEXT_PIN_FIELD.fullmatch(normalized)
        if match is not None:
            selected.append(match.group(1).lower())
        elif CURRENT_PIN_FIELD.fullmatch(normalized):
            continue
        elif (NEXT_PIN_LABEL.match(normalized)
              or (RELEASE_IN_LINE.search(normalized)
                  and (SELECTION_HINT.search(normalized)
                       or UNKNOWN_SELECTION_FIELD.match(normalized)
                       or (PIN_WORD.search(normalized) and not CURRENT_PIN_PROSE.fullmatch(normalized))))):
            refuse("TODO next-pin selection is ambiguous or uses an unknown label")
    if len(selected) > 1:
        refuse("TODO has multiple next-pin selections")
    return selected[0] if selected else "not selected"


def marker_snapshot(path: Path) -> tuple[dict[str, int | str], bytes]:
    """Read one regular final marker through a stable, non-symlink descriptor."""
    if path.is_symlink():
        refuse("qualification final marker must not be a symlink")
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | getattr(os, "O_NOFOLLOW", 0))
        with os.fdopen(descriptor, "rb") as stream:
            identity = os.fstat(stream.fileno())
            if not stat.S_ISREG(identity.st_mode):
                refuse("qualification final marker must be a regular file")
            data = stream.read()
    except OSError as exc:
        raise qualification.Refused(f"qualification final marker is unavailable: {path}") from exc
    return ({"sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data),
             "device": identity.st_dev, "inode": identity.st_ino,
             "mtime_ns": identity.st_mtime_ns, "ctime_ns": identity.st_ctime_ns}, data)


def require_marker_unchanged(path: Path, expected: dict[str, int | str]) -> None:
    current, _ = marker_snapshot(path)
    if current != expected:
        refuse(f"qualification final marker changed during replay: {path}")


def refuse_source_output_overlap(output: Path) -> None:
    """Keep read-only verification output physically outside the owned source."""
    source = ROOT.resolve()
    if output.resolve().is_relative_to(source):
        refuse("Task19 replay output overlaps the frozen source checkout")
    # On a case-insensitive filesystem resolve() may retain an alternate
    # spelling. A bind/volume alias can likewise name the same directory.
    for ancestor in (output, *output.parents):
        if ancestor.is_dir() and os.path.samefile(ancestor, source):
            refuse("Task19 replay output overlaps the frozen source checkout")


def replay_committed_write(row: dict[str, object], side: str, root: Path,
                           expected_head: str) -> dict[str, object]:
    """Replay the actual write stage and source-derived native scalar matrix."""
    entry = row[side]
    release = entry["release"]
    run_dir = root / row["id"] / side
    journal = qualification._read_object(run_dir / "execution-status.json", "execution journal")
    config = qualification._read_object(run_dir / "inputs" / "config.json", "execution config")
    report = qualification._report_for(run_dir, journal, release, "write")
    if entry.get("write_report_sha256") != qualification.rehearsal.sha256_json(report):
        refuse("committed row write digest differs from checked report")
    report_path = run_dir / journal["releases"][release]["reports"]["write"]["path"]
    checkout = (run_dir / "checkouts" / executor._safe_name(release)).resolve()
    target = Path(config["target_directories"][release]).resolve()
    identity = entry["instrument"]["native_identity"]
    source, lib, perl = (Path(identity["source"]["path"]), Path(identity["lib"]["path"]),
                         identity["perl"]["path"])
    native = (source, lib, source / "exiftool")
    try:
        checked = executor._stage_result(
            report_path, release, "write", entry["instrument"]["native_probe_sha256"],
            checkout=checkout, source_commit=expected_head,
            source_tree=executor._source_tree(checkout), target=target, native=native, perl=perl)
        build = qualification._report_for(run_dir, journal, release, "build")
        if checked != report or checked["writer_binary"] != build["writer_binary"]:
            refuse("write stage or writer differs from committed build")
        binding = config["write_fixture_bindings"][release]
        if (checked["fixtures"]["manifest"] != binding["path"]
                or checked["fixtures"]["manifest_sha256"] != binding["sha256"]
                or executor._require_fixture_proof(checked) != binding["fixtures"]):
            refuse("write fixtures differ from immutable selected scope")
        raw = qualification._read_object(Path(checked["raw_report"]["path"]), "raw write report")
        matrices = checked.get("matrix_reports")
        fixtures = checked["fixtures"]["entries"]
        if (not isinstance(matrices, list) or not matrices or len(matrices) != len(fixtures)
                or raw.get("state") != "ok" or raw.get("matrix_reports") != matrices
                or len(raw.get("commands", [])) != len(matrices)
                or any(command.get("state") != "ok" for command in raw["commands"])):
            refuse("write matrix list or raw command outcomes are incomplete")
        paths = stage_adapter._matrix_source_artifacts(checkout)
        source_proof = stage_adapter._source_proof(paths)
        _, _, expected, original_subset = stage_adapter._matrix_contract(paths)
        mode = checked["write_mode"]
        if (mode.get("source_contract") != source_proof
                or mode.get("original_subset") != original_subset
                or mode.get("expanded_matrix") != len(expected)
                or len(expected) != 1530):
            refuse("write mode differs from source-derived 1,530-case cohort")
        args = argparse.Namespace(release=release, source_commit=expected_head)
        verified = []
        for index, matrix in enumerate(matrices):
            expected_path = report_path.parent / "raw" / "write-matrix" / f"{index:04d}" / "report.json"
            if matrix.get("path") != str(expected_path):
                refuse("write matrix path differs from committed run")
            parsed = stage_adapter._matrix_report(
                expected_path, args=args, native_perl=Path(perl), native_lib=lib,
                writer=checked["writer_binary"], paths=paths,
                pin=checkout / ".exiftool-version")
            if parsed != matrix or parsed["declared"] != 1530 or parsed["passed"] != 1530:
                refuse("write matrix replay differs from committed 1,530-case result")
            verified.append(parsed)
        if checked["denominator"] != sum(item["declared"] for item in verified):
            refuse("write denominator differs from replayed native matrix")
    except (executor.Refused, stage_adapter.Refused, OSError, KeyError, TypeError, ValueError) as exc:
        raise qualification.Refused(f"committed write replay refused: {exc}") from exc
    return {"writer_binary": checked["writer_binary"], "write_report_sha256": entry["write_report_sha256"],
            "matrix_reports": verified, "source_contract": source_proof}


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
    markers: dict[Path, dict[str, int | str]] = {}
    for required_row, path in zip(ROWS, paths, strict=True):
        marker, marker_bytes = marker_snapshot(path)
        # This replays the final marker, exact manifest, quiescent lease,
        # row receipts, read union, snapshots, and native read pair policy.
        result = qualification.load_committed_result(path)
        try:
            loaded_bytes = json.loads(marker_bytes)
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise qualification.Refused("qualification final marker bytes are malformed") from exc
        if result != loaded_bytes:
            refuse("replayed result differs from the loaded final marker bytes")
        require_marker_unchanged(path, marker)
        markers[path] = marker
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
            write_proof = replay_committed_write(row, side, path.parent, expected_head)
            binaries[side] = expected
            sides[side] = {"release": release, "source_identity": entry["source_identity"],
                           "native_identity": instrument["native_identity"],
                           "read_fixture_manifest_sha256": instrument["read_fixture_manifest_sha256"],
                           "read_report_sha256": entry["read_report_sha256"],
                           "write_report_sha256": entry["write_report_sha256"],
                           "execution_journal_sha256": entry["execution_journal_sha256"],
                           "committed_write": write_proof}
        require_marker_unchanged(path, marker)
        accepted[row["id"]] = {
            "result": {"path": str(path.resolve()), **marker},
            "run_id": result["run_id"], "binaries": binaries,
            "sides": sides, "read_payload_floors": row["read_payload_floors"],
            "read_policy_pair": row["read_policy_pair"],
        }
    if set(accepted) != set(ROWS):
        refuse("committed results omit a required transition row")
    for path, marker in markers.items():
        require_marker_unchanged(path, marker)
    return {"schema": 1, "kind": "oxidex_task19_committed_replay",
            "status": "verified_read_only", "promotion": "forbidden",
            "candidate": {"head": expected_head, "tree": expected_tree, "pin": "13.59"},
            "matrix_sha256": expected_matrix_sha256,
            "read_policy_input": policy_binding, "tools": expected_tools,
            "rows": {name: accepted[name] for name in ROWS},
            "next_pin": {"selection": "not selected",
                         "todo_sha256": sha(ROOT / "TODO_RELEASE_BETA.md")}}


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
        before = source_snapshot(args.expected_head, args.expected_tree)
        selection = next_pin_selection((ROOT / "TODO_RELEASE_BETA.md").read_text(encoding="utf-8"))
        if selection != "not selected":
            refuse("TODO selects a next ExifTool release; the checked three-row matrix and read policy do not support that transition")
        tools = tool_expectations(args.expected_tool)
        binaries = binary_expectations(args.expected_binary)
        output = qualification._evidence_location(args.output, "Task19 replay output")
        refuse_source_output_overlap(output)
        if output.exists() or output.is_symlink():
            refuse("Task19 replay output already exists")
        value = verify_results(
            paths=(args.same_pin_result, args.forward_result, args.reverse_result),
            expected_head=args.expected_head, expected_tree=args.expected_tree,
            expected_matrix_sha256=args.expected_matrix_sha256,
            expected_policy_sha256=args.expected_read_policy_sha256,
            expected_tools=tools, expected_binaries=binaries)
        if source_snapshot(args.expected_head, args.expected_tree) != before:
            refuse("owned source changed during Task19 replay")
        for row, path in zip(ROWS, (args.same_pin_result, args.forward_result, args.reverse_result), strict=True):
            require_marker_unchanged(
                path, {key: val for key, val in value["rows"][row]["result"].items()
                       if key != "path"})
        refuse_source_output_overlap(output)
        if output.exists() or output.is_symlink():
            refuse("Task19 replay output already exists")
        qualification._atomic_json(output, value)
    except (qualification.Refused, OSError, KeyError, TypeError, ValueError) as exc:
        print(f"Task19 replay refused: {exc}", file=sys.stderr)
        return 2
    print(str(output))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
