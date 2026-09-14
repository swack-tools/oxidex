#!/usr/bin/env python3
"""Compare generated TIFF/JPEG writes with actual pinned native writes.

Supply the lib-test executable built from this checkout. Select --route public-api to exercise public modify/remove operations.
This does not certify new JPEG EXIF blocks, other tags, or other releases.
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


def selected_rehearsal_contract(identity: dict, release: str, pin_file: Path, ledger_path: Path) -> dict:
    """Bind an opt-in historical rehearsal without changing the 13.59 gate.

    This is deliberately separate from native.assert_contract_version(): the
    normal command remains the reviewed 13.59 acceptance baseline. Historical
    execution instead requires one selected release to agree across the owned
    checkout pin, live native identity, and freshly regenerated writer ledger.
    """
    if not isinstance(release, str) or not re.fullmatch(r"[0-9]+\.[0-9]+", release):
        raise ValueError("selected rehearsal release is malformed")
    try:
        pin = pin_file.read_text(encoding="utf-8").strip()
        ledger = json.loads(ledger_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError("selected rehearsal pin or ledger is unreadable") from error
    if pin != release:
        raise ValueError("owned checkout pin differs from selected rehearsal release")
    if identity.get("result", {}).get("exiftool_version") != release:
        raise ValueError("selected native identity differs from rehearsal release")
    if ledger.get("exiftool_version") != release:
        raise ValueError("generated final-stage ledger differs from rehearsal release")
    if ledger.get("emitted") is not True or ledger.get("reason") is not None:
        raise ValueError("generated final-stage ledger did not emit a rehearsal cohort")
    return {"mode": "selected-release-rehearsal", "release": release, "pin": str(pin_file.resolve()),
            "pin_sha256": hashlib.sha256(pin_file.read_bytes()).hexdigest(),
            "ledger_exiftool_version": ledger["exiftool_version"]}


def changed_tag_ids(target_tag_id: int | set[int] | frozenset[int] | None) -> set[str]:
    """Normalize a source-derived changed physical identity set for comparison."""
    if target_tag_id is None:
        return set()
    if isinstance(target_tag_id, int):
        return {str(target_tag_id)}
    if isinstance(target_tag_id, (set, frozenset)) and all(type(value) is int and value >= 0 for value in target_tag_id):
        return {str(value) for value in target_tag_id}
    raise ValueError("changed target identities are malformed")


def compare(seed, expected, actual, target_tag_id: int | set[int] | frozenset[int] | None):
    changed = changed_tag_ids(target_tag_id)
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
        if tag not in changed and native.entry_storage(value) != native.entry_storage(actual["tags"].get(tag)):
            raise AssertionError(f"generated write changed unrelated tag {tag}")
    for name, child in expected_children.items():
        compare(seed["children"][name], child, actual_children[name], None)


def compare_carrier(seed, expected, actual, carrier, target_tag_id: int | set[int] | frozenset[int] | None):
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
    parser.add_argument("--rehearsal-release", help="opt-in selected native release for version rehearsal")
    parser.add_argument("--rehearsal-pin", type=Path, help="owned checkout .exiftool-version for rehearsal")
    parser.add_argument("--route", choices=("final-key", "resolved-address", "public-api"), default="final-key",
                        help="dispatch path exercised; public-api calls public modify_tag/remove_tag")
    args = parser.parse_args()
    if (args.rehearsal_release is None) != (args.rehearsal_pin is None):
        parser.error("--rehearsal-release and --rehearsal-pin must be supplied together")
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
    contract = (selected_rehearsal_contract(identity, args.rehearsal_release, args.rehearsal_pin, args.ledger)
                if args.rehearsal_release is not None else None)
    if contract is None:
        native.assert_contract_version(identity)
        contract = {"mode": "pinned-13.59-contract", "release": native.CONTRACT_EXIFTOOL_RELEASE}
    print_header(tool="generated_scalar_write_matrix_v2", git=state, binary=binary,
                 dirty_overridden=overridden,
                 extra=[f"native: {identity}", f"contract: {contract['mode']} {contract['release']}",
                        f"{declared} TIFF/JPEG operations via {args.route}; existing EXIF blocks"])
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
                    requests.append({"route": args.route, "carrier": carrier, "input": str(seeded), "output": str(output), "key": name, "scalar": scalar, "value": value})
                    rows.append({"carrier": carrier, "id": stem, "target": {"raw_tag_id": target.raw_tag_id, "name": target.name, "table_group0": target.table_group0, "physical_write_group": target.physical_write_group}, "requested_name": name, "operation": operation, "seeded": str(seeded), "native_output": str(expected), "output": str(output), "native_call": operation_call})
    request_path, result_path = root / "requests.json", root / "results.json"
    request_path.write_text(json.dumps(requests, indent=2) + "\n")
    env = os.environ.copy()
    env.update(OXIDEX_SCALAR_WRITE_REQUESTS=str(request_path), OXIDEX_SCALAR_WRITE_RESULTS=str(result_path))
    result = subprocess.run([str(binary.path), DRIVER, "--exact", "--ignored", "--nocapture"], env=env, capture_output=True, text=True, timeout=120)
    (root / "driver.log").write_text(result.stdout + result.stderr)
    result.check_returncode()
    results = json.loads(result_path.read_text())
    report = {"instrument": "generated_scalar_write_matrix_v2", "route": args.route, "native_identity": identity,
              "contract": contract,
              "source_commit": state.commit, "dirty_files": state.dirty_files,
              "test_binary_sha256": hashlib.sha256(binary.path.read_bytes()).hexdigest(),
              "ledger_sha256": hashlib.sha256(args.ledger.read_bytes()).hexdigest(),
              "rules_sha256": hashlib.sha256(args.rules.read_bytes()).hexdigest(),
              "cohort": [{"raw_tag_id": target.raw_tag_id, "name": target.name,
                          "table_group0": target.table_group0, "physical_write_group": target.physical_write_group,
                          "qualifiers": list(target.qualifiers)} for target in targets],
              "declared": declared, "passed": 0, "rows": rows,
              "limitations": ["New JPEG EXIF blocks and empty existing IFDs remain untested; public modify/remove covered only with route public-api.",
                              "Generated final-scalar ledger cohort only; this does not establish public SetNewValue admission.",
                              f"Native contract mode: {contract['mode']} for release {contract['release']}."]}
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
    print(f"Scalar write operations matched via {args.route}: {report['passed']}/{declared}")
    return 0 if report["passed"] == declared else 1


if __name__ == "__main__":
    raise SystemExit(main())
