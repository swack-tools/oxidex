#!/usr/bin/env python3
"""Inventory recorded ExifTool table shapes with immutable selector imports.

This is a source-shape census, not a claim about emitted rows, runtime routes,
manual producers, output parity, or an automation percentage.  Every run pins
one recorded dump and one selector commit.  The selectors are imported from a
Git archive of that commit, never from the caller's working tree.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import subprocess
import sys
import tarfile
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha_file(path: Path) -> str:
    return sha(path.read_bytes())


def git(repo: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(repo), *args], text=True).strip()


def archive_tools(repo: Path, commit: str, out: Path) -> dict[str, Any]:
    """Archive the exact selector commit and return paths relative to ``out``."""
    archive = subprocess.check_output(
        ["git", "-C", str(repo), "archive", "--format=tar", commit, "tools/exiftool-tables"]
    )
    archive_path = out / "selector-snapshot.tar"
    archive_path.write_bytes(archive)
    extract = archive_path.with_suffix("")
    root = extract.resolve()
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as tf:
        members = tf.getmembers()
        for member in members:
            target = (extract / member.name).resolve()
            if os.path.commonpath((str(root), str(target))) != str(root):
                raise RuntimeError(f"unsafe archive member: {member.name!r}")
            if member.issym() or member.islnk():
                raise RuntimeError(f"linked archive member is unsupported: {member.name!r}")
        tf.extractall(extract, members=members)
    tools = extract / "tools/exiftool-tables"
    required = ("codegen.py", "keyed_directory.py", "word_directory.py", "conds.py", "subdirs.py")
    missing = [name for name in required if not (tools / name).is_file()]
    if missing:
        raise RuntimeError(f"selector archive lacks required files: {', '.join(missing)}")
    return {
        "archive": str(archive_path.relative_to(out)),
        "archive_sha256": sha(archive),
        "commit": commit,
        "tools_directory": str(tools.relative_to(out)),
        "key_files": {name: sha_file(tools / name) for name in required},
    }


def run_selectors(tools: Path, metas: list[dict[str, Any]]) -> list[dict[str, bool]]:
    """Run all predicates in an archived source directory, never live code."""
    body = """
import json, sys
sys.path.insert(0, sys.argv[1])
import codegen
import keyed_directory
import word_directory
metas = json.load(sys.stdin)
print(json.dumps([
    {
        "binary": codegen.is_binary_table(meta),
        "ifd": codegen.is_ifd_table(meta),
        "keyed_profile": keyed_directory.is_keyed_directory_table(meta),
        "keyed_word_candidate": word_directory.is_candidate(meta.get("PROCESS_PROC")),
    }
    for meta in metas
]))
"""
    env = {"PATH": os.environ.get("PATH", ""), "PYTHONNOUSERSITE": "1"}
    proc = subprocess.run(
        [sys.executable, "-c", body, str(tools)],
        input=json.dumps(metas),
        text=True,
        capture_output=True,
        env=env,
        check=False,
    )
    if proc.returncode:
        raise RuntimeError(f"archived selector failed: {proc.stderr}")
    result = json.loads(proc.stdout)
    if not isinstance(result, list) or len(result) != len(metas):
        raise RuntimeError("archived selector returned malformed length")
    if any(not isinstance(item, dict) for item in result):
        raise RuntimeError("archived selector returned a non-object row")
    return result


def perl_truthy(value: Any) -> bool:
    return not (value is None or value is False or value == "" or value == "0" or value == 0)


def is_named(value: dict[str, Any]) -> bool:
    return isinstance(value.get("Name"), str) and bool(value["Name"].strip())


def count_rows(tags: dict[str, Any]) -> Counter:
    """Count valid source alternatives.

    A direct key is one alternative. A ``_variants`` key is one alternative per
    list item. A non-list variant container is a malformed container slot, not
    a named alternative; valid replay input rejects it before this function is
    used for a report.
    """
    counts = Counter(raw_tag_keys=len(tags))
    for value in tags.values():
        if not isinstance(value, dict):
            counts["raw_tag_value_non_dict"] += 1
            counts["alternative_slots"] += 1
            counts["alternative_non_dict"] += 1
            continue
        if "_variants" in value:
            counts["raw_variant_keys"] += 1
            variants = value["_variants"]
            if not isinstance(variants, list):
                counts["variant_container_non_list"] += 1
                counts["alternative_slots"] += 1
                counts["malformed_variant_container_slots"] += 1
                continue
            counts["alternative_slots"] += len(variants)
            for alternative in variants:
                if not isinstance(alternative, dict):
                    counts["alternative_non_dict"] += 1
                elif is_named(alternative):
                    counts["named_alternatives"] += 1
                    if perl_truthy(alternative.get("Unknown")):
                        counts["unknown_alternatives"] += 1
                else:
                    counts["unnamed_dict_alternatives"] += 1
                    if perl_truthy(alternative.get("Unknown")):
                        counts["unknown_alternatives"] += 1
            continue
        counts["raw_direct_keys"] += 1
        counts["alternative_slots"] += 1
        if is_named(value):
            counts["named_alternatives"] += 1
        else:
            counts["unnamed_dict_alternatives"] += 1
        if perl_truthy(value.get("Unknown")):
            counts["unknown_alternatives"] += 1
    require(
        counts["alternative_slots"] == (
            counts["named_alternatives"]
            + counts["unnamed_dict_alternatives"]
            + counts["alternative_non_dict"]
            + counts["malformed_variant_container_slots"]
        ),
        f"alternative slots do not conserve: {dict(counts)}",
    )
    require(
        counts["raw_tag_keys"] == (
            counts["raw_direct_keys"] + counts["raw_variant_keys"] + counts["raw_tag_value_non_dict"]
        ),
        f"raw keys do not conserve: {dict(counts)}",
    )
    return counts


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def validate_document(doc: Any) -> list[tuple[str, str, dict[str, Any], dict[str, Any], dict[str, Any]]]:
    """Reject incomplete dump structure before it can define a denominator."""
    require(isinstance(doc, dict), "dump root must be an object")
    modules = doc.get("modules")
    require(isinstance(modules, dict) and modules, "dump modules must be a non-empty object")
    require(doc.get("modules_failed") == 0, "dump has failed modules")
    require(doc.get("modules_ok") == len(modules), "dump modules_ok does not match module count")
    entries = []
    declared_tables = 0
    for module, module_data in sorted(modules.items()):
        require(isinstance(module, str) and module, "dump has an invalid module name")
        require(isinstance(module_data, dict), f"{module}: module data must be an object")
        tables = module_data.get("tables")
        table_count = module_data.get("table_count")
        require(isinstance(tables, dict), f"{module}: tables must be an object")
        require(isinstance(table_count, int) and table_count >= 0, f"{module}: table_count must be non-negative integer")
        require(table_count == len(tables), f"{module}: table_count {table_count} != table objects {len(tables)}")
        declared_tables += table_count
        for table, data in sorted(tables.items()):
            require(isinstance(table, str) and table, f"{module}: invalid table name")
            require(isinstance(data, dict), f"{module}::{table}: table data must be an object")
            meta = data.get("meta")
            tags = data.get("tags")
            tag_count = data.get("tag_count")
            full_name = data.get("full_name")
            require(isinstance(meta, dict), f"{module}::{table}: meta must be an object")
            require(isinstance(tags, dict), f"{module}::{table}: tags must be an object")
            require(isinstance(tag_count, int) and tag_count >= 0, f"{module}::{table}: tag_count must be non-negative integer")
            require(tag_count == len(tags), f"{module}::{table}: tag_count {tag_count} != raw keys {len(tags)}")
            require(isinstance(full_name, str) and full_name, f"{module}::{table}: full_name must be non-empty string")
            for raw_id, value in tags.items():
                require(isinstance(raw_id, str), f"{module}::{table}: non-string raw tag key")
                require(isinstance(value, dict), f"{module}::{table}::{raw_id}: tag value must be an object")
                if "_variants" in value:
                    require(set(value) == {"_variants"}, f"{module}::{table}::{raw_id}: variant outer metadata is unsupported")
                    variants = value["_variants"]
                    require(isinstance(variants, list), f"{module}::{table}::{raw_id}: variants must be a list")
                    for index, alternative in enumerate(variants):
                        require(isinstance(alternative, dict), f"{module}::{table}::{raw_id}#{index}: variant must be an object")
            entries.append((module, table, data, meta, tags))
    require(declared_tables == len(entries), "module table counts do not conserve table records")
    return entries


def processor(meta: dict[str, Any]) -> tuple[str | None, str]:
    raw = meta.get("PROCESS_PROC")
    if raw is None:
        return None, "absent_default_ProcessExif_candidate"
    if isinstance(raw, dict):
        name = raw.get("__name")
        return (
            name if isinstance(name, str) and name else None,
            "deparsed_named_function" if isinstance(name, str) and name else "deparsed_unnamed_function",
        )
    if isinstance(raw, str):
        return raw, "literal_processor_name"
    return None, f"nonstandard_processor_{type(raw).__name__}"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dump", type=Path, required=True)
    parser.add_argument("--repo", type=Path, required=True, help="repository containing the selector commit")
    parser.add_argument("--selector-ref", required=True)
    parser.add_argument("--expected-dump-sha", required=True)
    parser.add_argument("--expected-selector-commit", required=True)
    parser.add_argument("--out", type=Path, required=True)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if not args.dump.is_file():
        raise SystemExit(f"dump not found: {args.dump}")
    dump_sha = sha_file(args.dump)
    if dump_sha != args.expected_dump_sha:
        raise SystemExit(f"dump SHA mismatch: {dump_sha} != {args.expected_dump_sha}")
    commit = git(args.repo, "rev-parse", f"{args.selector_ref}^{{commit}}")
    if commit != args.expected_selector_commit:
        raise SystemExit(f"selector commit mismatch: {commit} != {args.expected_selector_commit}")
    if args.out.exists():
        raise SystemExit(f"output path already exists: {args.out}")
    doc = json.loads(args.dump.read_text())
    try:
        entries = validate_document(doc)
    except ValueError as exc:
        raise SystemExit(f"invalid dump structure: {exc}") from exc
    runner_sha = sha_file(Path(__file__).resolve())
    print("=== instrument: inventory_source_processors ===", file=sys.stderr)
    print(f"runner_sha256: {runner_sha}", file=sys.stderr)
    print(f"immutable_selector_commit: {commit}", file=sys.stderr)
    print(f"recorded_dump: ExifTool {doc.get('exiftool_version')} sha256={dump_sha}", file=sys.stderr)
    print("scope: recorded-input only; no oxidex or live Perl oracle run", file=sys.stderr)
    args.out.mkdir(parents=True)
    snapshot = archive_tools(args.repo, commit, args.out)
    metas = [meta for _, _, _, meta, _ in entries]
    selected = run_selectors(args.out / snapshot["tools_directory"], metas)
    rows, totals, selections = [], Counter(), Counter()
    families: dict[tuple[str | None, str], Counter] = defaultdict(Counter)
    require(len(entries) == len(selected), "selector result length does not match table records")
    for (module, table, data, meta, tags), result in zip(entries, selected):
        if result.get("binary"):
            selection = "binary"
        elif result.get("keyed_profile"):
            selection = "keyed_profile"
        elif result.get("keyed_word_candidate"):
            selection = "keyed_word_candidate"
        elif result.get("ifd"):
            selection = "ifd"
        else:
            selection = "other_unclassified"
        counts = count_rows(tags)
        ident, shape = processor(meta)
        row = {
            "module": module,
            "table": table,
            "source_identity": data["full_name"],
            "processor_identity": ident,
            "processor_shape": shape,
            "selection": selection,
            "declared_tag_count": data["tag_count"],
            "counts": dict(sorted(counts.items())),
        }
        rows.append(row)
        totals.update(counts)
        totals["declared_tag_count"] += data["tag_count"]
        totals["tables"] += 1
        selections[selection] += 1
        family = families[(ident, shape)]
        family.update(counts)
        family["tables"] += 1
        family[f"selection_{selection}_tables"] += 1
    require(totals["tables"] == len(entries), "table records do not conserve")
    require(totals["declared_tag_count"] == totals["raw_tag_keys"], "declared tag counts do not conserve")
    require(totals["alternative_slots"] == (
        totals["named_alternatives"]
        + totals["unnamed_dict_alternatives"]
        + totals["alternative_non_dict"]
        + totals["malformed_variant_container_slots"]
    ), "alternative slots do not conserve")
    require(totals["raw_tag_keys"] == totals["raw_direct_keys"] + totals["raw_variant_keys"] + totals["raw_tag_value_non_dict"], "raw keys do not conserve")
    family_rows = []
    for (ident, shape), counts in families.items():
        family_rows.append({
            "processor_identity": ident,
            "processor_shape": shape,
            "counts": dict(sorted(counts.items())),
            "sample_source_identities": [
                row["source_identity"]
                for row in rows
                if row["processor_identity"] == ident and row["processor_shape"] == shape
            ][:5],
        })
    family_rows.sort(key=lambda row: (-row["counts"].get("named_alternatives", 0), -row["counts"]["tables"], row["processor_identity"] or "", row["processor_shape"]))
    summary = {
        "runner_sha256": runner_sha,
        "scope": "one recorded dump only; source-shape inventory, not generated acceptance, runtime reachability, manual share, output parity, or an automation percentage",
        "dump": {
            "sha256": dump_sha,
            "exiftool_version": doc.get("exiftool_version"),
            "modules": len(doc["modules"]),
            "modules_ok": doc["modules_ok"],
            "modules_failed": doc["modules_failed"],
        },
        "selector_snapshot": snapshot,
        "row_definition": {
            "direct": "one alternative per direct raw key",
            "variants": "one alternative per _variants list item; an empty list remains a raw key with zero alternatives",
            "unknown": "Unknown overlaps the named/unnamed partition",
            "malformed": "external malformed structures are rejected before output; count_rows labels a non-list variant container as malformed, never as a named alternative",
        },
        "conservation": {
            "table_records": totals["tables"],
            "declared_tag_count_sum": totals["declared_tag_count"],
            "raw_tag_keys": totals["raw_tag_keys"],
            "alternative_slots": totals["alternative_slots"],
            "named_alternatives": totals["named_alternatives"],
            "unnamed_dict_alternatives": totals["unnamed_dict_alternatives"],
            "tables_with_zero_named_alternatives": sum(1 for row in rows if row["counts"].get("named_alternatives", 0) == 0),
        },
        "selection_table_counts": dict(sorted(selections.items())),
        "gaps": [
            "The dump is not a whole-source module-discovery or dispatch inventory.",
            "IFD is a compiler candidate predicate and over-includes tables with absent PROCESS_PROC.",
            "The keyed profile is schema-only and does not assert a reader or enablement.",
            "The inventory does not measure generated acceptance, omissions, runtime routes, manual producers, or output parity.",
        ],
    }
    (args.out / "tables.json").write_text(json.dumps(rows, indent=2, sort_keys=True) + "\n")
    (args.out / "processor-families.json").write_text(json.dumps(family_rows, indent=2, sort_keys=True) + "\n")
    (args.out / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"tables": len(rows), "named_alternatives": totals["named_alternatives"], "selection": dict(selections)}, sort_keys=True))


if __name__ == "__main__":
    main()
