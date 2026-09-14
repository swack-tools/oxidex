#!/usr/bin/env python3
"""Build a non-promoting inventory of native ExifTool write declarations.

The input is one ``dump_tables.pl`` JSON capture.  This tool deliberately does
not execute OxiDex or ExifTool, discover a file format route, or turn a table
declaration into a successful operation.  Every emitted record is therefore
``unexercised`` and the overall coverage percentage is null.

Rows are flattened only to give conditional alternatives stable identities.
Their original source entry, including unknown controls and nested variants,
is retained in each record so a later operation runner can choose a concrete
native context without losing evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from collections import Counter
from pathlib import Path
from typing import Any, Mapping, Sequence


SCHEMA = "oxidex_native_write_definition_catalog_v2"
UNEXERCISED = "unexercised_no_validated_operation"


class CatalogError(ValueError):
    """The supplied capture cannot support an honest definition inventory."""


def _mapping(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise CatalogError(f"{context} must be an object")
    return value


def _sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _canonical_sha256(value: Any) -> str:
    return _sha256_bytes(json.dumps(value, sort_keys=True, separators=(",", ":"),
                                      ensure_ascii=False).encode("utf-8"))


def _fact(value: Any, context: str) -> tuple[str, Any]:
    """Return ``present``, ``absent`` or ``malformed`` without discarding it."""
    if not isinstance(value, Mapping) or not isinstance(value.get("present"), bool):
        return "malformed", value
    if value["present"]:
        return ("present", value.get("value")) if "value" in value else ("malformed", value)
    return ("absent", None) if "value" not in value else ("malformed", value)


def _native_truth(value: Any) -> str:
    """Classify a captured scalar truth value without coercing references."""
    if value is None or value is False or value == 0 or value == "" or value == "0":
        return "false"
    if value is True or isinstance(value, (int, float, str)):
        return "true"
    return "unknown_non_scalar"


def _effective_writable(entry: Mapping[str, Any]) -> dict[str, Any]:
    properties = entry.get("effective_properties")
    if not isinstance(properties, Mapping):
        return {"state": "unknown_no_effective_properties", "fact": None}
    fact = properties.get("Writable")
    state, value = _fact(fact, "effective Writable")
    if state == "present":
        return {"state": f"native_declared_{_native_truth(value)}", "fact": fact}
    if state == "absent":
        return {"state": "unknown_effective_writable_absent", "fact": fact}
    return {"state": "unknown_effective_writable_malformed", "fact": fact}


def _write_directory(entry: Mapping[str, Any]) -> dict[str, Any]:
    properties = entry.get("effective_properties")
    if not isinstance(properties, Mapping):
        return {"state": "unknown_no_effective_properties", "effective_fact": None}
    fact = properties.get("WriteGroup")
    fact_state, value = _fact(fact, "effective WriteGroup")
    if fact_state == "present" and isinstance(value, str):
        return {"state": "resolved_literal", "effective_fact": fact, "value": value}
    if fact_state == "present":
        return {"state": "unresolved_nonliteral", "effective_fact": fact}
    if fact_state == "absent":
        return {"state": "not_declared", "effective_fact": fact}
    return {"state": "malformed", "effective_fact": fact}


def _name_hint(entry: Mapping[str, Any]) -> Any:
    properties = entry.get("effective_properties")
    if not isinstance(properties, Mapping):
        return None
    state, value = _fact(properties.get("Name"), "effective Name")
    return value if state == "present" and isinstance(value, str) else None


def _alternatives(entry: Any, path: tuple[int, ...] = ()) -> list[tuple[tuple[int, ...], Any, str]]:
    """Flatten arrays; an empty or malformed array remains an explicit record."""
    if isinstance(entry, Mapping) and entry.get("entry_kind") == "ARRAY" and isinstance(entry.get("alternatives"), list):
        if not entry["alternatives"]:
            return [(path, entry, "unresolved_empty_alternatives")]
        result: list[tuple[tuple[int, ...], Any]] = []
        for index, child in enumerate(entry["alternatives"]):
            result.extend(_alternatives(child, path + (index,)))
        return result
    if isinstance(entry, Mapping) and entry.get("entry_kind") == "ARRAY":
        return [(path, entry, "unresolved_malformed_alternatives")]
    return [(path, entry, "resolved_source_variant")]


def _row_record(module: str, table_name: str, table: Mapping[str, Any], raw_id: str,
                table_context_ref: str, source_entry_ref: str, variant_path: tuple[int, ...],
                variant: Any, variant_state: str) -> dict[str, Any]:
    entry = variant if isinstance(variant, Mapping) else {}
    return {
        "identity": {
            "module": module,
            "table": table_name,
            "full_name": table.get("full_name"),
            "raw_id": raw_id,
            "variant_path": list(variant_path),
            "tag_name_hint": _name_hint(entry),
            "table_context_ref": table_context_ref,
            "source_entry_ref": source_entry_ref,
            "source_variant_sha256": _canonical_sha256(variant),
            "source_variant_state": variant_state,
        },
        "native_definition": {
            "effective_writable": _effective_writable(entry),
            "effective_write_directory": _write_directory(entry),
            "row_write_controls": entry.get("write_controls"),
            "row_unknown_controls": entry.get("unknown_properties"),
            "effective_resolution": entry.get("effective_resolution"),
            "effective_table_binding": entry.get("effective_table_binding"),
        },
        "operation_coverage": {
            "state": UNEXERCISED,
            "carrier_route": "not_inferred_from_definition_inventory",
            "validated_observations": [],
        },
    }


def _table_context(context_id: str, module: str, table_name: str, table: Mapping[str, Any]) -> dict[str, Any]:
    """Store the full shared table context once, excluding only its row map."""
    return {
        "id": context_id,
        "module": module,
        "table": table_name,
        "full_name": table.get("full_name"),
        "table_properties": table.get("table_properties"),
        "write_controls": table.get("write_controls"),
        "unknown_table_properties": table.get("unknown_table_properties"),
        "effective_write_proc": table.get("effective_write_proc"),
        "effective_check_proc": table.get("effective_check_proc"),
        "effective_row_resolver": table.get("effective_row_resolver"),
        "effective_row_context": table.get("effective_row_context"),
    }


def resolve_source_variant(catalog: Mapping[str, Any], record: Mapping[str, Any]) -> Any:
    """Return a record's retained source branch, for consumers and regression tests."""
    identity = _mapping(record.get("identity"), "record.identity")
    source_id = identity.get("source_entry_ref")
    entries = _mapping(catalog.get("source_entries"), "catalog.source_entries")
    source = _mapping(entries.get(source_id), "catalog source entry")
    value = source.get("entry")
    for index in identity.get("variant_path", []):
        node = _mapping(value, "source variant array")
        alternatives = node.get("alternatives")
        if node.get("entry_kind") != "ARRAY" or not isinstance(alternatives, list) or not isinstance(index, int):
            raise CatalogError("record variant path cannot be resolved from retained source entry")
        value = alternatives[index]
    return value


def _root_fact(document: Mapping[str, Any], name: str) -> dict[str, Any]:
    return {"present": name in document, "value": document.get(name)} if name in document else {"present": False}


def _global_capture_completeness(document: Mapping[str, Any]) -> str:
    failed = document.get("modules_failed")
    load_errors = document.get("load_errors")
    if isinstance(failed, int) and failed > 0:
        return "partial_modules_failed"
    if isinstance(load_errors, list) and load_errors:
        return "partial_load_errors"
    if not isinstance(document.get("modules"), Mapping) or not isinstance(document.get("modules_ok"), int):
        return "unknown_module_capture"
    # ``modules_ok`` only says the supplied module selection loaded.  A narrow
    # Exif-only capture must never masquerade as a complete all-module capture.
    return "no_reported_module_failures_scope_unknown"


def build_catalog(document: Mapping[str, Any], *, dump_sha256: str) -> dict[str, Any]:
    """Compile all native-write rows into deterministic, unexercised records."""
    document = _mapping(document, "dump document")
    tables = _mapping(document.get("native_write_tables"), "native_write_tables")
    records: list[dict[str, Any]] = []
    contexts: dict[str, dict[str, Any]] = {}
    source_entries: dict[str, dict[str, Any]] = {}
    table_count = 0
    raw_row_count = 0

    for module in sorted(tables):
        table_map = _mapping(tables[module], f"native_write_tables[{module!r}]")
        for table_name in sorted(table_map):
            table = _mapping(table_map[table_name], f"native_write_tables[{module!r}][{table_name!r}]")
            rows = _mapping(table.get("rows"), f"{module}::{table_name}.rows")
            table_count += 1
            table_context_ref = f"table-{table_count:05d}"
            contexts[table_context_ref] = _table_context(table_context_ref, module, table_name, table)
            for raw_id in sorted(rows):
                raw_row_count += 1
                source_row = rows[raw_id]
                source_entry_ref = f"row-{raw_row_count:07d}"
                source_entries[source_entry_ref] = {
                    "id": source_entry_ref,
                    "module": module,
                    "table": table_name,
                    "raw_id": raw_id,
                    "entry_sha256": _canonical_sha256(source_row),
                    "entry": source_row,
                }
                for path, variant, variant_state in _alternatives(source_row):
                    records.append(_row_record(module, table_name, table, raw_id, table_context_ref,
                                               source_entry_ref, path, variant, variant_state))

    records.sort(key=lambda item: (
        item["identity"]["module"], item["identity"]["table"], item["identity"]["raw_id"],
        item["identity"]["variant_path"], item["identity"]["source_variant_sha256"],
    ))
    writable_counts = Counter(record["native_definition"]["effective_writable"]["state"] for record in records)
    directory_counts = Counter(record["native_definition"]["effective_write_directory"]["state"] for record in records)
    capture = document.get("native_write_capture_context")
    registry = document.get("native_write_format_registry")
    capture_status = "absent"
    if isinstance(capture, Mapping):
        capture_status = "resolved" if capture.get("resolved") is True else "unresolved_or_incomplete"
    registry_status = registry.get("state") if isinstance(registry, Mapping) else "absent"
    return {
        "schema": SCHEMA,
        "source": {
            "dump_sha256": dump_sha256,
            "canonical_dump_sha256": _canonical_sha256(document),
            "exiftool_version": document.get("exiftool_version"),
            "capture_completeness": {
                "native_write_tables": "present",
                "global_module_capture": _global_capture_completeness(document),
                "native_writer_context": capture_status,
                "native_tiff_type_registry": registry_status,
            },
            "root_module_facts": {name: _root_fact(document, name) for name in
                                  ("modules", "modules_ok", "modules_failed", "load_errors")},
            "native_writer_context": capture,
            # This is Exif.pm's TIFF field-type registry, never a file-format
            # or carrier-reachability assertion.
            "native_tiff_type_registry": registry,
        },
        "metrics": {
            "native_definition_tables": table_count,
            "native_definition_raw_rows": raw_row_count,
            "native_definition_variant_records": len(records),
            "effective_writable_states": dict(sorted(writable_counts.items())),
            "effective_write_directory_states": dict(sorted(directory_counts.items())),
            "validated_operation_records": 0,
            "coverage_percentage": None,
            "coverage_reason": "No validated OxiDex/native write observations are present in a source definition catalog.",
        },
        "table_contexts": contexts,
        "source_entries": source_entries,
        "records": records,
    }


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dump", type=Path, required=True, help="captured dump_tables.pl JSON")
    parser.add_argument("--output", type=Path, required=True, help="catalog JSON to write")
    args = parser.parse_args(argv)
    raw = args.dump.read_bytes()
    try:
        document = json.loads(raw)
    except json.JSONDecodeError as error:
        parser.error(f"--dump is not JSON: {error}")
    try:
        catalog = build_catalog(document, dump_sha256=_sha256_bytes(raw))
    except CatalogError as error:
        parser.error(str(error))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(catalog, sort_keys=True, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
