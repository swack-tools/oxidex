#!/usr/bin/env python3
"""Extract three verbatim hydrated tables from a recorded full pinned dump.

This reads the full JSON document; run it under the host's shared heavy-job lock.
The supplied commit identifies the dump tool used by the preceding capture.
It is checked against the caller's separately recorded capture evidence.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

import quicktime_atom_tables as selector


def resolve_shared(value, shared):
    """Materialize only hydrated HASH references; table references are wrappers."""
    if isinstance(value, list):
        return [resolve_shared(item, shared) for item in value]
    if not isinstance(value, dict):
        return value
    if set(value) == {"__ref", "object_id"}:
        if value.get("__ref") != "HASH" or not isinstance(value["object_id"], str):
            raise ValueError("hydrated QuickTime shared reference is malformed")
        target = shared.get(value["object_id"])
        if not isinstance(target, dict) or target.get("kind") != "HASH" or not isinstance(target.get("properties"), dict):
            raise ValueError("hydrated QuickTime shared reference is missing or malformed")
        return resolve_shared(target["properties"], shared)
    return {key: resolve_shared(item, shared) for key, item in value.items()}


def project_row(row, *, table, raw_key, defaults, shared):
    row = resolve_shared(row, shared)
    if not isinstance(row, dict):
        raise ValueError("hydrated QuickTime row is malformed")
    if "_variants" in row:
        variants = row.pop("_variants")
        if not isinstance(variants, list) or not variants:
            raise ValueError("hydrated QuickTime variants are malformed")
        return {"_variants": [project_row(item, table=table, raw_key=raw_key, defaults=defaults, shared=shared)
                              for item in variants]}
    tag_id, table_ref = row.pop("TagID", raw_key), row.pop("Table", None)
    if tag_id != raw_key or not isinstance(table_ref, dict) or table not in table_ref.get("table_full_names", []):
        raise ValueError("hydrated QuickTime row identity is malformed")
    extras = row.pop("_extra_properties", {})
    if not isinstance(extras, dict):
        raise ValueError("hydrated QuickTime row extras are malformed")
    index = extras.pop("Index", None)
    if index is not None and not isinstance(index, str):
        raise ValueError("hydrated QuickTime variant index is malformed")
    extras = set(extras) - {"GotGroups", "Preferred"}
    if extras:
        row["_extra_keys"] = sorted(extras)
    groups = row.get("Groups")
    if groups is not None:
        if not isinstance(groups, dict):
            raise ValueError("hydrated QuickTime Groups are malformed")
        groups = {key: value for key, value in groups.items() if defaults.get(key) != value}
        if groups:
            row["Groups"] = groups
        else:
            row.pop("Groups")
    return row


def hydrated_tables(document):
    layouts = document.get("hydrated_layouts")
    if not isinstance(layouts, dict) or not isinstance(layouts.get("tables"), dict) or not isinstance(layouts.get("shared_reference_objects"), dict):
        raise ValueError("full dump lacks hydrated QuickTime layouts")
    tables, shared, result = layouts["tables"], layouts["shared_reference_objects"], {}
    for name in selector.TABLES:
        full = "Image::ExifTool::QuickTime::" + name
        table = resolve_shared(tables.get(full), shared)
        if not isinstance(table, dict) or table.get("full_name") != full or not isinstance(table.get("meta"), dict):
            raise ValueError("hydrated QuickTime table is malformed: " + full)
        meta = table["meta"]
        defaults = meta.get("GROUPS", {})
        if not isinstance(defaults, dict):
            raise ValueError("hydrated QuickTime table groups are malformed: " + full)
        tags = table.get("tags")
        if not isinstance(tags, dict):
            raise ValueError("hydrated QuickTime tags are malformed: " + full)
        result[name] = {"full_name": full, "meta": meta, "tag_count": len(tags),
                        "tags": {key: project_row(value, table=full, raw_key=key, defaults=defaults, shared=shared)
                                 for key, value in tags.items()}}
    return result


def extract(document, *, full_hash, source_commit, perl_version, tool_hash):
    pin = (selector.ROOT / ".exiftool-version").read_text().strip()
    if document.get("exiftool_version") != pin or document.get("modules_failed") != 0:
        raise ValueError("full dump must match the pin and have no failed modules")
    module = document["modules"]["QuickTime"]
    tables = hydrated_tables(document)
    if module["table_count"] != len(module["tables"]):
        raise ValueError("QuickTime table count does not conserve captured identities")
    result = {"exiftool_version": pin,
            "modules": {"QuickTime": {"module": module["module"],
                                        "package": module["package"],
                                        "table_count": len(tables), "tables": tables}},
            "capture_scope": {"kind": "verbatim selected tables from hydrated dump",
                              "tables": ["QuickTime::" + name for name in selector.TABLES],
                              "source_module_table_count": module["table_count"],
                              "source_commit": source_commit, "full_dump_sha256": full_hash,
                              "dump_tool_sha256": tool_hash, "perl_version": perl_version}}

    if "quicktime_itemlist_reader_protocol" in document:
        result["quicktime_itemlist_reader_protocol"] = document["quicktime_itemlist_reader_protocol"]
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dump", required=True, type=Path)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--perl-version", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    selector.validate_outputs(args.dump, [args.output])
    state = selector.instrument.git_state(selector.ROOT)
    override = selector.instrument.refuse_if_dirty(state, "capture_quicktime_baseline.py")
    selector.instrument.print_header(tool="capture_quicktime_baseline.py", git=state,
                                     dirty_overridden=override,
                                     extra=["scope: extract selected hydrated tables; no support claim"])
    source_commit = subprocess.check_output(["git", "-C", str(selector.ROOT), "rev-parse",
                                             args.source_commit + "^{commit}"], text=True).strip()
    tool = subprocess.check_output(["git", "-C", str(selector.ROOT), "show",
                                    source_commit + ":tools/exiftool-tables/dump_tables.pl"])
    with args.dump.open("rb") as stream:
        full_hash = hashlib.file_digest(stream, "sha256").hexdigest()
    with args.dump.open() as stream:
        document = json.load(stream)
    result = extract(document, full_hash=full_hash, source_commit=source_commit,
                     perl_version=args.perl_version, tool_hash=hashlib.sha256(tool).hexdigest())
    encoded = selector.serialized(result)
    selector.report(encoded.encode())  # validate provenance and all selected rows before writing
    with args.output.open("x") as target:
        target.write(encoded)


if __name__ == "__main__":
    main()
