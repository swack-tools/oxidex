#!/usr/bin/env python3
"""Native acceptance for fresh JPEG public batches that meet mandatory defaults.

The public-batch fixture interface comes from the root's existing ignored test.
This tool does not modify that fixture.  It derives both generated targets and
mandatory IFD0 override identities from committed source-generated artifacts;
pinned native ExifTool supplies every expected type/count/value byte.
"""
from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from typing import Any, Iterable

import native_write_matrix as native
from fresh_jpeg_public_write_matrix import CARRIERS, jfif_payloads, write_carrier, _compare_tiff
from generated_tiff_write_matrix import DRIVER, GeneratedTarget, generated_targets

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from instrument import git_state, print_header, refuse_if_dirty, resolve_binary, staleness_note

LEDGER = ROOT / "tools/exiftool-tables/tiff_scalar_final_ledger.json"
RULES = ROOT / "src/writers/generated_tiff_scalar_final_rules.rs"
MANDATORY_LEDGER = ROOT / "tools/exiftool-tables/mandatory_defaults_ledger.json"
ADDRESS_RULES = ROOT / "src/writers/generated_setnewvalue_address_rules.rs"

BATCH_CASES = (
    "generated-delete-legacy-set",
    "generated-set-mandatory-override",
    "generated-set-mandatory-delete",
)

# Parsed from rendered generated Rust: no handwritten public tag list.
_ADDRESS = re.compile(
    r'StaticSetNewValueAddress \{\s*index: \d+,\s*module: "(?P<module>[^"]+)",\s*'
    r'table: "(?P<table>[^"]+)",\s*full_name: "(?P<full_name>[^"]+)",\s*'
    r'raw_id: "(?P<raw_id>[^"]+)",\s*name: "(?P<name>[^"]+)",\s*'
    r'group0: "(?P<group0>[^"]+)",\s*group1: "(?P<group1>[^"]+)",\s*'
    r'write_group: "(?P<write_group>[^"]+)",\s*\}',
    re.DOTALL,
)


@dataclass(frozen=True)
class MandatoryCandidate:
    raw_tag_id: int
    name: str
    group0: str
    write_group: str

    @property
    def qualifier(self) -> str:
        return f"{self.write_group}:{self.name}"


def mandatory_ifd0_candidate(ledger_path: Path = MANDATORY_LEDGER,
                              address_path: Path = ADDRESS_RULES) -> MandatoryCandidate:
    """Join the selected mandatory IFD0 default ID to a source address row."""
    ledger = json.loads(ledger_path.read_text(encoding="utf-8"))
    recipe = ledger.get("recipe")
    if ledger.get("writer_tables_joined") is not True or not isinstance(recipe, dict):
        raise ValueError("mandatory default ledger is unresolved")
    directories = recipe.get("directories")
    if not isinstance(directories, list):
        raise ValueError("mandatory default directories are unavailable")
    ifd0 = [item for item in directories if isinstance(item, dict) and item.get("directory") == "IFD0"]
    if len(ifd0) != 1 or not isinstance(ifd0[0].get("defaults"), list):
        raise ValueError("mandatory default ledger has no unique IFD0 defaults")
    raw_ids = sorted(item.get("tag_id") for item in ifd0[0]["defaults"] if type(item.get("tag_id")) is int)
    if not raw_ids:
        raise ValueError("mandatory IFD0 default IDs are unavailable")
    rows = []
    for match in _ADDRESS.finditer(address_path.read_text(encoding="utf-8")):
        values = match.groupdict()
        try:
            raw_id = int(values["raw_id"], 0)
        except ValueError:
            continue
        if (raw_id in raw_ids and values["module"] == "Exif" and values["table"] == "Main"
                and values["full_name"] == "Image::ExifTool::Exif::Main"
                and values["write_group"] == "IFD0"):
            rows.append(MandatoryCandidate(raw_id, values["name"], values["group0"], values["write_group"]))
    rows.sort(key=lambda item: (item.raw_tag_id, item.name, item.group0))
    if not rows:
        raise ValueError("mandatory IFD0 default has no generated address identity")
    candidate = rows[0]
    if sum(row.raw_tag_id == candidate.raw_tag_id for row in rows) != 1:
        raise ValueError("mandatory IFD0 default address is ambiguous")
    return candidate


def public_item(key: str, scalar: str, value: str | None = None) -> dict[str, str]:
    item = {"key": key, "scalar": scalar}
    if value is not None:
        item["value"] = value
    return item


def native_item(key: str, scalar: str, value: str | None = None) -> dict[str, str]:
    item = {"tag": key, "scalar": scalar}
    if value is not None:
        item["value"] = value
    return item


def batch_case(target: GeneratedTarget, mandatory: MandatoryCandidate, case: str) -> dict[str, Any]:
    generated_name = target.qualifiers[0]
    if case == "generated-delete-legacy-set":
        return {
            "native": [native_item(generated_name, "undefined"), native_item("IFD0:Artist", "utf8", "batch-artist")],
            "public": [public_item(generated_name, "undefined"), public_item("IFD0:Artist", "utf8", "batch-artist")],
            "target_present": False,
            "mandatory_state": "present-or-native-omitted",
        }
    if case == "generated-set-mandatory-override":
        # Integer is the public typed input; native SetNewValue receives its
        # textual spelling and is the oracle for conversion/type/count/bytes.
        return {
            "native": [native_item(generated_name, "utf8", "batch-target"), native_item(mandatory.qualifier, "utf8", "2")],
            "public": [public_item(generated_name, "utf8", "batch-target"), public_item(mandatory.qualifier, "integer", "2")],
            "target_present": True,
            "mandatory_state": "present",
        }
    if case == "generated-set-mandatory-delete":
        return {
            "native": [native_item(generated_name, "utf8", "batch-target"), native_item(mandatory.qualifier, "undefined")],
            "public": [public_item(generated_name, "utf8", "batch-target"), public_item(mandatory.qualifier, "undefined")],
            "target_present": True,
            "mandatory_state": "absent",
        }
    raise ValueError("unknown fresh public batch case")


def assert_native_batch(call: dict[str, Any], label: str, expected: list[dict[str, str]]) -> None:
    if call.get("returncode") != 0 or not isinstance(call.get("result"), dict):
        raise AssertionError(f"{label}: native batch process failed")
    result = call["result"]
    if result.get("write_return") != 1 or result.get("error") is not None:
        raise AssertionError(f"{label}: native batch failed or became a no-op: {result}")
    actual = result.get("set_calls")
    if not isinstance(actual, list) or len(actual) != len(expected):
        raise AssertionError(f"{label}: native batch operand count differs")
    for observed, planned in zip(actual, expected, strict=True):
        if observed.get("tag") != planned["tag"] or observed.get("return") not in (1, 2):
            raise AssertionError(f"{label}: native batch operand order/name differs")
        scalar = planned["scalar"]
        expected_input: dict[str, Any] = {"defined": scalar != "undefined"}
        if scalar != "undefined":
            value = planned.get("value")
            if not isinstance(value, str):
                raise AssertionError(f"{label}: defined native batch operand is missing its source value")
            expected_input.update(
                utf8=scalar == "utf8",
                hex=value.encode("utf-8").hex() if scalar == "utf8" else value,
            )
        actual_input = observed.get("input")
        if not isinstance(actual_input, dict):
            raise AssertionError(f"{label}: native batch input state is unavailable")
        for key, expected_value in expected_input.items():
            if actual_input.get(key) != expected_value:
                raise AssertionError(f"{label}: native batch operand {key} differs")


def _entry(document: dict[str, Any], raw_tag_id: int) -> dict[str, Any] | None:
    exif = document["exif"]
    return None if exif is None else exif["tags"].get(str(raw_tag_id))


def compare_batch(source: Path, native_output: Path, generated_output: Path,
                  target: GeneratedTarget, mandatory: MandatoryCandidate, spec: dict[str, Any]) -> None:
    source_doc, native_doc, generated_doc = (native.parse_jpeg(path) for path in (source, native_output, generated_output))
    for key in ("sos_to_end_sha256", "non_exif_sha256"):
        if source_doc[key] != native_doc[key] or source_doc[key] != generated_doc[key]:
            raise AssertionError(f"mixed public batch changed JPEG {key}")
    source_jfif, native_jfif, generated_jfif = (jfif_payloads(path) for path in (source, native_output, generated_output))
    if source_jfif != native_jfif or source_jfif != generated_jfif:
        raise AssertionError("mixed public batch changed raw JFIF fields")
    if native_doc["exif"] is None or generated_doc["exif"] is None:
        raise AssertionError("mixed public batch lost fresh/empty EXIF APP1")
    _compare_tiff(native_doc["exif"], generated_doc["exif"])
    native_target, generated_target = _entry(native_doc, target.raw_tag_id), _entry(generated_doc, target.raw_tag_id)
    if (native_target is not None) != spec["target_present"] or native_target != generated_target:
        raise AssertionError("mixed public batch generated target transition differs")
    native_mandatory = _entry(native_doc, mandatory.raw_tag_id)
    generated_mandatory = _entry(generated_doc, mandatory.raw_tag_id)
    if spec["mandatory_state"] == "present" and native_mandatory is None:
        raise AssertionError("authored mandatory override was lost to generated defaults")
    if spec["mandatory_state"] == "absent" and native_mandatory is not None:
        raise AssertionError("authored mandatory removal was overwritten by generated defaults")
    if native_mandatory != generated_mandatory:
        raise AssertionError("mixed public mandatory type/count/value bytes differ")


def run_matrix(*, test_binary: Path, perl: Path, library: Path, output: Path,
               ledger: Path, rules: Path, mandatory_ledger: Path, address_rules: Path) -> dict[str, Any]:
    if not hasattr(native, "run_native_batch"):
        raise RuntimeError("fresh public batch matrix requires the committed native batch oracle helper")
    targets = generated_targets(ledger, rules)
    mandatory = mandatory_ifd0_candidate(mandatory_ledger, address_rules)
    root = output.parent / "fresh-jpeg-public-batch-files"
    root.mkdir(parents=True, exist_ok=False)
    rows, requests = [], []
    for carrier in CARRIERS:
        source = root / f"{carrier.label}-source.jpg"
        write_carrier(source, carrier)
        for target in targets:
            for case in BATCH_CASES:
                spec = batch_case(target, mandatory, case)
                stem = f"{carrier.label}-{target.raw_tag_id:04x}-{case}"
                native_output, generated_output = root / f"{stem}-native.jpg", root / f"{stem}-generated.jpg"
                native_call = native.run_native_batch(perl, library, source, native_output, spec["native"])
                assert_native_batch(native_call, stem, spec["native"])
                requests.append({"route": "public-batch", "carrier": "jpeg", "input": str(source),
                                 "output": str(generated_output), "batch": spec["public"]})
                rows.append({"id": stem, "carrier": asdict(carrier), "case": case,
                             "target": asdict(target), "mandatory": asdict(mandatory), "spec": spec,
                             "source": str(source), "native_output": str(native_output),
                             "output": str(generated_output), "native_call": native_call})
    request_path, result_path = root / "requests.json", root / "results.json"
    request_path.write_text(json.dumps(requests, indent=2) + "\n", encoding="utf-8")
    env = os.environ.copy() | {"OXIDEX_SCALAR_WRITE_REQUESTS": str(request_path),
                               "OXIDEX_SCALAR_WRITE_RESULTS": str(result_path)}
    completed = subprocess.run([str(test_binary), DRIVER, "--exact", "--ignored", "--nocapture"],
                               env=env, text=True, capture_output=True, timeout=180)
    (root / "driver.log").write_text(completed.stdout + completed.stderr, encoding="utf-8")
    completed.check_returncode()
    results = json.loads(result_path.read_text(encoding="utf-8"))
    if not isinstance(results, list) or len(results) != len(rows):
        raise AssertionError("public batch fixture results differ from requests")
    report: dict[str, Any] = {"instrument": "fresh_jpeg_public_batch_matrix_v1", "declared": len(rows),
                              "passed": 0, "mandatory_candidate": asdict(mandatory), "rows": rows}
    for row, result in zip(rows, results, strict=True):
        row["driver_result"] = result
        try:
            if not isinstance(result, dict) or result.get("output") != row["output"] or result.get("ok") is not True:
                raise AssertionError(f"public batch refused or failed: {result}")
            compare_batch(Path(row["source"]), Path(row["native_output"]), Path(row["output"]),
                          GeneratedTarget(**row["target"]), MandatoryCandidate(**row["mandatory"]), row["spec"])
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
    parser.add_argument("--ledger", type=Path, default=LEDGER)
    parser.add_argument("--rules", type=Path, default=RULES)
    parser.add_argument("--mandatory-ledger", type=Path, default=MANDATORY_LEDGER)
    parser.add_argument("--address-rules", type=Path, default=ADDRESS_RULES)
    args = parser.parse_args(argv)
    state = git_state(ROOT)
    overridden = refuse_if_dirty(state, "fresh_jpeg_public_batch_matrix")
    binary = resolve_binary(args.test_binary, "oxidex-lib-test")
    if note := staleness_note(binary, state):
        raise RuntimeError(note)
    perl, library = native.resolve_perl(args.perl), native.resolve_library(args.lib)
    identity = native.native_identity(perl, library)
    native.assert_contract_version(identity)
    targets = generated_targets(args.ledger, args.rules)
    declared = len(CARRIERS) * len(targets) * len(BATCH_CASES)
    print_header(tool="fresh_jpeg_public_batch_matrix_v1", git=state, binary=binary,
                 dirty_overridden=overridden,
                 extra=[f"native: {identity}", f"{declared} fresh/empty public batches"])
    report = run_matrix(test_binary=binary.path, perl=perl, library=library, output=args.output,
                        ledger=args.ledger, rules=args.rules, mandatory_ledger=args.mandatory_ledger,
                        address_rules=args.address_rules)
    report.update({"source_commit": state.commit, "native_identity": identity,
                   "test_binary_sha256": hashlib.sha256(binary.path.read_bytes()).hexdigest(),
                   "ledger_sha256": hashlib.sha256(args.ledger.read_bytes()).hexdigest()})
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"Fresh/empty public mandatory batches matched: {report['passed']}/{report['declared']}")
    return 0 if report["passed"] == report["declared"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
