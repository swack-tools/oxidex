#!/usr/bin/env python3
"""Acceptance instrument for public writes to fresh and empty-EXIF JPEGs.

This is deliberately an oracle comparison, not a list of expected tag defaults.
Every requested tag identity is read from the emitted final-scalar ledger and
actual selected ExifTool 13.59 output supplies the type/count/value bytes.  The
Rust fixture driver is the public ``modify_tag``/``remove_tag`` route; until
that route supports fresh JPEG EXIF creation this instrument records failures
and exits non-zero with the native evidence retained.
"""
from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Any, Iterable

import native_write_matrix as native
from generated_tiff_write_matrix import DRIVER, GeneratedTarget, generated_targets

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from instrument import git_state, print_header, refuse_if_dirty, resolve_binary, staleness_note

DEFAULT_JPEG = ROOT / "tests/fixtures/jpeg/edge_cases/orientation_2.jpg"
LEDGER = ROOT / "tools/exiftool-tables/tiff_scalar_final_ledger.json"
RULES = ROOT / "src/writers/generated_tiff_scalar_final_rules.rs"

# The public API request has no byte-order option surface.  These are the only
# no-override routes it can express.  A future explicit public byte-order
# option needs its own source-authenticated request contract and test cases.
OPERATIONS = (
    ("fresh-insert", "insert", "utf8", "insert-value"),
    ("defined-empty", "empty", "utf8", ""),
    # ExifTool returns write_return=2 for this source-absent removal: this is
    # the public delete/no-op case and must keep an EXIF APP1 absent.
    ("delete-absent-noop", "delete", "undefined", None),
)


@dataclass(frozen=True)
class RawJfif:
    label: str
    unit: int | None
    x_density: int | None
    y_density: int | None


@dataclass(frozen=True)
class CarrierCase:
    label: str
    jfif: RawJfif
    empty_ifd0_order: str | None


CARRIERS = (
    CarrierCase("fresh-no-jfif", RawJfif("absent", None, None, None), None),
    CarrierCase("fresh-jfif-zero", RawJfif("zero", 0, 0, 0), None),
    CarrierCase("fresh-jfif-nonzero", RawJfif("nonzero", 1, 72, 300), None),
    CarrierCase("empty-ifd0-little", RawJfif("nonzero", 1, 72, 300), "little"),
    CarrierCase("empty-ifd0-big", RawJfif("zero", 0, 0, 0), "big"),
)


def _segment(marker: int, payload: bytes) -> bytes:
    if not 0 <= marker <= 0xFF or len(payload) + 2 > 0xFFFF:
        raise ValueError("invalid JPEG segment")
    return b"\xff" + bytes([marker]) + (len(payload) + 2).to_bytes(2, "big") + payload


def _base_tail(base: bytes) -> bytes:
    """Remove the fixture's APP0 only; retain its non-EXIF image stream."""
    if not base.startswith(b"\xff\xd8\xff\xe0"):
        raise ValueError("base JPEG has no first APP0 segment")
    length = int.from_bytes(base[4:6], "big")
    end = 2 + 2 + length
    if length < 2 or end >= len(base) or base[6:11] != b"JFIF\0":
        raise ValueError("base JPEG APP0 is not the expected JFIF carrier")
    return base[end:]


def raw_jfif_payload(jfif: RawJfif) -> bytes | None:
    """Return raw APP0 bytes; fields are inputs, never expected output tags."""
    if jfif.unit is None:
        return None
    if not all(isinstance(value, int) and 0 <= value <= 0xFFFF
               for value in (jfif.x_density, jfif.y_density)) or jfif.unit not in (0, 1, 2):
        raise ValueError("raw JFIF density input is malformed")
    return (b"JFIF\0\x01\x02" + bytes([jfif.unit])
            + jfif.x_density.to_bytes(2, "big") + jfif.y_density.to_bytes(2, "big") + b"\0\0")


def empty_ifd0_tiff(order: str) -> bytes:
    if order == "little":
        return b"II*\0\x08\0\0\0\0\0\0\0\0\0"
    if order == "big":
        return b"MM\0*\0\0\0\x08\0\0\0\0\0\0"
    raise ValueError("empty IFD0 byte order is unsupported")


def write_carrier(path: Path, case: CarrierCase, base: Path = DEFAULT_JPEG) -> None:
    source = base.read_bytes()
    data = bytearray(b"\xff\xd8")
    if payload := raw_jfif_payload(case.jfif):
        data += _segment(0xE0, payload)
    if case.empty_ifd0_order is not None:
        data += _segment(0xE1, b"Exif\0\0" + empty_ifd0_tiff(case.empty_ifd0_order))
    data += _base_tail(source)
    path.write_bytes(data)


def jfif_payloads(path: Path) -> tuple[str, ...]:
    """Extract APP0 JFIF payload bytes to prove exact raw preservation."""
    data, position, values = path.read_bytes(), 2, []
    if not data.startswith(b"\xff\xd8"):
        raise ValueError("not JPEG")
    while position < len(data):
        if data[position] != 0xFF:
            raise ValueError("JPEG marker prefix absent")
        start = position
        while position < len(data) and data[position] == 0xFF:
            position += 1
        if position >= len(data):
            raise ValueError("JPEG marker is truncated")
        marker = data[position]
        position += 1
        if marker == 0xDA:
            break
        if marker == 0xD9:
            break
        if marker in {0x01, *range(0xD0, 0xD8)}:
            continue
        if position + 2 > len(data):
            raise ValueError("JPEG segment is truncated")
        length = int.from_bytes(data[position:position + 2], "big")
        if length < 2 or position + length > len(data):
            raise ValueError("JPEG segment bounds are invalid")
        payload = data[position + 2:position + length]
        if marker == 0xE0 and payload.startswith(b"JFIF\0"):
            values.append(payload.hex())
        position += length
    return tuple(values)


def _stored(entry: dict[str, Any] | None) -> dict[str, Any] | None:
    if entry is None:
        return None
    return {key: entry[key] for key in ("type", "count", "value_hex")}


def _compare_tiff(expected: dict[str, Any], actual: dict[str, Any]) -> None:
    if expected["byte_order"] != actual["byte_order"]:
        raise AssertionError("native/generated TIFF byte order differs")
    if set(expected["tags"]) != set(actual["tags"]):
        raise AssertionError("native/generated TIFF tag identities differ")
    for tag, entry in expected["tags"].items():
        if _stored(entry) != _stored(actual["tags"][tag]):
            raise AssertionError(f"native/generated TIFF tag {tag} type/count/value bytes differ")
    if expected["children"].keys() != actual["children"].keys():
        raise AssertionError("native/generated TIFF child directories differ")
    for name, child in expected["children"].items():
        _compare_tiff(child, actual["children"][name])


def compare_jpeg(source: Path, native_output: Path, generated_output: Path,
                 target: GeneratedTarget, operation: str) -> None:
    source_doc, native_doc, generated_doc = (native.parse_jpeg(path) for path in (source, native_output, generated_output))
    source_jfif, native_jfif, generated_jfif = (jfif_payloads(path) for path in (source, native_output, generated_output))
    if source_jfif != native_jfif or source_jfif != generated_jfif:
        raise AssertionError("raw JFIF APP0 bytes changed")
    for key in ("sos_to_end_sha256", "non_exif_sha256"):
        if source_doc[key] != native_doc[key] or source_doc[key] != generated_doc[key]:
            raise AssertionError(f"JPEG {key} changed outside Exif APP1")
    if operation == "delete-absent-noop" and source_doc["exif"] is None:
        if native_doc["exif"] is not None or generated_doc["exif"] is not None:
            raise AssertionError("delete/no-op unexpectedly created an Exif APP1")
        return
    if native_doc["exif"] is None or generated_doc["exif"] is None:
        raise AssertionError("native/generated fresh write disagrees on EXIF APP1 presence")
    _compare_tiff(native_doc["exif"], generated_doc["exif"])
    native_target = native_doc["exif"]["tags"].get(str(target.raw_tag_id))
    generated_target = generated_doc["exif"]["tags"].get(str(target.raw_tag_id))
    if _stored(native_target) != _stored(generated_target):
        raise AssertionError("requested final-ledger target type/count/value bytes differ")


def assert_native_oracle(call: dict[str, Any], label: str) -> None:
    if call["returncode"] != 0 or not isinstance(call["result"], dict):
        raise AssertionError(f"{label}: native process failed: {call['stderr'] or call['stdout']}")
    result = call["result"]
    if result.get("write_return") not in (1, 2) or result.get("error") is not None:
        raise AssertionError(f"{label}: native write failed: {result}")
    if not all(item.get("return") in (1, 2) for item in result.get("set_calls", [])):
        raise AssertionError(f"{label}: native SetNewValue rejected generated identity: {result}")


def _request(input_path: Path, output_path: Path, target: GeneratedTarget,
             qualifier: str, scalar: str, value: str | None) -> dict[str, Any]:
    # Keep this exact existing fixture-driver schema.  It has
    # deny_unknown_fields, so report identity lives beside the request instead.
    return {"route": "public-api", "carrier": "jpeg", "input": str(input_path),
            "output": str(output_path), "key": qualifier, "scalar": scalar, "value": value}


def run_matrix(*, test_binary: Path, perl: Path, library: Path, output: Path,
               base_jpeg: Path, ledger: Path, rules: Path) -> dict[str, Any]:
    targets = generated_targets(ledger, rules)
    root = output.parent / "fresh-jpeg-public-write-files"
    root.mkdir(parents=True, exist_ok=False)
    native_rows: list[dict[str, Any]] = []
    requests: list[dict[str, Any]] = []
    for case in CARRIERS:
        source = root / f"{case.label}-source.jpg"
        write_carrier(source, case, base_jpeg)
        source_doc = native.parse_jpeg(source)
        if case.empty_ifd0_order is None:
            if source_doc["exif"] is not None:
                raise AssertionError("fresh carrier unexpectedly has Exif APP1")
        elif source_doc["exif"] is None or source_doc["exif"]["byte_order"] != case.empty_ifd0_order:
            raise AssertionError("empty IFD0 carrier has the wrong byte order")
        for target in targets:
            for qualifier in target.qualifiers:
                for label, native_action, scalar, value in OPERATIONS:
                    stem = f"{case.label}-{target.raw_tag_id:04x}-{qualifier.replace(':', '_')}-{label}"
                    native_output, generated_output = root / f"{stem}-native.jpg", root / f"{stem}-generated.jpg"
                    call = native.run_native(perl, library, source, native_output, native_action, qualifier)
                    assert_native_oracle(call, stem)
                    request = _request(source, generated_output, target, qualifier, scalar, value)
                    requests.append(request)
                    native_rows.append({"id": stem, "carrier": asdict(case), "target": asdict(target),
                                        "qualifier": qualifier, "operation": label, "source": str(source),
                                        "native_output": str(native_output), "output": str(generated_output),
                                        "native_call": call})
    request_path, result_path = root / "requests.json", root / "results.json"
    request_path.write_text(json.dumps(requests, indent=2) + "\n", encoding="utf-8")
    env = os.environ.copy()
    env.update(OXIDEX_SCALAR_WRITE_REQUESTS=str(request_path), OXIDEX_SCALAR_WRITE_RESULTS=str(result_path))
    driver = subprocess.run([str(test_binary), DRIVER, "--exact", "--ignored", "--nocapture"],
                            env=env, text=True, capture_output=True, timeout=120)
    (root / "driver.log").write_text(driver.stdout + driver.stderr, encoding="utf-8")
    driver.check_returncode()
    results = json.loads(result_path.read_text(encoding="utf-8"))
    if not isinstance(results, list) or len(results) != len(requests):
        raise AssertionError("public fixture driver result population differs")
    report: dict[str, Any] = {"instrument": "fresh_jpeg_public_write_matrix_v1", "declared": len(requests),
                              "passed": 0, "native_rows": native_rows,
                              "user_byte_order_override": {"state": "not_expressible_by_public_api_request",
                                  "reason": "public modify_tag/remove_tag has no byte-order options; only default and existing-byte-order paths are exercised"},
                              "limitations": ["Selected generated final-ledger EXIF cohort only.",
                                  "User byte-order options are not represented by the public API request contract."]}
    for row, result in zip(native_rows, results, strict=True):
        row["driver_result"] = result
        try:
            if not isinstance(result, dict) or result.get("output") != row["output"] or result.get("ok") is not True:
                raise AssertionError(f"public write refused or failed: {result}")
            compare_jpeg(Path(row["source"]), Path(row["native_output"]), Path(row["output"]),
                         GeneratedTarget(**row["target"]), row["operation"])
            row["state"] = "passed"
            report["passed"] += 1
        except (AssertionError, OSError, ValueError) as error:
            row.update(state="failed", error=str(error))
        output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return report


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-binary", required=True, type=Path)
    parser.add_argument("--perl", required=True, type=Path)
    parser.add_argument("--lib", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--jpeg-base", type=Path, default=DEFAULT_JPEG)
    parser.add_argument("--ledger", type=Path, default=LEDGER)
    parser.add_argument("--rules", type=Path, default=RULES)
    args = parser.parse_args(argv)
    state = git_state(ROOT)
    overridden = refuse_if_dirty(state, "fresh_jpeg_public_write_matrix")
    binary = resolve_binary(args.test_binary, "oxidex-lib-test")
    if note := staleness_note(binary, state):
        raise RuntimeError(note)
    perl, library = native.resolve_perl(args.perl), native.resolve_library(args.lib)
    identity = native.native_identity(perl, library)
    native.assert_contract_version(identity)
    if not args.jpeg_base.is_file():
        parser.error("JPEG base fixture is absent")
    targets = generated_targets(args.ledger, args.rules)
    declared = len(CARRIERS) * sum(len(target.qualifiers) for target in targets) * len(OPERATIONS)
    print_header(tool="fresh_jpeg_public_write_matrix_v1", git=state, binary=binary,
                 dirty_overridden=overridden,
                 extra=[f"native: {identity}", f"{declared} selected public JPEG requests; fresh + empty IFD0 carriers"])
    report = run_matrix(test_binary=binary.path, perl=perl, library=library, output=args.output,
                        base_jpeg=args.jpeg_base, ledger=args.ledger, rules=args.rules)
    report.update({"source_commit": state.commit, "native_identity": identity,
                   "test_binary_sha256": hashlib.sha256(binary.path.read_bytes()).hexdigest(),
                   "ledger_sha256": hashlib.sha256(args.ledger.read_bytes()).hexdigest(),
                   "rules_sha256": hashlib.sha256(args.rules.read_bytes()).hexdigest()})
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"Fresh/empty public JPEG operations matched: {report['passed']}/{report['declared']}")
    return 0 if report["passed"] == report["declared"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
