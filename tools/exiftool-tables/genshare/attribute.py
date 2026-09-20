#!/usr/bin/env python3
"""Create and replay authenticated generated-route census receipts.

The maintained format is ``genshare-receipt/v3``.  A census stages an explicit
manifest into a fresh durable run directory, records raw oracle/candidate child
evidence for six individual silence tokens and their independently executed
union, and emits an ``observed_unreviewed`` receipt.  Only controller review can
promote an observation to acceptance; zero loss is always ``unexercised``.
"""

from __future__ import annotations

import argparse
import collections
import ctypes
import datetime as dt
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import time
import uuid


TOKENS = ("engine", "legacy-l1", "legacy-l2", "producers", "serial", "keyed")
UNION = ",".join(TOKENS)
SCHEMA = "genshare-receipt/v3"
COUNTERS = (
    "oracle_occurrences",
    "candidate_occurrences",
    "matched_occurrences",
    "missing_occurrences",
    "extra_occurrences",
    "value_occurrences",
    "rename_source_occurrences",
    "rename_target_occurrences",
)
SCRUBBED_ENV = (
    "PERL5LIB",
    "PERLLIB",
    "PERL5OPT",
    "EXIFTOOL",
    "EXIFTOOL_CACHE_DIR",
    "EXIFTOOL_PERL",
)
RECEIPT_FILES = {
    "receipt.pending.json",
    "receipt.json",
    "receipt.sha256",
    "receipt.failed.json",
    "receipt.failed.sha256",
    "validator.json",
    ".complete",
}
TASK8_RUST_BOUNDARY = (
    "src/exiftool_tables/attribution.rs",
    "src/exiftool_tables/engine.rs",
    "src/exiftool_tables/ifd_engine.rs",
    "src/exiftool_tables/keyed_engine.rs",
    "src/exiftool_tables/mod.rs",
    "src/exiftool_tables/runtime.rs",
    "src/exiftool_tables/serial_engine.rs",
    "src/main.rs",
    "src/composite/compute.rs",
    "src/composite/mod.rs",
    "src/core/file_metadata.rs",
    "src/core/operations.rs",
    "src/parsers/archive/ar.rs",
    "src/parsers/canon_vrd/mod.rs",
    "src/parsers/elf/metadata_extractor.rs",
    "src/parsers/flir_fpf.rs",
    "src/parsers/jpeg/app_segments/infiray.rs",
    "src/parsers/macho/metadata_extractor.rs",
    "src/parsers/specialized/fits.rs",
    "src/parsers/tiff/geotiff_parser.rs",
    "src/parsers/tiff/makernotes/canon/custom_functions2.rs",
    "src/parsers/tiff/makernotes/nikon/settings.rs",
    "src/parsers/tiff/makernotes/shared/binary_subdir.rs",
    "src/parsers/tiff/makernotes/sony.rs",
    "src/parsers/tiff/makernotes/sony/binary_data.rs",
)


class ReceiptError(ValueError):
    """An input or retained receipt cannot authenticate its claim."""


class ChildProcessError(ReceiptError):
    """A retained child execution did not complete and parse successfully."""


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    with path.open("rb") as handle:
        digest = hashlib.sha256()
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def canonical_bytes(value) -> bytes:
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")


def canonical_sha256(value) -> str:
    return sha256_bytes(canonical_bytes(value))


def path_set_sha256(paths: list[str]) -> str:
    return canonical_sha256(paths)


def _atomic_write(path: Path, data: bytes, mode: int = 0o644) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}.pending")
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, mode)
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(data)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        directory_fd = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    finally:
        if temporary.exists():
            temporary.unlink()


def write_json(path: Path, value) -> None:
    _atomic_write(path, json.dumps(value, indent=2, sort_keys=True).encode() + b"\n")


def validate_token_request(raw: str) -> dict:
    parts = raw.split(",")
    if any(not part for part in parts):
        raise ReceiptError("token contract contains an empty token")
    if len(parts) != len(set(parts)):
        raise ReceiptError("token contract contains a duplicate token")
    refused = [part for part in parts if part not in TOKENS]
    if refused:
        label = "unsafe" if "conv" in refused else "unknown"
        raise ReceiptError(f"{label} genshare token(s): {','.join(refused)}")
    if tuple(parts) != TOKENS:
        raise ReceiptError(f"token contract must be exactly {UNION}")
    return {"individual": list(TOKENS), "union": UNION}


def require_success_status(status: str) -> None:
    if status != "success":
        raise ReceiptError(f"receipt status {status!r} is not success")


def _manifest_rows(manifest: Path) -> list[str]:
    try:
        text = manifest.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as exc:
        raise ReceiptError(f"cannot read UTF-8 manifest {manifest}: {exc}") from exc
    rows = text.splitlines()
    if not rows or any(not row for row in rows):
        raise ReceiptError("manifest contains a blank path")
    if text and not text.endswith("\n"):
        raise ReceiptError("manifest must end with a newline")
    if len(rows) != len(set(rows)):
        raise ReceiptError("manifest contains a duplicate path")
    return rows


def _safe_relative(raw: str) -> PurePosixPath:
    if "\\" in raw:
        raise ReceiptError(f"manifest path uses a backslash alias: {raw!r}")
    path = PurePosixPath(raw)
    if path.is_absolute() or not path.parts or any(part in ("", ".", "..") for part in path.parts):
        raise ReceiptError(f"manifest path is not a safe relative POSIX path: {raw!r}")
    if str(path) != raw:
        raise ReceiptError(f"manifest path is not normalized: {raw!r}")
    return path


def _file_identity(path: Path, *, relative_path: str | None = None) -> dict:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode):
        raise ReceiptError(f"evidence path is not a regular file: {path}")
    result = {
        "size": info.st_size,
        "mode": stat.S_IMODE(info.st_mode),
        "sha256": sha256_file(path),
    }
    if relative_path is not None:
        result["relative_path"] = relative_path
    return result


def _stage_selection(corpus: Path, manifest: Path, run_root: Path, min_files: int) -> dict:
    corpus = corpus.resolve(strict=True)
    if not corpus.is_dir():
        raise ReceiptError(f"corpus root is not a directory: {corpus}")
    if min_files <= 0:
        raise ReceiptError("min-files must be a positive integer")
    rows = _manifest_rows(manifest)
    if len(rows) != min_files:
        raise ReceiptError(
            f"bounded manifest selects {len(rows)} files; exact min-files contract is {min_files}"
        )
    selection_root = run_root / "selection"
    selection_root.mkdir(mode=0o755)
    records = []
    for raw in rows:
        relative = _safe_relative(raw)
        source = corpus.joinpath(*relative.parts)
        try:
            source_lstat = source.lstat()
        except OSError as exc:
            raise ReceiptError(f"missing manifest file {raw}: {exc}") from exc
        if stat.S_ISLNK(source_lstat.st_mode) or not stat.S_ISREG(source_lstat.st_mode):
            raise ReceiptError(f"manifest file is not a regular non-symlink: {raw}")
        resolved = source.resolve(strict=True)
        try:
            resolved.relative_to(corpus)
        except ValueError as exc:
            raise ReceiptError(f"manifest path escapes corpus root: {raw}") from exc
        staged = selection_root.joinpath(*relative.parts)
        staged.parent.mkdir(parents=True, exist_ok=True)
        source_bytes = source.read_bytes()
        fd = os.open(staged, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
        with os.fdopen(fd, "wb") as handle:
            handle.write(source_bytes)
            handle.flush()
            os.fsync(handle.fileno())
        if staged.read_bytes() != source_bytes:
            raise ReceiptError(f"staged copy differs from source: {raw}")
        identity = _file_identity(source)
        records.append(
            {
                "relative_path": raw,
                "source_path": str(resolved),
                "staged_path": str(staged.resolve()),
                **identity,
            }
        )
    manifest_commitment = [
        {key: row[key] for key in ("relative_path", "size", "mode", "sha256")}
        for row in records
    ]
    return {
        "source_root": str(corpus),
        "selection_root": str(selection_root.resolve()),
        "ordered_paths": rows,
        "ordered_manifest": records,
        "manifest_sha256": canonical_sha256(manifest_commitment),
        "path_set_sha256": path_set_sha256(rows),
        "selected_files": len(rows),
    }


def create_bounded_selection(corpus: Path, manifest: Path, run_root: Path, min_files: int) -> dict:
    corpus, manifest, run_root = Path(corpus), Path(manifest), Path(run_root)
    if run_root.exists() or run_root.is_symlink():
        raise ReceiptError(f"output path already exists: {run_root}")
    run_root.parent.mkdir(parents=True, exist_ok=True)
    os.mkdir(run_root, 0o755)
    return _stage_selection(corpus, manifest, run_root.resolve(), min_files)


def _reject_duplicate_pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ReceiptError(f"duplicate JSON key: {key!r}")
        result[key] = value
    return result


def parse_json_output(raw: bytes, side: str) -> dict:
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise ReceiptError(f"{side} output is not UTF-8: {exc}") from exc
    try:
        decoded = json.loads(text, object_pairs_hook=_reject_duplicate_pairs, parse_float=str)
    except (json.JSONDecodeError, ReceiptError) as exc:
        raise ReceiptError(f"{side} output is invalid JSON: {exc}") from exc
    if not isinstance(decoded, list) or len(decoded) != 1 or not isinstance(decoded[0], dict):
        raise ReceiptError(f"{side} output must be a one-object JSON array")
    return decoded[0]


def occurrence_sequence(candidate: dict, *, normalize_access_date: bool) -> list[dict]:
    """Preserve ordered, typed output occurrences without sorting or collapsing."""
    counts = {}
    rows = []
    for position, (raw_key, original) in enumerate(candidate.items()):
        group, _, name = raw_key.partition(":")
        identity = (group, name)
        counts[identity] = counts.get(identity, 0) + 1
        value = (
            "<NORMALIZED:FileAccessDate>"
            if normalize_access_date and raw_key == "System:FileAccessDate"
            else original
        )
        rows.append(
            {
                "raw_key": raw_key,
                "group": group if name else "",
                "name": name if name else raw_key,
                "duplicate_instance": counts[identity],
                "position": position,
                "type": type(original).__name__,
                "value": value,
                "raw_serialized": json.dumps(value, ensure_ascii=False, separators=(",", ":")),
            }
        )
    return rows


def _artifact_record(path: Path) -> dict:
    return {"path": str(path.resolve()), **_file_identity(path)}


def _kernel_process_identity(pid: int) -> dict:
    """Read a non-display kernel identity for one exact PID instance."""
    if sys.platform.startswith("linux"):
        try:
            boot_id = Path("/proc/sys/kernel/random/boot_id").read_text(encoding="ascii").strip()
            fields = Path(f"/proc/{pid}/stat").read_text(encoding="utf-8").rsplit(") ", 1)[1].split()
            start_ticks = fields[19]
        except (OSError, IndexError, ValueError) as exc:
            raise ReceiptError(f"cannot read Linux kernel identity for PID {pid}: {exc}") from exc
        if not boot_id or not start_ticks.isdigit():
            raise ReceiptError(f"Linux kernel identity for PID {pid} is incomplete")
        return {"platform": "linux", "boot_id": boot_id, "start_token": start_ticks}
    if sys.platform == "darwin":
        try:
            # proc_bsdinfo is 136 bytes; its start timeval occupies bytes 120..136.
            info = ctypes.create_string_buffer(136)
            libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
            size = libproc.proc_pidinfo(pid, 3, 0, info, 136)
            if size < 136:
                raise ReceiptError(
                    f"macOS libproc returned {size} bytes for PID {pid}, expected 136"
                )
            raw = info.raw
            seconds = int.from_bytes(raw[120:128], sys.byteorder)
            microseconds = int.from_bytes(raw[128:136], sys.byteorder)
        except (OSError, AttributeError) as exc:
            raise ReceiptError(f"cannot read macOS kernel identity for PID {pid}: {exc}") from exc
        if seconds <= 0 or not (0 <= microseconds < 1_000_000):
            raise ReceiptError(f"macOS kernel identity for PID {pid} is incomplete")
        return {
            "platform": "darwin",
            "boot_id": None,
            "start_token": f"{seconds}:{microseconds}",
        }
    raise ReceiptError(f"unsupported platform for kernel process identity: {sys.platform}")


def _validate_process_identity_record(record: dict) -> None:
    identity = record.get("process_identity")
    if not isinstance(record.get("pid"), int) or not isinstance(identity, dict):
        raise ReceiptError("child record lacks PID-bound kernel identity")
    captured = identity.get("captured")
    verified = identity.get("verified_before_communicate")
    if identity.get("pid") != record["pid"] or not isinstance(captured, dict) \
            or captured != verified or not captured.get("start_token"):
        raise ReceiptError("child PID kernel identity is absent or unstable")
    if captured.get("platform") not in ("linux", "darwin"):
        raise ReceiptError("child PID kernel identity platform is invalid")
    if captured["platform"] == "linux" and not captured.get("boot_id"):
        raise ReceiptError("Linux child PID identity lacks boot ID")


def capture_process(
    argv: list[str],
    cwd: Path,
    child_dir: Path,
    side: str,
    environment: dict,
    *,
    timeout: int = 120,
) -> dict:
    """Run one child and retain raw bytes, outcome, parse status, and identities."""
    child_dir.mkdir(parents=True, exist_ok=True)
    stdout_path = child_dir / f"{side}.stdout"
    stderr_path = child_dir / f"{side}.stderr"
    returncode_path = child_dir / f"{side}.returncode"
    parsed_path = child_dir / f"{side}.parsed.json"
    process_env = os.environ.copy()
    for key in SCRUBBED_ENV:
        process_env.pop(key, None)
    for key, declaration in environment.items():
        state = declaration.get("state")
        if state == "absent":
            process_env.pop(key, None)
        elif state in ("empty", "value"):
            process_env[key] = declaration.get("value", "")
        else:
            raise ReceiptError(f"invalid environment state for {key}: {state!r}")
    process_env.update({"LC_ALL": "C", "LANG": "C"})
    started_at = utc_now()
    started_ns = time.time_ns()
    timed_out = False
    signal = None
    identity_error = None
    process = subprocess.Popen(
        argv,
        cwd=cwd,
        env=process_env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    pid = process.pid
    try:
        captured_identity = _kernel_process_identity(pid)
        verified_identity = _kernel_process_identity(pid)
        if captured_identity != verified_identity:
            identity_error = (
                f"{side} child PID {pid} identity changed before communicate: "
                f"{captured_identity!r} != {verified_identity!r}"
            )
            process.kill()
        try:
            stdout, stderr = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            process.kill()
            stdout, stderr = process.communicate()
    except (ReceiptError, OSError) as exc:
        identity_error = f"{side} child PID {pid} identity unavailable: {exc}"
        process.kill()
        stdout, stderr = process.communicate()
        captured_identity = None
        verified_identity = None
    returncode = None if timed_out else process.returncode
    if returncode is not None and returncode < 0:
        signal = -returncode
    _atomic_write(stdout_path, stdout)
    _atomic_write(stderr_path, stderr)
    _atomic_write(returncode_path, f"{returncode if returncode is not None else 'timeout'}\n".encode())
    record = {
        "argv": list(argv),
        "cwd": str(Path(cwd).resolve()),
        "environment": environment,
        "scrubbed_environment": list(SCRUBBED_ENV),
        "locale": {"LC_ALL": "C", "LANG": "C"},
        "started_at": started_at,
        "completed_at": utc_now(),
        "elapsed_ns": time.time_ns() - started_ns,
        "pid": pid,
        "process_identity": {
            "pid": pid,
            "captured": captured_identity,
            "verified_before_communicate": verified_identity,
        },
        "timeout_seconds": timeout,
        "timed_out": timed_out,
        "signal": signal,
        "returncode": returncode,
        "stdout": _artifact_record(stdout_path),
        "stderr": _artifact_record(stderr_path),
        "returncode_artifact": _artifact_record(returncode_path),
        "parse_status": "not-attempted",
        "parsed": None,
    }
    if timed_out:
        write_json(child_dir / f"{side}.process.json", record)
        raise ChildProcessError(f"{side} child timed out after {timeout}s")
    if identity_error is not None:
        write_json(child_dir / f"{side}.process.json", record)
        raise ChildProcessError(identity_error)
    if returncode != 0:
        write_json(child_dir / f"{side}.process.json", record)
        raise ChildProcessError(f"{side} child returned return code {returncode}")
    try:
        parsed = parse_json_output(stdout, side)
    except ReceiptError as exc:
        record["parse_status"] = "failed"
        write_json(child_dir / f"{side}.process.json", record)
        raise ChildProcessError(str(exc)) from exc
    write_json(parsed_path, parsed)
    record["parse_status"] = "ok"
    record["parsed"] = _artifact_record(parsed_path)
    write_json(child_dir / f"{side}.process.json", record)
    return record


_COMPARATOR = None


def _comparator():
    global _COMPARATOR
    if _COMPARATOR is None:
        path = Path(__file__).resolve().parent.parent / "conformance.py"
        spec = importlib.util.spec_from_file_location("genshare_conformance", path)
        module = importlib.util.module_from_spec(spec)
        if spec.loader is None:
            raise ReceiptError(f"cannot load comparator {path}")
        spec.loader.exec_module(module)
        _COMPARATOR = module
    return _COMPARATOR


def project_file(oracle: dict, candidate: dict) -> dict:
    comparator = _comparator()
    result = comparator.compare(oracle, candidate)
    projection = {
        "oracle_occurrences": comparator.occurrence_count(oracle, comparator.split_oracle_key),
        "candidate_occurrences": comparator.occurrence_count(candidate, comparator.split_oxidex_key),
        "matched_occurrences": len(result["matched"]),
        "missing_occurrences": len(result["missing"]),
        "extra_occurrences": len(result["extra"]),
        "value_occurrences": len(result["value_diff"]),
        "rename_source_occurrences": len(result["renames"]),
        "rename_target_occurrences": len(result["renames"]),
        "missing": {key: list(value) for key, value in result["missing"].items()},
        "extra": {key: list(value) for key, value in result["extra"].items()},
        "value_diff": [list(row) for row in result["value_diff"]],
        "renames": [list(row) for row in result["renames"]],
    }
    validate_equations(projection)
    return projection


def validate_equations(counts: dict) -> None:
    for key in COUNTERS:
        if not isinstance(counts.get(key), int) or counts[key] < 0:
            raise ReceiptError(f"projection {key} must be a non-negative integer")
    oracle_rebuilt = (
        counts["matched_occurrences"]
        + counts["value_occurrences"]
        + counts["missing_occurrences"]
        + counts["rename_source_occurrences"]
    )
    candidate_rebuilt = (
        counts["matched_occurrences"]
        + counts["value_occurrences"]
        + counts["extra_occurrences"]
        + counts["rename_target_occurrences"]
    )
    if counts["oracle_occurrences"] != oracle_rebuilt:
        raise ReceiptError("oracle occurrence equation does not reconcile")
    if counts["candidate_occurrences"] != candidate_rebuilt:
        raise ReceiptError("candidate occurrence equation does not reconcile")


def reconcile(control: dict, probe: dict) -> dict:
    validate_equations(control)
    validate_equations(probe)
    matched_lost = control["matched_occurrences"] - probe["matched_occurrences"]
    oracle_rebuild = (
        probe["value_occurrences"] - control["value_occurrences"]
        + probe["missing_occurrences"] - control["missing_occurrences"]
        + probe["rename_source_occurrences"] - control["rename_source_occurrences"]
    )
    candidate_delta = probe["candidate_occurrences"] - control["candidate_occurrences"]
    candidate_rebuild = (
        probe["matched_occurrences"] - control["matched_occurrences"]
        + probe["value_occurrences"] - control["value_occurrences"]
        + probe["extra_occurrences"] - control["extra_occurrences"]
        + probe["rename_target_occurrences"] - control["rename_target_occurrences"]
    )
    return {
        "matched_lost": matched_lost,
        "oracle_rebuild": oracle_rebuild,
        "oracle_residual": matched_lost - oracle_rebuild,
        "candidate_delta": candidate_delta,
        "candidate_rebuild": candidate_rebuild,
        "candidate_residual": candidate_delta - candidate_rebuild,
        "gained_rows": max(0, -matched_lost),
    }


def exercise_status(control: dict, probe: dict, *, production_reachable: bool) -> str:
    if not production_reachable:
        return "unexercised"
    result = reconcile(control, probe)
    return "observed_unreviewed" if result["matched_lost"] > 0 else "unexercised"


def _inside(root: Path, path: Path) -> bool:
    try:
        path.resolve(strict=True).relative_to(root.resolve(strict=True))
    except (OSError, ValueError):
        return False
    return True


def validate_v3_receipt(receipt: dict, run_root: Path, *, replay: bool = True) -> None:
    root = Path(run_root).resolve(strict=True)
    required = {
        "schema", "status", "run_root", "token_contract", "artifact_index",
        "projections", "reconciliations", "pre_seam", "pre_seam_control",
        "failed_stage", "failure",
    }
    missing = sorted(required - receipt.keys())
    if missing:
        raise ReceiptError(f"receipt is missing required fields: {', '.join(missing)}")
    if receipt.get("schema") != SCHEMA:
        raise ReceiptError(f"receipt schema must be {SCHEMA}")
    status = receipt.get("status")
    if status not in ("success", "failed", "observed_unreviewed"):
        raise ReceiptError("receipt status is invalid")
    if status == "failed":
        failure = receipt.get("failure")
        if not isinstance(receipt.get("failed_stage"), str) or not isinstance(failure, dict) \
                or not isinstance(failure.get("exit_code"), int) or failure["exit_code"] == 0 \
                or not isinstance(failure.get("started_corpus_children"), int):
            raise ReceiptError("failed receipt lacks a terminal failure record")
    elif receipt.get("failed_stage") is not None or receipt.get("failure") is not None:
        raise ReceiptError("non-failed receipt carries a terminal failure record")
    if Path(receipt.get("run_root", "")).resolve() != root:
        raise ReceiptError("receipt run_root does not match its location")
    contract = receipt.get("token_contract")
    if contract != {"individual": list(TOKENS), "union": UNION}:
        raise ReceiptError("receipt token contract is not the exact six-token contract")
    index = receipt.get("artifact_index")
    if not isinstance(index, list):
        raise ReceiptError("artifact index is required")
    seen = set()
    for row in index:
        relative = row.get("relative_path") if isinstance(row, dict) else None
        if not isinstance(relative, str) or relative in seen:
            raise ReceiptError("artifact index has an invalid or duplicate path")
        seen.add(relative)
        safe = _safe_relative(relative)
        path = root.joinpath(*safe.parts)
        if path.is_symlink() or not _inside(root, path):
            raise ReceiptError(f"artifact is outside run root or symlinked: {relative}")
        actual = _file_identity(path, relative_path=relative)
        if actual != row:
            raise ReceiptError(f"artifact identity does not verify: {relative}")
    if index != _artifact_index(root):
        raise ReceiptError("artifact index is not the exact retained artifact set")
    if replay:
        _replay_receipt(receipt, root)


def _replay_receipt(receipt: dict, root: Path) -> None:
    projections = receipt.get("projections")
    reconciliations = receipt.get("reconciliations")
    if not isinstance(projections, dict) or not isinstance(reconciliations, dict):
        raise ReceiptError("receipt lacks replayable projections/reconciliations")
    runs = receipt.get("runs")
    if not isinstance(runs, dict) or set(runs) != {
        "pre-seam-control", "control-unset", "control-empty", *TOKENS, "union"
    }:
        raise ReceiptError("run mode set is not exact")
    recomputed = {}
    expected_paths = receipt.get("selection", {}).get("ordered_paths")
    expected_path_hash = path_set_sha256(expected_paths) if isinstance(expected_paths, list) else None
    for mode, run in runs.items():
        children = run.get("children") if isinstance(run, dict) else None
        if not isinstance(children, list):
            raise ReceiptError(f"run {mode} lacks child records")
        paths = [row.get("relative_path") for row in children]
        if paths != expected_paths or run.get("path_set_sha256") != expected_path_hash:
            raise ReceiptError(f"run {mode} path set is not exact")
        per_file = {}
        for child in children:
            process_path = Path(child["process"]["path"])
            if not _inside(root, process_path) or _artifact_record(process_path) != child["process"]:
                raise ReceiptError(f"run {mode} child process artifact does not verify")
            process = json.loads(
                process_path.read_text(encoding="utf-8"), object_pairs_hook=_reject_duplicate_pairs
            )
            sides = {}
            for side in ("oracle", "candidate"):
                record = process.get(side)
                if not isinstance(record, dict) or record.get("returncode") != 0 \
                        or record.get("timed_out") or record.get("signal") is not None \
                        or record.get("parse_status") != "ok":
                    raise ReceiptError(f"run {mode} {side} child outcome is not successful")
                _validate_process_identity_record(record)
                for artifact_name in ("stdout", "stderr", "returncode_artifact", "parsed"):
                    artifact = record.get(artifact_name)
                    path = Path(artifact.get("path", "")) if isinstance(artifact, dict) else Path("")
                    if not _inside(root, path) or _artifact_record(path) != artifact:
                        raise ReceiptError(f"run {mode} {side} {artifact_name} does not verify")
                raw = Path(record["stdout"]["path"]).read_bytes()
                parsed = parse_json_output(raw, side)
                retained = json.loads(
                    Path(record["parsed"]["path"]).read_text(encoding="utf-8"),
                    object_pairs_hook=_reject_duplicate_pairs,
                )
                if parsed != retained:
                    raise ReceiptError(f"run {mode} {side} parsed output does not match raw stdout")
                sides[side] = parsed
            relative = child["relative_path"]
            per_file[relative] = project_file(sides["oracle"], sides["candidate"])
        recomputed[mode] = {"per_file": per_file, "aggregate": _sum_projection(per_file)}
        if recomputed[mode] != projections.get(mode):
            raise ReceiptError(f"projection counters or rows do not replay: {mode}")
    if set(projections) != set(recomputed):
        raise ReceiptError("projection mode set is not exact")
    for mode, projection in projections.items():
        aggregate = projection.get("aggregate") if isinstance(projection, dict) else None
        if not isinstance(aggregate, dict):
            raise ReceiptError(f"projection {mode} lacks aggregate counters")
        validate_equations(aggregate)
    selection = receipt.get("selection")
    if not isinstance(selection, dict):
        raise ReceiptError("selection is required for replay")
    rows = selection.get("ordered_manifest")
    if not isinstance(rows, list) or [row.get("relative_path") for row in rows] != expected_paths:
        raise ReceiptError("selection manifest path order does not verify")
    commitment = [
        {key: row.get(key) for key in ("relative_path", "size", "mode", "sha256")}
        for row in rows
    ]
    if selection.get("manifest_sha256") != canonical_sha256(commitment) \
            or selection.get("path_set_sha256") != expected_path_hash \
            or selection.get("selected_files") != len(expected_paths):
        raise ReceiptError("selection manifest counters or hashes do not verify")
    for row in rows:
        staged = Path(row.get("staged_path", ""))
        if not _inside(root, staged) or _file_identity(staged)["sha256"] != row.get("sha256"):
            raise ReceiptError(f"staged selection artifact does not verify: {row.get('relative_path')}")
    control = projections.get("control-empty", {}).get("aggregate")
    if control is None:
        raise ReceiptError("control-empty projection is required")
    expected_modes = [*TOKENS, "union"]
    if set(reconciliations) != set(expected_modes):
        raise ReceiptError("reconciliation token/mode set is not exact")
    for mode in expected_modes:
        calculated = reconcile(control, projections[mode]["aggregate"])
        recorded = reconciliations[mode]
        for key, value in calculated.items():
            if recorded.get(key) != value:
                raise ReceiptError(f"reconciliation counter does not verify: {mode}.{key}")
        if calculated["oracle_residual"] or calculated["candidate_residual"]:
            raise ReceiptError(f"reconciliation residual is nonzero: {mode}")
    if _validate_inertness(runs, selection) != receipt.get("inertness"):
        raise ReceiptError("inertness claim does not replay")
    if _validate_pre_seam_control(runs, selection) != receipt.get("pre_seam_control"):
        raise ReceiptError("pre-seam ordinary-binary control does not replay")
    _validate_pre_seam_proof(receipt.get("pre_seam"), root, live=False)
    fixture = receipt.get("fixture_contract")
    if not isinstance(fixture, dict):
        raise ReceiptError("fixture contract is required")
    expected_fixture = _fixture_observations(
        runs,
        recomputed,
        {mode: reconcile(recomputed["control-empty"]["aggregate"], recomputed[mode]["aggregate"])
         for mode in expected_modes},
        {"sha256": fixture.get("expectations_sha256")},
    )
    if expected_fixture != fixture:
        raise ReceiptError("fixture observations do not replay")


def _artifact_index(root: Path) -> list[dict]:
    rows = []
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.is_symlink() or path.name in RECEIPT_FILES:
            continue
        relative = path.relative_to(root).as_posix()
        rows.append(_file_identity(path, relative_path=relative))
    return rows


def _failure_receipt(root: Path, stage: str, message: str, started: int = 0) -> dict:
    return {
        "schema": SCHEMA,
        "status": "failed",
        "run_id": root.name,
        "created_at": utc_now(),
        "completed_at": utc_now(),
        "run_root": str(root.resolve()),
        "source": None,
        "build": None,
        "comparator": None,
        "oracle": None,
        "selection": None,
        "floors": None,
        "token_contract": {"individual": list(TOKENS), "union": UNION},
        "controls": None,
        "runs": None,
        "fixture_contract": None,
        "inertness": None,
        "pre_seam": None,
        "pre_seam_control": None,
        "projections": {},
        "reconciliations": {},
        "artifact_index": _artifact_index(root),
        "validator": None,
        "failed_stage": stage,
        "failure": {
            "message": message,
            "exit_code": 2,
            "started_corpus_children": started,
        },
    }


def _parse_census(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(prog="attribute.py census")
    parser.add_argument("--repository", required=True)
    parser.add_argument("--target-dir", required=True)
    parser.add_argument("--output", "--run-root", dest="output", required=True)
    parser.add_argument("--corpus", required=True)
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--min-files", type=int, required=True)
    parser.add_argument("--min-tags", type=int, required=True)
    parser.add_argument("--perl", required=True)
    parser.add_argument("--exiftool-dir", required=True)
    parser.add_argument("--tokens", required=True)
    return parser.parse_args(argv)


def _git(repository: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(repository), *args], capture_output=True, text=True, check=False
    )
    if result.returncode != 0:
        raise ReceiptError(f"git {' '.join(args)} failed: {result.stderr.strip()}")
    return result.stdout.strip()


def _source_identity(repository: Path) -> dict:
    root = Path(_git(repository, "rev-parse", "--show-toplevel")).resolve()
    dirty_files = _git(repository, "status", "--porcelain=v1").splitlines()
    return {
        "root": str(root),
        "commit": _git(repository, "rev-parse", "HEAD"),
        "tree": _git(repository, "rev-parse", "HEAD^{tree}"),
        "clean": not dirty_files,
        "dirty_files": dirty_files,
    }


def _resolve_pre_seam(repository: Path) -> dict:
    """Resolve the unique commit that introduced the attribution seam and its parent."""
    path = "src/exiftool_tables/attribution.rs"
    additions = _git(
        repository,
        "log",
        "--follow",
        "--diff-filter=A",
        "--format=%H",
        "HEAD",
        "--",
        path,
    ).splitlines()
    if len(additions) != 1:
        raise ReceiptError(
            f"expected exactly one introducing commit for {path}, found {len(additions)}"
        )
    introducing = additions[0]
    parents = _git(repository, "show", "-s", "--format=%P", introducing).split()
    if len(parents) != 1:
        raise ReceiptError("attribution introducing commit must have exactly one parent")
    parent = parents[0]
    absent = subprocess.run(
        ["git", "-C", str(repository), "cat-file", "-e", f"{parent}:{path}"],
        capture_output=True,
        check=False,
    )
    if absent.returncode == 0:
        raise ReceiptError("resolved pre-seam parent already contains attribution.rs")
    return {
        "path": path,
        "introducing_commit": introducing,
        "introducing_tree": _git(repository, "rev-parse", f"{introducing}^{{tree}}"),
        "introducing_parents": parents,
        "parent_commit": parent,
        "parent_tree": _git(repository, "rev-parse", f"{parent}^{{tree}}"),
    }


def _append_stage(root: Path, stage: str, event: str, *, exit_code: int | None = None) -> None:
    row = {"at": utc_now(), "stage": stage, "event": event, "exit_code": exit_code}
    path = root / "stages.jsonl"
    fd = os.open(path, os.O_WRONLY | os.O_APPEND | os.O_CREAT, 0o644)
    with os.fdopen(fd, "ab") as handle:
        handle.write(canonical_bytes(row) + b"\n")
        handle.flush()
        os.fsync(handle.fileno())


def _run_raw(
    argv: list[str], cwd: Path, output_dir: Path, label: str, *, timeout: int = 600,
    environment: dict[str, str] | None = None,
) -> dict:
    output_dir.mkdir(parents=True, exist_ok=True)
    stdout_path = output_dir / f"{label}.stdout"
    stderr_path = output_dir / f"{label}.stderr"
    started = utc_now()
    env = os.environ.copy()
    for key in SCRUBBED_ENV:
        env.pop(key, None)
    env.update({"LC_ALL": "C", "LANG": "C"})
    env.update(environment or {})
    result = subprocess.run(
        argv, cwd=cwd, env=env, capture_output=True, timeout=timeout, check=False
    )
    _atomic_write(stdout_path, result.stdout)
    _atomic_write(stderr_path, result.stderr)
    record = {
        "argv": argv,
        "cwd": str(cwd.resolve()),
        "environment": {"LC_ALL": "C", "LANG": "C", **(environment or {})},
        "scrubbed_environment": list(SCRUBBED_ENV),
        "started_at": started,
        "completed_at": utc_now(),
        "returncode": result.returncode,
        "stdout": _artifact_record(stdout_path),
        "stderr": _artifact_record(stderr_path),
    }
    write_json(output_dir / f"{label}.process.json", record)
    if result.returncode != 0:
        raise ChildProcessError(f"{label} returned return code {result.returncode}")
    return record


def _build_pre_seam(repository: Path, root: Path) -> dict:
    resolution = _resolve_pre_seam(repository)
    checkout = root.parent / f"{root.name}.pre-seam-source"
    target = root.parent / f"{root.name}.pre-seam-target"
    for path in (checkout, target):
        if path.exists() or path.is_symlink():
            raise ReceiptError(f"pre-seam run-owned path already exists: {path}")
    proof_root = root / "pre-seam"
    clone = _run_raw(
        ["git", "clone", "--shared", "--no-checkout", str(repository), str(checkout)],
        repository,
        proof_root,
        "clone",
    )
    checkout_record = _run_raw(
        ["git", "checkout", "--detach", resolution["parent_commit"]],
        checkout,
        proof_root,
        "checkout",
    )
    checkout_identity = _source_identity(checkout)
    if checkout_identity["commit"] != resolution["parent_commit"] \
            or checkout_identity["tree"] != resolution["parent_tree"] \
            or not checkout_identity["clean"]:
        raise ReceiptError("pre-seam checkout identity does not match resolved parent")
    target.mkdir(mode=0o755)
    build = _run_raw(
        ["cargo", "build", "--release", "--bin", "oxidex"],
        checkout,
        proof_root,
        "cargo",
        timeout=3600,
        environment={"CARGO_TARGET_DIR": str(target.resolve())},
    )
    built_binary = target / "release/oxidex"
    if not built_binary.is_file():
        raise ReceiptError(f"pre-seam build did not produce expected binary {built_binary}")
    retained_binary = proof_root / "oxidex"
    shutil.copy2(built_binary, retained_binary)
    proof = {
        "resolution": resolution,
        "source_repository": str(repository.resolve()),
        "strategy": "run-owned shared clone with detached checkout; no protected ref mutation",
        "checkout": checkout_identity,
        "target_dir": str(target.resolve()),
        "clone": clone,
        "checkout_process": checkout_record,
        "build": build,
        "built_binary": _artifact_record(built_binary),
        "binary": _artifact_record(retained_binary),
        "run_mode": "pre-seam-control",
        "environment": _mode_environment("pre-seam-control"),
    }
    proof_path = proof_root / "proof.json"
    write_json(proof_path, proof)
    proof["artifact"] = _artifact_record(proof_path)
    return proof


def _validate_pre_seam_proof(proof: dict, root: Path, *, live: bool) -> None:
    if not isinstance(proof, dict):
        raise ReceiptError("pre-seam build proof is missing")
    resolution = proof.get("resolution")
    checkout = proof.get("checkout")
    if not isinstance(resolution, dict) or not isinstance(checkout, dict):
        raise ReceiptError("pre-seam commit/tree proof is incomplete")
    hashes = (
        resolution.get("introducing_commit"),
        resolution.get("introducing_tree"),
        resolution.get("parent_commit"),
        resolution.get("parent_tree"),
    )
    if any(not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{40}", value) for value in hashes):
        raise ReceiptError("pre-seam commit/tree proof contains an invalid object ID")
    if resolution.get("introducing_parents") != [resolution["parent_commit"]] \
            or checkout.get("commit") != resolution["parent_commit"] \
            or checkout.get("tree") != resolution["parent_tree"] \
            or checkout.get("clean") is not True \
            or proof.get("run_mode") != "pre-seam-control" \
            or proof.get("environment") != _mode_environment("pre-seam-control"):
        raise ReceiptError("pre-seam parent checkout or mode proof does not reconcile")
    for label in ("clone", "checkout_process", "build"):
        process = proof.get(label)
        if not isinstance(process, dict) or process.get("returncode") != 0:
            raise ReceiptError(f"pre-seam {label} proof is not successful")
        for stream in ("stdout", "stderr"):
            artifact = process.get(stream)
            path = Path(artifact.get("path", "")) if isinstance(artifact, dict) else Path("")
            if not _inside(root, path) or _artifact_record(path) != artifact:
                raise ReceiptError(f"pre-seam {label} {stream} artifact does not verify")
    binary = proof.get("binary")
    binary_path = Path(binary.get("path", "")) if isinstance(binary, dict) else Path("")
    if not _inside(root, binary_path) or _artifact_record(binary_path) != binary:
        raise ReceiptError("retained pre-seam binary does not verify")
    built = proof.get("built_binary")
    identity_keys = ("size", "mode", "sha256")
    if not isinstance(built, dict) or any(key not in built for key in identity_keys) \
            or any(built[key] != binary[key] for key in identity_keys):
        raise ReceiptError("pre-seam built and retained binary identity differs")
    proof_artifact = proof.get("artifact")
    proof_path = Path(proof_artifact.get("path", "")) if isinstance(proof_artifact, dict) else Path("")
    if not _inside(root, proof_path) or _artifact_record(proof_path) != proof_artifact:
        raise ReceiptError("pre-seam proof artifact does not verify")
    retained = json.loads(
        proof_path.read_text(encoding="utf-8"), object_pairs_hook=_reject_duplicate_pairs
    )
    if retained != {key: value for key, value in proof.items() if key != "artifact"}:
        raise ReceiptError("retained pre-seam proof differs from receipt")
    if live:
        current_repository = Path(proof.get("source_repository", ""))
        if _resolve_pre_seam(current_repository) != resolution:
            raise ReceiptError("live pre-seam resolution changed")
        if _source_identity(Path(checkout["root"])) != checkout:
            raise ReceiptError("live pre-seam checkout identity changed")
        built_path = Path(built.get("path", "")) if isinstance(built, dict) else Path("")
        if _artifact_record(built_path) != built or _artifact_record(binary_path) != binary:
            raise ReceiptError("live pre-seam binary identity changed")


def _source_manifest_rows(root: Path) -> list[dict]:
    files = [root / "exiftool"]
    files.extend(sorted((root / "lib").rglob("*")))
    rows = []
    for path in files:
        if path.is_symlink() or not path.is_file():
            continue
        rows.append(
            {
                "relative_path": path.relative_to(root).as_posix(),
                **_file_identity(path),
            }
        )
    return rows


def _source_manifest(root: Path, output: Path) -> dict:
    rows = _source_manifest_rows(root)
    manifest = {"root": str(root.resolve()), "files": rows, "sha256": canonical_sha256(rows)}
    write_json(output, manifest)
    return {**manifest, "artifact": _artifact_record(output)}


def _recheck_live_inputs(receipt: dict) -> None:
    if receipt.get("status") == "failed":
        raise ReceiptError("failed receipts have no complete live-input contract")
    source = receipt.get("source")
    if not isinstance(source, dict) or _source_identity(Path(source["root"])) != source:
        raise ReceiptError("live source identity does not match receipt")
    binary = receipt.get("build", {}).get("binary")
    binary_path = Path(binary.get("path", "")) if isinstance(binary, dict) else Path("")
    expected_binary = {
        key: binary[key] for key in ("size", "mode", "sha256")
    } if isinstance(binary, dict) and all(key in binary for key in ("size", "mode", "sha256")) else None
    if expected_binary is None or _file_identity(binary_path) != expected_binary:
        raise ReceiptError("live candidate binary identity does not match receipt")
    comparator = receipt.get("comparator")
    comparator_path = Path(comparator.get("path", "")) if isinstance(comparator, dict) else Path("")
    if not comparator_path.is_file() or sha256_file(comparator_path) != comparator.get("sha256"):
        raise ReceiptError("live comparator identity does not match receipt")
    selection = receipt.get("selection")
    if not isinstance(selection, dict):
        raise ReceiptError("receipt selection is missing")
    _verify_selection(selection)
    oracle = receipt.get("oracle")
    if not isinstance(oracle, dict):
        raise ReceiptError("receipt oracle identity is missing")
    perl = Path(oracle.get("perl", {}).get("path", ""))
    expected_perl = {key: oracle["perl"][key] for key in ("size", "mode", "sha256")}
    if _file_identity(perl) != expected_perl:
        raise ReceiptError("live Perl identity does not match receipt")
    oracle_root = Path(oracle["source_root"])
    if canonical_sha256(_source_manifest_rows(oracle_root)) != oracle["source_manifest"]["sha256"]:
        raise ReceiptError("live ExifTool source manifest does not match receipt")
    _validate_pre_seam_proof(receipt.get("pre_seam"), Path(receipt["run_root"]), live=True)


def _sum_projection(per_file: dict[str, dict]) -> dict:
    aggregate = {key: 0 for key in COUNTERS}
    for projection in per_file.values():
        for key in COUNTERS:
            aggregate[key] += projection[key]
    validate_equations(aggregate)
    return aggregate


def _load_parsed(record: dict) -> dict:
    path = Path(record["parsed"]["path"])
    if _artifact_record(path) != record["parsed"]:
        raise ReceiptError(f"parsed output identity changed: {path}")
    decoded = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=_reject_duplicate_pairs)
    if not isinstance(decoded, dict):
        raise ReceiptError(f"parsed output is not an object: {path}")
    return decoded


def _mode_environment(mode: str) -> dict:
    if mode in ("control-unset", "pre-seam-control"):
        declaration = {"state": "absent"}
    elif mode == "control-empty":
        declaration = {"state": "empty", "value": ""}
    elif mode == "union":
        declaration = {"state": "value", "value": UNION}
    else:
        declaration = {"state": "value", "value": mode}
    return {"OXIDEX_GENSHARE_SILENCE": declaration}


def _run_mode(
    root: Path,
    mode: str,
    selection: dict,
    repository: Path,
    binary: Path,
    perl: Path,
    exiftool_root: Path,
) -> tuple[dict, dict]:
    mode_root = root / "runs" / mode
    children = []
    per_file = {}
    oracle_hashes = {}
    started = utc_now()
    for number, entry in enumerate(selection["ordered_manifest"], 1):
        relative = entry["relative_path"]
        child = mode_root / "children" / f"{number:06d}"
        staged = Path(entry["staged_path"])
        if sha256_file(staged) != entry["sha256"]:
            raise ReceiptError(f"staged input drift before {mode}: {relative}")
        oracle_argv = [
            str(perl),
            f"-I{exiftool_root / 'lib'}",
            str(exiftool_root / "exiftool"),
            "-config",
            "",
            "-G0:1:4",
            "-s",
            "-j",
            "-a",
            str(staged),
        ]
        oracle_record = capture_process(
            oracle_argv, repository, child, "oracle", {}, timeout=120
        )
        candidate_record = capture_process(
            [str(binary), "-j", "-G1", "-a", str(staged)],
            repository,
            child,
            "candidate",
            _mode_environment(mode),
            timeout=120,
        )
        oracle = _load_parsed(oracle_record)
        candidate = _load_parsed(candidate_record)
        oracle_hashes[relative] = canonical_sha256(oracle)
        per_file[relative] = project_file(oracle, candidate)
        raw_occurrences = occurrence_sequence(candidate, normalize_access_date=False)
        normalized_occurrences = occurrence_sequence(candidate, normalize_access_date=True)
        process = {
            "relative_path": relative,
            "source_sha256": entry["sha256"],
            "staged_sha256": sha256_file(staged),
            "oracle": oracle_record,
            "candidate": candidate_record,
            "candidate_occurrences_raw": raw_occurrences,
            "candidate_occurrences": normalized_occurrences,
            "raw_occurrences_sha256": canonical_sha256(raw_occurrences),
            "normalized_occurrences_sha256": canonical_sha256(normalized_occurrences),
            "normalization_count": sum(
                raw["value"] != normalized["value"]
                for raw, normalized in zip(raw_occurrences, normalized_occurrences)
            ),
        }
        process_path = child / "process.json"
        write_json(process_path, process)
        children.append({"relative_path": relative, "process": _artifact_record(process_path)})
    driver_stdout = mode_root / "driver.stdout"
    driver_stderr = mode_root / "driver.stderr"
    _atomic_write(driver_stdout, b"")
    _atomic_write(driver_stderr, b"")
    mode_record = {
        "mode": mode,
        "tokens": list(TOKENS) if mode == "union" else (
            [] if mode in ("control-unset", "control-empty", "pre-seam-control") else [mode]
        ),
        "started_at": started,
        "completed_at": utc_now(),
        "driver_returncode": 0,
        "driver_stdout": _artifact_record(driver_stdout),
        "driver_stderr": _artifact_record(driver_stderr),
        "path_set_sha256": path_set_sha256(selection["ordered_paths"]),
        "oracle_path_set_sha256": path_set_sha256(list(oracle_hashes)),
        "candidate_path_set_sha256": path_set_sha256([row["relative_path"] for row in children]),
        "scored_path_set_sha256": path_set_sha256(list(per_file)),
        "oracle_parsed_sha256": oracle_hashes,
        "children": children,
    }
    mode_path = mode_root / "mode.json"
    write_json(mode_path, mode_record)
    projection = {"per_file": per_file, "aggregate": _sum_projection(per_file)}
    projection_path = mode_root / "projection.json"
    write_json(projection_path, projection)
    mode_record["mode_artifact"] = _artifact_record(mode_path)
    mode_record["projection_artifact"] = _artifact_record(projection_path)
    return mode_record, projection


def _verify_selection(selection: dict) -> None:
    for row in selection["ordered_manifest"]:
        source = Path(row["source_path"])
        staged = Path(row["staged_path"])
        if sha256_file(source) != row["sha256"] or sha256_file(staged) != row["sha256"]:
            raise ReceiptError(f"source or staged corpus drift: {row['relative_path']}")


def _load_expectations(manifest: Path, selection: dict, root: Path) -> dict:
    path = manifest.with_name("bounded-corpus-expectations.json")
    try:
        document = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=_reject_duplicate_pairs)
    except (OSError, ValueError) as exc:
        raise ReceiptError(f"cannot load bounded expectations {path}: {exc}") from exc
    fixtures = document.get("fixtures") if isinstance(document, dict) else None
    if not isinstance(fixtures, list):
        raise ReceiptError("bounded expectations fixtures are required")
    expected = [(row.get("relative_path"), row.get("sha256")) for row in fixtures]
    actual = [(row["relative_path"], row["sha256"]) for row in selection["ordered_manifest"]]
    if expected != actual:
        raise ReceiptError("bounded fixture paths or content hashes do not match expectations")
    retained = root / "contracts" / "bounded-corpus-expectations.json"
    _atomic_write(retained, path.read_bytes())
    return {
        "source_path": str(path.resolve()),
        "retained": _artifact_record(retained),
        "sha256": sha256_file(path),
        "document": document,
    }


def _route_ledger(repository: Path, root: Path) -> dict:
    names = list(TASK8_RUST_BOUNDARY)
    missing_sources = [name for name in names if not (repository / name).is_file()]
    if missing_sources:
        raise ReceiptError(
            "Task8 Rust boundary is incomplete: " + ",".join(missing_sources)
        )
    rows = [{"path": name, "sha256": sha256_file(repository / name)} for name in names]
    variant_to_token = {
        "Engine": "engine",
        "LegacyL1": "legacy-l1",
        "LegacyL2": "legacy-l2",
        "Producers": "producers",
        "Serial": "serial",
        "Keyed": "keyed",
    }
    guard_sites = {token: [] for token in TOKENS}
    pattern = re.compile(r"attribution::Token::([A-Za-z0-9_]+)")
    for name in names:
        for line_number, line in enumerate(
            (repository / name).read_text(encoding="utf-8").splitlines(), 1
        ):
            match = pattern.search(line)
            if match and match.group(1) in variant_to_token:
                guard_sites[variant_to_token[match.group(1)]].append(
                    {"path": name, "line": line_number, "source": line.strip()}
                )
    missing_guards = [token for token, sites in guard_sites.items() if not sites]
    if missing_guards:
        raise ReceiptError(f"source route ledger has no maintained guard for: {','.join(missing_guards)}")

    def external_calls(symbol: str, engine_file: str) -> list[dict]:
        calls = []
        for relative in names:
            if relative == engine_file:
                continue
            path = repository / relative
            in_tests = False
            for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
                if line.strip().startswith("#[cfg(test)]"):
                    in_tests = True
                if not in_tests and f"{symbol}(" in line:
                    calls.append({"path": relative, "line": line_number, "source": line.strip()})
        return calls

    serial_calls = external_calls("process_serial_directory", "src/exiftool_tables/serial_engine.rs")
    keyed_calls = external_calls("process_keyed_directory", "src/exiftool_tables/keyed_engine.rs")
    ledger = {
        "tokens": list(TOKENS),
        "sources": rows,
        "guard_sites": guard_sites,
        "production_calls": {"serial": serial_calls, "keyed": keyed_calls},
        "production_reachable": {
            **{token: True for token in TOKENS if token not in ("serial", "keyed")},
            "serial": bool(serial_calls),
            "keyed": bool(keyed_calls),
        },
        "keyed_note": "no public production caller; zero remains unexercised",
    }
    if keyed_calls:
        raise ReceiptError("keyed route ledger changed: production caller requires controller review")
    path = root / "contracts" / "route-ledger.json"
    write_json(path, ledger)
    return {"artifact": _artifact_record(path), "sha256": canonical_sha256(ledger), **ledger}


def _fixture_observations(
    runs: dict, projections: dict, reconciliations: dict, expectations: dict
) -> dict:
    control = runs["control-empty"]
    observations = {}
    for token in [*TOKENS, "union"]:
        observations[token] = {
            "status": exercise_status(
                projections["control-empty"]["aggregate"],
                projections[token]["aggregate"],
                production_reachable=token != "keyed",
            ),
            "matched_lost": reconciliations[token]["matched_lost"],
        }
    engine_icc = reconcile(
        projections["control-empty"]["per_file"]["ICC_Profile.icc"],
        projections["engine"]["per_file"]["ICC_Profile.icc"],
    )
    if engine_icc["matched_lost"] <= 0:
        raise ReceiptError("ICC engine fixture did not produce a positive observed loss")
    icc_delta = sequence_delta(
        _candidate_sequence(control, "ICC_Profile.icc"),
        _candidate_sequence(runs["engine"], "ICC_Profile.icc"),
    )
    if icc_delta["added"] or icc_delta["changed_or_reordered"]:
        raise ReceiptError("ICC engine observation added, changed, or reordered surviving output")
    for mode in [*TOKENS, "union"]:
        baseline = _candidate_sequence(control, "AAC.aac")
        if _candidate_sequence(runs[mode], "AAC.aac") != baseline:
            raise ReceiptError(f"AAC hand-only control changed under {mode}")
    keyed_deltas = {
        relative: sequence_delta(
            _candidate_sequence(control, relative),
            _candidate_sequence(runs["keyed"], relative),
        )
        for relative in ("ICC_Profile.icc", "AAC.aac", "OOXML.docx")
    }
    if observations["keyed"]["matched_lost"] != 0 or any(
        delta["removed"] or delta["added"] or delta["changed_or_reordered"]
        for delta in keyed_deltas.values()
    ):
        raise ReceiptError("keyed changed output despite having no production caller")
    return {
        "review_status": "observed_unreviewed",
        "expectations_sha256": expectations["sha256"],
        "token_observations": observations,
        "icc_engine": engine_icc,
        "icc_engine_sequence_delta": icc_delta,
        "keyed_sequence_deltas": keyed_deltas,
        "exact_loss_expectations": None,
    }


def _candidate_sequence(mode: dict, relative_path: str) -> list[dict]:
    child_row = next(row for row in mode["children"] if row["relative_path"] == relative_path)
    process = json.loads(Path(child_row["process"]["path"]).read_text(encoding="utf-8"))
    return process["candidate_occurrences"]


def sequence_delta(control: list[dict], probe: list[dict]) -> dict:
    """Describe removals/additions and prove common occurrences retain order."""
    def signature(row):
        return canonical_sha256({
            key: row[key]
            for key in ("raw_key", "group", "name", "duplicate_instance", "type", "value", "raw_serialized")
        })

    control_signatures = [signature(row) for row in control]
    probe_signatures = [signature(row) for row in probe]
    control_counts = collections.Counter(control_signatures)
    probe_counts = collections.Counter(probe_signatures)
    removed_counts = control_counts - probe_counts
    added_counts = probe_counts - control_counts
    removed = []
    for row, key in zip(control, control_signatures):
        if removed_counts[key]:
            removed.append(row)
            removed_counts[key] -= 1
    added = []
    for row, key in zip(probe, probe_signatures):
        if added_counts[key]:
            added.append(row)
            added_counts[key] -= 1
    common_counts = control_counts & probe_counts
    remaining_control = common_counts.copy()
    common_control = []
    for key in control_signatures:
        if remaining_control[key]:
            common_control.append(key)
            remaining_control[key] -= 1
    remaining_probe = common_counts.copy()
    common_probe = []
    for key in probe_signatures:
        if remaining_probe[key]:
            common_probe.append(key)
            remaining_probe[key] -= 1
    return {
        "removed": removed,
        "added": added,
        "changed_or_reordered": common_control != common_probe,
    }


def _validate_inertness(runs: dict, selection: dict) -> dict:
    differences = []
    for relative in selection["ordered_paths"]:
        if _candidate_sequence(runs["control-unset"], relative) != _candidate_sequence(
            runs["control-empty"], relative
        ):
            differences.append(relative)
        unset_child = next(
            row for row in runs["control-unset"]["children"] if row["relative_path"] == relative
        )
        empty_child = next(
            row for row in runs["control-empty"]["children"] if row["relative_path"] == relative
        )
        unset_process = json.loads(Path(unset_child["process"]["path"]).read_text())
        empty_process = json.loads(Path(empty_child["process"]["path"]).read_text())
        if unset_process["candidate"]["stderr"]["sha256"] != empty_process["candidate"]["stderr"]["sha256"]:
            differences.append(relative)
    if differences:
        raise ReceiptError(f"unset and empty controls differ: {differences}")
    return {
        "equal": True,
        "normalization": "replace value of exact key System:FileAccessDate only",
        "differences": [],
        "path_set_sha256": path_set_sha256(selection["ordered_paths"]),
    }


def _validate_pre_seam_control(runs: dict, selection: dict) -> dict:
    differences = []
    for relative in selection["ordered_paths"]:
        maintained = _candidate_sequence(runs["control-unset"], relative)
        pre_seam = _candidate_sequence(runs["pre-seam-control"], relative)
        maintained_child = next(
            row for row in runs["control-unset"]["children"]
            if row["relative_path"] == relative
        )
        pre_seam_child = next(
            row for row in runs["pre-seam-control"]["children"]
            if row["relative_path"] == relative
        )
        maintained_process = json.loads(
            Path(maintained_child["process"]["path"]).read_text(encoding="utf-8"),
            object_pairs_hook=_reject_duplicate_pairs,
        )
        pre_seam_process = json.loads(
            Path(pre_seam_child["process"]["path"]).read_text(encoding="utf-8"),
            object_pairs_hook=_reject_duplicate_pairs,
        )
        same_stderr = (
            maintained_process["candidate"]["stderr"]["sha256"]
            == pre_seam_process["candidate"]["stderr"]["sha256"]
        )
        if maintained != pre_seam or not same_stderr:
            differences.append(relative)
    if differences:
        raise ReceiptError(f"pre-seam ordinary control differs from maintained unset: {differences}")
    return {
        "equal": True,
        "maintained_mode": "control-unset",
        "ordinary_binary_mode": "pre-seam-control",
        "environment_state": "absent",
        "comparison": "normalized ordered candidate occurrences and raw stderr SHA-256",
        "differences": [],
        "path_set_sha256": path_set_sha256(selection["ordered_paths"]),
    }


def census_main(argv: list[str]) -> int:
    args = _parse_census(argv)
    root = Path(args.output).expanduser()
    if root.exists() or root.is_symlink():
        print(f"output path already exists: {root}", file=sys.stderr)
        return 2
    root.parent.mkdir(parents=True, exist_ok=True)
    os.mkdir(root, 0o755)
    root = root.resolve()
    stage = "token-contract"
    started_children = 0
    try:
        _append_stage(root, stage, "start")
        token_contract = validate_token_request(args.tokens)
        _append_stage(root, stage, "end", exit_code=0)
        stage = "selection"
        _append_stage(root, stage, "start")
        selection = _stage_selection(Path(args.corpus), Path(args.manifest), root, args.min_files)
        expectations = _load_expectations(Path(args.manifest), selection, root)
        _append_stage(root, stage, "end", exit_code=0)
        stage = "preflight"
        _append_stage(root, stage, "start")
        if args.min_tags <= 0:
            raise ReceiptError("min-tags must be a positive integer")
        repository = Path(args.repository).resolve(strict=True)
        exiftool_root = Path(args.exiftool_dir).resolve(strict=True)
        perl = Path(args.perl).resolve(strict=True)
        for path, kind in ((repository, "repository"), (exiftool_root, "ExifTool root")):
            if not path.is_dir():
                raise ReceiptError(f"{kind} is not a directory: {path}")
        instrument_repository = Path(__file__).resolve().parents[3]
        if instrument_repository != repository:
            raise ReceiptError(
                f"census instrument checkout {instrument_repository} does not match --repository {repository}"
            )
        if not perl.is_file() or not os.access(perl, os.X_OK):
            raise ReceiptError(f"Perl is not executable: {args.perl}")
        source = _source_identity(repository)
        if not source["clean"]:
            raise ReceiptError(f"repository must be clean: {source['dirty_files']}")
        comparator_path = repository / "tools/exiftool-tables/conformance.py"
        comparator = {
            "path": str(comparator_path.resolve()),
            "sha256": sha256_file(comparator_path),
            "source_commit": source["commit"],
            "projection": "genshare-v3 projection using conformance.py pure comparison functions",
        }
        route_ledger = _route_ledger(repository, root)
        _append_stage(root, stage, "end", exit_code=0)

        stage = "oracle"
        _append_stage(root, stage, "start")
        oracle_dir = root / "oracle"
        version = _run_raw(
            [str(perl), f"-I{exiftool_root / 'lib'}", str(exiftool_root / "exiftool"), "-config", "", "-ver"],
            repository,
            oracle_dir,
            "version",
        )
        if Path(version["stdout"]["path"]).read_text().strip() != "13.59":
            raise ReceiptError("oracle version is not pinned ExifTool 13.59")
        capability_path = exiftool_root / "t/images/OOXML.docx"
        capability = _run_raw(
            [str(perl), f"-I{exiftool_root / 'lib'}", str(exiftool_root / "exiftool"), "-config", "", "-s3", "-FileType", str(capability_path)],
            repository,
            oracle_dir,
            "docx-capability",
        )
        if Path(capability["stdout"]["path"]).read_text().strip() != "DOCX":
            raise ReceiptError("oracle DOCX capability probe failed")
        oracle_manifest = _source_manifest(exiftool_root, oracle_dir / "source-manifest.json")
        oracle = {
            "version": "13.59",
            "perl": {"path": str(perl), **_file_identity(perl)},
            "source_root": str(exiftool_root),
            "source_manifest": oracle_manifest,
            "base_argv": [str(perl), f"-I{exiftool_root / 'lib'}", str(exiftool_root / "exiftool"), "-config", ""],
            "environment": {"LC_ALL": "C", "LANG": "C"},
            "version_probe": version,
            "docx_capability": capability,
        }
        _append_stage(root, stage, "end", exit_code=0)

        stage = "build"
        _append_stage(root, stage, "start")
        target = Path(args.target_dir).resolve()
        target.mkdir(parents=True, exist_ok=True)
        build = _run_raw(
            ["cargo", "build", "--release", "--bin", "oxidex"],
            repository,
            root / "build",
            "cargo",
            timeout=3600,
            environment={"CARGO_TARGET_DIR": str(target)},
        )
        binary = target / "release/oxidex"
        # CARGO_TARGET_DIR is explicit evidence, not inherited ambient state.
        if not binary.is_file():
            default_binary = repository / "target/release/oxidex"
            if default_binary.is_file() and target == (repository / "target").resolve():
                binary = default_binary
            else:
                raise ReceiptError(f"build did not produce expected binary {binary}")
        binary_identity = {"path": str(binary.resolve()), **_file_identity(binary), "mtime_ns": binary.stat().st_mtime_ns}
        build.update({"target_dir": str(target), "binary": binary_identity})
        _append_stage(root, stage, "end", exit_code=0)

        stage = "pre-seam-build"
        _append_stage(root, stage, "start")
        pre_seam = _build_pre_seam(repository, root)
        pre_seam_binary = Path(pre_seam["binary"]["path"])
        pre_seam_repository = Path(pre_seam["checkout"]["root"])
        _validate_pre_seam_proof(pre_seam, root, live=True)
        _append_stage(root, stage, "end", exit_code=0)

        stage = "measurement"
        _append_stage(root, stage, "start")
        modes = ["pre-seam-control", "control-unset", "control-empty", *TOKENS, "union"]
        runs = {}
        projections = {}
        for mode in modes:
            mode_repository = pre_seam_repository if mode == "pre-seam-control" else repository
            mode_binary = pre_seam_binary if mode == "pre-seam-control" else binary
            run, projection = _run_mode(
                root, mode, selection, mode_repository, mode_binary, perl, exiftool_root
            )
            runs[mode] = run
            projections[mode] = projection
        exact_path_hash = selection["path_set_sha256"]
        for mode, run in runs.items():
            hashes = {
                run["path_set_sha256"], run["oracle_path_set_sha256"],
                run["candidate_path_set_sha256"], run["scored_path_set_sha256"],
            }
            if hashes != {exact_path_hash}:
                raise ReceiptError(f"selected/oracle/candidate/scored path sets differ in {mode}")
        baseline_oracles = runs["control-empty"]["oracle_parsed_sha256"]
        if any(run["oracle_parsed_sha256"] != baseline_oracles for run in runs.values()):
            raise ReceiptError("oracle parsed output changed between modes")
        aggregate = projections["control-empty"]["aggregate"]
        if aggregate["oracle_occurrences"] < args.min_tags:
            raise ReceiptError(
                f"oracle occurrence floor failed: {aggregate['oracle_occurrences']} < {args.min_tags}"
            )
        reconciliations = {
            mode: reconcile(aggregate, projections[mode]["aggregate"])
            for mode in [*TOKENS, "union"]
        }
        for mode, result in reconciliations.items():
            if result["oracle_residual"] or result["candidate_residual"]:
                raise ReceiptError(f"Task 6 reconciliation failed for {mode}")
        inertness = _validate_inertness(runs, selection)
        pre_seam_control = _validate_pre_seam_control(runs, selection)
        fixture_contract = _fixture_observations(
            runs, projections, reconciliations, expectations
        )
        _append_stage(root, stage, "end", exit_code=0)

        stage = "final-input-recheck"
        _append_stage(root, stage, "start")
        _verify_selection(selection)
        if _source_identity(repository) != source:
            raise ReceiptError("source repository changed during census")
        if _file_identity(binary) != {key: binary_identity[key] for key in ("size", "mode", "sha256")}:
            raise ReceiptError("candidate binary changed during census")
        _validate_pre_seam_proof(pre_seam, root, live=True)
        if _source_manifest(exiftool_root, oracle_dir / "source-manifest.recheck.json")["sha256"] != oracle_manifest["sha256"]:
            raise ReceiptError("oracle source changed during census")
        _append_stage(root, stage, "end", exit_code=0)

        stage = "finalize"
        _append_stage(root, stage, "start")
        _append_stage(root, stage, "end", exit_code=0)
        receipt = {
            "schema": SCHEMA,
            "status": "observed_unreviewed",
            "run_id": root.name,
            "created_at": version["started_at"],
            "completed_at": utc_now(),
            "run_root": str(root),
            "source": source,
            "build": build,
            "pre_seam": pre_seam,
            "pre_seam_control": pre_seam_control,
            "comparator": comparator,
            "oracle": oracle,
            "selection": selection,
            "floors": {"min_files": args.min_files, "min_tags": args.min_tags},
            "token_contract": token_contract,
            "controls": {
                "ordinary_pre_seam": "pre-seam-control",
                "unset": "control-unset",
                "empty": "control-empty",
            },
            "runs": runs,
            "fixture_contract": fixture_contract,
            "route_ledger": route_ledger,
            "inertness": inertness,
            "projections": projections,
            "reconciliations": reconciliations,
            "artifact_index": _artifact_index(root),
            "validator": {"implementation": str(Path(__file__).resolve()), "replayed": True},
            "failed_stage": None,
            "failure": None,
        }
        pending = root / "receipt.pending.json"
        write_json(pending, receipt)
        validate_v3_receipt(receipt, root, replay=True)
        os.replace(pending, root / "receipt.json")
        receipt_hash = sha256_file(root / "receipt.json")
        _atomic_write(root / "receipt.sha256", f"{receipt_hash}  receipt.json\n".encode())
        write_json(
            root / "validator.json",
            {"returncode": 0, "receipt_sha256": receipt_hash, "completed_at": utc_now()},
        )
        _atomic_write(root / ".complete", b"observed_unreviewed\n")
        print(root / "receipt.json")
        return 0
    except (ReceiptError, OSError, subprocess.SubprocessError) as exc:
        try:
            _append_stage(root, stage, "end", exit_code=2)
        except OSError:
            pass
        started_children = len(list((root / "runs").rglob("*.returncode"))) \
            if (root / "runs").exists() else started_children
        receipt = _failure_receipt(root, stage, str(exc), started_children)
        receipt["token_contract"] = token_contract if "token_contract" in locals() else {
            "individual": list(TOKENS), "union": UNION
        }
        receipt["selection"] = selection if "selection" in locals() else None
        failed_path = root / "receipt.failed.json"
        write_json(failed_path, receipt)
        _atomic_write(
            root / "receipt.failed.sha256",
            f"{sha256_file(failed_path)}  {failed_path.name}\n".encode(),
        )
        print(str(exc), file=sys.stderr)
        return 2


def validate_main(argv: list[str], *, failure_only: bool = False) -> int:
    parser = argparse.ArgumentParser(prog="attribute.py validate")
    parser.add_argument("--receipt", required=True)
    parser.add_argument("--require-success", action="store_true")
    parser.add_argument("--recheck-live-inputs", action="store_true")
    args = parser.parse_args(argv)
    path = Path(args.receipt).resolve(strict=True)
    sidecar = path.with_suffix(".sha256")
    if not sidecar.is_file():
        raise ReceiptError(f"receipt hash sidecar is missing: {sidecar}")
    fields = sidecar.read_text(encoding="utf-8").strip().split()
    if len(fields) != 2 or fields[0] != sha256_file(path) or fields[1] != path.name:
        raise ReceiptError("receipt hash sidecar does not verify")
    receipt = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=_reject_duplicate_pairs)
    validate_v3_receipt(receipt, path.parent, replay=not failure_only)
    if args.recheck_live_inputs:
        _recheck_live_inputs(receipt)
    if failure_only and receipt.get("status") != "failed":
        raise ReceiptError("validate-failure requires a failed receipt")
    if args.require_success:
        require_success_status(receipt.get("status"))
    print(f"validated {path}")
    return 0


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    if not args:
        print("usage: attribute.py {census|validate|validate-failure} ...", file=sys.stderr)
        return 2
    command = args.pop(0)
    try:
        if command == "census":
            return census_main(args)
        if command == "validate":
            return validate_main(args)
        if command == "validate-failure":
            return validate_main(args, failure_only=True)
        raise ReceiptError(f"unknown command: {command}")
    except (ReceiptError, OSError, ValueError) as exc:
        print(str(exc), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
