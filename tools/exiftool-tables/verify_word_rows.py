"""Independent row audit for selected word-directory processor tables.

This module consumes only the generic native processor inventory and a parsed
Rust-like schema projection.  It does not select a processor from generated
output and it never imports a compiler or generator.  The caller must provide
the fully-qualified native PROCESS_PROC names and a generated projection for
that layout family; this makes deletion of every selected generated table a
failure, including a native table with zero rows.
"""

from __future__ import annotations

from dataclasses import dataclass
import re
from typing import Any, Iterable

from verify_processor_inventory import ProcessorInventory, ProcessorKey, ProcessorRowKey


@dataclass(frozen=True)
class WordRowMismatch:
    """One source-to-generated audit discrepancy."""

    identity: tuple[str, ...]
    reason: str


@dataclass(frozen=True)
class WordRowAudit:
    """Counters and discrepancies for one explicit processor selection."""

    expected_tables: int
    expected_rows: int
    generated_tables: int
    generated_rows: int
    mismatches: tuple[WordRowMismatch, ...]

    @property
    def ok(self) -> bool:
        return not self.mismatches


_NUMERIC_ID = re.compile(r"(?:0|[1-9]\d*)\Z")
_FLAG_NAMES = ("Unknown", "Binary", "List", "Protected", "Avoid", "Priority")

# These properties affect a selected value, parsing position/format, nested
# reads, tag eligibility, or read-side presentation.  WordDirectory has no
# representation for them.  Keep documentation and write-only properties
# (Description, Notes, Writable, *ConvInv) out of this set.
# Table properties which are documentation or write-path-only under the
# current scoped contract. Everything else present in the complete native
# special-tag map is rejected until a WordDirectory member models it.
_IGNORED_TABLE_METADATA = frozenset(("NOTES", "WRITABLE", "WRITE_PROC", "CHECK_PROC"))

_UNMODELED_RUNTIME_PROPERTIES = (
    "Mask", "BitShift", "BitsPerWord", "BitsTotal", "ByteOrder", "DataMember",
    "Hook", "Base", "Offset", "ChangeBase", "FixFormat", "SubIFD", "RelatedTag",
    "SeparateTable", "PrintHex", "Require", "Desire", "Inhibit", "Hidden",
)


def _mismatches(mismatches: list[WordRowMismatch], identity: tuple[str, ...], reason: str) -> None:
    mismatches.append(WordRowMismatch(identity, reason))


def _property(row: dict[str, Any], name: str) -> tuple[bool, Any | None, str | None]:
    """Return property presence, typed value, and an error for unknown shape."""
    if row.get("entry_kind") != "HASH":
        return False, None, "row has no auditable HASH property map"
    item = row.get("properties", {}).get(name)
    if not isinstance(item, dict) or not isinstance(item.get("present"), bool):
        return False, None, f"{name} property is malformed"
    if not item["present"]:
        return False, None, None
    value = item.get("value")
    if not isinstance(value, dict) or not isinstance(value.get("kind"), str):
        return True, None, f"{name} property has no typed native value"
    return True, value, None


def _scalar(value: Any) -> str | None:
    if isinstance(value, dict) and value.get("kind") == "scalar" and isinstance(value.get("value"), str):
        return value["value"]
    return None


def _groups(value: Any, what: str) -> tuple[tuple[str | None, str | None, str | None] | None, str | None]:
    if not isinstance(value, dict) or value.get("kind") != "hash" or not isinstance(value.get("map"), dict):
        return None, f"{what} must be a scalar hash"
    out: list[str | None] = []
    for index in range(3):
        entry = value["map"].get(str(index))
        if entry is None:
            out.append(None)
        elif entry.get("kind") == "undef":
            out.append(None)
        else:
            text = _scalar(entry)
            if text is None:
                return None, f"{what}[{index}] is not a scalar/undef"
            out.append(text)
    if set(value["map"]) - {"0", "1", "2"}:
        return None, f"{what} has an unsupported group index"
    return tuple(out), None


def _table_groups(table: dict[str, Any]) -> tuple[tuple[str, str, str] | None, str | None]:
    prop = table.get("groups")
    if not isinstance(prop, dict) or not isinstance(prop.get("present"), bool):
        return None, "table Groups fact is malformed"
    if not prop["present"]:
        return ("", "", ""), None
    groups, error = _groups(prop.get("value"), "table Groups")
    if error:
        return None, error
    return tuple(value or "" for value in groups), None


def _effective_flags(row: dict[str, Any]) -> tuple[tuple[bool, bool, bool, bool, bool, int | None] | None, str | None]:
    values = row.get("effective_flags")
    if not isinstance(values, dict) or set(values) != set(_FLAG_NAMES):
        return None, "effective Flags are malformed"
    booleans: list[bool] = []
    for name in _FLAG_NAMES[:-1]:
        value = values[name]
        if not isinstance(value, dict):
            return None, f"effective {name} is malformed"
        if value.get("kind") == "undef":
            booleans.append(False)
        elif _scalar(value) in {"0", "1"}:
            booleans.append(_scalar(value) == "1")
        else:
            return None, f"effective {name} is not undef/0/1"
    priority = values["Priority"]
    if not isinstance(priority, dict):
        return None, "effective Priority is malformed"
    if priority.get("kind") == "undef":
        numeric = None
    else:
        text = _scalar(priority)
        if text is None or not re.fullmatch(r"-?(?:0|[1-9]\d*)", text):
            return None, "effective Priority is not undef/an integer"
        numeric = int(text)
    return (*booleans, numeric), None


def _enum(value: Any) -> tuple[dict[str, str] | None, str | None]:
    if not isinstance(value, dict) or value.get("kind") != "hash" or not isinstance(value.get("map"), dict):
        return None, "PrintConv is not an enum hash"
    result: dict[str, str] = {}
    for key, item in value["map"].items():
        # The HandleTag shape is an int8u value.  Do not borrow CIFF key
        # high-bit rules, bitmask directives, or arbitrary Perl conversions.
        if not isinstance(key, str) or not re.fullmatch(r"-?(?:0|[1-9]\d*)", key):
            return None, "PrintConv enum key is not an integer literal"
        text = _scalar(item)
        if text is None:
            return None, "PrintConv enum value is not a scalar"
        result[key] = text
    return result, None


def _identity(row_key: ProcessorRowKey) -> tuple[str, str, str]:
    if _NUMERIC_ID.fullmatch(row_key.raw_id) is None:
        raise ValueError("native word raw id is not a decimal integer")
    suffix = "" if row_key.variant == "-" else f"#{row_key.variant}"
    return (row_key.module, row_key.table, f"{row_key.raw_id}{suffix}")


def _selected_tables(inventory: ProcessorInventory, processor_names: Iterable[str], mismatches: list[WordRowMismatch]) -> set[ProcessorKey]:
    expected = tuple(sorted(set(processor_names)))
    if not expected:
        _mismatches(mismatches, ("processor",), "no expected native PROCESS_PROC names supplied")
        return set()
    present = {fact.get("__name") for fact in inventory.processors.values()}
    for name in expected:
        if name not in present:
            _mismatches(mismatches, ("processor", name), "expected native PROCESS_PROC is absent")
    return {key for key, fact in inventory.processors.items() if fact.get("__name") in expected}


def _attr(obj: Any, name: str, default: Any) -> Any:
    return getattr(obj, name, default)


def _field_facts(row: dict[str, Any], key: tuple[str, str, str], generated: Any, mismatches: list[WordRowMismatch]) -> None:
    facts = _attr(generated, "facts", {}).get(key)
    source = _attr(generated, "source_facts", {}).get(key)
    if facts is None or source is None:
        _mismatches(mismatches, key, "generated row lacks parsed facts/source facts")
        return
    # ProcessCanonCustom supplies HandleTag selection metadata Format int8u /
    # Count 1.  The native integer value itself is not constrained to eight
    # bits. A word descriptor keeps absent per-row Format/Count as `None`;
    # its layout carries the processor-supplied int8u/1 selection metadata.
    # An explicitly repeated int8u/1 must retain
    # its raw source facts and use the corresponding typed projection.
    for prop, generated_value, source_value, absent_generated, explicit_generated, explicit_source in (
        ("Format", _attr(facts, "format", None), _attr(source, "format", None), "None", "Some(Fmt::Int8u)", "int8u"),
        ("Count", _attr(facts, "count", None), _attr(source, "count", None), "None", "Some(1)", "1"),
    ):
        present, value, error = _property(row, prop)
        if error:
            _mismatches(mismatches, key, error)
            continue
        if not present:
            if generated_value != absent_generated or source_value is not None:
                _mismatches(mismatches, key, f"{prop} does not match authenticated HandleTag default")
            continue
        text = _scalar(value)
        if text != explicit_source or generated_value != explicit_generated or source_value != explicit_source:
            _mismatches(mismatches, key, f"native {prop} is not the exact supported int8u/1 projection")

    present, value, error = _property(row, "Condition")
    if error:
        _mismatches(mismatches, key, error)
    elif present:
        text = _scalar(value)
        if text is None or _attr(source, "condition", None) != text or not _attr(facts, "condition", False):
            _mismatches(mismatches, key, "native Condition is not represented exactly")
    elif _attr(source, "condition", None) is not None or _attr(facts, "condition", False):
        _mismatches(mismatches, key, "generated Condition exists without a native source property")

    for prop, fact_name in (("RawConv", "raw_conv"), ("ValueConv", "value_conv")):
        present, _value, error = _property(row, prop)
        if error:
            _mismatches(mismatches, key, error)
        elif bool(_attr(facts, fact_name, False)) != present:
            _mismatches(mismatches, key, f"{prop} presence differs from native source")
        elif present:
            # This audit has no expression interpreter.  Presence alone must
            # never certify that an arbitrary native conversion is modeled.
            _mismatches(mismatches, key, f"unsupported native {prop} projection")

    present, value, error = _property(row, "Groups")
    if error:
        _mismatches(mismatches, key, error)
    elif present:
        native_groups, group_error = _groups(value, "Groups")
        if group_error or _attr(source, "groups", None) != native_groups or _attr(facts, "groups", None) != native_groups:
            _mismatches(mismatches, key, group_error or "native Groups is not represented exactly")
    elif _attr(source, "groups", None) != (None, None, None) or _attr(facts, "groups", None) != (None, None, None):
        _mismatches(mismatches, key, "generated Groups exists without a native source property")

    flags, flag_error = _effective_flags(row)
    if flag_error or _attr(source, "flags", None) != flags or _attr(facts, "flags", None) != flags:
        _mismatches(mismatches, key, flag_error or "effective native Flags are not represented exactly")

    for prop in _UNMODELED_RUNTIME_PROPERTIES:
        present, _value, error = _property(row, prop)
        if error:
            _mismatches(mismatches, key, error)
        elif present:
            _mismatches(mismatches, key, f"unsupported native {prop} has no WordDirectory representation")

    present, value, error = _property(row, "SubDirectory")
    if error:
        _mismatches(mismatches, key, error)
    elif present or _attr(source, "subdir", None) is not None or _attr(facts, "edge", None) is not None:
        _mismatches(mismatches, key, "unsupported native/generated SubDirectory projection")


def audit_word_rows(generated: Any, omissions: Any, inventory: ProcessorInventory, processor_names: Iterable[str]) -> WordRowAudit:
    """Audit one caller-selected word layout against full native row inventory.

    Fresh native records carry a complete typed table metadata map over live
    ExifTool `%specialTags`; older records remain parseable but are refused by
    this audit because that coverage is unavailable.

    ``generated`` may be the whole ParsedKeyedRust-like object when it exposes
    ``layouts``: only entries with a non-None WordDirectory descriptor belong
    to this audit.  Older callers may pass a layout-filtered projection with no
    ``layouts`` attribute.  Keeping processor selection outside this pure audit
    avoids discovering expected native processors from generated Rust.  ``omissions`` may be a ParsedNativeOmissions
    shape; all selected omissions fail closed until a word-specific omission
    proof is implemented.
    """
    mismatches: list[WordRowMismatch] = []
    selected = _selected_tables(inventory, processor_names, mismatches)
    expected_rows = {row for row in inventory.rows if row.table_key in selected}
    expected_ids: set[tuple[str, str, str]] = set()
    for row in expected_rows:
        try:
            identity = _identity(row)
        except ValueError as error:
            _mismatches(mismatches, (row.module, row.table, row.raw_id, row.variant), str(error))
            continue
        # An unnamed native alternative remains in the source row count, but
        # cannot have a generated public field.  It is checked below for an
        # accidental generated counterpart rather than reported as missing.
        if inventory.rows[row].get("name") is not None:
            expected_ids.add(identity)

    layouts = getattr(generated, "layouts", None)
    if layouts is None:
        generated_scope = set(_attr(generated, "table_scope", set()))
    elif not isinstance(layouts, dict):
        _mismatches(mismatches, ("generated",), "word layouts projection is malformed")
        generated_scope = set()
    else:
        generated_scope = {key for key, layout in layouts.items() if layout is not None}
    expected_scope = {(key.module, key.table) for key in selected}
    for identity in sorted(expected_scope - generated_scope):
        _mismatches(mismatches, identity, "selected native table is absent from generated word projection")
    for identity in sorted(generated_scope - expected_scope):
        _mismatches(mismatches, identity, "generated word table has no selected native processor table")

    table_groups = _attr(generated, "table_groups", {})
    for table_key in sorted(selected):
        identity = (table_key.module, table_key.table)
        native_table = inventory.tables[table_key]
        native_groups, error = _table_groups(native_table)
        if error or table_groups.get(identity) != native_groups:
            _mismatches(mismatches, identity, error or "table Groups differ from native source")
        # The complete map is driven by ExifTool's live `%specialTags`.
        # Missing metadata is intentionally a refusal: old inventory records
        # remain parseable, but cannot certify a future WordDirectory route.
        metadata = native_table.get("metadata")
        if not isinstance(metadata, dict):
            _mismatches(mismatches, identity, "complete native table metadata is unavailable")
        else:
            processor_name = inventory.processors[table_key].get("__name")
            for property_name, property_fact in sorted(metadata.items()):
                if not isinstance(property_fact, dict) or not isinstance(property_fact.get("present"), bool):
                    _mismatches(mismatches, identity, f"table {property_name} metadata fact is malformed")
                    continue
                if not property_fact["present"]:
                    continue
                if property_name == "GROUPS":
                    continue
                if property_name == "PROCESS_PROC":
                    native = property_fact.get("value")
                    if not isinstance(native, dict) or native.get("kind") != "code" or native.get("name") != processor_name:
                        _mismatches(mismatches, identity, "table PROCESS_PROC does not match processor provenance")
                    continue
                if property_name in _IGNORED_TABLE_METADATA:
                    continue
                _mismatches(mismatches, identity, f"unsupported native table metadata {property_name} has no WordDirectory representation")
        # Keep these legacy top-level copies checked for records generated
        # before `metadata`; fresh records above see the same properties too.
        for property_name, label in (("format", "FORMAT"), ("first_entry", "FIRST_ENTRY")):
            property_fact = native_table.get(property_name)
            if not isinstance(property_fact, dict) or not isinstance(property_fact.get("present"), bool):
                _mismatches(mismatches, identity, f"table {label} fact is malformed")
            elif property_fact["present"]:
                _mismatches(mismatches, identity, f"unsupported native table {label} has no WordDirectory representation")

    fields = _attr(generated, "fields", {})
    generated_ids = {key for key in fields if tuple(key[:2]) in generated_scope}
    for key in sorted(expected_ids - generated_ids):
        _mismatches(mismatches, key, "named native row is absent from generated word projection")
    for key in sorted(generated_ids - expected_ids):
        _mismatches(mismatches, key, "generated word row has no named native row")

    omissions_rows = _attr(omissions, "rows", {})
    for omitted_key in omissions_rows:
        if len(omitted_key) < 3:
            _mismatches(mismatches, ("omission",), "malformed word omission identity")
            continue
        if tuple(omitted_key[:2]) in expected_scope:
            _mismatches(mismatches, tuple(omitted_key[:3]), "unverified word omission")

    enums = _attr(generated, "enums", {})
    for row_key in sorted(expected_rows):
        if row_key.variant != "-" and not _NUMERIC_ID.fullmatch(row_key.variant):
            _mismatches(mismatches, (row_key.module, row_key.table, row_key.raw_id, row_key.variant), "invalid native variant identity")
            continue
        try:
            key = _identity(row_key)
        except ValueError:
            continue
        row = inventory.rows[row_key]
        name = row.get("name")
        if name is None:
            if key in fields:
                _mismatches(mismatches, key, "unnamed native row was emitted")
            continue
        if not isinstance(name, str):
            _mismatches(mismatches, key, "native row name is malformed")
            continue
        if fields.get(key) != name:
            _mismatches(mismatches, key, "generated name differs from native row")
        _field_facts(row, key, generated, mismatches)
        present, value, error = _property(row, "PrintConv")
        if error:
            _mismatches(mismatches, key, error)
        elif not present:
            if key in enums:
                _mismatches(mismatches, key, "generated enum exists without native PrintConv")
        else:
            native_enum, enum_error = _enum(value)
            if enum_error or enums.get(key) != native_enum:
                _mismatches(mismatches, key, enum_error or "generated enum differs from native PrintConv")

    return WordRowAudit(len(selected), len(expected_rows), len(generated_scope), len(generated_ids), tuple(mismatches))
