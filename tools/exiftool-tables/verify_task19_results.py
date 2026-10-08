#!/usr/bin/env python3
"""Read-only replay of three committed Task19 transitions for a frozen candidate."""
from __future__ import annotations

import argparse
from contextlib import ExitStack, contextmanager
from dataclasses import dataclass
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
MAX_MARKER_BYTES = 16 * 1024 * 1024
POLICY_SHA256 = "a354c24dfbadae4c1b243b26b706f303bc25326b5dfd255e751f9fadd472dac5"
WRITE_COHORT = "tests/fixtures/jpeg/tag_matrix_base.jpg"
WRITE_COHORT_SHA256 = "9109ff5542f71c6c247e0ac372280cf81d00004d5fe319d195a0659f69dab8a6"
WRITE_COHORT_BYTES = 771
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
SYSTEM_GIT = Path("/usr/bin/git")
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


def _rooted_directory(path: Path, root: Path, label: str) -> Path:
    try:
        qualification.ops_paths.durable_root(path, label)
    except ValueError as exc:
        raise qualification.Refused(str(exc)) from exc
    if (not path.is_absolute() or path == root or not path.is_relative_to(root)
            or path.is_symlink() or path.resolve() != path or not path.is_dir()):
        refuse(f"{label} is not a canonical directory beneath configured durable root {root}: {path}")
    return path


def _rooted_executable(path: Path, root: Path, label: str) -> Path:
    try:
        qualification.ops_paths.durable_root(path, label)
    except ValueError as exc:
        raise qualification.Refused(str(exc)) from exc
    if (not path.is_absolute() or path == root or not path.is_relative_to(root)
            or path.is_symlink() or path.resolve() != path or not path.is_file()):
        refuse(f"{label} is not a canonical file beneath configured durable root {root}: {path}")
    return path


def _durable_executable_roots() -> tuple[Path, Path, Path]:
    try:
        targets = qualification.ops_paths.target_root()
        ops = qualification.ops_paths.ops_root()
    except ValueError as exc:
        raise qualification.Refused(f"configured executable root is not durable: {exc}") from exc
    approved_perl = ops / "toolchains/perl-5.38.2/prefix/bin/perl5.38.2"
    return targets, ops, approved_perl


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



@dataclass(frozen=True)
class BoundGit:
    path: Path
    sha256: str
    device: int
    inode: int
    size: int
    mtime_ns: int
    ctime_ns: int

    def revalidate(self) -> None:
        try:
            current = self.path.lstat()
            identity = (current.st_dev, current.st_ino, current.st_size,
                        current.st_mtime_ns, current.st_ctime_ns)
            if (not stat.S_ISREG(current.st_mode) or current.st_uid != 0
                    or current.st_mode & 0o022
                    or identity != (self.device, self.inode, self.size,
                                    self.mtime_ns, self.ctime_ns)
                    or sha(self.path) != self.sha256):
                refuse("bound system Git executable changed or lost custody")
        except OSError as exc:
            raise qualification.Refused("bound system Git executable is unavailable") from exc

    def evidence(self) -> dict[str, str | int]:
        return {"path": str(self.path), "sha256": self.sha256,
                "device": self.device, "inode": self.inode}


def bind_git() -> BoundGit:
    """Trust only the root-owned system Git, independent of caller PATH."""
    path = SYSTEM_GIT
    try:
        observed = path.lstat()
        if (not path.is_absolute() or not stat.S_ISREG(observed.st_mode)
                or observed.st_uid != 0 or observed.st_mode & 0o022):
            refuse("approved system Git executable is unavailable")
        bound = BoundGit(path, sha(path), observed.st_dev, observed.st_ino,
                         observed.st_size, observed.st_mtime_ns, observed.st_ctime_ns)
        bound.revalidate()
        return bound
    except OSError as exc:
        raise qualification.Refused("approved system Git executable is unavailable") from exc


def _git_bytes(git: BoundGit, *arguments: str) -> bytes:
    git.revalidate()
    try:
        result = subprocess.check_output(
            [str(git.path), "-C", str(ROOT), *arguments],
            stderr=subprocess.PIPE, timeout=30)
    except (OSError, subprocess.SubprocessError) as exc:
        raise qualification.Refused("cannot inspect exact owned source") from exc
    git.revalidate()
    return result


def _git(git: BoundGit, *arguments: str) -> str:
    return _git_bytes(git, *arguments).decode("utf-8").strip()


def _git_blob(git: BoundGit, name: str) -> bytes:
    return _git_bytes(git, "show", f"HEAD:{name}")


def _tracked_source_bytes(git: BoundGit) -> None:
    """Reject hidden index flags and compare every tracked byte and mode to HEAD."""
    try:
        listing = _git_bytes(git, "ls-files", "--stage", "-v", "-z")
        algorithm = _git(git, "rev-parse", "--show-object-format")
        digest_type = {"sha1": hashlib.sha1, "sha256": hashlib.sha256}[algorithm]
        for record in filter(None, listing.split(b"\0")):
            metadata, relative = record.split(b"\t", 1)
            flag, mode, oid, stage = metadata.decode("ascii").split()
            name = relative.decode("utf-8", "surrogateescape")
            path = ROOT / name
            if flag != "H" or stage != "0" or name.startswith("/") or ".." in Path(name).parts:
                refuse(f"frozen source has hidden index flags or invalid path: {name}")
            file_stat = path.lstat()
            if mode == "120000":
                if not stat.S_ISLNK(file_stat.st_mode):
                    refuse(f"tracked symlink mode differs: {name}")
                data = os.fsencode(os.readlink(path))
            elif mode in ("100644", "100755"):
                if not stat.S_ISREG(file_stat.st_mode) or bool(file_stat.st_mode & 0o111) != (mode == "100755"):
                    refuse(f"tracked file mode differs: {name}")
                data = path.read_bytes()
            else:
                refuse(f"unsupported tracked source mode: {name}")
            digest = digest_type(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
            if digest != oid:
                refuse(f"tracked source bytes differ from frozen index: {name}")
    except qualification.Refused:
        raise
    except (OSError, subprocess.SubprocessError, ValueError, KeyError) as exc:
        raise qualification.Refused("cannot compare tracked source bytes and modes") from exc


def require_imported_source_paths() -> None:
    imported = ((qualification, TOOL_FILES[0]), (executor, TOOL_FILES[2]),
                (stage_adapter, TOOL_FILES[3]))
    if any(Path(module.__file__).resolve() != (ROOT / name).resolve()
           for module, name in imported):
        refuse("imported replay verifier differs from frozen source path")


def source_snapshot(expected_head: str, expected_tree: str, git: BoundGit) -> dict[str, str]:
    """Require actual clean bytes and index at the frozen signed source."""
    git.revalidate()
    if _git(git, "status", "--porcelain=v1", "--untracked-files=all"):
        refuse("adapter source has tracked or untracked changes")
    head, tree, index = _git(git, "rev-parse", "HEAD"), _git(git, "rev-parse", "HEAD^{tree}"), _git(git, "write-tree")
    if (head, tree, index) != (expected_head, expected_tree, expected_tree):
        refuse("adapter HEAD, tree, or index differs from frozen candidate")
    _tracked_source_bytes(git)
    require_imported_source_paths()
    pin = ROOT / ".exiftool-version"
    todo = ROOT / "TODO_RELEASE_BETA.md"
    if pin.read_bytes() != b"13.59\n":
        refuse("actual caller pin is not 13.59")
    if pin.read_bytes() != _git_blob(git, ".exiftool-version"):
        refuse("actual caller pin differs from committed source")
    if todo.read_bytes() != _git_blob(git, "TODO_RELEASE_BETA.md"):
        refuse("actual TODO differs from committed source")
    git.revalidate()
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


@dataclass(frozen=True)
class PinnedMarker:
    descriptor: int
    binding: dict[str, int | str]
    data: bytes


def _marker_binding(identity: os.stat_result, data: bytes) -> dict[str, int | str]:
    return {"sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data),
            "device": identity.st_dev, "inode": identity.st_ino,
            "mtime_ns": identity.st_mtime_ns, "ctime_ns": identity.st_ctime_ns}


@contextmanager
def pin_marker(path: Path):
    """Retain the original final-marker inode through replay and publication."""
    if path.is_symlink():
        refuse("qualification final marker must not be a symlink")
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | getattr(os, "O_NOFOLLOW", 0))
    except OSError as exc:
        raise qualification.Refused(f"qualification final marker is unavailable: {path}") from exc
    try:
        identity = os.fstat(descriptor)
        if not stat.S_ISREG(identity.st_mode):
            refuse("qualification final marker must be a regular file")
        if identity.st_size > MAX_MARKER_BYTES:
            refuse("qualification final marker exceeds bounded size")
        data = os.pread(descriptor, MAX_MARKER_BYTES + 1, 0)
        if len(data) > MAX_MARKER_BYTES:
            refuse("qualification final marker exceeds bounded size")
        current = path.lstat()
        if (not stat.S_ISREG(current.st_mode)
                or (current.st_dev, current.st_ino) != (identity.st_dev, identity.st_ino)):
            refuse(f"qualification final marker changed during open: {path}")
        yield PinnedMarker(descriptor, _marker_binding(identity, data), data)
    except OSError as exc:
        raise qualification.Refused(f"qualification final marker read failed: {path}") from exc
    finally:
        os.close(descriptor)


def marker_snapshot(path: Path) -> tuple[dict[str, int | str], bytes]:
    """One-shot read for callers outside an outer pinned replay lifetime."""
    with pin_marker(path) as pinned:
        return pinned.binding, pinned.data


def require_marker_unchanged(path: Path, expected: dict[str, int | str],
                             pinned: PinnedMarker | None = None) -> None:
    if pinned is not None:
        try:
            identity = os.fstat(pinned.descriptor)
            data = os.pread(pinned.descriptor, MAX_MARKER_BYTES + 1, 0)
        except OSError as exc:
            raise qualification.Refused(f"qualification final marker descriptor read failed: {path}") from exc
        if (_marker_binding(identity, data) != pinned.binding
                or pinned.binding != expected or data != pinned.data):
            refuse(f"qualification final marker changed on bound descriptor: {path}")
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


def canonical_row(result: dict[str, object], final_path: Path,
                  target_root: Path) -> dict[str, object]:
    """Reconstruct the execution selections from the checked matrix."""
    run_id = result["run_id"]
    output_root = final_path.parent.parent
    matrix = qualification.materialize_matrix(
        qualification.load_matrix(qualification.CANONICAL_MATRIX, "13.59"),
        output_root=output_root, target_root=target_root, run_id=run_id)
    row = result["rows"][0]
    selected = next((item for item in matrix["rows"] if item["id"] == row["id"]), None)
    if selected is None:
        refuse("committed row is absent from canonical matrix")
    if (selected["durable_output_directory"] != str(final_path.parent / row["id"])
            or final_path.parent != output_root / run_id):
        refuse("committed row is outside its canonical output")
    return selected


def require_canonical_side(config: dict[str, object], row: dict[str, object],
                           side: str, release: str, run_dir: Path,
                           committed: dict[str, object]) -> None:
    """Bind rehashed side inputs to the selected execution contract."""
    fixtures = row["fixtures"][side]
    expected_target = str(Path(row["target_directory"]) / side)
    expected_bundle = row["immutable_source_identities"][side]["input_bundle"]
    expected_write = fixtures["write_manifest"]
    expected_read = fixtures["read_manifest"]
    expected_cases = qualification._read_array(Path(fixtures["native_cases"]), "canonical native cases")
    carrier = Path(row["durable_output_directory"]).parents[1] / "write-cohort/tag_matrix_base.jpg"
    expected_carrier = {"path": str(carrier), "sha256": WRITE_COHORT_SHA256,
                        "bytes": WRITE_COHORT_BYTES}
    write_binding = executor._write_fixture_binding(expected_write)
    if (sha(ROOT / WRITE_COHORT) != WRITE_COHORT_SHA256
            or (ROOT / WRITE_COHORT).stat().st_size != WRITE_COHORT_BYTES
            or write_binding["fixtures"] != [expected_carrier]):
        refuse(f"{row['id']} {side} write carrier differs from committed canonical cohort")
    if (run_dir != Path(row["durable_output_directory"]) / side
            or config.get("target_directories", {}).get(release) != expected_target
            or config.get("verified_input_bundle") != expected_bundle
            or config.get("write_fixture_manifests", {}).get(release) != expected_write
            or config.get("write_fixture_bindings", {}).get(release) != write_binding
            or config.get("native_cases", {}).get(release) != expected_cases
            or committed.get("read_union", {}).get("original_manifests", {}).get(side, {}).get("path")
            != expected_read):
        refuse(f"{row['id']} {side} selections differ from canonical matrix")


def require_canonical_lease(final_path: Path) -> Path:
    """Every operational record must name the shared physical lease."""
    root = final_path.parent
    lease = root.parent / "transition.host.lock"
    if lease.is_symlink() or not lease.is_file() or lease.resolve() != lease:
        refuse("canonical transition host lock is unavailable")
    expected = str(lease)
    for name in ("lease-owner.json", "lease-expiry.json", "lease-release.json"):
        record = qualification._read_object(root / name, name)
        if record.get("lock_path") != expected or record.get("lock_realpath") != expected:
            refuse(f"{name} differs from canonical transition host lock")
    for name, field in (("lease-heartbeat.jsonl", "lock_path"),
                        ("handoff.jsonl", "lease")):
        for line in (root / name).read_text(encoding="utf-8").splitlines():
            record = json.loads(line)
            if record.get(field) != expected:
                refuse(f"{name} differs from canonical transition host lock")
    return lease


def require_native_library(source: Path, lib: Path) -> None:
    if (not source.is_absolute() or not lib.is_absolute()
            or source.resolve() != source or lib.resolve() != lib
            or not lib.is_relative_to(source)):
        refuse("selected native library must belong to selected native source")


def require_artifact_side(entry: dict[str, object], generate: dict[str, object],
                          build: dict[str, object], read: dict[str, object],
                          checkout: Path, label: str) -> None:
    artifacts = entry.get("generated_artifacts")
    try:
        observed = executor._require_generated_artifacts(generate, checkout)
    except (executor.Refused, OSError, ValueError) as exc:
        raise qualification.Refused(f"{label} generated artifact inventory refused: {exc}") from exc
    if (artifacts != generate.get("generated_artifacts")
            or artifacts != build.get("generated_artifacts")
            or artifacts != read.get("generated_artifacts")
            or observed != artifacts):
        refuse(f"{label} generated artifacts differ from authenticated stages")


def require_artifact_delta(row: dict[str, object], selected: dict[str, object]) -> None:
    try:
        delta = qualification._compare_sides(selected, row["before"], row["after"])
    except (qualification.Refused, KeyError, TypeError, ValueError) as exc:
        raise qualification.Refused(f"{row['id']} generated artifact transition refused: {exc}") from exc
    if row.get("artifact_delta") != delta:
        refuse(f"{row['id']} generated artifact delta differs from canonical comparison")


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
    require_native_library(source, lib)
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
        commands = raw.get("commands")
        if not isinstance(commands, list) or any(not isinstance(command, dict) for command in commands):
            refuse("raw write commands must be a list of objects")
        matrices = checked.get("matrix_reports")
        fixtures = checked["fixtures"]["entries"]
        if (not isinstance(matrices, list) or not matrices or len(matrices) != len(fixtures)
                or raw.get("state") != "ok" or raw.get("matrix_reports") != matrices
                or len(commands) != len(matrices)
                or any(command.get("state") != "ok" for command in commands)):
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
    if any(path.is_symlink() for path in paths):
        refuse("Task19 committed result path must not be a symlink")
    paths = tuple(qualification._evidence_location(path, "Task19 committed result") for path in paths)
    if len({path.resolve() for path in paths}) != 3:
        refuse("final paths must identify three distinct committed runs")
    if set(expected_binaries) != {(row, side) for row in ROWS for side in qualification.SIDES}:
        refuse("six explicit binary identities are required")
    target_root, ops_root, perl_path = _durable_executable_roots()
    for expected in expected_binaries.values():
        _rooted_executable(Path(expected["path"]), target_root, "expected OxiDex binary")
    _rooted_executable(perl_path, ops_root, "approved Perl executable")
    if qualification._perl(ops_root) != perl_path:
        refuse("approved Perl differs from configured durable installation")
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
        selected_row = canonical_row(result, path, target_root)
        canonical_lease = require_canonical_lease(path)
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
            run_dir = path.parent / row["id"] / side
            journal = qualification._read_object(run_dir / "execution-status.json", "execution journal")
            config = qualification._read_object(run_dir / "inputs" / "config.json", "execution config")
            require_canonical_side(config, selected_row, side, release, run_dir, row)
            if (config.get("host_lock") != str(canonical_lease)
                    or journal.get("host_lock") != str(canonical_lease)):
                refuse(f"{row['id']} {side} did not use canonical transition host lock")
            _rooted_directory(Path(config["target_directories"][release]), target_root,
                              "committed Cargo target")
            _rooted_executable(Path(expected["path"]), Path(config["target_directories"][release]),
                               "expected side binary")
            native_perl = Path(instrument["native_identity"]["perl"]["path"])
            if native_perl != perl_path:
                refuse("committed Perl differs from approved durable executable")
            checkout = run_dir / "checkouts" / executor._safe_name(release)
            build = qualification._report_for(run_dir, journal, release, "build")
            generate = qualification._report_for(run_dir, journal, release, "generate")
            read = qualification._report_for(run_dir, journal, release, "read")
            require_artifact_side(entry, generate, build, read, checkout, f"{row['id']} {side}")
            build_environment = qualification._build_environment_receipt(build, release, checkout)
            release_tests = qualification._release_test_receipt(run_dir, journal, release)
            fixture_dependencies = qualification.replay_fixture_dependencies(
                row, side, run_dir, release, release_tests)
            test_report = qualification._report_for(run_dir, journal, release, "test")
            executor._require_test_suite_proof(test_report)
            if entry.get("build_environment") != build_environment or entry.get("release_tests") != release_tests:
                refuse(f"{row['id']} {side} build or release-test receipt differs from accepted proof")
            bundle = config.get("verified_input_bundle")
            if not isinstance(bundle, str):
                refuse("committed side lacks verified source input bundle")
            source_identity = qualification.resolve_source_identity(
                selected_row["immutable_source_identities"][side],
                qualification._evidence_location(Path(bundle), "verified source input bundle"))
            source_fields = ("release", "tag_object", "peeled_commit", "source_directory",
                             "source_tree_sha256", "materialization_sha256")
            verified_source = {key: source_identity[key] for key in source_fields}
            if entry.get("source_identity") != verified_source:
                refuse(f"{row['id']} {side} source identity differs from verified input bundle")
            write_proof = replay_committed_write(row, side, path.parent, expected_head)
            _rooted_executable(Path(write_proof["writer_binary"]["path"]),
                               Path(config["target_directories"][release]), "committed writer binary")
            binaries[side] = expected
            sides[side] = {"release": release, "source_identity": verified_source,
                           "source_root": source_identity["source_root"],
                           "target_directory": config["target_directories"][release],
                           "source_dependencies": source_identity["dependencies"],
                           "materialized_trees": source_identity["materialized_trees"],
                           "fixture_dependencies": fixture_dependencies,
                           "native_identity": instrument["native_identity"],
                           "read_fixture_manifest_sha256": instrument["read_fixture_manifest_sha256"],
                           "read_report_sha256": entry["read_report_sha256"],
                           "write_report_sha256": entry["write_report_sha256"],
                           "execution_journal_sha256": entry["execution_journal_sha256"],
                           "committed_write": write_proof}
        require_artifact_delta(row, selected_row)
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
    corpus_proof = qualification._fixture_corpus_authority()["directory_proof"]
    return {"schema": 1, "kind": "oxidex_task19_committed_replay",
            "status": "verified_read_only", "promotion": "forbidden",
            "candidate": {"head": expected_head, "tree": expected_tree, "pin": "13.59"},
            "matrix_sha256": expected_matrix_sha256,
            "fixture_corpus_directory": corpus_proof,
            "read_policy_input": policy_binding, "tools": expected_tools,
            "rows": {name: accepted[name] for name in ROWS},
            "next_pin": {"selection": "not selected",
                         "todo_sha256": sha(ROOT / "TODO_RELEASE_BETA.md")}}


def subordinate_snapshot(paths: tuple[Path, Path, Path],
                         value: dict[str, object]) -> dict[str, tuple[int, int, int, int, int, str]]:
    """Bind the complete run directories and external executables across replay."""
    files: set[Path] = {qualification.CANONICAL_MATRIX,
                        Path(value["read_policy_input"]["path"])}
    if value.get("kind") == "oxidex_task19_committed_replay":
        expected_corpus = value.get("fixture_corpus_directory")
        if (not isinstance(expected_corpus, dict)
                or qualification._fixture_corpus_authority()["directory_proof"] != expected_corpus):
            refuse("actual combined corpus directory changed during replay or publication")
        target_root, ops_root, perl_path = _durable_executable_roots()
    else:
        target_root = ops_root = perl_path = None
    directories: set[Path] = {
        qualification._evidence_location(marker, "Task19 committed result").parent
        for marker in paths}
    for row in value["rows"].values():
        for side in qualification.SIDES:
            proof = row["sides"][side]
            native = proof["native_identity"]
            if target_root is not None:
                target = _rooted_directory(Path(proof["target_directory"]), target_root,
                                           "replayed Cargo target")
                _rooted_executable(Path(row["binaries"][side]["path"]), target,
                                   "replayed OxiDex binary")
                _rooted_executable(Path(proof["committed_write"]["writer_binary"]["path"]),
                                   target, "replayed writer binary")
                if Path(native["perl"]["path"]) != perl_path:
                    refuse("replayed Perl differs from approved durable executable")
                _rooted_executable(perl_path, ops_root, "replayed approved Perl executable")
            source_name = Path(proof["source_identity"]["source_directory"])
            if source_name.is_absolute() or len(source_name.parts) != 1 or source_name.name in (".", ".."):
                refuse("verified materialization source name is not a single relative directory")
            source_root = qualification._evidence_location(
                Path(proof["source_root"]), "verified source root")
            native_source = qualification._evidence_location(
                source_root / source_name, "verified native source")
            if native_source != Path(native["source"]["path"]).resolve():
                refuse("native source differs from verified materialization directory")
            materialized_trees = proof.get("materialized_trees")
            if not isinstance(materialized_trees, list) or not materialized_trees:
                refuse("verified materialized tree closure is incomplete")
            selected_paths: set[Path] = set()
            for binding in materialized_trees:
                if (not isinstance(binding, dict)
                        or set(binding) != {"path", "tree_sha256"}
                        or not isinstance(binding["path"], str)
                        or not HEX64.fullmatch(str(binding["tree_sha256"]))):
                    refuse("verified materialized tree binding is malformed")
                selected = qualification._evidence_location(
                    Path(binding["path"]), "selected materialized source")
                if (str(selected) != binding["path"] or selected.parent != source_root
                        or selected in selected_paths or selected.is_symlink() or not selected.is_dir()):
                    refuse("verified materialized tree path changed during replay")
                selected_paths.add(selected)
                try:
                    current_tree = qualification.catalog_stage._tree_identity(selected)
                except qualification.catalog_stage.Refused as exc:
                    raise qualification.Refused("verified materialized source tree changed during replay") from exc
                if current_tree["tree_sha256"] != binding["tree_sha256"]:
                    refuse("verified materialized source tree changed during replay")
                directories.add(selected)
            if native_source not in selected_paths:
                refuse("native source is absent from verified materialized trees")
            files.add(Path(row["binaries"][side]["path"]))
            files.add(Path(proof["committed_write"]["writer_binary"]["path"]))
            files.add(Path(native["lib"]["path"]) / "Image" / "ExifTool.pm")
            files.add(Path(native["perl"]["path"]))
            source_dependencies = proof.get("source_dependencies")
            fixture_dependencies = proof.get("fixture_dependencies")
            if (not isinstance(source_dependencies, list) or len(source_dependencies) < 7
                    or not isinstance(fixture_dependencies, list) or not fixture_dependencies):
                refuse("verified input dependency closure is incomplete")
            dependencies = source_dependencies + fixture_dependencies
            for dependency in dependencies:
                if (not isinstance(dependency, dict)
                        or set(dependency) != {"path", "sha256", "bytes"}
                        or not isinstance(dependency["path"], str)
                        or not HEX64.fullmatch(str(dependency["sha256"]))
                        or type(dependency["bytes"]) is not int or dependency["bytes"] < 1):
                    refuse("verified source dependency binding is malformed")
                path = qualification._evidence_location(
                    Path(dependency["path"]), "verified source dependency")
                if str(path) != dependency["path"]:
                    refuse("verified source dependency is not canonical")
                observed = path.lstat()
                if (not stat.S_ISREG(observed.st_mode)
                        or observed.st_size != dependency["bytes"]
                        or observed.st_size > qualification.catalog_stage.MAX_ARCHIVE_BYTES
                        or qualification._sha_file(path) != dependency["sha256"]):
                    refuse("verified source dependency changed during replay")
                files.add(path)
    for directory in directories:
        if directory.is_symlink() or not directory.is_dir():
            refuse("committed evidence directory disappeared during replay")
        files.add(directory)
        for item in directory.rglob("*"):
            files.add(item)
            if len(files) > 200_000:
                refuse("committed evidence exceeds bounded file inventory")
    captured: dict[str, tuple[int, int, int, int, int, str]] = {}
    for path in sorted(files):
        observed = path.lstat()
        if stat.S_ISLNK(observed.st_mode):
            data = os.fsencode(os.readlink(path))
            digest = hashlib.sha256(data).hexdigest()
        elif stat.S_ISREG(observed.st_mode):
            digest = qualification._sha_file(path)
        elif stat.S_ISDIR(observed.st_mode):
            digest = "directory"
        else:
            refuse(f"committed evidence contains a non-regular file: {path}")
        after = path.lstat()
        fields = (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns,
                  after.st_mode)
        if fields != (observed.st_dev, observed.st_ino, observed.st_size,
                      observed.st_mtime_ns, observed.st_mode):
            refuse(f"committed evidence changed while hashing: {path}")
        captured[str(path)] = (*fields, digest)
    return captured


def _write_owned_state(descriptor: int, payload: bytes) -> None:
    """Use unbuffered writes so descriptor close can never flush stale success."""
    os.ftruncate(descriptor, 0)
    os.lseek(descriptor, 0, os.SEEK_SET)
    written = 0
    while written < len(payload):
        count = os.write(descriptor, payload[written:])
        if count <= 0:
            raise OSError("Task19 output write made no progress")
        written += count
    os.fsync(descriptor)


@contextmanager
def pinned_output_parent(output: Path):
    """Walk from the filesystem root using retained, non-following directory FDs."""
    if (not output.is_absolute() or ".." in output.parts or output.name in ("", ".", "..")
            or not hasattr(os, "O_NOFOLLOW")):
        refuse("Task19 output requires an absolute no-follow parent chain")
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    descriptors: list[int] = []
    names: list[str] = []
    changed: list[Path] = []
    current = Path("/")
    try:
        descriptors.append(os.open(current, flags))
        for name in output.parent.parts[1:]:
            parent_fd = descriptors[-1]
            child_path = current / name
            try:
                child_fd = os.open(name, flags, dir_fd=parent_fd)
            except FileNotFoundError:
                os.mkdir(name, 0o700, dir_fd=parent_fd)
                child_fd = os.open(name, flags, dir_fd=parent_fd)
                changed.append(current)
            descriptors.append(child_fd)
            names.append(name)
            current = child_path

        def check_chain() -> None:
            try:
                root = os.stat("/", follow_symlinks=False)
                held = os.fstat(descriptors[0])
                if (root.st_dev, root.st_ino) != (held.st_dev, held.st_ino):
                    refuse("Task19 output root changed during publication")
                for index, name in enumerate(names, 1):
                    link = os.stat(name, dir_fd=descriptors[index - 1], follow_symlinks=False)
                    held = os.fstat(descriptors[index])
                    if (not stat.S_ISDIR(link.st_mode)
                            or (link.st_dev, link.st_ino) != (held.st_dev, held.st_ino)):
                        refuse("Task19 output parent lost physical custody")
            except OSError as exc:
                raise qualification.OutcomeUnknown("Task19 output parent custody cannot be established") from exc

        def sync_chain() -> None:
            for descriptor in reversed(descriptors):
                os.fsync(descriptor)

        check_chain()
        yield descriptors[-1], check_chain, sync_chain, [output.parent, *changed]
    except FileExistsError as exc:
        raise qualification.Refused("Task19 output parent changed during creation") from exc
    finally:
        for descriptor in reversed(descriptors):
            os.close(descriptor)


def publish_receipt_no_replace(output: Path, value: dict[str, object],
                               validate_inputs=None, close_inputs=None) -> None:
    """Own output recovery through both validation gates and input custody closure.

    Success is returned only after retained inputs and the primary output FD
    close. Any earlier failure invalidates the owned inode through its duplicate.
    An uncertain close is never retried; no pathname is removed or overwritten.
    """
    payload = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")
    pending = b'{"status":"publication_pending","promotion":"forbidden"}\n'
    failed = b'{"status":"publication_failed","promotion":"forbidden"}\n'
    with pinned_output_parent(output) as (parent_fd, check_parent, sync_parent_chain, changed):
        try:
            check_parent()
            descriptor = os.open(output.name, os.O_RDWR | os.O_CREAT | os.O_EXCL |
                                 os.O_NOFOLLOW, 0o600, dir_fd=parent_fd)
        except FileExistsError as exc:
            raise qualification.Refused("Task19 replay output already exists") from exc
        recovery = None
        try:
            recovery = os.dup(descriptor)
            owned = os.fstat(descriptor)
            def check_output_custody() -> None:
                try:
                    check_parent()
                    current = os.stat(output.name, dir_fd=parent_fd, follow_symlinks=False)
                    retained = os.fstat(recovery)
                except FileNotFoundError as exc:
                    raise qualification.OutcomeUnknown("Task19 output disappeared during publication") from exc
                except OSError as exc:
                    raise qualification.OutcomeUnknown("Task19 output custody cannot be established") from exc
                if ((current.st_dev, current.st_ino) != (owned.st_dev, owned.st_ino)
                        or (retained.st_dev, retained.st_ino) != (owned.st_dev, owned.st_ino)):
                    raise qualification.OutcomeUnknown("Task19 output was replaced during publication")
                if (not stat.S_ISREG(current.st_mode) or not stat.S_ISREG(retained.st_mode)
                        or current.st_nlink != 1 or retained.st_nlink != 1
                        or stat.S_IMODE(current.st_mode) != 0o600
                        or stat.S_IMODE(retained.st_mode) != 0o600
                        or current.st_uid != owned.st_uid):
                    refuse("Task19 output lost regular, private, single-link custody")
            _write_owned_state(descriptor, pending)
            if validate_inputs is not None:
                validate_inputs()
            _write_owned_state(descriptor, payload)
            for directory in dict.fromkeys(changed):
                qualification._fsync_directory(directory)
            sync_parent_chain()
            check_output_custody()
            if validate_inputs is not None:
                validate_inputs()
            if close_inputs is not None:
                close_inputs()
            closing = descriptor
            descriptor = None  # close(2) may have closed it even when it raises.
            try:
                os.close(closing)
            except OSError as exc:
                raise qualification.OutcomeUnknown("Task19 output close outcome is uncertain") from exc
            check_output_custody()
            try:
                final_bytes = os.pread(recovery, len(payload) + 1, 0)
            except OSError as exc:
                raise qualification.OutcomeUnknown("Task19 output bytes cannot be established") from exc
            if final_bytes != payload:
                refuse("Task19 output bytes changed during publication")
            check_output_custody()
        except BaseException:
            # Recovery remains open even if the primary descriptor's close failed.
            # Every callback/cleanup failure must invalidate success. No pathname
            # is used or removed, including when a foreign inode replaced it.
            if recovery is not None:
                try:
                    _write_owned_state(recovery, failed)
                except BaseException as exc:
                    raise qualification.OutcomeUnknown("Task19 owned output invalidation is uncertain") from exc
            raise
        finally:
            if descriptor is not None:
                closing = descriptor
                descriptor = None
                try:
                    os.close(closing)
                except OSError:
                    pass
            if recovery is not None:
                # This duplicate has no userspace buffer and all writes were fsynced.
                # Its close cannot change durable receipt bytes.
                try:
                    os.close(recovery)
                except OSError:
                    pass


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
        git = bind_git()
        before = source_snapshot(args.expected_head, args.expected_tree, git)
        selection = next_pin_selection((ROOT / "TODO_RELEASE_BETA.md").read_text(encoding="utf-8"))
        if selection != "not selected":
            refuse("TODO selects a next ExifTool release; the checked three-row matrix and read policy do not support that transition")
        tools = tool_expectations(args.expected_tool)
        binaries = binary_expectations(args.expected_binary)
        output = qualification._evidence_location(args.output, "Task19 replay output")
        refuse_source_output_overlap(output)
        if output.exists() or output.is_symlink():
            refuse("Task19 replay output already exists")
        replay_paths = tuple(qualification._evidence_location(path, "Task19 committed result")
                             for path in (args.same_pin_result, args.forward_result, args.reverse_result))
        with ExitStack() as marker_stack:
            pins = {path: marker_stack.enter_context(pin_marker(path)) for path in replay_paths}
            for path, pin in pins.items():
                require_marker_unchanged(path, pin.binding, pin)
            value = verify_results(
                paths=replay_paths,
                expected_head=args.expected_head, expected_tree=args.expected_tree,
                expected_matrix_sha256=args.expected_matrix_sha256,
                expected_policy_sha256=args.expected_read_policy_sha256,
                expected_tools=tools, expected_binaries=binaries)
            subordinates = subordinate_snapshot(replay_paths, value)
            if source_snapshot(args.expected_head, args.expected_tree, git) != before:
                refuse("owned source changed during Task19 replay")
            for row, path in zip(ROWS, replay_paths, strict=True):
                require_marker_unchanged(
                    path, {key: val for key, val in value["rows"][row]["result"].items()
                           if key != "path"}, pins[path])
            refuse_source_output_overlap(output)
            if output.exists() or output.is_symlink():
                refuse("Task19 replay output already exists")
            # Re-run the complete subordinate replay so an earlier side changed
            # while later rows were checked cannot be published as verified.
            if verify_results(
                    paths=replay_paths,
                    expected_head=args.expected_head, expected_tree=args.expected_tree,
                    expected_matrix_sha256=args.expected_matrix_sha256,
                    expected_policy_sha256=args.expected_read_policy_sha256,
                    expected_tools=tools, expected_binaries=binaries) != value:
                refuse("Task19 subordinate evidence changed before publication")
            if subordinate_snapshot(replay_paths, value) != subordinates:
                refuse("Task19 subordinate files changed during replay")
            if source_snapshot(args.expected_head, args.expected_tree, git) != before:
                refuse("owned source changed before Task19 publication")
            refuse_source_output_overlap(output)
            if subordinate_snapshot(replay_paths, value) != subordinates:
                refuse("Task19 subordinate files changed before publication")
            for path, pin in pins.items():
                require_marker_unchanged(path, pin.binding, pin)
            value["git"] = git.evidence()
            def validate_inputs() -> None:
                git.revalidate()
                for path, pin in pins.items():
                    require_marker_unchanged(path, pin.binding, pin)
                if subordinate_snapshot(replay_paths, value) != subordinates:
                    refuse("Task19 subordinate files changed during publication")
                if source_snapshot(args.expected_head, args.expected_tree, git) != before:
                    refuse("owned source changed during Task19 publication")
            publish_receipt_no_replace(output, value, validate_inputs=validate_inputs,
                                       close_inputs=marker_stack.close)
    except qualification.OutcomeUnknown as exc:
        print(f"Task19 replay publication outcome unknown: {exc}", file=sys.stderr)
        return 4
    except (qualification.Refused, executor.Refused, stage_adapter.Refused,
            qualification.rehearsal.Refused, qualification.catalog_stage.Refused,
            OSError, KeyError, TypeError, AttributeError, ValueError) as exc:
        print(f"Task19 replay refused: {exc}", file=sys.stderr)
        return 2
    print(str(output))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
