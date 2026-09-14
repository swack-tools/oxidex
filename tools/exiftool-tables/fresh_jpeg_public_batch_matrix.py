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
    """One source-default operand with a native-authorized user override."""

    raw_tag_id: int
    name: str
    group0: str
    source_write_group: str
    directory: str
    default_value: int
    override_value: int

    @property
    def qualifier(self) -> str:
        # The `%mandatory` directory is where WriteExif applies the default;
        # `WriteGroup` is only a preferred address in the source table.
        return f"{self.directory}:{self.name}"


def mandatory_legacy_candidates(ledger_path: Path = MANDATORY_LEDGER,
                                address_path: Path = ADDRESS_RULES) -> tuple[MandatoryCandidate, ...]:
    """Join every integer default to its selected native table identity.

    An override value is a *different* integer default from the same source
    directory.  It is source data, never a handwritten expected default.  The
    later native probe selects only an operand that SetNewValue accepts.
    """
    ledger = json.loads(ledger_path.read_text(encoding="utf-8"))
    recipe = ledger.get("recipe")
    if ledger.get("writer_tables_joined") is not True or not isinstance(recipe, dict):
        raise ValueError("mandatory default ledger is unresolved")
    directories = recipe.get("directories")
    if not isinstance(directories, list):
        raise ValueError("mandatory default directories are unavailable")
    address_rows = []
    for match in _ADDRESS.finditer(address_path.read_text(encoding="utf-8")):
        values = match.groupdict()
        try:
            raw_id = int(values["raw_id"], 0)
        except ValueError:
            continue
        if ((values["module"], values["table"], values["full_name"]) ==
                ("Exif", "Main", "Image::ExifTool::Exif::Main")):
            address_rows.append((raw_id, values))
    candidates: list[MandatoryCandidate] = []
    for directory in directories:
        if not isinstance(directory, dict) or not isinstance(directory.get("directory"), str):
            raise ValueError("mandatory default directory is malformed")
        defaults = directory.get("defaults")
        if not isinstance(defaults, list):
            raise ValueError("mandatory default entries are unavailable")
        integers = [item for item in defaults if isinstance(item, dict)
                    and type(item.get("tag_id")) is int and type(item.get("value")) is int
                    and item.get("kind") == "Integer"]
        values = sorted({item["value"] for item in integers})
        for item in integers:
            alternatives = [value for value in values if value != item["value"]]
            if not alternatives:
                continue
            rows = [values for raw_id, values in address_rows if raw_id == item["tag_id"]]
            if len(rows) > 1:
                raise ValueError("mandatory default address is ambiguous")
            if not rows:
                continue
            row = rows[0]
            candidates.append(MandatoryCandidate(
                item["tag_id"], row["name"], row["group0"], row["write_group"],
                directory["directory"], item["value"], alternatives[0],
            ))
    if not candidates:
        raise ValueError("mandatory defaults have no source-addressed integer override candidates")
    return tuple(sorted(candidates, key=lambda item: (
        item.directory, item.raw_tag_id, item.name, item.override_value,
    )))


def jfif_adjusted_candidates(carrier: Any, ledger_path: Path = MANDATORY_LEDGER,
                             address_path: Path = ADDRESS_RULES) -> tuple[MandatoryCandidate, ...]:
    """Derive IFD0 JFIF-adjusted mandatory defaults for one raw JFIF carrier."""
    if getattr(carrier, "jfif", None) is None or carrier.jfif.unit is None:
        return ()
    recipe = json.loads(ledger_path.read_text(encoding="utf-8")).get("recipe")
    override = recipe.get("jfif_override") if isinstance(recipe, dict) else None
    if not isinstance(override, dict) or override.get("directory") != "IFD0":
        raise ValueError("JFIF mandatory override is unresolved")
    raw = carrier.jfif
    props = {"JFIFXResolution": raw.x_density, "JFIFYResolution": raw.y_density,
             "JFIFResolutionUnit": raw.unit}
    assignments = override.get("assignments")
    if not isinstance(assignments, list):
        raise ValueError("JFIF mandatory assignments are unavailable")
    derived = []
    for item in assignments:
        if not (isinstance(item, list) and len(item) == 3 and type(item[0]) is int
                and isinstance(item[1], str) and type(item[2]) is int and item[1] in props):
            raise ValueError("JFIF mandatory assignment is malformed")
        derived.append((item[0], item[1], props[item[1]] + item[2]))
    rows = {}
    for match in _ADDRESS.finditer(address_path.read_text(encoding="utf-8")):
        row = match.groupdict()
        try: raw_id = int(row["raw_id"], 0)
        except ValueError: continue
        if (row["module"], row["table"], row["full_name"]) == ("Exif", "Main", "Image::ExifTool::Exif::Main"):
            rows.setdefault(raw_id, []).append(row)
    candidates=[]
    for raw_id, _property, default in derived:
        matches=rows.get(raw_id, [])
        if len(matches) > 1: raise ValueError("JFIF mandatory address is ambiguous")
        if not matches: continue
        alternatives=[value for other_id, _prop, value in derived if other_id != raw_id and value != default]
        if not alternatives: continue
        row=matches[0]
        candidates.append(MandatoryCandidate(raw_id,row['name'],row['group0'],row['write_group'],"IFD0",default,alternatives[0]))
    return tuple(sorted(candidates,key=lambda item:(item.raw_tag_id,item.name,item.override_value)))


def select_native_jfif_candidate(*, perl: Path, library: Path, root: Path, target: GeneratedTarget,
                                 carrier: Any, ledger_path: Path, address_path: Path) -> tuple[MandatoryCandidate, list[dict[str, Any]]]:
    """Choose an executable IFD0 adjusted default using the raw JFIF carrier."""
    probe_root=root/f"jfif-selection-{carrier.label}"; probe_root.mkdir()
    source=probe_root/'source.jpg'; write_carrier(source,carrier); probes=[]
    for index,candidate in enumerate(jfif_adjusted_candidates(carrier,ledger_path,address_path)):
        set_spec=[native_item(target.qualifiers[0],"utf8","batch-target"),native_item(candidate.qualifier,"utf8",str(candidate.override_value))]
        delete_spec=[native_item(target.qualifiers[0],"utf8","batch-target"),native_item(candidate.qualifier,"undefined")]
        set_call=native.run_native_batch(perl,library,source,probe_root/f'{index}-set.jpg',set_spec)
        delete_call=native.run_native_batch(perl,library,source,probe_root/f'{index}-delete.jpg',delete_spec)
        accepted=all(call.get('returncode')==0 and isinstance(call.get('result'),dict) and call['result'].get('write_return')==1 and all(x.get('return') in (1,2) for x in call['result'].get('set_calls',[])) for call in (set_call,delete_call))
        probes.append({'candidate':asdict(candidate),'set':set_call,'delete':delete_call,'accepted':accepted})
        if accepted:return candidate,probes
    raise ValueError(f'no source-addressed JFIF mandatory default accepted override/deletion for {carrier.label}')


def select_native_mandatory_candidate(*, perl: Path, library: Path, root: Path,
                                      target: GeneratedTarget, ledger_path: Path = MANDATORY_LEDGER,
                                      address_path: Path = ADDRESS_RULES) -> tuple[MandatoryCandidate, list[dict[str, Any]]]:
    """Select a default only when native SetNewValue accepts both operations."""
    probe_root = root / "mandatory-selection"
    probe_root.mkdir()
    source = probe_root / "fresh-source.jpg"
    write_carrier(source, CARRIERS[0])
    probes: list[dict[str, Any]] = []
    for index, candidate in enumerate(mandatory_legacy_candidates(ledger_path, address_path)):
        set_spec = [
            native_item(target.qualifiers[0], "utf8", "batch-target"),
            native_item(candidate.qualifier, "utf8", str(candidate.override_value)),
        ]
        delete_spec = [
            native_item(target.qualifiers[0], "utf8", "batch-target"),
            # This source-independent legacy edit makes the candidate's
            # directory exist, so the requested deletion must win over its
            # generated mandatory defaults.
            native_item(f"{candidate.directory}:Artist", "utf8", "batch-artist"),
            native_item(candidate.qualifier, "undefined"),
        ]
        set_call = native.run_native_batch(perl, library, source, probe_root / f"{index}-set.jpg", set_spec)
        delete_call = native.run_native_batch(perl, library, source, probe_root / f"{index}-delete.jpg", delete_spec)
        accepted = all(
            call.get("returncode") == 0
            and isinstance(call.get("result"), dict)
            and call["result"].get("write_return") == 1
            and all(item.get("return") in (1, 2) for item in call["result"].get("set_calls", []))
            for call in (set_call, delete_call)
        )
        probes.append({"candidate": asdict(candidate), "set": set_call, "delete": delete_call,
                       "accepted": accepted})
        if accepted:
            return candidate, probes
    raise ValueError("no source-addressed mandatory default accepted both native override and deletion")


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
            "native": [native_item(generated_name, "utf8", "batch-target"),
                       native_item(mandatory.qualifier, "utf8", str(mandatory.override_value))],
            "public": [public_item(generated_name, "utf8", "batch-target"),
                       public_item(mandatory.qualifier, "integer", str(mandatory.override_value))],
            "target_present": True,
            "mandatory_state": "present",
        }
    if case == "generated-set-mandatory-delete":
        return {
            "native": [native_item(generated_name, "utf8", "batch-target"),
                       native_item(f"{mandatory.directory}:Artist", "utf8", "batch-artist"),
                       native_item(mandatory.qualifier, "undefined")],
            "public": [public_item(generated_name, "utf8", "batch-target"),
                       public_item(f"{mandatory.directory}:Artist", "utf8", "batch-artist"),
                       public_item(mandatory.qualifier, "undefined")],
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
        if observed.get("tag") != planned["tag"]:
            raise AssertionError(f"{label}: native batch operand order/name differs: {observed.get('tag')!r} != {planned['tag']!r}")
        if observed.get("return") not in (1, 2):
            raise AssertionError(
                f"{label}: native batch operand was rejected for {planned['tag']!r}: "
                f"return={observed.get('return')!r}; stderr={call.get('stderr', '').strip()!r}; "
                f"error={result.get('error')!r}"
            )
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


def _entry(document: dict[str, Any], raw_tag_id: int, directory: str = "IFD0") -> dict[str, Any] | None:
    exif = document["exif"]
    if exif is None:
        return None
    if directory == "IFD0":
        tree = exif
    elif directory == "IFD1":
        # `native.parse_jpeg` names TIFF's next top-level IFD `NextIFD`.
        # This is a parser-tree location, not a tag-name or ID allowlist.
        tree = exif.get("children", {}).get("NextIFD")
    else:
        raise ValueError(f"mandatory candidate directory is outside parsed JPEG scope: {directory}")
    return None if tree is None else tree["tags"].get(str(raw_tag_id))


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
    if ((native_target is not None) != spec["target_present"]
            or native.entry_storage(native_target) != native.entry_storage(generated_target)):
        raise AssertionError("mixed public batch generated target transition differs")
    native_mandatory = _entry(native_doc, mandatory.raw_tag_id, mandatory.directory)
    generated_mandatory = _entry(generated_doc, mandatory.raw_tag_id, mandatory.directory)
    if spec["mandatory_state"] == "present" and native_mandatory is None:
        raise AssertionError("authored mandatory override was lost to generated defaults")
    if spec["mandatory_state"] == "absent" and native_mandatory is not None:
        raise AssertionError("authored mandatory removal was overwritten by generated defaults")
    if native.entry_storage(native_mandatory) != native.entry_storage(generated_mandatory):
        raise AssertionError("mixed public mandatory type/count/value bytes differ")


def run_matrix(*, test_binary: Path, perl: Path, library: Path, output: Path,
               ledger: Path, rules: Path, mandatory_ledger: Path, address_rules: Path) -> dict[str, Any]:
    if not hasattr(native, "run_native_batch"):
        raise RuntimeError("fresh public batch matrix requires the committed native batch oracle helper")
    all_targets = generated_targets(ledger, rules)
    targets = tuple(target for target in all_targets if target.case_family == "native_string_scalar")
    root = output.parent / "fresh-jpeg-public-batch-files"
    root.mkdir(parents=True, exist_ok=False)
    # Candidate admission is a source/native fact: Protected and PrintConv
    # controls may reject a mandatory table row even though WriteExif uses it.
    mandatory, mandatory_probes = select_native_mandatory_candidate(
        perl=perl, library=library, root=root, target=targets[0],
        ledger_path=mandatory_ledger, address_path=address_rules,
    )
    jfif_candidates: dict[str, MandatoryCandidate] = {}
    jfif_probes: dict[str, list[dict[str, Any]]] = {}
    for carrier in CARRIERS:
        if carrier.jfif.unit is not None:
            candidate, probes = select_native_jfif_candidate(
                perl=perl, library=library, root=root, target=targets[0], carrier=carrier,
                ledger_path=mandatory_ledger, address_path=address_rules,
            )
            jfif_candidates[carrier.label], jfif_probes[carrier.label] = candidate, probes
    rows, requests = [], []
    for carrier in CARRIERS:
        source = root / f"{carrier.label}-source.jpg"
        write_carrier(source, carrier)
        cohorts = [(mandatory, BATCH_CASES, "lexical")]
        if carrier.label in jfif_candidates:
            cohorts.append((jfif_candidates[carrier.label], BATCH_CASES[1:], "jfif-adjusted"))
        for target in targets:
            for active_mandatory, cases, source_kind in cohorts:
                for case in cases:
                    spec = batch_case(target, active_mandatory, case)
                    stem = f"{carrier.label}-{target.raw_tag_id:04x}-{source_kind}-{active_mandatory.raw_tag_id:04x}-{case}"
                    native_output, generated_output = root / f"{stem}-native.jpg", root / f"{stem}-generated.jpg"
                    native_call = native.run_native_batch(perl, library, source, native_output, spec["native"])
                    assert_native_batch(native_call, stem, spec["native"])
                    requests.append({"route": "public-batch", "carrier": "jpeg", "input": str(source),
                                     "output": str(generated_output), "batch": spec["public"]})
                    rows.append({"id": stem, "carrier": asdict(carrier), "case": case,
                                 "target": asdict(target), "mandatory": asdict(active_mandatory), "mandatory_source": source_kind, "spec": spec,
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
    report: dict[str, Any] = {"instrument": "fresh_jpeg_public_batch_matrix_v2", "declared": len(rows),
        "anchor_case_family": "native_string_scalar",
        "numeric_cohort_instrument": "generated_scalar_write_matrix_v4",
        "non_anchor_source_targets": [asdict(target) for target in all_targets if target.case_family != "native_string_scalar"],
                              "passed": 0, "mandatory_candidate": asdict(mandatory),
                              "mandatory_selection_probes": mandatory_probes,
                              "jfif_adjusted_candidates": {key: asdict(value) for key, value in jfif_candidates.items()},
                              "jfif_adjusted_selection_probes": jfif_probes, "rows": rows}
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
    all_targets = generated_targets(args.ledger, args.rules)
    targets = tuple(target for target in all_targets if target.case_family == "native_string_scalar")
    declared = len(CARRIERS) * len(targets) * len(BATCH_CASES) + sum(
        2 * len(targets) for carrier in CARRIERS if carrier.jfif.unit is not None
    )
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
