"""Replay the generated Garmin FIT reader before joining it to catalog rows.

The committed FIT ledger and Rust are accepted only when they replay exactly
from the bounded Garmin source fixture, whose Garmin module must equal the
authenticated full dump the IFD join already binds, whose protocol fact must
equal a fresh capture_garmin_fit_fact.pl capture of the pinned tree, and whose
ProcessFIT source must be the catalog's own pinned Garmin.pm.  Conversions are admitted
only through that dump's source-bound expression-oracle ledger.

This module classifies source-derived declarations only.  Observed read credit
requires a separate authenticated fixture receipt.
"""
from __future__ import annotations

import json
import tempfile
from pathlib import Path

import codegen
import garmin_fit_specs as specs

# Connections ExifTool's default extraction reaches (garmin-fit-source-review.md).
DEFAULT_MODE_CONNECTIONS = frozenset({
    "default_mode_field_list", "default_mode_common", "protocol_header",
    "message_edge", "message_edge_synthesized_table", "common_edge",
})
# Reached natively only with the Unknown option, which OxiDex does not expose.
OPTION_GATED_CONNECTIONS = frozenset({"unknown_option_not_exposed"})


def implementation(ledger, bounded_source, emitted_rust, *, dump_source, expr_ledger, protocol_fact,
                   catalog_sources, rust_matches):
    """-> {(table, raw_key, variant_index): row} for every replayed FIT source row."""
    supplied = (ledger, bounded_source, emitted_rust)
    if all(value is None for value in supplied):
        return {}
    if any(value is None for value in supplied):
        raise ValueError("Garmin FIT replay requires source, ledger, and Rust together")
    if not isinstance(bounded_source, bytes) or not isinstance(emitted_rust, str) or not isinstance(ledger, dict):
        raise ValueError("Garmin FIT replay input has an invalid type")
    if dump_source is None or expr_ledger is None or protocol_fact is None:
        raise ValueError("Garmin FIT replay requires the authenticated full dump, expression ledger and fresh protocol fact")
    document = json.loads(bounded_source)
    dump = json.loads(dump_source)
    protocol = document.get("garmin_fit_reader_protocol")
    if (document.get("exiftool_version") != dump.get("exiftool_version")
            or document.get("modules", {}).get(specs.MODULE) != dump.get("modules", {}).get(specs.MODULE)):
        raise ValueError("Garmin FIT bounded source differs from the authenticated full dump")
    # Base types, format sizes, integer width and dependency bodies all feed
    # the generated Rust; only a fresh native capture vouches for them.
    if protocol != json.loads(protocol_fact):
        raise ValueError("Garmin FIT protocol fact differs from the fresh native capture")
    process_fit = (protocol or {}).get("process_fit") or {}
    source_fact = catalog_sources.get(process_fit.get("source_file"))
    if not isinstance(source_fact, dict) or source_fact.get("sha256") != process_fit.get("source_sha256"):
        raise ValueError("Garmin FIT protocol source differs from catalog provenance")
    with tempfile.TemporaryDirectory(prefix="oxidex-fit-replay-") as directory:
        dump_path, oracle_path = Path(directory) / "tables.json", Path(directory) / "expr-ledger.json"
        dump_path.write_bytes(dump_source)
        oracle_path.write_bytes(expr_ledger)
        try:
            verified = codegen.load_oracle_ledger(str(oracle_path), str(dump_path),
                                                  str(dump.get("exiftool_version") or ""))
        except SystemExit as exc:
            raise ValueError("Garmin FIT expression ledger fails source-bound validation") from exc
    rust, expected = specs.generate(document, verified, protocol)
    if ledger != json.loads(specs.serialized(expected)) or not rust_matches(rust, emitted_rust):
        raise ValueError("Garmin FIT artifacts differ from complete source replay")
    if not expected["protocol"]["admitted"]:
        raise ValueError("Garmin FIT protocol is refused; no row can be a reader declaration")
    rows = {}
    for record in expected["rows"]:
        identity = record["identity"]
        key = (identity["table"], identity["raw_key"], identity["variant_index"])
        if key in rows:
            raise ValueError("Garmin FIT ledger duplicates a source identity")
        connection = record["runtime_connection"]
        if connection not in DEFAULT_MODE_CONNECTIONS | OPTION_GATED_CONNECTIONS:
            raise ValueError(f"Garmin FIT runtime connection is unclassified: {connection}")
        rows[key] = {"generated": record["generated"], "reasons": record["reasons"],
                     "runtime_connection": connection, "name": record["name"]}
    counts = expected["counts"]
    if len(rows) != counts["source_rows"] or sum(row["generated"] for row in rows.values()) != counts["generated"]:
        raise ValueError("Garmin FIT source identity conservation failed")
    return rows


def reader_state(row) -> tuple[str, list | None]:
    """Catalog reader classification and refusal reasons for one replayed row."""
    if not row["generated"]:
        return "blocked_generated_reader_refusal", row["reasons"]
    if row["runtime_connection"] in OPTION_GATED_CONNECTIONS:
        return "generated_reader_declaration_option_gated", None
    return "generated_reader_declaration_unobserved", None
