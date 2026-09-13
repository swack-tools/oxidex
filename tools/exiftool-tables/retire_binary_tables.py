#!/usr/bin/env python3
"""Remove shared-owned tables from a generated legacy binary-table artifact.

The ownership manifest selects a producer/table pair only.  It intentionally
contains no offsets, names, conversions, or other native semantics.  This
tool preserves surviving fields except for child-index remapping, removes the
selected `BinTable` and its backing `Tn` declaration, regenerates table indices,
and updates literal child-table indices from the parsed old ordering.

It is deliberately a narrow reader for the stable generated Rust shape.  A
shape it cannot prove causes a refusal before the output is replaced.

When an accepted legacy producer later regenerates this source, run this tool
again with the same ownership manifest and verified shared inputs.  A source
that already bears the matching marker is intentionally idempotent; a fresh
producer output is transformed and gets a new input/output identity record.
This transform does not migrate root dispatches. In Rust files below the
consumer root it checks literal `idx::TABLE` handles and these numeric forms:
`TABLES[n]`, `process(_with_ids)(TABLES, n, ...)`, and `table: Some(n)`.
Aliases, macros, other index expressions and general data flow are outside
this scan. The current Sony callers also received a separate source review.

The checks establish route presence and mechanically safe retirement. They do
not prove that the legacy and shared readers have equivalent native semantics;
the independent native inventory and runtime gates own that proof. Whole-file
shared/enabled hashes in the ledger are provenance for the original run, not a
replay gate: replay checks the current selected route separately.
An in-place replay leaves output and ledger untouched. A replay to a different
output path materializes the verified bytes there without changing the ledger.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import tempfile


class Refusal(ValueError):
    pass


TABLES_OPEN = "pub static TABLES: &[BinTable] = &[\n"
TABLES_CLOSE = "];\n\n/// Table numbers, by ExifTool table name.\n"
IDX_OPEN = "#[allow(dead_code)]\npub mod idx {\n"
MARKER_PREFIX = "// shared-table-retirement: "
TABLE_ENTRY = re.compile(
    r"    BinTable \{\n"
    r"        name: (?P<quote>\")(?P<name>[A-Za-z][A-Za-z0-9]*)(?P=quote),\n"
    r"        fmt: (?P<fmt>[^\n]+),\n"
    r"        tags: (?P<tag>T[0-9]+),\n"
    r"    \},\n"
)
TAG_BLOCK = re.compile(
    r"(?ms)^#\[rustfmt::skip\]\n"
    r"static (?P<tag>T[0-9]+): &\[BinTag\] = &\[\n"
    r"(?P<body>.*?)^\];\n"
)
BIN_TAG_FIELD = re.compile(r"^    BinTag \{ (?P<body>.*) \},\n?$")
SUBDIR_LABEL = re.compile(r", subdir\s*:")
SUBDIR_VALUE = re.compile(r", subdir: (?P<value>None|Some\((?P<index>[0-9]+)\))(?= \})")
ENABLED_OPEN = "pub static ENABLED: &[(&str, &str)] = &[\n"
ENABLED_CLOSE = "];\n\n/// Whether the generic engine may walk `table`.\n"
SOURCE_PATH = re.compile(r"(?:[A-Za-z0-9_.-]+/)*[A-Za-z0-9_.-]+\.rs")


def refuse(message):
    raise Refusal(message)


def canonical_source(source):
    if not isinstance(source, str) or not SOURCE_PATH.fullmatch(source):
        refuse("manifest source is not a relative Rust path")
    if source.startswith("/") or ".." in source.split("/"):
        refuse("manifest source escapes the repository")
    return source


def migrations_for(manifest_path, source):
    try:
        doc = json.loads(Path(manifest_path).read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise Refusal(f"unreadable ownership manifest: {error}") from error
    if not isinstance(doc, dict) or set(doc) != {"schema", "migrations"}:
        refuse("ownership manifest shape differs")
    if doc["schema"] != 1 or not isinstance(doc["migrations"], list):
        refuse("unsupported ownership manifest schema")
    selected = []
    seen = set()
    for entry in doc["migrations"]:
        if not isinstance(entry, dict) or set(entry) != {"source", "module", "table", "owner"}:
            refuse("ownership migration shape differs")
        entry_source = canonical_source(entry["source"])
        if not all(isinstance(entry[key], str) and entry[key] for key in ("module", "table", "owner")):
            refuse("ownership migration contains an empty identity")
        identity = (entry_source, entry["module"], entry["table"])
        if identity in seen:
            refuse("duplicate ownership migration")
        seen.add(identity)
        if entry_source == source:
            if entry["owner"] != "shared":
                refuse("retirement requires shared ownership")
            selected.append(entry)
    if not selected:
        refuse("ownership manifest selects no table for this source")
    tables = [entry["table"] for entry in selected]
    if len(set(tables)) != len(tables):
        refuse("source has duplicate table migrations")
    return selected


def parse_tables(text):
    if text.count(TABLES_OPEN) != 1 or text.count(TABLES_CLOSE) != 1:
        refuse("generated TABLES declaration differs")
    start = text.index(TABLES_OPEN) + len(TABLES_OPEN)
    end = text.index(TABLES_CLOSE, start)
    body = text[start:end]
    tables = []
    at = 0
    for match in TABLE_ENTRY.finditer(body):
        if body[at:match.start()].strip():
            refuse("unreadable BinTable declaration")
        tables.append({"name": match["name"], "tag": match["tag"], "text": match.group(0)})
        at = match.end()
    if not tables or body[at:].strip():
        refuse("unreadable or empty TABLES declaration")
    if len({table["name"] for table in tables}) != len(tables):
        refuse("duplicate BinTable name")
    if len({table["tag"] for table in tables}) != len(tables):
        refuse("duplicate BinTable backing declaration")
    return tables, start, end


def parse_tag_blocks(text, tables):
    blocks = {match["tag"]: match for match in TAG_BLOCK.finditer(text)}
    if len(blocks) != len(list(TAG_BLOCK.finditer(text))):
        refuse("duplicate BinTag declaration")
    expected = {table["tag"] for table in tables}
    if set(blocks) != expected:
        refuse("TABLES and BinTag declarations disagree")
    for block in blocks.values():
        parse_bin_tag_fields(block["body"])
    return blocks


def parse_bin_tag_fields(body):
    """Accept only the generated one-line BinTag shape and its subdir value."""
    fields = []
    for line in body.splitlines(keepends=True):
        field = BIN_TAG_FIELD.fullmatch(line)
        if field is None:
            refuse("unreadable BinTag field")
        labels = list(SUBDIR_LABEL.finditer(line))
        values = list(SUBDIR_VALUE.finditer(line))
        if len(labels) != 1 or len(values) != 1:
            refuse("unreadable BinTag subdir expression")
        fields.append((line, values[0]))
    return fields


def remap_subdirectories(text, old_to_new):
    """Rewrite only recognized `subdir` fields inside recognized BinTag rows."""
    pieces = []
    at = 0
    for block in TAG_BLOCK.finditer(text):
        pieces.append(text[at:block.start()])
        body = []
        for line, value in parse_bin_tag_fields(block["body"]):
            index = value["index"]
            if index is None:
                body.append(line)
                continue
            old = int(index)
            if old not in old_to_new:
                refuse("surviving field points to retired child table")
            body.append(
                line[:value.start("value")]
                + f"Some({old_to_new[old]})"
                + line[value.end("value"):]
            )
        pieces.append(block.group(0).replace(block["body"], "".join(body), 1))
        at = block.end()
    pieces.append(text[at:])
    return "".join(pieces)


def parse_indices(text, tables):
    start, end = index_bounds(text)
    content_start = start + len(IDX_OPEN)
    content = text[content_start:end]
    expected = "".join(
        f"    pub const {table['name'].upper()}: usize = {index};\n"
        for index, table in enumerate(tables)
    )
    if content != expected:
        refuse("generated table-index values differ from TABLES ordering")
    return start, end


def index_bounds(text):
    if text.count(IDX_OPEN) != 1:
        refuse("generated table-index module differs")
    start = text.index(IDX_OPEN)
    if not text.endswith("}\n"):
        refuse("generated table-index module has a trailing declaration")
    end = len(text) - 2
    return start, end


def marker_for(migrations):
    identities = [
        {key: migration[key] for key in ("module", "owner", "source", "table")}
        for migration in migrations
    ]
    return MARKER_PREFIX + json.dumps(
        {"migrations": identities, "schema": 1}, sort_keys=True, separators=(",", ":")
    )


def retirement_marker(text):
    lines = [line for line in text.splitlines() if line.startswith(MARKER_PREFIX)]
    if len(lines) > 1:
        refuse("duplicate retirement marker")
    if not lines:
        return None
    try:
        value = json.loads(lines[0][len(MARKER_PREFIX):])
    except json.JSONDecodeError as error:
        raise Refusal("unreadable retirement marker") from error
    if not isinstance(value, dict) or set(value) != {"migrations", "schema"} or value["schema"] != 1:
        refuse("retirement marker shape differs")
    return lines[0]


def retire(text, migrations):
    retired_names = [migration["table"] for migration in migrations]
    expected_marker = marker_for(migrations)
    tables, tables_start, tables_end = parse_tables(text)
    blocks = parse_tag_blocks(text, tables)
    idx_start, idx_end = parse_indices(text, tables)
    names = {table["name"] for table in tables}
    missing = sorted(set(retired_names) - names)
    if missing:
        marker = retirement_marker(text)
        if marker == expected_marker and not (names & set(retired_names)):
            return text, True
        refuse("manifest selects absent table: " + ", ".join(missing))
    if retirement_marker(text) is not None:
        refuse("retirement marker exists before all selected tables are removed")

    retired = set(retired_names)
    surviving = [table for table in tables if table["name"] not in retired]
    if not surviving:
        refuse("retirement would remove every table")
    old_to_new = {
        old: new for new, old in enumerate(index for index, table in enumerate(tables)
                                         if table["name"] not in retired)
    }

    parts = []
    at = 0
    for table in tables:
        if table["name"] not in retired:
            continue
        block = blocks[table["tag"]]
        parts.append(text[at:block.start()])
        at = block.end()
    parts.append(text[at:])
    result = "".join(parts)

    # The source offsets above were from the original text; reparse before
    # replacing the table list so deletion cannot accidentally join syntax.
    remaining_tables, current_start, current_end = parse_tables(result)
    if [(table["name"], table["tag"], table["text"]) for table in remaining_tables] != [
        (table["name"], table["tag"], table["text"]) for table in tables
    ]:
        refuse("retired BinTag declaration was not removed exactly")
    table_text = "".join(table["text"] for table in surviving)
    result = result[:current_start] + table_text + result[current_end:]

    result = remap_subdirectories(result, old_to_new)
    new_tables, _, _ = parse_tables(result)
    if [table["name"] for table in new_tables] != [table["name"] for table in surviving]:
        refuse("table rewrite changed surviving identities")
    blocks = parse_tag_blocks(result, new_tables)
    if any(table["name"] in retired for table in new_tables):
        refuse("retired table identity remains in TABLES")

    new_idx = "".join(
        f"    pub const {table['name'].upper()}: usize = {index};\n"
        for index, table in enumerate(new_tables)
    )
    idx_start, idx_end = index_bounds(result)
    result = result[:idx_start] + IDX_OPEN + new_idx + result[idx_end:]
    parse_indices(result, new_tables)
    needle = "\nuse super::binary_data"
    if result.count(needle) != 1:
        refuse("generated Rust prelude differs")
    result = result.replace(needle, "\n" + expected_marker + needle)
    return result, False


def verify_shared_route(migrations, shared_tables, enabled_tables):
    try:
        binary = Path(shared_tables).read_bytes().decode("utf-8")
        enabled = Path(enabled_tables).read_text()
    except (OSError, UnicodeDecodeError) as error:
        raise Refusal(f"unreadable shared route input: {error}") from error
    for migration in migrations:
        module = re.escape(migration["module"])
        table = re.escape(migration["table"])
        pattern = re.compile(
            rf"(?ms)^pub static [A-Z0-9_]+: BinaryTable = BinaryTable \{{\n"
            rf"    module: \"{module}\",\n"
            rf"    table: \"{table}\",\n.*?^\}};\n"
        )
        tables = pattern.findall(binary)
        if len(tables) != 1:
            refuse("shared table is absent or ambiguous: " + migration["module"] + "::" + migration["table"])
        gate = re.findall(r"(?m)^    gate_a: GateA \{ blocked_by: &\[(?P<blocked>.*?)\] \},$", tables[0])
        if len(gate) != 1:
            refuse("shared table has an unreadable Gate A: " + migration["module"] + "::" + migration["table"])
        if gate[0].strip():
            refuse("shared table is blocked by Gate A: " + migration["module"] + "::" + migration["table"])
        if (migration["module"], migration["table"]) not in parse_enabled_entries(enabled):
            refuse("shared table is not enabled: " + migration["module"] + "::" + migration["table"])


def parse_enabled_entries(text):
    if text.count(ENABLED_OPEN) != 1 or text.count(ENABLED_CLOSE) != 1:
        refuse("enabled-table initializer differs")
    start = text.index(ENABLED_OPEN) + len(ENABLED_OPEN)
    end = text.index(ENABLED_CLOSE, start)
    entries = []
    for line in text[start:end].splitlines():
        code = line.split("//", 1)[0].strip()
        if not code:
            continue
        entry = re.fullmatch(r'\("(?P<module>[A-Za-z0-9_]+)", "(?P<table>[A-Za-z0-9_]+)"\),', code)
        if entry is None:
            refuse("unreadable enabled-table initializer")
        entries.append((entry["module"], entry["table"]))
    if len(entries) != len(set(entries)):
        refuse("duplicate enabled-table entry")
    return entries


def check_consumers(root, shifted_indexes, retired_tables, generated_input):
    root = Path(root)
    if not root.is_dir():
        refuse("consumer root is not a directory")
    generated_input = Path(generated_input).resolve()
    patterns = [
        re.compile(rf"TABLES\s*\[\s*{index}\s*\]")
        for index in shifted_indexes
    ] + [
        re.compile(rf"process(?:_with_ids)?\s*\(\s*TABLES\s*,\s*{index}\s*,")
        for index in shifted_indexes
    ] + [
        re.compile(rf"table\s*:\s*Some\(\s*{index}\s*\)")
        for index in shifted_indexes
    ]
    patterns.extend(
        re.compile(rf"\bidx\s*::\s*{re.escape(table.upper())}\b")
        for table in retired_tables
    )
    for path in root.rglob("*.rs"):
        if path.resolve() == generated_input:
            continue
        text = path.read_text()
        if any(pattern.search(text) for pattern in patterns):
            refuse("shifted or retired legacy table handle in consumer: " + str(path))


def identity_record(source, migrations, input_text, output_text, shared_tables, enabled_tables,
                    unsafe_indexes):
    return {
        "schema": 1,
        "source": source,
        "migrations": [
            {key: migration[key] for key in ("module", "owner", "source", "table")}
            for migration in migrations
        ],
        "input_sha256": hashlib.sha256(input_text.encode()).hexdigest(),
        "output_sha256": hashlib.sha256(output_text.encode()).hexdigest(),
        "shared_tables_sha256": hashlib.sha256(Path(shared_tables).read_bytes()).hexdigest(),
        "enabled_tables_sha256": hashlib.sha256(Path(enabled_tables).read_bytes()).hexdigest(),
        "unsafe_indexes": list(unsafe_indexes),
    }


def read_identity_ledger(path):
    path = Path(path)
    if not path.exists():
        return {"schema": 1, "records": []}
    try:
        ledger = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise Refusal(f"unreadable identity ledger: {error}") from error
    if not isinstance(ledger, dict) or set(ledger) != {"schema", "records"}:
        refuse("identity ledger shape differs")
    if ledger["schema"] != 1 or not isinstance(ledger["records"], list):
        refuse("unsupported identity ledger schema")
    expected = {
        "schema", "source", "migrations", "input_sha256", "output_sha256",
        "shared_tables_sha256", "enabled_tables_sha256", "unsafe_indexes",
    }
    if any(
        not isinstance(record, dict)
        or set(record) != expected
        or not isinstance(record["unsafe_indexes"], list)
        or any(not isinstance(index, int) or index < 0 for index in record["unsafe_indexes"])
        for record in ledger["records"]
    ):
        refuse("identity ledger record shape differs")
    return ledger


def require_recorded_replay(ledger, source, migrations, input_text):
    output_sha256 = hashlib.sha256(input_text.encode()).hexdigest()
    for previous in ledger["records"]:
        if (
            previous["source"] == source
            and previous["migrations"] == [
                {key: migration[key] for key in ("module", "owner", "source", "table")}
                for migration in migrations
            ]
            and previous["output_sha256"] == output_sha256
        ):
            return previous
    refuse("idempotent output has no matching immutable identity record")


def append_identity_record(ledger, record):
    if any(previous["input_sha256"] == record["input_sha256"] for previous in ledger["records"]):
        refuse("identity ledger already records this input")
    ledger["records"].append(record)
    return ledger


def replace_output(path, text):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", dir=path.parent, delete=False) as temporary:
        temporary.write(text)
        temporary_path = Path(temporary.name)
    try:
        os.replace(temporary_path, path)
    finally:
        temporary_path.unlink(missing_ok=True)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--source", required=True,
                        help="repository-relative generated source identity")
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--shared-tables", type=Path, required=True)
    parser.add_argument("--enabled-tables", type=Path, required=True)
    parser.add_argument("--consumer-root", type=Path, required=True)
    parser.add_argument("--identity-out", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        source = canonical_source(args.source)
        migrations = migrations_for(args.manifest, source)
        retired = [entry["table"] for entry in migrations]
        verify_shared_route(migrations, args.shared_tables, args.enabled_tables)
        input_text = args.input.read_text()
        old_tables, _, _ = parse_tables(input_text)
        retired_indexes = [index for index, table in enumerate(old_tables) if table["name"] in retired]
        shifted_indexes = range(min(retired_indexes), len(old_tables)) if retired_indexes else []
        rendered, idempotent = retire(input_text, migrations)
        ledger = read_identity_ledger(args.identity_out)
        if idempotent:
            previous = require_recorded_replay(ledger, source, migrations, input_text)
            check_consumers(args.consumer_root, previous["unsafe_indexes"], retired, args.input)
            if args.output.resolve() != args.input.resolve():
                replace_output(args.output, rendered)
            print("retire_binary_tables: " + json.dumps(
                {"replay": True, "recorded_input_sha256": previous["input_sha256"],
                 "recorded_output_sha256": previous["output_sha256"]}, sort_keys=True
            ))
            return 0
        check_consumers(args.consumer_root, shifted_indexes, retired, args.input)
        record = identity_record(source, migrations, input_text, rendered,
                                 args.shared_tables, args.enabled_tables, shifted_indexes)
        ledger = append_identity_record(ledger, record)
        replace_output(args.output, rendered)
        # Output and ledger are separate files, so a process crash between
        # these replacements can leave a recoverable partial write. Logical
        # refusals have already happened before either replacement.
        replace_output(args.identity_out, json.dumps(ledger, indent=2, sort_keys=True) + "\n")
        print("retire_binary_tables: " + json.dumps(record, sort_keys=True))
        return 0
    except (OSError, Refusal) as error:
        parser.exit(1, f"retire_binary_tables: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
