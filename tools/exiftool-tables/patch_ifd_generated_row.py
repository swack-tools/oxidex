#!/usr/bin/env python3
"""Replace one generated IFD row from a fresh source dump without a bulk refresh.

This is deliberately narrower than ``codegen.py --ifd-out``: an artifact may
be on an older formatter while the source-derived behavior being forward
ported needs one row. The tool compiles that row through the same
``gen_ifd_tag_literal`` producer, verifies the existing generated row's id and
name before replacing it, and updates only the matching identity-ledger row.
It never accepts a caller-supplied Rust literal or map.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

import codegen
import table_modules


def tag_block(text: str, tag_id: int, expected_name: str) -> tuple[int, int, str]:
    """Return one current ``IfdTag`` literal, failing closed.

    The producer may emit a compact literal while older checked-in artifacts
    retain rustfmt's multi-line layout, so accept whitespace in either form.
    """
    pattern = re.compile(rf"        IfdTag\s*\{{\s*id:\s*0x{tag_id:04x},")
    matches = list(pattern.finditer(text))
    if not matches:
        raise SystemExit(f"generated row 0x{tag_id:04x} was not found")
    if len(matches) != 1:
        raise SystemExit(f"generated row 0x{tag_id:04x} is ambiguous")
    start = matches[0].start()

    depth = 0
    end = None
    for index in range(start, len(text)):
        if text[index] == "{":
            depth += 1
        elif text[index] == "}":
            depth -= 1
            if depth == 0:
                end = index + 1
                break
    if end is None or not text[end:].startswith(","):
        raise SystemExit(f"generated row 0x{tag_id:04x} has an unreadable brace boundary")
    block = text[start:end]
    if f'name: "{expected_name}"' not in block:
        raise SystemExit(
            f"generated row 0x{tag_id:04x} did not name {expected_name!r}; refusing replacement"
        )
    return start, end, block


def normalized_rust_literal(literal: str) -> str:
    """Return rustfmt's canonical syntax for one generated ``IfdTag``.

    Whitespace deletion is insufficient because rustfmt also inserts optional
    trailing commas in struct fields and arrays. Formatting both sides in the
    same parse context compares Rust syntax while preserving string bytes.
    """
    root = Path(__file__).resolve().parents[2]
    with tempfile.TemporaryDirectory(prefix="oxidex-ifd-row-format-") as directory:
        path = Path(directory) / "row.rs"
        path.write_text(f"fn generated_row() -> IfdTag {{\n{literal}\n}}\n")
        result = subprocess.run(
            [
                "rustfmt",
                "--edition",
                "2024",
                "--config-path",
                str(root / "rustfmt.toml"),
                str(path),
            ],
            capture_output=True,
            text=True,
        )
        if result.returncode:
            raise SystemExit(f"generated Rust literal did not rustfmt: {result.stderr.strip()[-400:]}")
        return path.read_text()


def compiled_row(doc: dict, module: str, table: str, tag_id: int) -> tuple[str, dict, dict]:
    try:
        source_tag = doc["modules"][module]["tables"][table]["tags"][str(tag_id)]
    except KeyError as exc:
        raise SystemExit(f"fresh source has no {module}::{table} tag {tag_id}") from exc
    expected_name = source_tag.get("Name")
    if not isinstance(expected_name, str):
        raise SystemExit(f"fresh source {module}::{table} tag {tag_id} has no usable Name")

    stats = codegen.new_ifd_stats()
    context = codegen.IfdGenContext.from_doc(doc)
    literal, refusal = codegen.gen_ifd_tag_literal(
        source_tag,
        tag_id,
        stats,
        set(),
        context,
        doc["modules"][module]["tables"][table].get("meta") or {},
        enclosing=(module, table),
    )
    if literal is None or refusal is not None:
        raise SystemExit(
            f"fresh source {module}::{table} tag {tag_id} was not emitted: {refusal!r}"
        )
    if "Omitted::NONE" not in literal:
        raise SystemExit(f"fresh source {module}::{table} tag {tag_id} remains withheld")
    _table_source, identities = codegen.gen_ifd_table(
        module,
        table,
        doc["modules"][module]["tables"][table],
        codegen.new_ifd_stats(),
        set(),
        context,
        include_identity_ledger=True,
    )
    matches = [
        row
        for row in identities
        if row.get("raw_key") == str(tag_id)
        and row.get("variant_path") == []
        and row.get("name") == expected_name
    ]
    if len(matches) != 1:
        raise SystemExit(
            f"fresh source did not produce exactly one identity for {module}::{table}:{tag_id}"
        )
    identity = {
        "module": module,
        "table": table,
        "full_name": f"Image::ExifTool::{module}::{table}",
        **matches[0],
    }
    return literal, source_tag, identity


def ledger_counts(rows: list[dict]) -> dict:
    """Recompute and validate the aggregate classification census."""
    for index, row in enumerate(rows):
        artifact = row.get("artifact_state")
        if artifact not in ("emitted", "refused") or row.get("state") != artifact:
            raise SystemExit(f"identity ledger row {index} has inconsistent artifact state")
        omissions = row.get("omissions")
        if not isinstance(omissions, list):
            raise SystemExit(f"identity ledger row {index} has invalid omissions")
        expected_reader = (
            "refused" if artifact == "refused" else "omitted" if omissions else "eligible"
        )
        if row.get("reader_state") != expected_reader:
            raise SystemExit(f"identity ledger row {index} has inconsistent reader classification")
    return {
        "rows": len(rows),
        "emitted": sum(row["artifact_state"] == "emitted" for row in rows),
        "refused": sum(row["artifact_state"] == "refused" for row in rows),
        "reader_eligible": sum(row["reader_state"] == "eligible" for row in rows),
        "reader_omitted": sum(row["reader_state"] == "omitted" for row in rows),
    }


def updated_ledger(
    path: Path,
    source_bytes: bytes,
    exiftool_version: str,
    expected_row: dict,
    artifact_sha256: str,
    check: bool,
) -> bytes:
    document = json.loads(path.read_text())
    binding = document.get("source")
    if (
        document.get("schema") != "oxidex_ifd_identity_ledger_v1"
        or document.get("exiftool_version") != exiftool_version
        or not isinstance(binding, dict)
        or binding.get("tables_json_sha256") != hashlib.sha256(source_bytes).hexdigest()
        or binding.get("ifd_rust_hash_format") != codegen.IFD_RUST_HASH_FORMAT
    ):
        raise SystemExit("identity ledger has inconsistent source or artifact bindings")
    rows = document.get("rows")
    if not isinstance(rows, list):
        raise SystemExit("identity ledger rows are not a list")
    matches = [
        (index, row)
        for index, row in enumerate(rows)
        if row.get("module") == expected_row["module"]
        and row.get("table") == expected_row["table"]
        and row.get("raw_key") == expected_row["raw_key"]
        and row.get("variant_path") == expected_row["variant_path"]
    ]
    if len(matches) != 1:
        raise SystemExit("identity ledger did not contain exactly one source coordinate")
    index, current_row = matches[0]
    if (
        current_row.get("name") != expected_row["name"]
        or current_row.get("full_name") != expected_row["full_name"]
        or current_row.get("source_sha256") != expected_row["source_sha256"]
    ):
        raise SystemExit("identity ledger row does not match the fresh source identity")

    result = copy.deepcopy(document)
    result["rows"][index] = expected_row
    expected_counts = ledger_counts(result["rows"])
    if check:
        if current_row != expected_row:
            raise SystemExit("identity ledger row classification does not match fresh production")
        if document.get("counts") != expected_counts:
            raise SystemExit("identity ledger aggregate counts do not match its rows")
        if binding.get("ifd_rust_sha256") != artifact_sha256:
            raise SystemExit("identity ledger artifact digest does not match generated IFD Rust")
    result["counts"] = expected_counts
    result["source"]["ifd_rust_sha256"] = artifact_sha256
    return (json.dumps(result, indent=2, sort_keys=True) + "\n").encode()


def write_transaction(outputs: list[tuple[Path, bytes]]) -> None:
    """Stage every output before replacing either destination."""
    staged: list[tuple[Path, Path]] = []
    try:
        for path, content in outputs:
            with tempfile.NamedTemporaryFile(dir=path.parent, prefix=f".{path.name}.", delete=False) as file:
                os.fchmod(file.fileno(), path.stat().st_mode & 0o7777)
                file.write(content)
                file.flush()
                os.fsync(file.fileno())
                staged.append((Path(file.name), path))
        for temporary, destination in staged:
            os.replace(temporary, destination)
    finally:
        for temporary, _destination in staged:
            temporary.unlink(missing_ok=True)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("tables_json", type=Path)
    parser.add_argument("--module", required=True)
    parser.add_argument("--table", required=True)
    parser.add_argument("--tag-id", required=True, type=lambda value: int(value, 0))
    parser.add_argument("--generated-row", required=True, type=Path)
    parser.add_argument("--identity-ledger", required=True, type=Path)
    parser.add_argument("--check", action="store_true", help="verify the exact replacement without writing")
    args = parser.parse_args()

    source_bytes = args.tables_json.read_bytes()
    doc = json.loads(source_bytes)
    literal, source_tag, identity = compiled_row(doc, args.module, args.table, args.tag_id)
    current = args.generated_row.read_text()
    start, end, _ = tag_block(current, args.tag_id, source_tag["Name"])
    replacement = f"        {literal}"
    if args.check:
        if normalized_rust_literal(current[start:end]) != normalized_rust_literal(replacement):
            raise SystemExit(
                f"generated row 0x{args.tag_id:04x} does not match fresh source output"
            )
        generated_text = current
    else:
        generated_text = current[:start] + replacement + current[end:]

    hub = args.generated_row.with_name(table_modules.MOD_RS)
    files = table_modules.read_files(hub)
    if args.generated_row.name not in files:
        raise SystemExit(f"{args.generated_row} is not declared by {hub}")
    files[args.generated_row.name] = generated_text
    artifact_sha256 = codegen._canonical_ifd_rust_sha256(files)
    ledger_bytes = updated_ledger(
        args.identity_ledger,
        source_bytes,
        str(doc.get("exiftool_version")),
        identity,
        artifact_sha256,
        args.check,
    )
    if not args.check:
        write_transaction(
            [
                (args.generated_row, generated_text.encode()),
                (args.identity_ledger, ledger_bytes),
            ]
        )
    print(
        f"{'verified' if args.check else 'updated'} {args.module}::{args.table} "
        f"0x{args.tag_id:04x} ({source_tag['Name']}) from {args.tables_json}"
    )


if __name__ == "__main__":
    main()
