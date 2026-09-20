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
import json
import re
import sys
from pathlib import Path

import codegen


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
    """Ignore layout outside strings while preserving every source data byte.

    The narrow producer emits a compact literal, then ``cargo fmt`` may lay
    that same Rust syntax across lines. Comparing this token shape permits the
    formatter-only difference without treating whitespace inside a generated
    map key or rendered value as interchangeable.
    """
    normalized = []
    in_string = False
    escaped = False
    for char in literal:
        if in_string:
            normalized.append(char)
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
        elif char == '"':
            normalized.append(char)
            in_string = True
        elif not char.isspace():
            normalized.append(char)
    if in_string or escaped:
        raise SystemExit("generated Rust literal had an unterminated string")
    return "".join(normalized)


def compiled_row(doc: dict, module: str, table: str, tag_id: int) -> tuple[str, dict]:
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
    )
    if literal is None or refusal is not None:
        raise SystemExit(
            f"fresh source {module}::{table} tag {tag_id} was not emitted: {refusal!r}"
        )
    if "Omitted::NONE" not in literal:
        raise SystemExit(f"fresh source {module}::{table} tag {tag_id} remains withheld")
    return literal, source_tag


def update_ledger(path: Path, module: str, table: str, tag_id: int, expected_name: str, write: bool) -> None:
    document = json.loads(path.read_text())
    matches = [
        row
        for row in document.get("rows", [])
        if row.get("module") == module
        and row.get("table") == table
        and row.get("raw_key") == str(tag_id)
        and row.get("name") == expected_name
    ]
    if len(matches) != 1:
        raise SystemExit(f"identity ledger did not contain exactly one {module}::{table}:{tag_id} row")
    row = matches[0]
    row["artifact_state"] = "emitted"
    row["reader_state"] = "eligible"
    row["omissions"] = []
    if write:
        path.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")


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

    doc = json.loads(args.tables_json.read_text())
    literal, source_tag = compiled_row(doc, args.module, args.table, args.tag_id)
    current = args.generated_row.read_text()
    start, end, _ = tag_block(current, args.tag_id, source_tag["Name"])
    replacement = f"        {literal}"
    if args.check:
        if normalized_rust_literal(current[start:end]) != normalized_rust_literal(replacement):
            raise SystemExit(
                f"generated row 0x{args.tag_id:04x} does not match fresh source output"
            )
    else:
        args.generated_row.write_text(current[:start] + replacement + current[end:])
    update_ledger(
        args.identity_ledger,
        args.module,
        args.table,
        args.tag_id,
        source_tag["Name"],
        not args.check,
    )
    print(
        f"{'verified' if args.check else 'updated'} {args.module}::{args.table} "
        f"0x{args.tag_id:04x} ({source_tag['Name']}) from {args.tables_json}"
    )


if __name__ == "__main__":
    main()
