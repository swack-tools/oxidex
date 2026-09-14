#!/usr/bin/env python3
"""Compare the internal generated TIFF writer with actual pinned native writes.

Supply the lib-test executable built from this checkout. This does not certify
public writer routing, creation of a new JPEG EXIF block, other tags, or other releases.
"""
import argparse
import hashlib
import json
import os
from dataclasses import dataclass
from pathlib import Path
import subprocess
import sys
import re

import native_write_matrix as native

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from instrument import git_state, print_header, refuse_if_dirty, resolve_binary, staleness_note

DRIVER = "writers::tiff_surgical::generated_scalar::tests::generated_scalar_fixture_driver"
LEDGER = ROOT / "tools/exiftool-tables/tiff_scalar_final_ledger.json"
RULES = ROOT / "src/writers/generated_tiff_scalar_final_rules.rs"


@dataclass(frozen=True)
class GeneratedTarget:
    raw_tag_id: int
    name: str
    table_group0: str
    physical_write_group: str

    @property
    def qualifiers(self) -> tuple[str, str]:
        return (f"{self.table_group0}:{self.name}", f"{self.physical_write_group}:{self.name}")


def generated_rule_targets(rules_path: Path = RULES) -> tuple[GeneratedTarget, ...]:
    """Extract the exact static final-rule identities rendered into Rust."""
    try:
        source = rules_path.read_text(encoding="utf-8")
    except OSError as error:
        raise ValueError(f"generated final-stage Rust rules are unavailable: {error}") from error
    pattern = re.compile(
        r"TiffScalarFinalStageRecipe \{\s*module: \"(?P<module>[^\"]+)\",\s*"
        r"table: \"(?P<table>[^\"]+)\",\s*full_name: \"(?P<full>[^\"]+)\",\s*"
        r"raw_tag_id: 0x(?P<id>[0-9a-f]+),\s*tag_name: \"(?P<name>[^\"]+)\",\s*"
        r"table_group0: \"(?P<table_group>[^\"]+)\",\s*physical_write_group: \"(?P<physical_group>[^\"]+)\",",
        re.DOTALL,
    )
    targets = []
    for match in pattern.finditer(source):
        if (match["module"], match["table"], match["full"]) != ("Exif", "Main", "Image::ExifTool::Exif::Main"):
            raise ValueError("generated final-stage Rust rule is outside the joined EXIF main cohort")
        targets.append(GeneratedTarget(int(match["id"], 16), match["name"], match["table_group"], match["physical_group"]))
    if not targets:
        raise ValueError("generated final-stage Rust rules have no final recipes")
    if len(set(targets)) != len(targets):
        raise ValueError("generated final-stage Rust rules have duplicate recipe identity")
    return tuple(sorted(targets, key=lambda target: target.raw_tag_id))


def generated_targets(ledger_path: Path = LEDGER, rules_path: Path = RULES) -> tuple[GeneratedTarget, ...]:
    """Read emitted source identities; this instrument owns no tag allowlist."""
    try:
        ledger = json.loads(ledger_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"generated final-stage ledger is unavailable: {error}") from error
    if ledger.get("emitted") is not True or ledger.get("reason") is not None:
        raise ValueError("generated final-stage ledger did not emit a cohort")
    recipes = ledger.get("recipes")
    if not isinstance(recipes, list) or not recipes:
        raise ValueError("generated final-stage ledger has no recipes")
    targets, identities = [], set()
    for recipe in recipes:
        if not isinstance(recipe, dict):
            raise ValueError("generated final-stage ledger recipe is malformed")
        raw_id, name = recipe.get("raw_tag_id"), recipe.get("name")
        table_group, physical_group = recipe.get("table_group0"), recipe.get("physical_write_group")
        if (type(raw_id) is not int or not 0 <= raw_id <= 0xffff
                or not all(isinstance(value, str) and value for value in (name, table_group, physical_group))):
            raise ValueError("generated final-stage ledger recipe identity is malformed")
        if (recipe.get("module"), recipe.get("table"), recipe.get("full_name")) != ("Exif", "Main", "Image::ExifTool::Exif::Main"):
            raise ValueError("generated final-stage ledger recipe is outside the joined EXIF main cohort")
        identity = (raw_id, name, table_group, physical_group)
        if identity in identities:
            raise ValueError("generated final-stage ledger has duplicate recipe identity")
        identities.add(identity)
        targets.append(GeneratedTarget(*identity))
    result = tuple(sorted(targets, key=lambda target: target.raw_tag_id))
    if result != generated_rule_targets(rules_path):
        raise ValueError("generated final-stage ledger and Rust rule identities differ")
    return result


def compare(seed, expected, actual, target_tag_id: int | None):
    if seed["image_payload_hex"] != actual["image_payload_hex"]:
        raise AssertionError("generated write changed image payload")
    if expected["image_payload_hex"] != actual["image_payload_hex"]:
        raise AssertionError("native/generated image payload differs")
    if expected["byte_order"] != actual["byte_order"]:
        raise AssertionError("native/generated TIFF byte order differs")
    if set(expected["tags"]) != set(actual["tags"]):
        raise AssertionError("native/generated tag identities differ")
    expected_children, actual_children = expected.get("children", {}), actual.get("children", {})
    if expected_children.keys() != actual_children.keys() or seed.get("children", {}).keys() != actual_children.keys():
        raise AssertionError("native/generated or preserved TIFF directory identities differ")
    for tag, value in expected["tags"].items():
        # Native may relocate the strip; its actual bytes are checked above.
        if tag == "273" and any(value[key] != actual["tags"][tag][key] for key in ("type", "count")):
            raise AssertionError("native/generated strip pointer type/count differs")
        pointer_name = native.IFD_POINTERS.get(tag)
        if pointer_name in expected_children:
            if any(value[key] != actual["tags"][tag][key] for key in ("type", "count")):
                raise AssertionError(f"native/generated {pointer_name} pointer type/count differs")
            # Compare the target directory below, never its physical offset.
            continue
        if tag != "273" and native.entry_storage(value) != native.entry_storage(actual["tags"][tag]):
            raise AssertionError(f"native/generated unrelated tag type/count/value differs for {tag}")
    for tag, value in seed["tags"].items():
        if (target_tag_id is None or tag != str(target_tag_id)) and native.entry_storage(value) != native.entry_storage(actual["tags"].get(tag)):
            raise AssertionError(f"generated write changed unrelated tag {tag}")
    for name, child in expected_children.items():
        compare(seed["children"][name], child, actual_children[name], None)


def compare_carrier(seed, expected, actual, carrier, target_tag_id: int):
    if carrier != "jpeg":
        return compare(seed, expected, actual, target_tag_id)
    for key in ("sos_to_end_sha256", "non_exif_sha256"):
        if actual[key] != seed[key]:
            raise AssertionError(f"generated JPEG changed {key}")
    if actual["sos_to_end_sha256"] != expected["sos_to_end_sha256"]:
        raise AssertionError("native/generated JPEG image payload differs")
    if any(document["exif"] is None for document in (seed, expected, actual)):
        raise AssertionError("JPEG operation lost its EXIF block")
    compare(seed["exif"], expected["exif"], actual["exif"], target_tag_id)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-binary", required=True)
    parser.add_argument("--perl", type=Path, required=True)
    parser.add_argument("--lib", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--jpeg-base", type=Path, help="also exercise generated JPEG cohort operations")
    parser.add_argument("--ledger", type=Path, default=LEDGER, help="emitted final-stage ledger")
    parser.add_argument("--rules", type=Path, default=RULES, help="rendered final-stage Rust rules")
    args = parser.parse_args()
    carriers = ("tiff_little", "tiff_big") + (("jpeg",) if args.jpeg_base else ())
    targets = generated_targets(args.ledger, args.rules)
    declared = len(carriers) * sum(len(target.qualifiers) for target in targets) * len(native.CASES)
    if args.jpeg_base and not args.jpeg_base.is_file():
        parser.error("JPEG base is not a file")
    state = git_state(ROOT)
    overridden = refuse_if_dirty(state, "generated_tiff_write_matrix")
    binary = resolve_binary(args.test_binary, "oxidex-lib-test")
    if note := staleness_note(binary, state):
        raise RuntimeError(note)
    perl, library = native.resolve_perl(args.perl), native.resolve_library(args.lib)
    identity = native.native_identity(perl, library)
    native.assert_contract_version(identity)
    print_header(tool="generated_scalar_write_matrix_v2", git=state, binary=binary,
                 dirty_overridden=overridden,
                 extra=[f"native: {identity}", f"{declared} internal TIFF/JPEG operations; public routing not covered"])
    root = args.output.parent / "generated-tiff-matrix-files"
    root.mkdir(parents=True, exist_ok=False)
    rows, requests = [], []
    for carrier in carriers:
        for target in targets:
            for name in target.qualifiers:
                for operation in native.CASES:
                    stem = f"{carrier}-{target.raw_tag_id:04x}-{name.replace(':', '_')}-{operation}"
                    suffix = ".jpg" if carrier == "jpeg" else ".tif"
                    source, seeded, expected, output = (root / f"{stem}-{part}{suffix}" for part in ("source", "seeded", "native", "generated"))
                    native.make_carrier(source, carrier, args.jpeg_base)
                    seed_action = "seed_artist" if operation == "insert" else "seed_artist_target"
                    seed_call = native.run_native(perl, library, source, seeded, seed_action, None if operation == "insert" else name)
                    native.assert_native(seed_call, stem + " seed")
                    operation_call = native.run_native(perl, library, seeded, expected, operation, name)
                    native.assert_native(operation_call, stem + " operation")
                    scalar = "undefined" if operation == "delete" else "utf8" if operation == "utf8" else "bytes"
                    value = None if operation == "delete" else native.CASE_INPUT_BYTES[operation].decode("utf-8") if scalar == "utf8" else native.CASE_INPUT_BYTES[operation].hex()
                    requests.append({"carrier": carrier, "input": str(seeded), "output": str(output), "key": name, "scalar": scalar, "value": value})
                    rows.append({"carrier": carrier, "id": stem, "target": {"raw_tag_id": target.raw_tag_id, "name": target.name, "table_group0": target.table_group0, "physical_write_group": target.physical_write_group}, "requested_name": name, "operation": operation, "seeded": str(seeded), "native_output": str(expected), "output": str(output), "native_call": operation_call})
    request_path, result_path = root / "requests.json", root / "results.json"
    request_path.write_text(json.dumps(requests, indent=2) + "\n")
    env = os.environ.copy()
    env.update(OXIDEX_SCALAR_WRITE_REQUESTS=str(request_path), OXIDEX_SCALAR_WRITE_RESULTS=str(result_path))
    result = subprocess.run([str(binary.path), DRIVER, "--exact", "--ignored", "--nocapture"], env=env, capture_output=True, text=True, timeout=120)
    (root / "driver.log").write_text(result.stdout + result.stderr)
    result.check_returncode()
    results = json.loads(result_path.read_text())
    report = {"instrument": "generated_scalar_write_matrix_v2", "native_identity": identity,
              "source_commit": state.commit, "dirty_files": state.dirty_files,
              "test_binary_sha256": hashlib.sha256(binary.path.read_bytes()).hexdigest(),
              "ledger_sha256": hashlib.sha256(args.ledger.read_bytes()).hexdigest(),
              "rules_sha256": hashlib.sha256(args.rules.read_bytes()).hexdigest(),
              "cohort": [{"raw_tag_id": target.raw_tag_id, "name": target.name,
                          "table_group0": target.table_group0, "physical_write_group": target.physical_write_group,
                          "qualifiers": list(target.qualifiers)} for target in targets],
              "declared": declared, "passed": 0, "rows": rows,
              "limitations": ["Internal composition only; public writer routing and new JPEG EXIF blocks remain untested.", "Generated final-scalar ledger cohort only, selected 13.59 only; this does not establish public SetNewValue admission."]}
    if len(results) != len(requests) or len(requests) != declared:
        raise AssertionError("fixture driver result population differs")
    for row, result in zip(rows, results, strict=True):
        row["driver_result"] = result
        try:
            if result.get("output") != row["output"] or not result.get("ok") or result.get("warnings"):
                raise AssertionError(f"generated writer did not succeed: {result}")
            seed, expected, actual = (native.inspect(Path(row[key]), row["carrier"]) for key in ("seeded", "native_output", "output"))
            native.assert_target_transition(seed, expected, row["carrier"], row["target"]["raw_tag_id"], row["operation"])
            compare_carrier(seed, expected, actual, row["carrier"], row["target"]["raw_tag_id"])
            row["state"] = "passed"
            report["passed"] += 1
        except (AssertionError, ValueError, OSError) as error:
            row.update(state="failed", error=str(error))
        args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(f"Internal scalar write operations matched: {report['passed']}/{declared}")
    return 0 if report["passed"] == declared else 1


if __name__ == "__main__":
    raise SystemExit(main())
