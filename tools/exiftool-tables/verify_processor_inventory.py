"""Strict parser for oracle.pl's generic custom-processor inventory.

This module consumes live native records only.  It deliberately has no compiler,
generator, or generated-Rust imports: presence and source shape are inventory
evidence, not proof that any processor layout is executable.
"""

from __future__ import annotations

from dataclasses import dataclass
import json
from pathlib import PurePosixPath
import re
from typing import Any


class ProcessorInventoryError(ValueError):
    """The oracle processor inventory is malformed or internally inconsistent."""


@dataclass(frozen=True, order=True)
class ProcessorKey:
    module: str
    table: str


@dataclass(frozen=True, order=True)
class ProcessorRowKey:
    module: str
    table: str
    raw_id: str
    variant: str

    @property
    def table_key(self) -> ProcessorKey:
        return ProcessorKey(self.module, self.table)


@dataclass(frozen=True)
class ProcessorInventory:
    """Native source facts keyed by processor table and raw entry alternative."""

    processors: dict[ProcessorKey, dict[str, Any]]
    tables: dict[ProcessorKey, dict[str, Any]]
    rows: dict[ProcessorRowKey, dict[str, Any]]


_FQ_NAME = re.compile(r"(?:[A-Za-z_]\w*::)+[A-Za-z_]\w*\Z")
_SHA256 = re.compile(r"[0-9a-f]{64}\Z")
_VARIANT = re.compile(r"(?:0|[1-9]\d*)\Z")
_ROW_PROPERTIES = frozenset((
    "Name", "Description", "Format", "Writable", "Count", "Groups", "Notes", "Mask",
    "BitShift", "Condition", "PrintConv", "ValueConv", "RawConv", "PrintConvInv",
    "ValueConvInv", "Hook", "SubDirectory", "Flags", "Unknown", "Hidden", "Avoid",
    "Binary", "Protected", "List", "Priority", "ByteOrder", "DataMember", "RelatedTag",
    "SeparateTable", "PrintHex", "Base", "Offset", "ChangeBase", "Require", "Desire",
    "Inhibit", "BitsPerWord", "BitsTotal", "FixFormat", "SubIFD",
))
_EFFECTIVE_FLAGS = frozenset(("Unknown", "Binary", "List", "Protected", "Avoid", "Priority"))


def _fail(message: str) -> None:
    raise ProcessorInventoryError(message)


def _text(value: Any, what: str) -> str:
    if not isinstance(value, str) or not value:
        _fail(f"{what} must be a non-empty string")
    if "\t" in value or "\n" in value or "\r" in value:
        _fail(f"{what} contains a record separator")
    return value


def _json_string(value: Any, what: str) -> str:
    if not isinstance(value, str) or "\t" in value or "\n" in value or "\r" in value:
        _fail(f"{what} must be a JSON string without a record separator")
    return value


def _relative_source(value: Any, what: str) -> None:
    value = _text(value, what)
    path = PurePosixPath(value)
    if path.is_absolute() or "\\" in value or path.as_posix() != value or any(part in {".", ".."} for part in path.parts):
        _fail(f"{what} must be a relative POSIX path")


def _code_fact(value: Any, where: str, *, depth: int = 0) -> None:
    if depth > 12:
        _fail(f"{where} dependency depth exceeds protocol limit")
    if not isinstance(value, dict):
        _fail(f"{where} must be an object")
    if value.get("__perl") != "CODE" or value.get("__opaque") is not True:
        _fail(f"{where} is not an opaque CODE fact")
    name = value.get("__name")
    if not isinstance(name, str) or _FQ_NAME.fullmatch(name) is None:
        _fail(f"{where} has an invalid code name")
    resolved = value.get("resolved")
    if not isinstance(resolved, bool):
        _fail(f"{where} resolved must be boolean")
    if resolved:
        if not isinstance(value.get("__deparse"), str):
            _fail(f"{where} is resolved without deparse text")
        _relative_source(value.get("source_file"), f"{where} source_file")
        if not isinstance(value.get("source_sha256"), str) or _SHA256.fullmatch(value["source_sha256"]) is None:
            _fail(f"{where} source_sha256 is invalid")
        if "reason" in value:
            _fail(f"{where} resolved fact carries a refusal reason")
    else:
        if not isinstance(value.get("reason"), str) or not value["reason"]:
            _fail(f"{where} unresolved fact lacks a reason")
        if value.get("__deparse") is not None or value.get("source_file") is not None or value.get("source_sha256") is not None:
            _fail(f"{where} unresolved fact carries source provenance")
    dependencies = value.get("dependencies", {})
    if not isinstance(dependencies, dict):
        _fail(f"{where} dependencies must be an object")
    for name, child in dependencies.items():
        if not isinstance(name, str) or _FQ_NAME.fullmatch(name) is None:
            _fail(f"{where} has an invalid dependency key")
        _code_fact(child, f"{where} dependency {name}", depth=depth + 1)


def _native_value(value: Any, where: str, *, depth: int = 0) -> None:
    if depth > 12:
        _fail(f"{where} value nesting exceeds protocol limit")
    if not isinstance(value, dict) or not isinstance(value.get("kind"), str):
        _fail(f"{where} must be a typed native value")
    kind = value["kind"]
    if kind == "undef":
        if set(value) != {"kind"}:
            _fail(f"{where} undef has extra fields")
    elif kind == "scalar":
        if set(value) != {"kind", "value"} or not isinstance(value["value"], str):
            _fail(f"{where} scalar is malformed")
    elif kind == "code":
        if set(value) - {"kind", "name"}:
            _fail(f"{where} code has unknown fields")
        if "name" in value and (not isinstance(value["name"], str) or _FQ_NAME.fullmatch(value["name"]) is None):
            _fail(f"{where} code name is invalid")
    elif kind == "scalar_ref":
        if set(value) != {"kind", "value"}:
            _fail(f"{where} scalar_ref is malformed")
        _native_value(value["value"], f"{where} scalar_ref", depth=depth + 1)
    elif kind == "array":
        if set(value) != {"kind", "items"} or not isinstance(value["items"], list):
            _fail(f"{where} array is malformed")
        for index, item in enumerate(value["items"]):
            _native_value(item, f"{where} array[{index}]", depth=depth + 1)
    elif kind == "hash":
        if set(value) != {"kind", "map"} or not isinstance(value["map"], dict):
            _fail(f"{where} hash is malformed")
        for key, item in value["map"].items():
            if not isinstance(key, str):
                _fail(f"{where} hash key must be a string")
            _native_value(item, f"{where} hash {key}", depth=depth + 1)
    elif kind in {"ref", "cycle"}:
        if set(value) != {"kind", "ref_kind"} or not isinstance(value["ref_kind"], str) or not value["ref_kind"]:
            _fail(f"{where} reference is malformed")
    elif kind == "deep":
        if set(value) != {"kind"}:
            _fail(f"{where} deep marker has extra fields")
    else:
        _fail(f"{where} has unknown native value kind {kind!r}")


def _property(value: Any, where: str) -> None:
    if not isinstance(value, dict) or not isinstance(value.get("present"), bool):
        _fail(f"{where} must carry boolean presence")
    if value["present"]:
        if set(value) != {"present", "value"}:
            _fail(f"{where} present property is malformed")
        _native_value(value["value"], where)
    elif set(value) != {"present"}:
        _fail(f"{where} absent property carries a value")


def _table_fact(value: Any, where: str, processor: dict[str, Any]) -> None:
    if not isinstance(value, dict):
        _fail(f"{where} must be an object")
    required = {"processor", "groups", "format", "first_entry", "row_record_count", "named_row_count"}
    allowed = required | {"metadata"}
    if not required <= set(value) or set(value) - allowed:
        _fail(f"{where} has unexpected or missing fields")
    _code_fact(value["processor"], f"{where} processor")
    if value["processor"] != processor:
        _fail(f"{where} processor fact differs from NATIVE_PROCESSOR")
    for name in ("groups", "format", "first_entry"):
        _property(value[name], f"{where} {name}")
    # Older saved inventories predate complete table metadata; keep those
    # parseable so consumers can report the coverage gap. Fresh oracle facts
    # carry every live `%specialTags` key as an explicit typed property.
    if "metadata" in value:
        metadata = value["metadata"]
        if not isinstance(metadata, dict) or not metadata:
            _fail(f"{where} metadata must be a non-empty object")
        for name, property_value in metadata.items():
            _text(name, f"{where} metadata name")
            _property(property_value, f"{where} metadata {name}")
    for name in ("row_record_count", "named_row_count"):
        if not isinstance(value[name], int) or isinstance(value[name], bool) or value[name] < 0:
            _fail(f"{where} {name} must be a non-negative integer")
    if value["named_row_count"] > value["row_record_count"]:
        _fail(f"{where} named_row_count exceeds row_record_count")


def _row_fact(value: Any, where: str) -> None:
    if not isinstance(value, dict) or not isinstance(value.get("entry_kind"), str):
        _fail(f"{where} must carry entry_kind")
    if value.get("name") is not None and not isinstance(value.get("name"), str):
        _fail(f"{where} name must be string or null")
    kind = value["entry_kind"]
    if kind != "HASH":
        if set(value) != {"entry_kind", "name"}:
            _fail(f"{where} non-hash entry has unsupported facts")
        return
    required = {"entry_kind", "name", "properties", "expanded_properties", "effective_flags"}
    if set(value) != required:
        _fail(f"{where} hash entry has unexpected or missing fields")
    properties = value["properties"]
    if not isinstance(properties, dict) or set(properties) != _ROW_PROPERTIES:
        _fail(f"{where} properties do not match protocol")
    for name, property_value in properties.items():
        _property(property_value, f"{where} property {name}")
    expanded = value["expanded_properties"]
    if not isinstance(expanded, dict):
        _fail(f"{where} expanded_properties must be object")
    for name, native in expanded.items():
        _text(name, f"{where} expanded property name")
        _native_value(native, f"{where} expanded property {name}")
    flags = value["effective_flags"]
    if not isinstance(flags, dict) or set(flags) != _EFFECTIVE_FLAGS:
        _fail(f"{where} effective_flags do not match protocol")
    for name, native in flags.items():
        _native_value(native, f"{where} effective flag {name}")


def _json(payload: str, where: str) -> dict[str, Any]:
    try:
        value = json.loads(payload)
    except json.JSONDecodeError as error:
        _fail(f"{where} has invalid JSON: {error.msg}")
    if not isinstance(value, dict):
        _fail(f"{where} JSON must be an object")
    return value


def parse_processor_inventory(text: str) -> ProcessorInventory:
    """Parse and reconcile native processor records from a mixed oracle stream.

    Existing binary, IFD, and KEYED rows are intentionally ignored.  The empty
    inventory is valid for an older oracle; once any processor record exists,
    its table/row companion records must be complete.
    """
    if not isinstance(text, str):
        _fail("oracle stream must be text")
    processors: dict[ProcessorKey, dict[str, Any]] = {}
    tables: dict[ProcessorKey, dict[str, Any]] = {}
    rows: dict[ProcessorRowKey, dict[str, Any]] = {}
    for line_number, line in enumerate(text.splitlines(), 1):
        fields = line.split("\t")
        marker = fields[0] if fields else ""
        if marker not in {"NATIVE_PROCESSOR", "NATIVE_PROCESSOR_TABLE", "NATIVE_PROCESSOR_ROW"}:
            continue
        if marker == "NATIVE_PROCESSOR":
            if len(fields) != 4:
                _fail(f"line {line_number}: NATIVE_PROCESSOR requires 4 fields")
            module, table = _text(fields[1], "module"), _text(fields[2], "table")
            key = ProcessorKey(module, table)
            if key in processors:
                _fail(f"line {line_number}: duplicate NATIVE_PROCESSOR {key}")
            fact = _json(fields[3], f"line {line_number} NATIVE_PROCESSOR")
            _code_fact(fact, f"line {line_number} NATIVE_PROCESSOR")
            processors[key] = fact
        elif marker == "NATIVE_PROCESSOR_TABLE":
            if len(fields) != 4:
                _fail(f"line {line_number}: NATIVE_PROCESSOR_TABLE requires 4 fields")
            module, table = _text(fields[1], "module"), _text(fields[2], "table")
            key = ProcessorKey(module, table)
            if key in tables:
                _fail(f"line {line_number}: duplicate NATIVE_PROCESSOR_TABLE {key}")
            tables[key] = _json(fields[3], f"line {line_number} NATIVE_PROCESSOR_TABLE")
        else:
            if len(fields) != 6:
                _fail(f"line {line_number}: NATIVE_PROCESSOR_ROW requires 6 fields")
            module, table = _text(fields[1], "module"), _text(fields[2], "table")
            raw_id = _text(fields[3], "raw_id")
            variant = fields[4]
            if variant != "-" and _VARIANT.fullmatch(variant) is None:
                _fail(f"line {line_number}: variant must be '-' or canonical non-negative decimal")
            key = ProcessorRowKey(module, table, raw_id, variant)
            if key in rows:
                _fail(f"line {line_number}: duplicate NATIVE_PROCESSOR_ROW {key}")
            fact = _json(fields[5], f"line {line_number} NATIVE_PROCESSOR_ROW")
            _row_fact(fact, f"line {line_number} NATIVE_PROCESSOR_ROW")
            rows[key] = fact

    if set(processors) != set(tables):
        missing_tables = sorted(set(processors) - set(tables))
        extra_tables = sorted(set(tables) - set(processors))
        _fail(f"processor/table universe mismatch: missing tables={missing_tables!r}, extra tables={extra_tables!r}")
    for key, table in tables.items():
        _table_fact(table, f"NATIVE_PROCESSOR_TABLE {key}", processors[key])
        table_rows = [(row_key, row) for row_key, row in rows.items() if row_key.table_key == key]
        for row_key, _row in table_rows:
            if row_key.table_key not in processors:
                _fail(f"orphan NATIVE_PROCESSOR_ROW {row_key}")
        count = len(table_rows)
        named = sum(row.get("name") is not None for _key, row in table_rows)
        if count != table["row_record_count"] or named != table["named_row_count"]:
            _fail(f"NATIVE_PROCESSOR_TABLE {key} row counts disagree with records")
    orphan_rows = sorted({row_key.table_key for row_key in rows} - set(processors))
    if orphan_rows:
        _fail(f"rows without processor table: {orphan_rows!r}")
    return ProcessorInventory(processors, tables, rows)
