#!/usr/bin/env python3
"""Select source-described QuickTime data-atom rows, retaining every refusal.

This is a capability inventory for a prospective generic ItemList reader.
Selection alone is neither connected runtime support nor observed extraction.
UserData and Keys have different framing/lookup protocols and remain explicit.
"""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path

TABLES = ("ItemList", "UserData", "Keys")
FORMATS = {None, "string", "int8u", "int16u", "int32u", "int64u"}
NON_READING_PROPERTIES = {
    "Name", "Description", "Notes", "Groups", "_shorthand", "Avoid",
    "Writable", "WriteGroup", "Preferred", "SeparateTable", "ValueConvInv",
    "PrintConvInv", "Format", "PrintConv",
}


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
                                     ensure_ascii=False).encode()).hexdigest()


def variants(row, path=()):
    if isinstance(row, dict) and "_variants" in row:
        children = row["_variants"]
        if set(row) != {"_variants"} or not isinstance(children, list) or not children:
            yield path, row, "unresolved_variant_container"
        else:
            for index, child in enumerate(children):
                yield from variants(child, path + (index,))
    else:
        yield path, row, None


def inspect_row(table_name, table, raw_key, path, row, structural_reason):
    original_row = row
    identity = {"module": "QuickTime", "table": table_name, "raw_key": raw_key,
                "variant_path": list(path), "source_sha256": digest(row)}
    reasons = []
    if structural_reason:
        reasons.append(structural_reason)
    if table_name != "ItemList":
        reasons.append("protocol_userdata_language_records" if table_name == "UserData"
                       else "protocol_keys_indexed_name_resolution")
    if path:
        reasons.append("conditional_variant_selection")
    if not isinstance(row, dict):
        reasons.append("non_object_source_row")
        row = {}
    try:
        key_bytes = raw_key.encode("latin-1")
    except UnicodeEncodeError:
        key_bytes = b""
    if table_name == "ItemList" and len(key_bytes) != 4:
        reasons.append("key_not_four_bytes")
    if not isinstance(row.get("Name"), str) or not row["Name"]:
        reasons.append("missing_literal_name")
    fmt = row.get("Format")
    if not isinstance(fmt, (str, type(None))) or fmt not in FORMATS:
        reasons.append("unsupported_format:" + str(fmt))
    for prop in sorted(set(row) - NON_READING_PROPERTIES):
        if prop == "_extra_keys" and isinstance(row[prop], list):
            reasons.extend("uncaptured_source_property:" + str(key) for key in row[prop])
        else:
            reasons.append("unsupported_source_property:" + prop)
    pc = row.get("PrintConv")
    enum = None
    if pc is not None:
        if (isinstance(pc, dict) and pc.get("kind") == "enum"
                and not (set(pc) - {"kind", "map", "directives"})
                and pc.get("directives") is None and isinstance(pc.get("map"), dict)
                and all(isinstance(k, str) and isinstance(v, str) for k, v in pc["map"].items())):
            enum = dict(sorted(pc["map"].items()))
        else:
            reasons.append("unsupported_print_conversion")
    groups = table.get("meta", {}).get("GROUPS", {})
    override = row.get("Groups", {})
    if not isinstance(groups, dict) or not isinstance(override, dict):
        group = None
    else:
        group = override.get("1", groups.get("1"))
    if not isinstance(group, str) or not group:
        reasons.append("missing_literal_output_group")
    result = {"identity": identity, "reasons": sorted(set(reasons)),
              "runtime_connected": False, "observed_read": None, "observed_write": None}
    if not reasons:
        result["spec"] = {"key_hex": key_bytes.hex(), "name": row["Name"],
                          "group": group, "format": fmt, "print_enum": enum}
    else:
        result["refused_source"] = original_row
    return result


def inventory(document):
    tables = document["modules"]["QuickTime"]["tables"]
    families = []
    for name in TABLES:
        table = tables[name]
        tags = table["tags"]
        if table["tag_count"] != len(tags):
            raise ValueError(f"{name}: declared count differs from captured keys")
        records = [inspect_row(name, table, key, path, row, problem)
                   for key, value in sorted(tags.items())
                   for path, row, problem in variants(value)]
        counts = Counter(reason for r in records for reason in r["reasons"])
        families.append({"table": name, "source_table_sha256": digest(table),
                         "raw_rows": len(tags), "variant_records": len(records),
                         "declarative_candidates": sum(not r["reasons"] for r in records),
                         "refused_records": sum(bool(r["reasons"]) for r in records),
                         "refusal_counts_nonexclusive": dict(sorted(counts.items())),
                         "records": records})
    return {"schema": "quicktime_source_capabilities_v1",
            "exiftool_version": document["exiftool_version"],
            "scope": "source capability candidates; no generated runtime or observed support claimed",
            "families": families}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dump", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    raw = args.dump.read_bytes()
    document = json.loads(raw)
    pinned = (Path(__file__).resolve().parents[2] / ".exiftool-version").read_text().strip()
    if document.get("exiftool_version") != pinned:
        parser.error("capture version differs from repository pin")
    result = inventory(document)
    result["source"] = {"dump_sha256": hashlib.sha256(raw).hexdigest(),
                        "selector_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    args.output.write_text(json.dumps(result, sort_keys=True, indent=2, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
