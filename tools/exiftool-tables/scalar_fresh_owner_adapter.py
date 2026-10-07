#!/usr/bin/env python3
"""Prepare fresh full-suite scalar-driver requests from a passed native owner matrix.

The owner instrument has already run and graded its own driver outputs. This
adapter binds its authentic seed/native pairs, rewrites only output paths for a
single later workspace ignored-suite run, and scores that run independently.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path


def sha(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_new(path: Path, value: object) -> None:
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, sort_keys=True, indent=2)
        stream.write("\n")


def git_identity(repo: Path) -> tuple[str, str]:
    if subprocess.check_output(["git", "-C", str(repo), "status", "--porcelain"], text=True).strip():
        raise RuntimeError(f"candidate checkout is dirty: {repo}")
    return tuple(subprocess.check_output(["git", "-C", str(repo), "rev-parse", arg], text=True).strip()
                 for arg in ("HEAD", "HEAD^{tree}"))


def helpers(repo: Path):
    sys.path.insert(0, str(repo / "tools" / "exiftool-tables"))
    import native_write_matrix as native
    import generated_tiff_write_matrix as owner
    return native, owner


def native_group1_pair(perl: Path, library: Path, script: Path,
                       expected: Path, actual: Path, native) -> tuple[dict, dict]:
    command = [str(perl), "-I" + str(library), str(script), "-config", "",
               "-q", "-j", "-a", "-G1", "-n", "-s", str(expected), str(actual)]
    env = {key: value for key, value in native.clean_env().items() if not key.startswith("EXIFTOOL_")}
    result = subprocess.run(command, env=env, capture_output=True, timeout=30)
    if result.returncode != 0:
        raise RuntimeError("pinned native Group1 readback failed: " + result.stderr.decode(errors="replace"))
    records = json.loads(result.stdout)
    if len(records) != 2:
        raise RuntimeError("pinned native Group1 pair returned wrong record count")
    if result.stderr:
        raise RuntimeError("pinned native Group1 readback emitted a warning: " + result.stderr.decode(errors="replace"))
    if any(key in record for record in records for key in ("ExifTool:Warning", "ExifTool:Error")):
        raise RuntimeError("pinned native Group1 readback contains an ExifTool Warning/Error")
    return tuple({key: value for key, value in record.items()
                  if not key.startswith(("File:", "System:")) and key != "SourceFile"}
                 for record in records)



def compare_group1_with_proven_strip_relocation(seed: dict, expected: dict, actual: dict,
                                                carrier: str, native_values: dict,
                                                actual_values: dict) -> dict:
    """Permit only an authenticated TIFF strip-address change after physical proof.

    The caller must first run compare_carrier, which proves exact strip payload
    bytes, all other TIFF entries, and directory topology. These explicit
    checks bind each displayed native -n address to its own parsed physical
    tag, so a dropped or fabricated Group1 field cannot be normalized away.
    """
    if native_values == actual_values:
        return {"policy": "exact_group1", "match": True}
    key = "IFD0:StripOffsets"
    if carrier not in ("tiff_little", "tiff_big"):
        raise AssertionError("pinned native Group1 readback differs outside TIFF strip relocation")
    if key not in native_values or key not in actual_values:
        raise AssertionError("TIFF strip relocation lacks native or candidate Group1 pointer")
    differing = {name for name in native_values.keys() | actual_values.keys()
                 if native_values.get(name) != actual_values.get(name)}
    if differing != {key}:
        raise AssertionError("pinned native Group1 readback differs outside IFD0:StripOffsets")
    offsets = {}
    for label, document, readback in (("seed", seed, None),
                                      ("native", expected, native_values[key]),
                                      ("candidate", actual, actual_values[key])):
        entry = document.get("tags", {}).get("273")
        length = document.get("tags", {}).get("279")
        if (not isinstance(entry, dict) or entry.get("type") != 4 or entry.get("count") != 1
                or not isinstance(length, dict) or length.get("type") != 4 or length.get("count") != 1
                or document.get("image_payload_hex") is None):
            raise AssertionError("TIFF strip relocation lacks one LONG pointer, byte count, or payload")
        pointer = int.from_bytes(bytes.fromhex(entry["value_hex"]), document["byte_order"])
        byte_count = int.from_bytes(bytes.fromhex(length["value_hex"]), document["byte_order"])
        if not isinstance(readback, int) and readback is not None:
            raise AssertionError("native Group1 strip pointer is not an integer")
        if readback is not None and pointer != readback:
            raise AssertionError("native Group1 strip pointer disagrees with physical TIFF entry")
        if len(bytes.fromhex(document["image_payload_hex"])) != byte_count:
            raise AssertionError("TIFF referenced strip length disagrees with StripByteCounts")
        offsets[label] = {"physical_address": pointer, "type": entry["type"],
                          "count": entry["count"], "byte_count": byte_count,
                          "referenced_payload_sha256": hashlib.sha256(
                              bytes.fromhex(document["image_payload_hex"])).hexdigest()}
    if not (seed["image_payload_hex"] == expected["image_payload_hex"] == actual["image_payload_hex"]):
        raise AssertionError("TIFF referenced strip bytes differ across seed/native/candidate")
    if len({row["byte_count"] for row in offsets.values()}) != 1:
        raise AssertionError("TIFF strip byte counts differ across seed/native/candidate")
    return {"policy": "IFD0:StripOffsets physical relocation only",
            "match": True, "readback": {"native": native_values[key],
                                         "candidate": actual_values[key]},
            "physical": offsets, "other_group1_keys_equal": True}



def proven_native_noop_copy_timestamp(row: dict, manifest_path: Path,
                                       driver_log: Path) -> dict | None:
    """Explain an old mtime only for a newly copied, byte-identical native no-op.

    On APFS, Rust std::fs::copy preserves the input mtime. The ignored fixture
    driver copies each input before the public write; a native no-op does not
    rewrite it. Fresh output ctime/inode and full-byte equality are required;
    the normal native-noop and physical/readback checks still run afterward.
    """
    output, seed, expected = (Path(row[key]) for key in ("output", "input", "expected"))
    if (row.get("effective_state") != "native_noop" or output.is_symlink()
            or not output.is_file() or not seed.is_file() or not expected.is_file()):
        return None
    out_stat, seed_stat = output.stat(), seed.stat()
    manifest_time, log_time = manifest_path.stat().st_mtime_ns, driver_log.stat().st_mtime_ns
    if out_stat.st_mtime_ns >= manifest_time:
        return None
    if (out_stat.st_mtime_ns != seed_stat.st_mtime_ns
            or (out_stat.st_dev, out_stat.st_ino) == (seed_stat.st_dev, seed_stat.st_ino)
            or not manifest_time <= out_stat.st_ctime_ns <= log_time):
        return None
    hashes = {label: sha(path) for label, path in
              (("seed", seed), ("native", expected), ("output", output))}
    if len(set(hashes.values())) != 1:
        return None
    return {"policy": "fresh native-noop copy preserves seed mtime",
            "output_device": out_stat.st_dev, "output_inode": out_stat.st_ino,
            "seed_device": seed_stat.st_dev, "seed_inode": seed_stat.st_ino,
            "output_mtime_ns": out_stat.st_mtime_ns,
            "seed_mtime_ns": seed_stat.st_mtime_ns,
            "output_ctime_ns": out_stat.st_ctime_ns,
            "manifest_mtime_ns": manifest_time,
            "driver_log_mtime_ns": log_time, "sha256": hashes}


def validate_owner(repo: Path, report_path: Path, requests_path: Path,
                   binary: Path, native_identity: dict) -> tuple[dict, list[dict]]:
    report = json.loads(report_path.read_text())
    requests = json.loads(requests_path.read_text())
    head, _ = git_identity(repo)
    if report.get("instrument") != "generated_scalar_write_matrix_v4" or report.get("route") != "public-api":
        raise RuntimeError("owner report is not the pinned public-api scalar matrix")
    if report.get("source_commit") != head or report.get("dirty_files"):
        raise RuntimeError("owner report does not bind to clean candidate HEAD")
    if (not isinstance(report.get("declared"), int) or report["declared"] < 1
            or report["declared"] != report.get("passed")
            or report["declared"] != len(report.get("rows", []))):
        raise RuntimeError("owner report has missing or failed native/driver cells")
    if len(requests) != report["declared"] or not requests:
        raise RuntimeError("owner request population differs from its passed report")
    if Path(report["test_binary_path"]).resolve() != binary.resolve() or sha(binary) != report["test_binary_sha256"]:
        raise RuntimeError("owner test binary identity differs from report")
    if report.get("native_identity", {}).get("result") != native_identity["result"]:
        raise RuntimeError("selected native source identity differs from owner report")
    if report.get("contract") != {"mode": "pinned-13.59-contract", "release": "13.59"}:
        raise RuntimeError("owner report is not the reviewed repository-pin contract")
    if report["ledger_sha256"] != sha(repo / "tools/exiftool-tables/tiff_scalar_final_ledger.json"):
        raise RuntimeError("candidate final scalar ledger changed since owner report")
    if report["rules_sha256"] != sha(repo / "src/writers/generated_tiff_scalar_final_rules.rs"):
        raise RuntimeError("candidate generated scalar rules changed since owner report")
    seen_ids, seen_outputs = set(), set()
    for index, (row, request) in enumerate(zip(report["rows"], requests, strict=True)):
        if row.get("state") != "passed" or row.get("driver_result", {}).get("ok") is not True:
            raise RuntimeError(f"owner row {index} is not passed")
        if row["seeded"] != request.get("input") or row["output"] != request.get("output"):
            raise RuntimeError(f"owner row/request paths differ at {index}")
        if request.get("route") != "public-api" or request.get("carrier") != row["carrier"]:
            raise RuntimeError(f"owner row/request route or carrier differs at {index}")
        if request.get("key") != row["requested_name"] or request.get("scalar") != row["public_scalar"]:
            raise RuntimeError(f"owner row/request selected scalar differs at {index}")
        if row["id"] in seen_ids or row["output"] in seen_outputs:
            raise RuntimeError(f"duplicate owner case or output at {index}")
        seen_ids.add(row["id"])
        seen_outputs.add(row["output"])
        if (row["driver_result"].get("output") != row["output"]
                or row["driver_result"].get("warnings")
                or request.get("value") != (None if row["requested_input_hex"] is None
                                             else row["requested_input_hex"] if row["public_scalar"] == "bytes"
                                             else bytes.fromhex(row["requested_input_hex"]).decode("utf-8"))):
            raise RuntimeError(f"owner request or driver result differs from passed row {index}")
        if not Path(row["seeded"]).is_file() or not Path(row["native_output"]).is_file():
            raise RuntimeError(f"owner seed/native output absent at {index}")
    return report, requests


def fresh_row(index: int, owner_row: dict, output: Path) -> dict:
    return {"index": index, "id": owner_row["id"], "carrier": owner_row["carrier"],
            "input": owner_row["seeded"], "input_sha256": sha(Path(owner_row["seeded"])),
            "expected": owner_row["native_output"],
            "expected_sha256": sha(Path(owner_row["native_output"])),
            "output": str(output), "target_tag_id": owner_row["target"]["raw_tag_id"],
            "target": owner_row["target"], "target_directory": owner_row["target_directory"],
            "operation": owner_row["operation"], "effective_operation": owner_row["effective_operation"],
            "requested_input_hex": owner_row["requested_input_hex"],
            "effective_state": owner_row["effective_state"]}


def prepare(args) -> int:
    repo, report_path, requests_path = (path.resolve() for path in
                                         (args.repo, args.owner_report, args.owner_requests))
    native, _owner = helpers(repo)
    head, tree = git_identity(repo)
    perl = native.resolve_perl(args.perl)
    library = native.resolve_library(args.source_root)
    identity = native.native_identity(perl, library)
    native.assert_contract_version(identity, repo / ".exiftool-version")
    script = args.source_root.resolve() / "exiftool"
    if not script.is_file():
        raise RuntimeError("pinned ExifTool script absent")
    binary = args.owner_binary.resolve()
    report, requests = validate_owner(repo, report_path, requests_path, binary, identity)
    round_dir = args.round_dir.resolve()
    round_dir.mkdir(parents=True, exist_ok=False)
    outputs = round_dir / "outputs"
    outputs.mkdir()
    if (round_dir / "requests.json").exists() or (round_dir / "results.json").exists():
        raise RuntimeError("fresh scalar request or result path already exists")
    fresh_requests, rows = [], []
    for index, (owner_row, request) in enumerate(zip(report["rows"], requests, strict=True)):
        suffix = ".jpg" if owner_row["carrier"] == "jpeg" else ".tif"
        output = outputs / f"{index:04d}{suffix}"
        fresh = dict(request, output=str(output))
        fresh_requests.append(fresh)
        rows.append(fresh_row(index, owner_row, output))
    fresh_path = round_dir / "requests.json"
    write_new(fresh_path, fresh_requests)
    manifest = {"schema": 1, "mode": "fresh-output-replay-of-passed-native-scalar-owner",
                "repo_head": head, "repo_tree": tree, "instrument_sha256": sha(Path(__file__)),
                "owner_report": str(report_path), "owner_report_sha256": sha(report_path),
                "owner_requests": str(requests_path), "owner_requests_sha256": sha(requests_path),
                "owner_binary": str(binary), "owner_binary_sha256": sha(binary),
                "owner_helper_sha256": {str(path): sha(path) for path in
                                        (repo / "tools/exiftool-tables/native_write_matrix.py",
                                         repo / "tools/exiftool-tables/generated_tiff_write_matrix.py")},
                "native_identity": identity, "perl": str(perl), "library": str(library),
                "script": str(script), "script_sha256": sha(script),
                "requests": str(fresh_path), "requests_sha256": sha(fresh_path),
                "results": str(round_dir / "results.json"), "rows": rows}
    write_new(round_dir / "manifest.json", manifest)
    print(json.dumps({"manifest": str(round_dir / "manifest.json"), "requests": str(fresh_path),
                      "results": manifest["results"], "cases": len(rows)}, sort_keys=True))
    return 0


def score(args) -> int:
    manifest_path, repo = args.manifest.resolve(), args.repo.resolve()
    manifest = json.loads(manifest_path.read_text())
    if manifest.get("schema") != 1 or manifest.get("mode") != "fresh-output-replay-of-passed-native-scalar-owner":
        raise RuntimeError("unrecognized scalar replay manifest")
    if args.score.resolve() != manifest_path.parent / "score.json":
        raise RuntimeError("scalar score must use the round's single fresh score.json path")
    if args.score.exists():
        raise RuntimeError("scalar replay has already been scored")
    if (Path(manifest["requests"]) != manifest_path.parent / "requests.json"
            or Path(manifest["results"]) != manifest_path.parent / "results.json"
            or (manifest_path.parent / "outputs").is_symlink()):
        raise RuntimeError("scalar round paths differ from the fresh manifest directory")
    native, owner = helpers(repo)
    if git_identity(repo) != (manifest["repo_head"], manifest["repo_tree"]):
        raise RuntimeError("candidate source changed since scalar preparation")
    if sha(Path(__file__)) != manifest["instrument_sha256"]:
        raise RuntimeError("scalar adapter changed since preparation")
    for path, expected in {manifest["owner_report"]: manifest["owner_report_sha256"],
                           manifest["owner_requests"]: manifest["owner_requests_sha256"],
                           manifest["owner_binary"]: manifest["owner_binary_sha256"],
                           manifest["script"]: manifest["script_sha256"],
                           manifest["requests"]: manifest["requests_sha256"],
                           **manifest["owner_helper_sha256"]}.items():
        if sha(Path(path)) != expected:
            raise RuntimeError(f"authenticated scalar source changed: {path}")
    identity = native.native_identity(Path(manifest["perl"]), Path(manifest["library"]))
    if identity["result"] != manifest["native_identity"]["result"]:
        raise RuntimeError("pinned native identity changed")
    owner_report, owner_requests = validate_owner(
        repo, Path(manifest["owner_report"]), Path(manifest["owner_requests"]),
        Path(manifest["owner_binary"]), identity)
    fresh_requests = json.loads(Path(manifest["requests"]).read_text())
    if len(fresh_requests) != len(manifest["rows"]):
        raise RuntimeError("fresh scalar request population differs from manifest")
    for index, (owner_row, owner_request, row, request) in enumerate(
            zip(owner_report["rows"], owner_requests, manifest["rows"], fresh_requests, strict=True)):
        expected_row = fresh_row(index, owner_row, Path(row["output"]))
        expected_request = dict(owner_request, output=row["output"])
        suffix = ".jpg" if owner_row["carrier"] == "jpeg" else ".tif"
        canonical_output = manifest_path.parent / "outputs" / f"{index:04d}{suffix}"
        if row != expected_row or request != expected_request or Path(row["output"]) != canonical_output:
            raise RuntimeError(f"fresh scalar case differs from authenticated owner row {index}")
    result_path = Path(manifest["results"])
    if result_path.is_symlink() or not result_path.is_file():
        raise RuntimeError("full-suite scalar results absent")
    if result_path.stat().st_mtime_ns < manifest_path.stat().st_mtime_ns:
        raise RuntimeError("scalar results predate fresh request manifest")
    results = json.loads(result_path.read_text())
    if not isinstance(results, list) or len(results) != len(manifest["rows"]):
        raise RuntimeError("full-suite scalar result population differs")
    binary, log = args.driver_binary.resolve(), args.driver_log.resolve()
    if not binary.is_file() or not log.is_file():
        raise RuntimeError("candidate-built driver binary and full invocation log required")
    if log.stat().st_mtime_ns < manifest_path.stat().st_mtime_ns:
        raise RuntimeError("full ignored-suite log predates fresh scalar manifest")
    if sha(binary) != manifest["owner_binary_sha256"]:
        raise RuntimeError("full-suite scalar driver binary differs from authenticated owner binary")
    scores = []
    for row, result in zip(manifest["rows"], results, strict=True):
        output = Path(row["output"])
        status, detail = "pass", {}
        timestamp_proof = proven_native_noop_copy_timestamp(row, manifest_path, log)
        if output.is_symlink():
            status, detail = "failed", {"error": "driver output is a symlink"}
        elif (output.exists() and output.stat().st_mtime_ns < manifest_path.stat().st_mtime_ns
              and timestamp_proof is None):
            status, detail = "failed", {"error": "driver output predates fresh request manifest"}
        elif result.get("output") != row["output"]:
            status, detail = "failed", {"error": "driver result output path mismatch"}
        elif sha(Path(row["input"])) != row["input_sha256"] or sha(Path(row["expected"])) != row["expected_sha256"]:
            status, detail = "failed", {"error": "native seed or expected output changed"}
        elif result.get("ok") is not True:
            status = "failed" if output.exists() else "refused"
            detail = {"error": result.get("error"), "partial_output": output.exists()}
        elif result.get("warnings") or not output.is_file():
            status, detail = "failed", {"error": "driver warning or missing output", "warnings": result.get("warnings")}
        else:
            try:
                seed, expected, actual = (native.inspect(Path(row[key]), row["carrier"])
                                          for key in ("input", "expected", "output"))
                path = tuple(row["target_directory"])
                target = owner.GeneratedTarget(**row["target"])
                requested_input = None if row["requested_input_hex"] is None else bytes.fromhex(row["requested_input_hex"])
                native_noop = owner.native_requested_insert_is_noop(
                    seed, expected, row["carrier"], path, target, row["operation"], requested_input)
                if row["effective_state"] != ("native_noop" if native_noop else "mutated"):
                    raise AssertionError("owner effective native state differs from replay")
                if native_noop:
                    owner.compare_carrier(seed, seed, expected, row["carrier"], row["target_tag_id"],
                                          target_directory=path)
                else:
                    owner.assert_selected_target_transition(seed, expected, row["carrier"], path,
                                                            row["target_tag_id"], row["effective_operation"])
                owner.compare_carrier(seed, expected, actual, row["carrier"], row["target_tag_id"],
                                      target_directory=path,
                                      allow_directory_removal=row["effective_operation"] == "delete")
                detail["physical_and_payload"] = "match"
                if timestamp_proof is not None:
                    detail["timestamp_origin"] = timestamp_proof
                native_values, actual_values = native_group1_pair(Path(manifest["perl"]), Path(manifest["library"]),
                                                                   Path(manifest["script"]), Path(row["expected"]),
                                                                   output, native)
                detail["group1"] = compare_group1_with_proven_strip_relocation(
                    seed, expected, actual, row["carrier"], native_values, actual_values)
            except (AssertionError, ValueError, RuntimeError, OSError) as error:
                status, detail = "failed", {"error": str(error), **detail}
        scores.append({"id": row["id"], "status": status, "output": row["output"],
                       "output_sha256": sha(output) if output.is_file() else None, "detail": detail})
    counts = {name: sum(case["status"] == name for case in scores) for name in ("pass", "failed", "refused")}
    write_new(args.score.resolve(), {"schema": 1, "manifest": str(manifest_path),
                                    "manifest_sha256": sha(manifest_path), "results": str(result_path),
                                    "results_sha256": sha(result_path), "driver_binary": str(binary),
                                    "driver_binary_sha256": sha(binary), "driver_log": str(log),
                                    "driver_log_sha256": sha(log), "counts": counts, "scores": scores})
    print(json.dumps({"score": str(args.score.resolve()), "counts": counts}, sort_keys=True))
    return 0 if counts["failed"] == counts["refused"] == 0 else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    prep = sub.add_parser("prepare")
    prep.add_argument("--repo", type=Path, required=True)
    prep.add_argument("--owner-report", type=Path, required=True)
    prep.add_argument("--owner-requests", type=Path, required=True)
    prep.add_argument("--owner-binary", type=Path, required=True)
    prep.add_argument("--source-root", type=Path, required=True)
    prep.add_argument("--perl", type=Path, required=True)
    prep.add_argument("--round-dir", type=Path, required=True)
    grading = sub.add_parser("score")
    grading.add_argument("--repo", type=Path, required=True)
    grading.add_argument("--manifest", type=Path, required=True)
    grading.add_argument("--score", type=Path, required=True)
    grading.add_argument("--driver-binary", type=Path, required=True)
    grading.add_argument("--driver-log", type=Path, required=True)
    args = parser.parse_args()
    return prepare(args) if args.command == "prepare" else score(args)


if __name__ == "__main__":
    raise SystemExit(main())
