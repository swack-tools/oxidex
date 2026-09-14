#!/usr/bin/env python3
"""Compare mixed public whole-map TIFF/JPEG write batches with pinned ExifTool.

The generated identity cohort comes only from the emitted final-stage ledger.
Every native row queues all of its SetNewValue operands on one ExifTool object
and commits them with one WriteInfo call.  The generated row supplies the same
whole-map delta to the public transaction path, which must commit once only
when both its legacy and generated portions succeed.

This is limited to existing EXIF/TIFF directories.  It deliberately does not
cover fresh or empty EXIF creation, ungenerated public names, or another
ExifTool release.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Any

import native_write_matrix as native
from generated_tiff_write_matrix import DRIVER, GeneratedTarget, compare_carrier, generated_targets

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from instrument import git_state, print_header, refuse_if_dirty, resolve_binary, staleness_note

BATCH_CASES = (
    "mixed_legacy_generated",
    "alias_replacement",
    "generated_delete_legacy_update",
    "conflicting_aliases",
    "forged_generated_identity_after_legacy",
)


def scalar_value(text: str) -> dict[str, str]:
    return {"scalar": "utf8", "value": text}


def bytes_value(value: bytes) -> dict[str, str]:
    return {"scalar": "bytes", "value": value.hex()}


def deletion() -> dict[str, str]:
    return {"scalar": "undefined"}


def omission() -> dict[str, str]:
    return {"scalar": "omitted"}


def item(key: str, encoded: dict[str, str]) -> dict[str, str]:
    return {"key": key, **encoded}


def native_item(key: str, encoded: dict[str, str]) -> dict[str, str]:
    return {"tag": key, **encoded}


def batch_case(target: GeneratedTarget, case: str) -> dict[str, Any]:
    """Build one source-derived generated target case without a tag allowlist."""
    table_name, physical_name = target.qualifiers
    artist = "IFD0:Artist"
    # The physical spelling matches the reader's IFD0 projection; the table
    # spelling is the alias used to exercise the public resolver.
    seed = [
        native_item(artist, scalar_value("seed-artist")),
        native_item(physical_name, scalar_value("seed-target")),
        native_item("IFD0:Software", scalar_value("unrelated-software")),
    ]
    if case == "mixed_legacy_generated":
        # This keeps a true Unicode scalar in the mixed transaction; the
        # native batch record must show a UTF-8-flagged SetNewValue operand.
        encoded = scalar_value("mixed-é")
        return {
            "seed": seed,
            "native": [native_item(artist, scalar_value("artist-mixed")), native_item(table_name, encoded)],
            "generated": [item(artist, scalar_value("artist-mixed")), item(table_name, encoded)],
            "changed_tags": {315, target.raw_tag_id},
            "expect_ok": True,
        }
    if case == "alias_replacement":
        encoded = scalar_value("alias-replacement")
        return {
            "seed": seed,
            # SetNewValue resolves these names to one physical entry; the
            # desired map removes the reader spelling before adding its alias.
            "native": [native_item(artist, scalar_value("artist-alias")), native_item(table_name, encoded)],
            "generated": [item(artist, scalar_value("artist-alias")), item(physical_name, omission()), item(table_name, encoded)],
            "changed_tags": {315, target.raw_tag_id},
            "expect_ok": True,
        }
    if case == "generated_delete_legacy_update":
        return {
            "seed": seed,
            "native": [native_item(artist, scalar_value("artist-delete")), native_item(table_name, deletion())],
            "generated": [item(artist, scalar_value("artist-delete")), item(physical_name, deletion())],
            "changed_tags": {315, target.raw_tag_id},
            "expect_ok": True,
        }
    if case == "conflicting_aliases":
        first, second = scalar_value("alias-first"), scalar_value("alias-second")
        return {
            "seed": seed,
            "native": [native_item(artist, scalar_value("artist-conflict")), native_item(table_name, first), native_item(physical_name, second)],
            "generated": [item(artist, scalar_value("artist-conflict")), item(table_name, first), item(physical_name, second)],
            "changed_tags": {315, target.raw_tag_id},
            "expect_ok": False,
            "failure": "conflicting aliases",
        }
    if case == "forged_generated_identity_after_legacy":
        encoded = scalar_value("after-legacy")
        return {
            "seed": seed,
            "native": [native_item(artist, scalar_value("artist-before-refusal")), native_item(table_name, encoded)],
            "generated": [item(artist, scalar_value("artist-before-refusal")), item(table_name, encoded)],
            "changed_tags": {315, target.raw_tag_id},
            "expect_ok": False,
            "fault": "forged-final-identity-after-legacy",
            "failure": "resolved address has no matching final source identity",
        }
    raise ValueError(f"unknown public batch case {case}")


def assert_native_batch(call: dict[str, Any], label: str, expected: list[dict[str, str]]) -> None:
    native.assert_native(call, label)
    observed = call["result"]["set_calls"]
    if len(observed) != len(expected):
        raise AssertionError(f"{label}: native set-call count differs")
    for actual, planned in zip(observed, expected, strict=True):
        if actual["tag"] != planned["tag"]:
            raise AssertionError(f"{label}: native operand order/name differs")
        value = planned.get("value")
        scalar = planned["scalar"]
        expected_state = {"defined": scalar != "undefined"}
        if scalar != "undefined":
            expected_state["utf8"] = scalar == "utf8"
            expected_state["hex"] = value.encode("utf-8").hex() if scalar == "utf8" else value
        for key, expected_value in expected_state.items():
            if actual["input"].get(key) != expected_value:
                raise AssertionError(f"{label}: native operand {key} differs")


def byte_identical(first: Path, second: Path) -> None:
    if first.read_bytes() != second.read_bytes():
        raise AssertionError("failed public transaction changed file bytes")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-binary", required=True)
    parser.add_argument("--perl", type=Path, required=True)
    parser.add_argument("--lib", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--jpeg-base", type=Path, help="exercise an existing JPEG EXIF carrier too")
    parser.add_argument("--ledger", type=Path, default=ROOT / "tools/exiftool-tables/tiff_scalar_final_ledger.json")
    parser.add_argument("--rules", type=Path, default=ROOT / "src/writers/generated_tiff_scalar_final_rules.rs")
    args = parser.parse_args(argv)
    if args.jpeg_base and not args.jpeg_base.is_file():
        parser.error("JPEG base is not a file")
    state = git_state(ROOT)
    overridden = refuse_if_dirty(state, "generated_public_write_batch_matrix")
    binary = resolve_binary(args.test_binary, "oxidex-lib-test")
    if note := staleness_note(binary, state):
        raise RuntimeError(note)
    perl, library = native.resolve_perl(args.perl), native.resolve_library(args.lib)
    identity = native.native_identity(perl, library)
    native.assert_contract_version(identity)
    targets = generated_targets(args.ledger, args.rules)
    carriers = ("tiff_little", "tiff_big") + (("jpeg",) if args.jpeg_base else ())
    declared = len(carriers) * len(targets) * len(BATCH_CASES)
    print_header(
        tool="generated_public_write_batch_matrix_v1", git=state, binary=binary,
        dirty_overridden=overridden,
        extra=[f"native: {identity}", f"{declared} mixed whole-map batches; existing EXIF only"],
    )
    root = args.output.parent / "generated-public-write-batch-files"
    root.mkdir(parents=True, exist_ok=False)
    rows: list[dict[str, Any]] = []
    requests: list[dict[str, Any]] = []
    for carrier in carriers:
        suffix = ".jpg" if carrier == "jpeg" else ".tif"
        for target in targets:
            for case in BATCH_CASES:
                spec = batch_case(target, case)
                stem = f"{carrier}-{target.raw_tag_id:04x}-{case}"
                source, seeded, expected, output = (
                    root / f"{stem}-{part}{suffix}" for part in ("source", "seeded", "native", "generated")
                )
                native.make_carrier(source, carrier, args.jpeg_base)
                seed_call = native.run_native_batch(perl, library, source, seeded, spec["seed"])
                assert_native_batch(seed_call, stem + " seed", spec["seed"])
                native_call = native.run_native_batch(perl, library, seeded, expected, spec["native"])
                assert_native_batch(native_call, stem + " native batch", spec["native"])
                request = {
                    "route": "public-batch", "carrier": carrier,
                    "input": str(seeded), "output": str(output), "batch": spec["generated"],
                }
                if "fault" in spec:
                    request["fault"] = spec["fault"]
                requests.append(request)
                rows.append({
                    "id": stem, "carrier": carrier, "case": case,
                    "target": {"raw_tag_id": target.raw_tag_id, "name": target.name,
                               "table_group0": target.table_group0,
                               "physical_write_group": target.physical_write_group},
                    "seeded": str(seeded), "native_output": str(expected), "output": str(output),
                    "seed_call": seed_call, "native_call": native_call,
                    "expect_ok": spec["expect_ok"], "changed_tags": sorted(spec["changed_tags"]),
                    "failure": spec.get("failure"),
                })
    request_path, result_path = root / "requests.json", root / "results.json"
    request_path.write_text(json.dumps(requests, indent=2) + "\n")
    env = os.environ.copy() | {
        "OXIDEX_SCALAR_WRITE_REQUESTS": str(request_path),
        "OXIDEX_SCALAR_WRITE_RESULTS": str(result_path),
    }
    completed = subprocess.run(
        [str(binary.path), DRIVER, "--exact", "--ignored", "--nocapture"],
        env=env, capture_output=True, text=True, timeout=180,
    )
    (root / "driver.log").write_text(completed.stdout + completed.stderr)
    completed.check_returncode()
    results = json.loads(result_path.read_text())
    report: dict[str, Any] = {
        "instrument": "generated_public_write_batch_matrix_v1", "source_commit": state.commit,
        "dirty_files": state.dirty_files, "native_identity": identity,
        "test_binary_sha256": hashlib.sha256(binary.path.read_bytes()).hexdigest(),
        "ledger_sha256": hashlib.sha256(args.ledger.read_bytes()).hexdigest(),
        "rules_sha256": hashlib.sha256(args.rules.read_bytes()).hexdigest(),
        "declared": declared, "passed": 0, "rows": rows,
        "limitations": [
            "Existing TIFF/JPEG EXIF only; fresh or empty EXIF creation is not exercised.",
            "The forged-final-identity case is a fixture-only lower-stage provenance fault after the production legacy in-memory rewrite; it is not an admission of a public input spelling.",
            "Only final-stage ledger identities and selected ExifTool 13.59 are exercised.",
        ],
    }
    if len(results) != len(rows):
        raise AssertionError("fixture driver result population differs")
    for row, result in zip(rows, results, strict=True):
        row["driver_result"] = result
        try:
            if result.get("output") != row["output"]:
                raise AssertionError("driver reported a different output path")
            if row["expect_ok"]:
                if not result.get("ok") or result.get("warnings"):
                    raise AssertionError(f"public batch did not succeed: {result}")
                seed, expected, actual = (
                    native.inspect(Path(row[key]), row["carrier"])
                    for key in ("seeded", "native_output", "output")
                )
                compare_carrier(seed, expected, actual, row["carrier"], set(row["changed_tags"]))
            else:
                if result.get("ok"):
                    raise AssertionError("public batch unexpectedly succeeded")
                if row["failure"] not in result.get("error", ""):
                    raise AssertionError(f"public batch refused for the wrong reason: {result}")
                byte_identical(Path(row["seeded"]), Path(row["output"]))
            row["state"] = "passed"
            report["passed"] += 1
        except (AssertionError, OSError, ValueError) as error:
            row.update(state="failed", error=str(error))
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(f"Public whole-map batches matched/refused atomically: {report['passed']}/{declared}")
    return 0 if report["passed"] == declared else 1


if __name__ == "__main__":
    raise SystemExit(main())
