#!/usr/bin/env python3
"""Compare generated TIFF/JPEG writes with actual pinned native writes.

Supply the lib-test executable built from this checkout. Select --route public-api to exercise public modify/remove operations.
This does not certify new JPEG EXIF blocks, other tags, or other releases.
"""
import argparse
import hashlib
import json
import os
from dataclasses import asdict, dataclass
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
MANDATORY_LEDGER = ROOT / "tools/exiftool-tables/mandatory_defaults_ledger.json"
ADDRESS_LEDGER = ROOT / "tools/exiftool-tables/setnewvalue_address_ledger.json"
PUBLIC_MIGRATION_LEDGER = ROOT / "tools/exiftool-tables/setnewvalue_public_migration_ledger.json"


@dataclass(frozen=True)
class GeneratedTarget:
    raw_tag_id: int
    name: str
    table_group0: str
    physical_write_group: str
    wire_format: str = "string"

    @property
    def case_family(self) -> str:
        if self.wire_format in ("string", "undef"):
            return "native_string_scalar"
        if self.wire_format in ("int16u", "rational64u"):
            return "native_unsigned_numeric_scalar"
        raise ValueError("generated target has no acceptance case family: " + self.wire_format)

    @property
    def qualifiers(self) -> tuple[str, str]:
        return (f"{self.table_group0}:{self.name}", f"{self.physical_write_group}:{self.name}")


def explicit_directory_operands(
    rules_path: Path = ROOT / "src/writers/generated_scalar_rules.rs",
    ledger_path: Path = ROOT / "tools/exiftool-tables/scalar_helper_ledger.json",
    address_path: Path = ROOT / "src/writers/generated_setnewvalue_address_rules.rs",
) -> tuple[str, ...]:
    """Read the executable operand and require its source identity joins."""
    fact = json.loads(ledger_path.read_text())["helpers"]["explicit_directories"]
    if fact.get("state") != "compiled":
        raise ValueError("explicit directory recipe was not compiled")
    source = rules_path.read_text()
    match = re.search(r'PUBLIC_SET_NEW_VALUE_CALLER:.*?directories:\s*&\[([^]]*)\].*?capture:.*?\{([^}]+)\}', source, re.S)
    if not match:
        raise ValueError("rendered explicit directory recipe is absent")
    directories = tuple(re.findall(r'"([^"\\]+)"', match[1]))
    if directories != tuple(fact.get("directories", ())):
        raise ValueError("explicit directory ledger and Rust operands differ")
    address = address_path.read_text().split("const SET_NEW_VALUE_ADDRESS_CAPTURE:", 1)[1]
    for field, value in fact["source_capture_identity"].items():
        for body in (match[2], address):
            emitted = re.search(r'\b' + field + r':\s*"([^"]+)"', body)
            if emitted is None or emitted[1] != value:
                raise ValueError("explicit directory and address source captures differ")
    return directories


def selected_qualifiers(target: GeneratedTarget, directories: tuple[str, ...]) -> tuple[str, ...]:
    # The compiled caller's explicit-directory override selects EXIF candidates.
    # All emitted final recipes in that source family receive the same cases.
    extra = tuple(f"{group}:{target.name}" for group in directories
                  if target.table_group0 == "EXIF" and group != target.physical_write_group)
    return target.qualifiers + extra


def directory_path(target: GeneratedTarget, name: str, directories: tuple[str, ...]) -> tuple[str, ...]:
    group = name.split(":", 1)[0]
    selected = group if group in directories else target.physical_write_group
    if selected == target.physical_write_group:
        return ()
    if selected not in directories:
        raise ValueError("selected directory is absent from source operand")
    # TIFF's linked-directory representation is carrier structure, not tag data.
    # This carrier executor supports the first two compiled source directories.
    return ("NextIFD",) if directories.index(selected) == 1 else ()


def at_directory(document, carrier, path, *, allow_missing=False):
    value = document["exif"] if carrier == "jpeg" else document
    for name in path:
        if value is None or name not in value.get("children", {}):
            if allow_missing:
                return None
            raise ValueError(f"selected TIFF directory is absent: {path!r}")
        value = value["children"][name]
    if value is None and not allow_missing:
        raise ValueError("selected TIFF root is absent")
    return value


def assert_selected_target_transition(seed, expected, carrier, path, raw_tag_id, operation):
    before = at_directory(seed, carrier, path)
    after = at_directory(expected, carrier, path, allow_missing=True)
    if after is None:
        # Native WriteExif can prune a child containing only mandatory defaults
        # after deleting its last ordinary tag. Preserve this structural fact;
        # compare_carrier must still reject an OxiDex child left behind.
        if not path or operation != "delete" or str(raw_tag_id) not in before["tags"]:
            raise AssertionError("native lost a directory without deleting its selected tag")
        return
    native.assert_target_transition(before, after, "tiff", raw_tag_id, operation)


def observed_operation(seed_document, carrier, path, raw_tag_id, requested_operation):
    """Return the physical transition that the native seed left to measure."""
    seeded_target = str(raw_tag_id) in at_directory(seed_document, carrier, path)["tags"]
    # Native creation of IFD1 installs mandatory resolution fields. A request
    # named `insert` is therefore an update when the native seed already owns
    # this physical tag; preserve both facts in the matrix report.
    effective_operation = "update" if requested_operation == "insert" and seeded_target else requested_operation
    return seeded_target, effective_operation


def authenticated_ifd1_mandatory_input(target: GeneratedTarget) -> bytes | None:
    """Read the pinned WriteExif IFD1 default operand for this generated target."""
    ledger = json.loads(MANDATORY_LEDGER.read_text())
    if ledger.get("writer_tables_joined") is not True:
        raise ValueError("mandatory default ledger did not join writer tables")
    directories = ledger.get("recipe", {}).get("directories")
    if not isinstance(directories, list):
        raise ValueError("mandatory default ledger has no compiled directories")
    defaults = [
        entry
        for directory in directories
        if directory.get("directory") == "IFD1"
        for entry in directory.get("defaults", [])
        if entry.get("tag_id") == target.raw_tag_id and entry.get("kind") == "Integer"
    ]
    if len(defaults) > 1:
        raise ValueError("ambiguous IFD1 mandatory default operand")
    return None if not defaults else str(defaults[0]["value"]).encode("ascii")


def native_requested_insert_is_noop(seed_document, expected_document, carrier, path, target, operation, requested_input):
    """Identify only source-authenticated native idempotent IFD1 inserts."""
    mandatory_input = authenticated_ifd1_mandatory_input(target)
    if (
        operation != "insert"
        or path != ("NextIFD",)
        or requested_input != mandatory_input
    ):
        return False
    seed = at_directory(seed_document, carrier, path)["tags"].get(str(target.raw_tag_id))
    expected = at_directory(expected_document, carrier, path)["tags"].get(str(target.raw_tag_id))
    return seed is not None and expected is not None and native.entry_storage(seed) == native.entry_storage(expected)

def make_existing_next_ifd_fixture(path: Path, carrier: str) -> None:
    """Provide an existing empty linked IFD for TIFF (native won't create it).

    This only changes TIFF carrier structure. Every tag/value is then seeded
    through native SetNewValue and checked for real physical placement.
    """
    if not carrier.startswith("tiff"):
        return
    data = bytearray(path.read_bytes())
    order = "little" if data[:2] == b"II" else "big"
    root = int.from_bytes(data[4:8], order)
    count = int.from_bytes(data[root:root + 2], order)
    link = root + 2 + count * 12
    if int.from_bytes(data[link:link + 4], order):
        raise ValueError("IFD1 fixture input already has a next directory")
    if len(data) % 2:
        data.append(0)
    data[link:link + 4] = len(data).to_bytes(4, order)
    data.extend(bytes(6))
    path.write_bytes(data)

def case_inputs(target: GeneratedTarget) -> dict[str, bytes | None]:
    """Source-format case families; retain every original string case."""
    if target.case_family == "native_string_scalar":
        return {case: native.CASE_INPUT_BYTES.get(case) for case in native.CASES}
    maximum = "65535" if target.wire_format == "int16u" else "4294967295"
    result = {"insert": b"72", "update": b"300", "maximum": maximum.encode(),
              "zero": b"0", "delete": None}
    result.update({"fraction": b"3/2", "fraction_zero_denominator": b"1/0"} if target.wire_format == "rational64u"
                  else {"minimum": b"1", "leading_zero": b"000258"})
    return result


def extended_numeric_inputs(target: GeneratedTarget) -> dict[str, bytes]:
    if target.case_family != "native_unsigned_numeric_scalar":
        return {}
    return {
        "numeric_float": b"1.5",
        "numeric_decimal_comma": b"1,5",
        "numeric_algorithm": b"3.14159265358979" if target.wire_format == "rational64u" else b"face",
        "numeric_bound": b"4294967296/1" if target.wire_format == "rational64u" else b"65535.49",
    }


def matrix_inputs(target: GeneratedTarget) -> dict[str, bytes | None]:
    return {**case_inputs(target), **extended_numeric_inputs(target)}


def public_scalar(target: GeneratedTarget, operation: str, value: bytes | None) -> str:
    if value is None:
        return "undefined"
    if operation == "numeric_float":
        return "float"
    if operation in extended_numeric_inputs(target):
        return "utf8"
    if target.case_family == "native_unsigned_numeric_scalar":
        return "rational" if operation.startswith("fraction") else "integer"
    return "utf8" if operation == "utf8" else "bytes"

def target_text(target: GeneratedTarget, text: str, *, numeric: str = "300") -> str:
    return text if target.case_family == "native_string_scalar" else numeric


def run_typed_native(perl, library, source, output, name, value):
    encoded = {"scalar": "undefined"} if value is None else {"scalar": "utf8", "value": value}
    return native.run_native_batch(perl, library, source, output, [{"tag": name, **encoded}])


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
        tail = source[match.end():source.index("}", match.end())]
        format_match = re.search(r'wire_format: "([^"]+)"', tail)
        if not format_match:
            raise ValueError("generated final-stage rule has no source wire format")
        targets.append(GeneratedTarget(int(match["id"], 16), match["name"], match["table_group"], match["physical_group"], format_match[1]))
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
        targets.append(GeneratedTarget(*identity, recipe.get("wire_format", "string")))
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


def predecessor_public_targets(targets: tuple[GeneratedTarget, ...] | None = None,
                               migration_path: Path = PUBLIC_MIGRATION_LEDGER) -> tuple[GeneratedTarget, ...]:
    """Return current recipes already current in the authenticated predecessor.

    This preserves the original public cohort from ledger history, without a
    tag/name list. New source admissions add matrix rows but cannot silently
    replace the pre-existing coverage denominator.
    """
    targets = generated_targets() if targets is None else targets
    try:
        ledger = json.loads(migration_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"public migration ledger is unavailable: {error}") from error
    current_source = ledger.get("source_identity")
    entries = ledger.get("entries")
    if not isinstance(current_source, str) or not isinstance(entries, list):
        raise ValueError("public migration ledger is malformed")
    current_by_identity = {
        (target.raw_tag_id, target.name, target.table_group0, target.physical_write_group): target
        for target in targets
    }
    frozen = ledger.get("predecessor_cohort")
    if not isinstance(frozen, list):
        raise ValueError("public migration ledger lacks a frozen predecessor cohort")
    frozen_identities = {(item.get("raw_tag_id"), item.get("name"), item.get("group0"), item.get("write_group"))
                         for item in frozen if isinstance(item, dict)}
    if len(frozen_identities) != len(frozen) or not frozen_identities:
        raise ValueError("public migration predecessor cohort is malformed")
    import hashlib
    rendered_cohort = json.dumps(
        sorted(frozen, key=lambda item: (item["raw_tag_id"], item["name"], item["group0"], item["write_group"])),
        sort_keys=True, separators=(",", ":"), ensure_ascii=False,
    ).encode("utf-8")
    if ledger.get("predecessor_cohort_sha256") != hashlib.sha256(rendered_cohort).hexdigest():
        raise ValueError("public migration predecessor cohort integrity differs")
    migrated_current, preserved = [], []
    for entry in entries:
        if not isinstance(entry, dict) or entry.get("state") != "current":
            continue
        history = entry.get("history")
        if not isinstance(history, list) or not history:
            raise ValueError("public migration history is malformed")
        identity = (entry.get("raw_tag_id"), entry.get("name"), entry.get("group0"), entry.get("write_group"))
        if identity not in current_by_identity:
            raise ValueError("predecessor public migration is absent from current final recipes")
        target = current_by_identity[identity]
        migrated_current.append(target)
        if identity in frozen_identities:
            preserved.append(target)
    if set(migrated_current) != set(targets) or len(migrated_current) != len(targets):
        raise ValueError("current public migrations do not exactly join final recipes")
    if len(set(preserved)) != len(preserved) or not preserved:
        raise ValueError("predecessor public migration cohort is empty or ambiguous")
    if { (target.raw_tag_id, target.name, target.table_group0, target.physical_write_group) for target in preserved } != frozen_identities:
        raise ValueError("predecessor public migration cohort is absent from current final recipes")
    return tuple(sorted(preserved, key=lambda target: target.raw_tag_id))


def changed_tag_ids(target_tag_id: int | set[int] | frozenset[int] | None) -> set[str]:
    """Normalize a source-derived changed physical identity set for comparison."""
    if target_tag_id is None:
        return set()
    if isinstance(target_tag_id, int):
        return {str(target_tag_id)}
    if isinstance(target_tag_id, (set, frozenset)) and all(type(value) is int and value >= 0 for value in target_tag_id):
        return {str(value) for value in target_tag_id}
    raise ValueError("changed target identities are malformed")


def mandatory_cleanup_recipe():
    """Load the capture-bound WriteExif cleanup operands once per assertion.

    The mandatory sidecar alone is insufficient: the selected address and
    public-migration captures must name the same WriteExif and Writer.pl bytes.
    This is the checker-side equivalent of the runtime provenance join.
    """
    ledger = json.loads(MANDATORY_LEDGER.read_text())
    address = json.loads(ADDRESS_LEDGER.read_text())
    migration = json.loads(PUBLIC_MIGRATION_LEDGER.read_text())
    recipe = ledger.get("recipe", {})
    cleanup = {
        "all_mandatory": True, "entry_count_shrinks_or_new": True,
        "no_next_ifd": True, "omit_empty_ifd1": True,
    }
    capture = address.get("source_capture_identity")
    migration_capture = migration.get("source", {}).get("capture")
    if (ledger.get("writer_tables_joined") is not True
            or recipe.get("cleanup") != cleanup
            or recipe.get("writer_source_file") != "Image/ExifTool/WriteExif.pl"
            or not isinstance(capture, dict) or not isinstance(migration_capture, dict)
            or recipe.get("writer_source_sha256") != capture.get("write_exif_source_sha256")
            or recipe.get("writer_source_sha256") != migration_capture.get("write_exif_source_sha256")
            or recipe.get("write_value_source_sha256") != capture.get("writer_source_sha256")
            or recipe.get("write_value_source_sha256") != migration_capture.get("writer_source_sha256")
            or recipe.get("core_source_sha256") != capture.get("main_source_sha256")
            or recipe.get("core_source_sha256") != migration_capture.get("main_source_sha256")
            or recipe.get("exif_source_sha256") != capture.get("exif_source_sha256")
            or recipe.get("exif_source_sha256") != migration_capture.get("exif_source_sha256")):
        raise ValueError("mandatory cleanup source capture does not join selected writer artifacts")
    for digest in (recipe.get("writer_source_sha256"), recipe.get("write_value_source_sha256"), recipe.get("core_source_sha256"), recipe.get("exif_source_sha256")):
        if not isinstance(digest, str) or re.fullmatch(r"[0-9a-f]{64}", digest) is None:
            raise ValueError("mandatory cleanup source digest is malformed")
    defaults = [entry for group in recipe.get("directories", [])
                if group.get("directory") == "IFD1"
                for entry in group.get("defaults", [])]
    survivor = recipe.get("survivor_encodings")
    if (not defaults or any(entry.get("kind") != "Integer" for entry in defaults)
            or not isinstance(survivor, list)):
        raise ValueError("mandatory IFD1 defaults are not source-representable")
    return defaults, survivor


def native_survivor_value(default, entry, byte_order, survivor_encodings):
    """Pinned WriteValue packing for the captured scalar integer operands.

    The physical record selects the native form during WriteExif cleanup, so
    type/count must come from the survivor rather than the new-directory
    default.  The checked forms are the capture-authenticated TIFF forms used
    by this release: fixed-width integers and rational64u.  Every other physical
    form is a non-match and therefore retains IFD1.
    """
    value = default["value"]
    field_type, count = entry.get("type"), entry.get("count")
    if type(value) is not int or type(field_type) is not int or type(count) is not int:
        return None
    admitted = {(item.get("format_name"), item.get("tiff_type"), item.get("width"), item.get("operation"))
               for item in survivor_encodings}
    integer_forms = {1: ("int8u", 1), 6: ("int8s", 1), 8: ("int16s", 2),
                     3: ("int16u", 2), 9: ("int32s", 4), 4: ("int32u", 4)}
    if field_type in integer_forms:
        format_name, width = integer_forms[field_type]
        if (format_name, field_type, width, "write_value_scalar") not in admitted or count != 1:
            return None
        return (value & ((1 << (width * 8)) - 1)).to_bytes(width, byte_order).hex()
    if field_type == 5 and ("rational64u", 5, 8, "write_value_scalar") in admitted:
        if count != 1 or not 0 <= value <= 0xffffffff:
            return None
        return value.to_bytes(4, byte_order).hex() + (1).to_bytes(4, byte_order).hex()
    return None


def assert_prunable_ifd1(directory, removed_tag_ids):
    """Allow only captured mandatory survivors in their native physical form."""
    if directory.get("children") or directory.get("image_payload_hex"):
        raise AssertionError("pruned IFD1 contained an unrelated subtree or image payload")
    defaults, survivor_encodings = mandatory_cleanup_recipe()
    expected = {str(entry["tag_id"]): entry for entry in defaults}
    for key, entry in directory["tags"].items():
        if key in removed_tag_ids:
            continue
        default = expected.get(key)
        value_hex = None if default is None else native_survivor_value(
            default, entry, directory["byte_order"], survivor_encodings
        )
        if value_hex is None or entry.get("value_hex") != value_hex:
            raise AssertionError(f"pruned IFD1 contained unrelated or nondefault tag {key}")

def compare(seed, expected, actual, target_tag_id: int | set[int] | frozenset[int] | None, *, target_directory=(), allow_directory_removal=False):
    changed = changed_tag_ids(target_tag_id) if not target_directory else set()
    if seed["image_payload_hex"] != actual["image_payload_hex"]:
        raise AssertionError("generated write changed image payload")
    if expected["image_payload_hex"] != actual["image_payload_hex"]:
        raise AssertionError("native/generated image payload differs")
    if expected["byte_order"] != actual["byte_order"]:
        raise AssertionError("native/generated TIFF byte order differs")
    if set(expected["tags"]) != set(actual["tags"]):
        raise AssertionError("native/generated tag identities differ")
    expected_children, actual_children = expected.get("children", {}), actual.get("children", {})
    if expected_children.keys() != actual_children.keys():
        raise AssertionError("native/generated TIFF directory identities differ")
    seed_children = seed.get("children", {})
    removed = seed_children.keys() - actual_children.keys()
    permitted_removal = {"NextIFD"} if allow_directory_removal and target_directory == ("NextIFD",) else set()
    if actual_children.keys() - seed_children.keys() or removed - permitted_removal:
        raise AssertionError("generated write changed unrelated TIFF directory identities")
    for name in removed:
        assert_prunable_ifd1(seed_children[name], changed_tag_ids(target_tag_id))
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
        pointer_name = native.IFD_POINTERS.get(tag)
        if pointer_name in expected_children:
            # The expected-to-actual pass above already verified this is the
            # same pointer type/count and recursively compared its child.
            # Native may relocate a recognized directory while rebuilding the
            # TIFF, so its raw offset is not an unrelated value mutation.
            continue
        if tag not in changed and native.entry_storage(value) != native.entry_storage(actual["tags"].get(tag)):
            raise AssertionError(f"generated write changed unrelated tag {tag}")
    for name, child in expected_children.items():
        selected_child = bool(target_directory) and name == target_directory[0]
        compare(seed["children"][name], child, actual_children[name], target_tag_id if selected_child else None,
                target_directory=target_directory[1:] if selected_child else (),
                allow_directory_removal=allow_directory_removal and selected_child)


def compare_carrier(seed, expected, actual, carrier, target_tag_id: int | set[int] | frozenset[int] | None, *, target_directory=(), allow_directory_removal=False):
    if carrier != "jpeg":
        return compare(seed, expected, actual, target_tag_id, target_directory=target_directory,
                       allow_directory_removal=allow_directory_removal)
    for key in ("sos_to_end_sha256", "non_exif_sha256"):
        if actual[key] != seed[key]:
            raise AssertionError(f"generated JPEG changed {key}")
    if actual["sos_to_end_sha256"] != expected["sos_to_end_sha256"]:
        raise AssertionError("native/generated JPEG image payload differs")
    if any(document["exif"] is None for document in (seed, expected, actual)):
        raise AssertionError("JPEG operation lost its EXIF block")
    compare(seed["exif"], expected["exif"], actual["exif"], target_tag_id, target_directory=target_directory,
            allow_directory_removal=allow_directory_removal)


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
    parser.add_argument("--readback-evidence", type=Path, help="opt-in authenticated native Group1 write-readback sidecar")
    parser.add_argument("--readback-source", type=Path, help="captured native source used to regenerate final/public artifacts")
    parser.add_argument("--readback-build-proof", type=Path, help="write_readback_evidence.py build-proof.json")
    args = parser.parse_args()
    readback_options = (args.readback_evidence, args.readback_source, args.readback_build_proof)
    if any(readback_options) and (not all(readback_options) or args.route != "public-api" or args.rehearsal_release):
        parser.error("readback requires all three --readback-* options, --route public-api, and the repository pin")
    if args.readback_evidence and (args.readback_evidence.exists() or args.readback_evidence.resolve() in {args.output.resolve(), args.readback_source.resolve(), args.readback_build_proof.resolve()}):
        parser.error("readback evidence output exists or aliases an input/output")
    if (args.rehearsal_release is None) != (args.rehearsal_pin is None):
        parser.error("--rehearsal-release and --rehearsal-pin must be supplied together")
    carriers = ("tiff_little", "tiff_big") + (("jpeg",) if args.jpeg_base else ())
    targets = generated_targets(args.ledger, args.rules)
    directories = explicit_directory_operands() if args.route != "final-key" else ()
    declared = len(carriers) * sum(len(selected_qualifiers(target, directories)) * len(matrix_inputs(target)) for target in targets)
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
    collector = None
    if args.readback_evidence:
        from write_readback_evidence import Collector
        collector = Collector(args.readback_source, args.readback_build_proof, binary.path,
                              args.ledger, args.rules, perl, library, identity)
    print_header(tool="generated_scalar_write_matrix_v4", git=state, binary=binary,
                 dirty_overridden=overridden,
                 extra=[f"native: {identity}", f"contract: {contract['mode']} {contract['release']}",
                        f"{declared} TIFF/JPEG operations via {args.route}; existing EXIF blocks"])
    root = args.output.parent / "generated-tiff-matrix-files"
    root.mkdir(parents=True, exist_ok=False)
    rows, requests = [], []
    for carrier in carriers:
        for target in targets:
            for name in selected_qualifiers(target, directories):
                path = directory_path(target, name, directories)
                for operation, input_bytes in matrix_inputs(target).items():
                    stem = f"{carrier}-{target.raw_tag_id:04x}-{name.replace(':', '_')}-{operation}"
                    suffix = ".jpg" if carrier == "jpeg" else ".tif"
                    source, seeded, expected, output = (root / f"{stem}-{part}{suffix}" for part in ("source", "seeded", "native", "generated"))
                    native.make_carrier(source, carrier, args.jpeg_base)
                    if path:
                        make_existing_next_ifd_fixture(source, carrier)
                        seed_batch = [
                            {"tag": "IFD0:Artist", "scalar": "utf8", "value": "seed-artist"},
                            {"tag": name.split(":", 1)[0] + ":Artist", "scalar": "utf8", "value": "directory-anchor"},
                            {"tag": target.physical_write_group + ":" + target.name, "scalar": "utf8", "value": target_text(target, "ifd0-preserved", numeric="91")},
                        ]
                        if operation != "insert":
                            seed_batch.append({"tag": name, "scalar": "utf8", "value": target_text(target, "seed-target", numeric="73")})
                        seed_call = native.run_native_batch(perl, library, source, seeded, seed_batch)
                    elif target.case_family == "native_string_scalar":
                        seed_action = "seed_artist" if operation == "insert" else "seed_artist_target"
                        seed_call = native.run_native(perl, library, source, seeded, seed_action, None if operation == "insert" else name)
                        operation_call = None
                    else:
                        seed_batch = [{"tag": "IFD0:Artist", "scalar": "utf8", "value": "seed-artist"}]
                        if operation != "insert":
                            seed_batch.append({"tag": name, "scalar": "utf8", "value": "73"})
                        seed_call = native.run_native_batch(perl, library, source, seeded, seed_batch)
                    native.assert_native(seed_call, stem + " seed")
                    seed_document = native.inspect(seeded, carrier)
                    seeded_target, effective_operation = observed_operation(
                        seed_document, carrier, path, target.raw_tag_id, operation
                    )
                    if target.case_family == "native_string_scalar":
                        operation_call = native.run_native(perl, library, seeded, expected, operation, name)
                    else:
                        operation_call = run_typed_native(perl, library, seeded, expected, name, None if input_bytes is None else input_bytes.decode())
                    native.assert_native(operation_call, stem + " operation")
                    scalar = public_scalar(target, operation, input_bytes)
                    value = None if input_bytes is None else input_bytes.hex() if scalar == "bytes" else input_bytes.decode("utf-8")
                    requests.append({"route": args.route, "carrier": carrier, "input": str(seeded), "output": str(output), "key": name, "scalar": scalar, "value": value})
                    rows.append({"carrier": carrier, "id": stem, "target": asdict(target), "case_family": target.case_family, "requested_name": name, "target_directory": list(path), "coverage_family": ("extended_" if operation in extended_numeric_inputs(target) else "") + ("selected_directory_" if path else "baseline_") + target.case_family, "public_scalar": scalar, "requested_operation": operation, "operation": operation, "requested_input_hex": None if input_bytes is None else input_bytes.hex(), "effective_operation": effective_operation, "effective_state": "pending_native_comparison", "effective_source": "pinned native SetNewValue against the seeded physical target", "seed_target_present": seeded_target, "seeded": str(seeded), "native_output": str(expected), "output": str(output), "native_call": operation_call})
    request_path, result_path = root / "requests.json", root / "results.json"
    request_path.write_text(json.dumps(requests, indent=2) + "\n")
    if collector is not None:
        collector.capture_inputs(rows)
    env = os.environ.copy()
    env.update(OXIDEX_SCALAR_WRITE_REQUESTS=str(request_path), OXIDEX_SCALAR_WRITE_RESULTS=str(result_path))
    result = subprocess.run([str(binary.path), DRIVER, "--exact", "--ignored", "--nocapture"], env=env, capture_output=True, text=True, timeout=120)
    (root / "driver.log").write_text(result.stdout + result.stderr)
    result.check_returncode()
    if collector is not None:
        from write_readback_evidence import completed_transcript
        collector.driver_call = completed_transcript([str(binary.path), DRIVER, "--exact", "--ignored", "--nocapture"], result)
    results = json.loads(result_path.read_text())
    report = {"instrument": "generated_scalar_write_matrix_v4", "route": args.route, "native_identity": identity,
              "contract": contract,
              "source_commit": state.commit, "dirty_files": state.dirty_files,
              "test_binary_path": str(binary.path),
              "test_binary_sha256": hashlib.sha256(binary.path.read_bytes()).hexdigest(),
              "ledger_sha256": hashlib.sha256(args.ledger.read_bytes()).hexdigest(),
              "rules_sha256": hashlib.sha256(args.rules.read_bytes()).hexdigest(),
              "cohort": [{"raw_tag_id": target.raw_tag_id, "name": target.name,
                          "table_group0": target.table_group0, "physical_write_group": target.physical_write_group,
                          "wire_format": target.wire_format, "case_family": target.case_family, "cases": list(matrix_inputs(target)), "case_inputs": {case: {"value_hex": None if value is None else value.hex(), "public_scalar": public_scalar(target, case, value)} for case, value in matrix_inputs(target).items()}, "qualifiers": list(selected_qualifiers(target, directories))} for target in targets],
              "declared": declared,
              "explicit_directories": list(directories),
              "declared_by_case_family": {family: sum(row["case_family"] == family for row in rows) for family in sorted({row["case_family"] for row in rows})},
              "declared_by_coverage_family": {family: sum(row["coverage_family"] == family for row in rows) for family in sorted({row["coverage_family"] for row in rows})},
              "declared_by_qualifier": {qualifier: sum(row["requested_name"].split(":", 1)[0] == qualifier for row in rows) for qualifier in sorted({row["requested_name"].split(":", 1)[0] for row in rows})}, "passed": 0,
              "passed_by_effective_state": {"mutated": 0, "native_noop": 0}, "rows": rows,
              "limitations": ["New JPEG EXIF blocks and empty existing IFDs remain untested; public modify/remove covered only with route public-api.", "Generated final-scalar ledger cohort only; this does not establish public SetNewValue admission.", f"Native contract mode: {contract['mode']} for release {contract['release']}."]}
    if len(results) != len(requests) or len(requests) != declared:
        raise AssertionError("fixture driver result population differs")
    for row, result in zip(rows, results, strict=True):
        row["driver_result"] = result
        try:
            if result.get("output") != row["output"] or not result.get("ok") or result.get("warnings"):
                raise AssertionError(f"generated writer did not succeed: {result}")
            seed, expected, actual = (native.inspect(Path(row[key]), row["carrier"]) for key in ("seeded", "native_output", "output"))
            path = tuple(row["target_directory"])
            target = GeneratedTarget(**row["target"])
            requested_input = (
                None
                if row["requested_input_hex"] is None
                else bytes.fromhex(row["requested_input_hex"])
            )
            if native_requested_insert_is_noop(
                seed, expected, row["carrier"], path, target, row["operation"], requested_input
            ):
                # The target comparison above establishes idempotence. This
                # separately proves pinned native left every carrier-level
                # state unchanged before generated output is compared.
                compare_carrier(seed, seed, expected, row["carrier"], row["target"]["raw_tag_id"], target_directory=path)
                row["effective_state"] = "native_noop"
                row["seed_vs_native_complete_state"] = "passed"
            else:
                assert_selected_target_transition(seed, expected, row["carrier"], path, row["target"]["raw_tag_id"], row["effective_operation"])
                row["effective_state"] = "mutated"
            compare_carrier(seed, expected, actual, row["carrier"], row["target"]["raw_tag_id"], target_directory=path,
                            allow_directory_removal=row["effective_operation"] == "delete")
            row["state"] = "passed"
            report["passed"] += 1
            report["passed_by_effective_state"][row["effective_state"]] += 1
        except (AssertionError, ValueError, OSError) as error:
            row.update(state="failed", error=str(error))
        args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    if collector is not None:
        evidence = collector.finish(report, args.output)
        args.readback_evidence.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n")
        print("Authenticated public write readback:", evidence["counts"])
    print(
        f"Scalar write operations matched via {args.route}: {report['passed']}/{declared} "
        f"(mutating {report['passed_by_effective_state']['mutated']}, "
        f"native-idempotent {report['passed_by_effective_state']['native_noop']})"
    )
    return 0 if report["passed"] == declared else 1


if __name__ == "__main__":
    raise SystemExit(main())
