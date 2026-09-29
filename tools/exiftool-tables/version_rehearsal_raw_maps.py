"""Capture the exact maps behind a signed read conformance transcript.

This is evidence, not a second scorer. The existing transcript remains the
authority; a capture is refused unless every recaptured row matches it.
"""
from __future__ import annotations

import base64
import hashlib
import json
from pathlib import Path
import subprocess
from typing import Any, Mapping

import conformance


class Refused(RuntimeError):
    """A raw-map capture cannot be tied to the existing read measurement."""


def _sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _git(checkout: Path, *args: str) -> str:
    result = subprocess.run(["git", "-C", str(checkout), *args], capture_output=True,
                            text=True, check=False)
    if result.returncode:
        raise Refused(f"signed snapshot Git check failed: {' '.join(args)}")
    return result.stdout.strip()


def _json_map(stdout: bytes, *, oracle: bool) -> tuple[dict[str, Any], list[list[Any]]]:
    def pairs_hook(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in pairs:
            # The canonical conformance JSON parser keeps the last duplicate.
            # It could authenticate that collapsed map but not the lost row.
            if key in result:
                raise Refused(f"duplicate JSON key is not transcript-authenticated: {key}")
            result[key] = value
        return result

    try:
        parsed = json.loads(stdout.decode("utf-8", errors="replace"),
                            parse_float=str, object_pairs_hook=pairs_hook)
    except json.JSONDecodeError as error:
        raise Refused("raw-map command returned invalid JSON") from error
    if oracle:
        if not isinstance(parsed, list) or len(parsed) != 1 or not isinstance(parsed[0], dict):
            raise Refused("native raw-map command did not return one JSON object")
        result = parsed[0]
    else:
        if isinstance(parsed, list):
            parsed = parsed[0] if len(parsed) == 1 else None
        if not isinstance(parsed, dict):
            raise Refused("candidate raw-map command did not return one JSON object")
        result = parsed
    return result, [[key, value] for key, value in result.items()]


def _run(argv: list[str], *, native: bool) -> tuple[subprocess.CompletedProcess[bytes], dict[str, Any], list[list[Any]]]:
    env = conformance._scrubbed_perl_env() if native else None
    try:
        completed = subprocess.run(argv, capture_output=True, check=False, env=env,
                                   timeout=120)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise Refused("raw-map command failed to complete") from error
    mapping, pairs = _json_map(completed.stdout, oracle=native)
    return completed, mapping, pairs


def _command_receipt(completed: subprocess.CompletedProcess[bytes]) -> dict[str, Any]:
    return {"argv": completed.args, "status": completed.returncode,
            "stdout_sha256": hashlib.sha256(completed.stdout).hexdigest(),
            "stderr_sha256": hashlib.sha256(completed.stderr).hexdigest(),
            "stdout_bytes": len(completed.stdout), "stderr_bytes": len(completed.stderr),
            "stdout_base64": base64.b64encode(completed.stdout).decode("ascii"),
            "stderr_base64": base64.b64encode(completed.stderr).decode("ascii")}


def capture_authenticated_maps(conformance_report: Path,
                               fixture_entries: list[dict[str, Any]],
                               snapshot_checkout: Path, binary: Path, perl: Path,
                               native_source: Path) -> dict[str, Any]:
    """Recapture ordered raw maps and reject drift from the saved transcript.

    The caller must first validate the signed snapshot and materialized native
    source bundle. This helper binds the actual commands and bytes to that
    already-authenticated measurement without changing its score.
    """
    report_path = conformance_report.resolve()
    checkout, binary, perl, native_source = (path.resolve() for path in
                                             (snapshot_checkout, binary, perl, native_source))
    try:
        report = json.loads(report_path.read_text(encoding="utf-8"), parse_float=str)
    except (OSError, json.JSONDecodeError) as error:
        raise Refused("conformance report cannot be read") from error
    instrument = report.get("instrument")
    if not isinstance(instrument, dict):
        raise Refused("conformance report lacks instrument identity")
    repo, measured_binary = instrument.get("repo"), instrument.get("binary")
    if (not isinstance(repo, dict) or not isinstance(measured_binary, dict)
            or repo.get("root") != str(checkout) or repo.get("dirty") is not False
            or repo.get("dirty_files") != [] or repo.get("dirty_overridden") is not False
            or repo.get("commit") != _git(checkout, "rev-parse", "HEAD")
            or repo.get("tree") != _git(checkout, "rev-parse", "HEAD^{tree}")
            or _git(checkout, "status", "--porcelain") != ""
            or measured_binary.get("path") != str(binary)
            or measured_binary.get("sha256") != _sha(binary)):
        raise Refused("raw-map capture differs from signed source or built binary")
    transcript = instrument.get("measurement_transcript")
    rows = transcript.get("rows") if isinstance(transcript, dict) else None
    if (not isinstance(rows, list) or transcript.get("schema") != 1
            or transcript.get("row_count") != len(rows)
            or transcript.get("sha256") != conformance._canonical_digest(rows)
            or len(rows) != len(fixture_entries) or not rows):
        raise Refused("conformance transcript is absent or has wrong fixture count")
    oracle = instrument.get("oracle")
    # Oracle's full module-tree identity is independently checked by the
    # materialization verifier. Here ensure the command we run uses that tree.
    if not isinstance(oracle, dict) or not isinstance(oracle.get("argv"), list):
        raise Refused("conformance report lacks the pinned oracle command")
    native_program = native_source / "exiftool"
    native_pm = native_source / "lib" / "Image" / "ExifTool.pm"
    if str(perl) not in oracle["argv"] or str(native_program) not in oracle["argv"]:
        raise Refused("raw-map oracle command differs from pinned Perl/source")
    if not perl.is_file() or not native_program.is_file() or not native_pm.is_file():
        raise Refused("pinned native executable is absent")
    initial_files = {path: _sha(path) for path in (report_path, binary, perl,
                                                   native_program, native_pm)}
    expected_paths = sorted(str(Path(item["corpus_path"]).resolve()) for item in fixture_entries)
    if [row.get("path") if isinstance(row, dict) else None for row in rows] != expected_paths:
        raise Refused("raw-map fixtures differ from transcript selection")
    entries_by_path = {str(Path(item["corpus_path"]).resolve()): item for item in fixture_entries}
    if len(entries_by_path) != len(fixture_entries):
        raise Refused("raw-map fixture manifest repeats a corpus path")
    captured = []
    for path, transcript_row in zip(expected_paths, rows, strict=True):
        item = entries_by_path[path]
        source = Path(item["source"]).resolve()
        staged = Path(path)
        if (not source.is_file() or not staged.is_file()
                or item.get("sha256") != _sha(source)
                or item.get("corpus_sha256") != _sha(staged)
                or item.get("sha256") != item.get("corpus_sha256")
                or item.get("bytes") != source.stat().st_size
                or item.get("corpus_bytes") != staged.stat().st_size):
            raise Refused("raw-map fixture changed since read measurement")
        native_argv = conformance._oracle_command(oracle, ["-G0:1:4", "-s", "-j", "-a", path])
        candidate_argv = [str(binary), "-j", path]
        native_run, native_map, native_pairs = _run(native_argv, native=True)
        if native_map:
            candidate_run, candidate_map, candidate_pairs = _run(candidate_argv, native=False)
            result = conformance.compare(native_map, candidate_map)
        else:
            candidate_run, candidate_map, candidate_pairs, result = None, {}, [], None
        replayed = conformance.transcript_row(path, native_map, candidate_map, result)
        if replayed != transcript_row:
            raise Refused(f"raw-map replay differs from authenticated transcript: {path}")
        if _sha(source) != item["sha256"] or _sha(staged) != item["sha256"]:
            raise Refused("raw-map fixture changed during command replay")
        captured.append({"fixture": {"source": str(source), "corpus_path": path,
                                      "sha256": item["sha256"], "bytes": item["bytes"]},
                         "oracle_raw_map": native_map, "candidate_raw_map": candidate_map,
                         "oracle_ordered_pairs": native_pairs,
                         "candidate_ordered_pairs": candidate_pairs,
                         "native_status": native_run.returncode,
                         "native_command": _command_receipt(native_run),
                         "candidate_command": (_command_receipt(candidate_run)
                                               if candidate_run is not None else None),
                         "transcript_row": transcript_row,
                         "transcript_row_sha256": conformance._canonical_digest(transcript_row)})
    if (any(_sha(path) != digest for path, digest in initial_files.items())
            or _git(checkout, "status", "--porcelain") != ""
            or _git(checkout, "rev-parse", "HEAD") != repo["commit"]
            or _git(checkout, "rev-parse", "HEAD^{tree}") != repo["tree"]):
        raise Refused("read inputs changed during raw-map capture")
    return {"schema": 1, "kind": "version_rehearsal_authenticated_raw_maps",
            "provenance": {"report_path": str(report_path), "report_sha256": initial_files[report_path],
                           "snapshot_checkout": str(checkout), "snapshot_commit": repo["commit"],
                           "snapshot_tree": repo["tree"], "binary": str(binary),
                           "binary_sha256": measured_binary["sha256"], "perl": str(perl),
                           "perl_sha256": initial_files[perl], "native_source": str(native_source),
                           "native_program_sha256": initial_files[native_program],
                           "native_exiftool_pm_sha256": initial_files[native_pm],
                           "transcript_sha256": transcript["sha256"]},
            "rows": captured}


def validate_capture(saved: Mapping[str, Any], conformance_report: Path,
                     fixture_entries: list[dict[str, Any]], snapshot_checkout: Path,
                     binary: Path, perl: Path, native_source: Path) -> None:
    """Verify saved raw bytes, then replay stable scored maps and provenance.

    FileAccessDate and other ignored fields may vary between otherwise
    identical commands. Keep their original bytes as evidence, but do not
    use them as acceptance criteria for a delayed replay.
    """
    fresh = capture_authenticated_maps(conformance_report, fixture_entries,
                                       snapshot_checkout, binary, perl, native_source)
    if (not isinstance(saved, dict) or saved.get("schema") != 1
            or saved.get("kind") != fresh["kind"]
            or saved.get("provenance") != fresh["provenance"]
            or not isinstance(saved.get("rows"), list)
            or len(saved["rows"]) != len(fresh["rows"])):
        raise Refused("saved raw-map provenance differs from fresh replay")
    for old, new in zip(saved["rows"], fresh["rows"], strict=True):
        if not isinstance(old, dict):
            raise Refused("saved raw-map row is malformed")
        for side in ("oracle", "candidate"):
            command = old.get("native_command" if side == "oracle" else "candidate_command")
            if command is None and side == "candidate":
                if (new["candidate_command"] is None
                        and old.get("candidate_raw_map") == {}
                        and old.get("candidate_ordered_pairs") == []):
                    continue
                raise Refused("saved candidate command is missing")
            if not isinstance(command, dict):
                raise Refused("saved raw-map command is missing")
            try:
                stdout = base64.b64decode(command["stdout_base64"], validate=True)
                stderr = base64.b64decode(command["stderr_base64"], validate=True)
            except (KeyError, ValueError) as error:
                raise Refused("saved raw-map command bytes are malformed") from error
            if (hashlib.sha256(stdout).hexdigest() != command.get("stdout_sha256")
                    or hashlib.sha256(stderr).hexdigest() != command.get("stderr_sha256")
                    or len(stdout) != command.get("stdout_bytes")
                    or len(stderr) != command.get("stderr_bytes")):
                raise Refused("saved raw-map command byte hashes changed")
            mapping, pairs = _json_map(stdout, oracle=side == "oracle")
            if (mapping != old.get(f"{side}_raw_map")
                    or pairs != old.get(f"{side}_ordered_pairs")):
                raise Refused("saved raw-map values differ from original stdout")
            fresh_command = new["native_command" if side == "oracle" else "candidate_command"]
            if (command.get("status") != fresh_command["status"]
                    or command.get("argv") != fresh_command["argv"]):
                raise Refused("raw-map command status or argv changed")
        if (old.get("fixture") != new["fixture"]
                or old.get("transcript_row") != new["transcript_row"]
                or old.get("transcript_row_sha256") != new["transcript_row_sha256"]
                or old.get("native_status") != new["native_status"]):
            raise Refused("raw-map fixture or transcript identity changed")
        for side, splitter in (("oracle", conformance.split_oracle_key),
                               ("candidate", conformance.split_oxidex_key)):
            old_scored = [pair for pair in old[f"{side}_ordered_pairs"]
                          if splitter(pair[0])[1] not in conformance.IGNORE]
            new_scored = [pair for pair in new[f"{side}_ordered_pairs"]
                          if splitter(pair[0])[1] not in conformance.IGNORE]
            if old_scored != new_scored:
                raise Refused("saved scored raw-map occurrences differ from fresh replay")
