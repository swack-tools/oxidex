#!/usr/bin/env python3
"""Prepare and score fresh, pinned-native inputs for raw_scoped_native_fixture_driver.

Evidence-only instrument.  Preparation never invokes the OxiDex driver; the
caller runs its ignored test exactly once with the emitted request/result paths.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path


# Each target is an ASCII TIFF entry in a real pinned source carrier.  The
# first native write establishes a known physical seed; the second gives the
# exact native wire value for a replacement, or removes the seeded entry.
TARGETS = (
    ("jpeg-ifd0", "jpeg", "GPS.jpg", "IFD0", "IFD0:Artist", 315,
     "raw-seed-artist", "raw-replaced-artist"),
    ("jpeg-exif", "jpeg", "GPS.jpg", "ExifIFD", "ExifIFD:DateTimeOriginal", 36867,
     "2001:02:03 04:05:06", "2007:08:09 10:11:12"),
    ("jpeg-gps", "jpeg", "GPS.jpg", "GPS", "GPS:GPSMapDatum", 18,
     "raw-seed-datum", "raw-replaced-datum"),
    ("jpeg-ifd1", "jpeg", "GPS.jpg", "IFD1", "IFD1:ImageDescription", 270,
     "raw-seed-thumbnail", "raw-replaced-thumbnail"),
    ("tiff-ifd0", "tiff_big", "ExifTool.tif", "IFD0", "IFD0:ImageDescription", 270,
     "raw-seed-description", "raw-replaced-description"),
)
DIRECTORY = {"IFD0": (), "ExifIFD": ("ExifIFD",), "GPS": ("GPS",), "IFD1": ("NextIFD",)}
OPS = ("set", "replace", "delete")
PREPARER = Path(__file__).resolve().with_name("raw_scoped_native_preparer.py")


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def create_json(path: Path, value: object) -> None:
    with path.open("x", encoding="utf-8") as f:
        json.dump(value, f, indent=2, sort_keys=True)
        f.write("\n")


def selected(document: dict, carrier: str, scope: str) -> dict | None:
    node = document if carrier.startswith("tiff") else document["exif"]
    for name in DIRECTORY[scope]:
        if node is None:
            return None
        node = node["children"].get(name)
    return node


def native_readback(perl: Path, library: Path, script: Path, path: Path, native) -> dict:
    command = [str(perl), "-I" + str(library), str(script), "-config", "", "-j", "-a", "-G1", "-n", "-s", str(path)]
    env = {key: value for key, value in native.clean_env().items() if not key.startswith("EXIFTOOL_")}
    done = subprocess.run(command, env=env, capture_output=True, timeout=30)
    if done.returncode != 0:
        raise RuntimeError(f"pinned native readback failed: {done.stderr.decode(errors='replace')}")
    records = json.loads(done.stdout)
    if len(records) != 1:
        raise RuntimeError("pinned native readback did not return one record")
    # SourceFile and filesystem values necessarily differ across isolated files.
    return {k: v for k, v in records[0].items()
            if not k.startswith(("File:", "System:")) and k != "SourceFile"}


def jpeg_tiff(path: Path) -> tuple[bytes, int]:
    """Return the one EXIF TIFF payload and its actual file offset."""
    data = path.read_bytes()
    if not data.startswith(b"\xff\xd8"):
        raise AssertionError("not a JPEG carrier")
    position, found, file_offset = 2, None, None
    while position < len(data):
        if data[position] != 0xff:
            raise AssertionError("malformed JPEG marker")
        while position < len(data) and data[position] == 0xff:
            position += 1
        if position >= len(data):
            raise AssertionError("truncated JPEG marker")
        marker = data[position]
        position += 1
        if marker in (0xda, 0xd9):
            break
        if marker == 0x01 or 0xd0 <= marker <= 0xd7:
            continue
        if position + 2 > len(data):
            raise AssertionError("truncated JPEG segment")
        length = int.from_bytes(data[position:position + 2], "big")
        if length < 2 or position + length > len(data):
            raise AssertionError("out-of-bounds JPEG segment")
        payload = data[position + 2:position + length]
        if marker == 0xe1 and payload.startswith(b"Exif\0\0"):
            if found is not None:
                raise AssertionError("ambiguous EXIF APP1 segments")
            found = payload[6:]
            file_offset = position + 2 + 6
        position += length
    if found is None:
        raise AssertionError("JPEG EXIF payload absent")
    return found, file_offset


def physical_payload_proof(paths: tuple[Path, Path, Path], documents: tuple[dict, dict, dict],
                           carrier: str) -> dict:
    """Authenticate the only recognized relocated payload in this raw cohort."""
    kind = "thumbnail" if carrier == "jpeg" else "strip"
    pointer_id, length_id = (("513", "514") if kind == "thumbnail" else ("273", "279"))
    records = []
    for path, document in zip(paths, documents, strict=True):
        tiff, file_offset = jpeg_tiff(path) if kind == "thumbnail" else (path.read_bytes(), 0)
        node = document["exif"]["children"].get("NextIFD") if kind == "thumbnail" else document
        if node is None:
            raise AssertionError("recognized payload directory absent")
        tags = node["tags"]
        pointer, length = tags.get(pointer_id), tags.get(length_id)
        if pointer is None or length is None or pointer["type"] != 4 or pointer["count"] != 1 or length["type"] != 4 or length["count"] != 1:
            raise AssertionError("recognized payload pointer type/count differs or is absent")
        order = node["byte_order"]
        offset = int.from_bytes(bytes.fromhex(pointer["value_hex"]), order)
        size = int.from_bytes(bytes.fromhex(length["value_hex"]), order)
        if size <= 0 or offset < 8 or offset > len(tiff) or size > len(tiff) - offset:
            raise AssertionError("recognized payload pointer is out of bounds")
        payload = tiff[offset:offset + size]
        records.append({"path": str(path), "offset": offset, "tiff_file_offset": file_offset,
                        "length": size,
                        "payload_sha256": hashlib.sha256(payload).hexdigest(),
                        "payload_hex": payload.hex()})
    if len({(record["length"], record["payload_sha256"]) for record in records}) != 1:
        raise AssertionError("recognized payload changed across seed/native/oxidex")
    # Keep physical addresses, but avoid repeating payload bytes in the score.
    return {"kind": kind, "records": [{k: v for k, v in record.items() if k != "payload_hex"}
                                       for record in records]}


def compare_relocated(seed: dict, expected: dict, actual: dict, paths: tuple[Path, Path, Path],
                      row: dict, native, compare_carrier) -> dict:
    proof = physical_payload_proof(paths, (seed, expected, actual), row["carrier"])
    normalized = [copy.deepcopy(document) for document in (seed, expected, actual)]
    if proof["kind"] == "thumbnail":
        # compare_carrier still checks every other physical tag, directory, JPEG
        # image stream and non-EXIF segment. The numeric thumbnail address is
        # normalized only after the three pointed payloads proved identical.
        canonical = normalized[2]["exif"]["children"]["NextIFD"]["tags"]["513"]["value_hex"]
        for document in normalized:
            document["exif"]["children"]["NextIFD"]["tags"]["513"]["value_hex"] = canonical
    compare_carrier(*normalized, "jpeg" if row["carrier"] == "jpeg" else "tiff",
                    row["tag_id"], target_directory=DIRECTORY[row["scope"]],
                    allow_directory_removal=row["scope"] == "IFD1" and row["op"] == "delete")
    return proof


def compare_group1(expected: dict, actual: dict, proof: dict) -> str:
    if expected.keys() != actual.keys():
        raise AssertionError("native Group1 readback identities differ")
    pointer_key = ("IFD1:ThumbnailOffset" if proof["kind"] == "thumbnail" else "IFD0:StripOffsets")
    native_record, oxidex_record = proof["records"][1:]
    native_address, oxidex_address = native_record["offset"], oxidex_record["offset"]
    # ExifTool's Group1 numeric readback is a file address. TIFF fields are
    # relative to their TIFF header, whose actual JPEG location may vary.
    if (expected.get(pointer_key) != native_address + native_record["tiff_file_offset"]
            or actual.get(pointer_key) != oxidex_address + oxidex_record["tiff_file_offset"]):
        raise AssertionError("native Group1 pointer readback disagrees with physical field")
    for key in expected:
        if key != pointer_key and expected[key] != actual[key]:
            raise AssertionError(f"native Group1 nonpointer readback differs for {key}")
    return "match after authenticated pointer relocation" if native_address != oxidex_address else "exact match"


def load_helpers(repo: Path):
    sys.path.insert(0, str(repo / "tools" / "exiftool-tables"))
    import native_write_matrix as native
    from generated_tiff_write_matrix import compare_carrier
    return native, compare_carrier


def repository_identity(repo: Path) -> tuple[str, str]:
    if subprocess.check_output(["git", "-C", str(repo), "status", "--porcelain"], text=True).strip():
        raise RuntimeError(f"candidate source tree is dirty: {repo}")
    head = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
    tree = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD^{tree}"], text=True).strip()
    return head, tree


def prepare(args) -> int:
    repo, source = args.repo.resolve(), args.source_root.resolve()
    head, tree = repository_identity(repo)
    native, _ = load_helpers(repo)
    perl = native.resolve_perl(args.perl)
    library = native.resolve_library(source)
    identity = native.native_identity(perl, library)
    native.assert_contract_version(identity, repo / ".exiftool-version")
    script = source / "exiftool"
    if not script.is_file():
        raise RuntimeError(f"pinned script absent: {script}")
    round_dir = args.round_dir.resolve()
    round_dir.mkdir(parents=True, exist_ok=False)
    files = round_dir / "files"
    files.mkdir()
    rows, unsupported, requests = [], [], []
    fixture_hashes = {}
    for label, carrier, filename, scope, tag, tag_id, seed_value, replacement in TARGETS:
        fixture = source / "t" / "images" / filename
        if not fixture.is_file():
            unsupported.extend({"target": label, "op": op, "reason": "native fixture absent",
                                "path": str(fixture)} for op in OPS)
            continue
        fixture_hashes[str(fixture)] = digest(fixture)
        suffix = ".jpg" if carrier == "jpeg" else ".tif"
        for op in OPS:
            prefix = files / f"{label}-{op}"
            seed = Path(str(prefix) + "-seed" + suffix)
            expected = Path(str(prefix) + "-native" + suffix)
            actual = Path(str(prefix) + "-oxidex" + suffix)
            seed_call = native.run_native_batch(perl, library, fixture, seed,
                                                [{"tag": tag, "scalar": "utf8", "value": seed_value}])
            try:
                native.assert_native(seed_call, f"{label}/{op} seed")
                before_doc = native.inspect(seed, carrier)
                before_node = selected(before_doc, carrier, scope)
                before = None if before_node is None else before_node["tags"].get(str(tag_id))
                if before is None or before["type"] != 2:
                    raise AssertionError("native seed did not create selected ASCII physical entry")
                set_value = ("2004:05:06 07:08:09" if scope == "ExifIFD" else f"raw-set-{label}")
                batch = [{"tag": tag, "scalar": "undefined"}] if op == "delete" else [
                    {"tag": tag, "scalar": "utf8", "value": set_value if op == "set" else replacement}]
                transition = native.run_native_batch(perl, library, seed, expected, batch)
                native.assert_native(transition, f"{label}/{op} transition")
                expected_doc = native.inspect(expected, carrier)
                expected_node = selected(expected_doc, carrier, scope)
                after = None if expected_node is None else expected_node["tags"].get(str(tag_id))
                if op == "delete":
                    if after is not None:
                        raise AssertionError("native delete left selected physical entry")
                    request = {"input": str(seed), "output": str(actual), "scope": scope,
                               "tag_id": tag_id, "op": "delete"}
                else:
                    if after is None or native.entry_storage(after) == native.entry_storage(before):
                        raise AssertionError("native replacement did not change selected physical entry")
                    storage = native.entry_storage(after)
                    if storage["type"] != 2:
                        raise AssertionError("native replacement is not ASCII")
                    request = {"input": str(seed), "output": str(actual), "scope": scope,
                               "tag_id": tag_id, "op": "set", "type": storage["type"],
                               "count": storage["count"], "value_hex": storage["value_hex"]}
                before_readback = native_readback(perl, library, script, seed, native)
                expected_readback = native_readback(perl, library, script, expected, native)
                if before_readback == expected_readback:
                    raise AssertionError("native transition has no Group1-visible effect")
            except (AssertionError, ValueError, RuntimeError) as error:
                unsupported.append({"target": label, "op": op, "reason": str(error),
                                    "seed_call": seed_call})
                continue
            rows.append({"target": label, "op": op, "carrier": carrier, "scope": scope,
                         "tag": tag, "tag_id": tag_id, "fixture": str(fixture),
                         "input": str(seed), "input_sha256": digest(seed),
                         "expected": str(expected), "expected_sha256": digest(expected),
                         "output": str(actual), "before_entry": native.entry_storage(before),
                         "expected_entry": native.entry_storage(after),
                         "before_group1": before_readback, "expected_group1": expected_readback,
                         "native_seed": seed_call, "native_transition": transition})
            requests.append(request)
    if not rows:
        raise RuntimeError("no native-backed raw requests qualified; inspect isolated round files")
    requests_path = round_dir / "requests.jsonl"
    with requests_path.open("x", encoding="utf-8") as f:
        for request in requests:
            f.write(json.dumps(request, sort_keys=True) + "\n")
    manifest = {"schema": 1, "mode": "pinned-native-real-fixture-raw-scoped",
                "repo_head": head, "repo_tree": tree,
                "instrument_sha256": digest(Path(__file__)),
                "helper_sha256": {str(repo / "tools/exiftool-tables" / name): digest(repo / "tools/exiftool-tables" / name)
                                  for name in ("native_write_matrix.py", "generated_tiff_write_matrix.py")},
                "source_sha256": {str(script): digest(script), **fixture_hashes},
                "native_identity": identity, "perl": str(perl), "library": str(library),
                "requests": str(requests_path), "requests_sha256": digest(requests_path),
                "results": str(round_dir / "results.jsonl"), "rows": rows, "unsupported": unsupported}
    create_json(round_dir / "manifest.json", manifest)
    print(json.dumps({"manifest": str(round_dir / "manifest.json"), "requests": str(requests_path),
                      "results": manifest["results"], "qualified": len(rows),
                      "unsupported": len(unsupported)}, sort_keys=True))
    return 0


def score(args) -> int:
    manifest_path = args.manifest.resolve()
    manifest = json.loads(manifest_path.read_text())
    repo = args.repo.resolve()
    if repository_identity(repo) != (manifest["repo_head"], manifest["repo_tree"]):
        raise RuntimeError("candidate source identity changed since preparation")
    native, compare_carrier = load_helpers(repo)
    if manifest["instrument_sha256"] != digest(PREPARER):
        raise RuntimeError("original preparation instrument changed since preparation")
    for path, expected in {**manifest["source_sha256"], **manifest["helper_sha256"],
                           manifest["requests"]: manifest["requests_sha256"]}.items():
        if digest(Path(path)) != expected:
            raise RuntimeError(f"prepared source/request changed: {path}")
    perl, library = Path(manifest["perl"]), Path(manifest["library"])
    identity = native.native_identity(perl, library)
    if identity["result"] != manifest["native_identity"]["result"]:
        raise RuntimeError("pinned native identity changed since preparation")
    results_path = Path(manifest["results"])
    if not results_path.is_file():
        raise RuntimeError(f"driver result absent: {results_path}")
    results = [json.loads(line) for line in results_path.read_text().splitlines() if line]
    if len(results) != len(manifest["rows"]):
        raise RuntimeError("driver result count differs from qualified requests")
    script = Path(next(path for path in manifest["source_sha256"] if path.endswith("/exiftool")))
    if not args.driver_binary.is_file() or not args.driver_log.is_file():
        raise RuntimeError("exact candidate driver binary and full invocation log are required")
    scores = []
    for row, result in zip(manifest["rows"], results, strict=True):
        status, checks = "pass", {}
        if result.get("input") != row["input"] or result.get("output") != row["output"]:
            status = "failed"
            checks["identity"] = "driver result path mismatch"
        elif digest(Path(row["input"])) != row["input_sha256"]:
            status = "failed"
            checks["input"] = "seed input changed after preparation"
        elif result.get("ok") is not True:
            status = "failed" if Path(row["output"]).exists() else "refused"
            checks["error"] = result.get("error")
            checks["partial_output"] = Path(row["output"]).exists()
        elif not Path(row["output"]).is_file():
            status = "failed"
            checks["output"] = "driver reported success without output"
        else:
            try:
                seed = native.inspect(Path(row["input"]), row["carrier"])
                expected = native.inspect(Path(row["expected"]), row["carrier"])
                actual = native.inspect(Path(row["output"]), row["carrier"])
                if digest(Path(row["expected"])) != row["expected_sha256"]:
                    raise AssertionError("native expected file changed")
                proof = compare_relocated(seed, expected, actual,
                                          (Path(row["input"]), Path(row["expected"]), Path(row["output"])),
                                          row, native, compare_carrier)
                checks["physical_and_payload"] = "match"
                checks["pointer_proof"] = proof
                actual_group1 = native_readback(perl, library, script, Path(row["output"]), native)
                checks["group1"] = compare_group1(row["expected_group1"], actual_group1, proof)
            except (AssertionError, ValueError, RuntimeError) as error:
                status = "failed"
                checks["parity_error"] = str(error)
        scores.append({"target": row["target"], "op": row["op"], "status": status,
                       "output": row["output"], "checks": checks})
    counts = {name: sum(row["status"] == name for row in scores)
              for name in ("pass", "failed", "refused")}
    counts["unsupported_preparation"] = len(manifest["unsupported"])
    report = {"schema": 1, "manifest": str(manifest_path), "manifest_sha256": digest(manifest_path),
              "results": str(results_path), "results_sha256": digest(results_path),
              "driver_binary": str(args.driver_binary.resolve()),
              "driver_binary_sha256": digest(args.driver_binary),
              "driver_log": str(args.driver_log.resolve()),
              "driver_log_sha256": digest(args.driver_log),
              "scorer_sha256": digest(Path(__file__)),
              "preparer_sha256": digest(PREPARER),
              "counts": counts, "scores": scores, "unsupported": manifest["unsupported"]}
    create_json(args.score.resolve(), report)
    print(json.dumps({"score": str(args.score.resolve()), "counts": counts}, sort_keys=True))
    return 0 if counts["failed"] == counts["refused"] == counts["unsupported_preparation"] == 0 else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    prep = sub.add_parser("prepare")
    prep.add_argument("--repo", type=Path, required=True)
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
