#!/usr/bin/env python3
"""Bind unchanged IFD declarations to a fresh, independently verified capture.

This is a CI producer, not an exemption in the catalog receipt validator. Both
the old and fresh expression ledgers retain their original source bindings.
Only an exact compiler artifact/row replay may produce a new identity ledger.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
from pathlib import Path
import re
import tempfile

import codegen
import table_modules


def digest(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def replay(source: bytes, expected: dict, rust: dict,
           old_oracle: bytes, fresh_oracle: bytes) -> dict:
    """`rust` is the committed IFD artifact's file set (`table_modules.read_files`
    of `src/exiftool_tables/ifd/mod.rs`); the ledger binds its
    `codegen.IFD_RUST_HASH_FORMAT` digest."""
    document = json.loads(source)
    if not isinstance(rust, dict):
        raise ValueError("IFD Rust artifact must be its file set, not one string")
    old, fresh = json.loads(old_oracle), json.loads(fresh_oracle)
    if not all(isinstance(item, dict) for item in (document, expected, old, fresh)):
        raise ValueError("IFD replay inputs must be objects")
    binding = expected.get("source", {})
    if (expected.get("schema") != "oxidex_ifd_identity_ledger_v1"
            or expected.get("exiftool_version") != document.get("exiftool_version")
            or binding.get("expr_ledger_sha256") != digest(old_oracle)
            or binding.get("ifd_rust_hash_format") != codegen.IFD_RUST_HASH_FORMAT):
        raise ValueError("committed IFD inputs have inconsistent source/oracle bindings")
    # The old source bytes need not exist on this runner. Their digest must
    # still agree between the committed identity ledger and oracle receipt.
    old_digest = binding.get("tables_json_sha256")
    if (not isinstance(old_digest, str) or not re.fullmatch(r"[a-f0-9]{64}", old_digest)
            or old.get("tables_sha256") != old_digest):
        raise ValueError("committed IFD and expression source bindings differ")
    for oracle in (old, fresh):
        if (oracle.get("schema") != codegen.LEDGER_SCHEMA
                or oracle.get("exiftool_version") != document.get("exiftool_version")
                or oracle.get("probe_counts", {}).get("fail") != 0
                or type(oracle.get("probe_counts", {}).get("pass")) is not int
                or oracle["probe_counts"]["pass"] <= 0):
            raise ValueError("IFD expression oracle has no successful compatible proof")
    # A newly successful but smaller oracle cohort cannot quietly reduce the
    # generated conversion coverage. Compare the complete approved inventory
    # and source use counts; interpreter paths/timing/probe counts may differ.
    for field in ("verified_expressions", "expression_counts", "use_counts"):
        if field not in old or old[field] != fresh.get(field):
            raise ValueError("fresh expression oracle inventory differs: " + field)
    with tempfile.TemporaryDirectory(prefix="oxidex-ifd-catalog-replay-") as directory:
        tables, oracle = Path(directory) / "tables.json", Path(directory) / "oracle.json"
        tables.write_bytes(source)
        oracle.write_bytes(fresh_oracle)
        try:
            verified = codegen.load_oracle_ledger(
                str(oracle), str(tables), str(document.get("exiftool_version")))
        except SystemExit as exc:
            raise ValueError("fresh expression oracle is not bound to this capture") from exc
    chunks, index, _, rows = codegen.gen_ifd_tables(
        document, sorted(document.get("modules", {})), verified)
    rendered = codegen.render_ifd_files(str(document.get("exiftool_version")), chunks, index)
    artifact_sha = codegen._canonical_ifd_rust_sha256(rust)
    if (binding.get("ifd_rust_sha256") != artifact_sha
            or codegen._canonical_ifd_rust_sha256(rendered) != artifact_sha):
        raise ValueError("IFD Rust differs from committed binding or fresh compiler replay")
    rows = sorted(rows, key=lambda row: (row["full_name"], row["raw_key"], tuple(row["variant_path"])))
    counts = {"rows": len(rows),
              "emitted": sum(row["artifact_state"] == "emitted" for row in rows),
              "refused": sum(row["artifact_state"] == "refused" for row in rows),
              "reader_eligible": sum(row["reader_state"] == "eligible" for row in rows),
              "reader_omitted": sum(row["reader_state"] == "omitted" for row in rows)}
    if rows != expected.get("rows") or counts != expected.get("counts"):
        raise ValueError("IFD complete rows/classifications/counts differ from fresh compiler replay")
    ownership_rows = codegen.gen_ownership_identities(
        document, sorted(document.get("modules", {}))
    )
    ownership_counts = codegen.ownership_identity_counts(ownership_rows)
    if (
        ownership_rows != expected.get("ownership_rows")
        or ownership_counts != expected.get("ownership_counts")
    ):
        raise ValueError("ownership rows/counts differ from fresh compiler replay")
    result = copy.deepcopy(expected)
    result["source"]["tables_json_sha256"] = digest(source)
    result["source"]["expr_ledger_sha256"] = digest(fresh_oracle)
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--committed-ledger", type=Path, required=True)
    parser.add_argument("--committed-rust", type=Path, required=True,
                        help="the committed IFD hub, src/exiftool_tables/ifd/mod.rs (its "
                             "module files are read from beside it)")
    parser.add_argument("--committed-expr-ledger", type=Path, required=True)
    parser.add_argument("--fresh-expr-ledger", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    inputs = (args.source, args.committed_ledger, args.committed_rust,
              args.committed_expr_ledger, args.fresh_expr_ledger)
    if args.output.exists() or args.output.resolve() in {path.resolve() for path in inputs}:
        parser.error("output must be a new path distinct from every input")
    result = replay(args.source.read_bytes(), json.loads(args.committed_ledger.read_bytes()),
                    table_modules.read_files(args.committed_rust), args.committed_expr_ledger.read_bytes(),
                    args.fresh_expr_ledger.read_bytes())
    with args.output.open("x", encoding="utf-8") as stream:
        stream.write(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print("=== instrument: replay_ifd_catalog_inputs.py ===")
    print(f"Exact IFD Rust and {len(result['rows'])} complete source rows match committed inputs.")
    print(f"Fresh source SHA256: {result['source']['tables_json_sha256']}")
    print(f"Fresh expression oracle SHA256: {result['source']['expr_ledger_sha256']}")


if __name__ == "__main__":
    main()
