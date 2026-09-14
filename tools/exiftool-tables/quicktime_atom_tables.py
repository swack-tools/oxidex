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
import sys
import re

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import instrument

TABLES = ("ItemList", "UserData", "Keys")
FORMATS = {None, "string", "int8u", "int16u", "int32u", "int64u"}
NON_READING_PROPERTIES = {
    "Name", "Description", "Notes", "Groups", "_shorthand", "Avoid",
    "Writable", "WriteGroup", "Preferred", "SeparateTable", "ValueConvInv",
    "PrintConvInv", "Format", "PrintConv", "PrintConvColumns",
}


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
                                     ensure_ascii=False).encode()).hexdigest()


def semantic_normal_form(row):
    """Remove only selector-declared behavior-inert source metadata markers."""
    if not isinstance(row, dict):
        return row
    return {key: value for key, value in row.items() if key != "_shorthand"}


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
            # BuildTagLookup uses PrintConvColumns only to lay out the HTML
            # enum table. It has no role in reading bytes or converting values.
            reasons.extend("uncaptured_source_property:" + str(key) for key in row[prop]
                           if key != "PrintConvColumns")
        else:
            reasons.append("unsupported_source_property:" + prop)
    # This captured field controls catalog enum-table presentation, not reads.
    # Its old uncaptured-key form is handled above; validate the newly exposed
    # scalar so an unsupported replacement is still visible in the ledger.
    if "PrintConvColumns" in row:
        columns = row["PrintConvColumns"]
        if not ((type(columns) is int and columns > 0)
                or (isinstance(columns, str) and re.fullmatch(r"[1-9][0-9]*", columns))):
            reasons.append("unsupported_display_column_count")
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
        group = group0 = None
    else:
        group = override.get("1", groups.get("1"))
        group0 = override.get("0", groups.get("0", "QuickTime"))
    if not isinstance(group, str) or not group:
        reasons.append("missing_literal_output_group")
    if not isinstance(group0, str) or not group0:
        reasons.append("missing_literal_family0_group")
    result = {"identity": identity, "reasons": sorted(set(reasons)),
              "runtime_connected": False, "observed_read": None, "observed_write": None}
    if not reasons:
        result["spec"] = {"key_hex": key_bytes.hex(), "name": row["Name"],
                          "group": group, "group0": group0, "format": fmt, "print_enum": enum}
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


def report(raw: bytes):
    document = json.loads(raw)
    pinned = (Path(__file__).resolve().parents[2] / ".exiftool-version").read_text().strip()
    if document.get("exiftool_version") != pinned:
        raise ValueError("capture version differs from repository pin")
    capture = document.get("capture_scope", {})
    if capture.get("kind") != "verbatim selected tables from hydrated dump":
        raise ValueError("capture kind must identify a hydrated table selection")
    if capture.get("tables") != ["QuickTime::" + table for table in TABLES]:
        raise ValueError("capture must identify the three selected source tables")
    parent_count = capture.get("source_module_table_count")
    if type(parent_count) is not int or parent_count < len(TABLES):
        raise ValueError("capture parent table count must cover the selected tables")
    module = document["modules"]["QuickTime"]
    if module.get("table_count") != len(TABLES) or set(module["tables"]) != set(TABLES):
        raise ValueError("selected module table count or identities disagree with capture scope")
    for key, length in (("source_commit", 40), ("full_dump_sha256", 64), ("dump_tool_sha256", 64)):
        if not re.fullmatch(r"[a-f0-9]{" + str(length) + "}", str(capture.get(key, ""))):
            raise ValueError("missing or malformed capture provenance: " + key)
    if not re.fullmatch(r"\d+\.\d+\.\d+", str(capture.get("perl_version", ""))):
        raise ValueError("missing Perl capture version")
    tool_sha256 = hashlib.sha256((ROOT / "tools/exiftool-tables/dump_tables.pl").read_bytes()).hexdigest()
    if tool_sha256 != capture["dump_tool_sha256"]:
        # A bounded table snapshot remains verbatim evidence of its recorded
        # hydrated full dump.  A later dump-tool improvement may add a source
        # fact without changing those selected tables; in that case a separate
        # bounded canonical-Perl refresh records exactly which new fact came
        # from the current tool.  Never let an undocumented stale tool hash
        # pass as a fresh capture.
        refresh = capture.get("source_fact_refresh")
        protocol = document.get("quicktime_itemlist_reader_protocol")
        if (not isinstance(refresh, dict)
                or refresh.get("kind") != "quicktime_itemlist_reader_protocol_from_canonical_perl"
                or refresh.get("dump_tool_sha256") != tool_sha256
                or refresh.get("captured_dump_tool_sha256") != capture["dump_tool_sha256"]
                or refresh.get("exiftool_version") != pinned
                or not re.fullmatch(r"\d+\.\d+\.\d+", str(refresh.get("perl_version", "")))
                or not isinstance(protocol, dict)
                or refresh.get("protocol_sha256") != hashlib.sha256(
                    json.dumps(protocol, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
                ).hexdigest()):
            raise ValueError("capture dump tool hash is stale; recapture using the current dump tool")
    result = inventory(document)
    result["source"] = {"dump_sha256": hashlib.sha256(raw).hexdigest(),
                        "selector_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                        "capture_scope": document.get("capture_scope")}
    return result


def summary(result):
    return {**result, "families": [{key: value for key, value in family.items()
                                    if key != "records"} for family in result["families"]]}


def serialized(result):
    return json.dumps(result, sort_keys=True, indent=2, ensure_ascii=False) + "\n"


def validate_outputs(source, paths, *, replace=False, check=False):
    def aliases(left, right):
        return left.resolve() == right.resolve() or (
            left.exists() and right.exists() and left.samefile(right))
    for index, path in enumerate(paths):
        if aliases(source, path):
            raise ValueError("output aliases the captured source dump")
        if any(aliases(path, other) for other in paths[:index]):
            raise ValueError("output artifacts alias one another")
        if path.is_symlink():
            raise ValueError("output symlinks are unsupported")
        if path.exists() and not (replace or check):
            raise ValueError("output exists; use --replace for intentional regeneration")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dump", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--summary", type=Path)
    parser.add_argument("--check", action="store_true", help="fail on artifact drift; write nothing")
    parser.add_argument("--replace", action="store_true", help="explicitly regenerate existing artifacts")
    args = parser.parse_args()
    paths = [args.output, *([args.summary] if args.summary else [])]
    try:
        validate_outputs(args.dump, paths, replace=args.replace, check=args.check)
    except ValueError as exc:
        parser.error(str(exc))
    state = instrument.git_state(ROOT)
    overridden = instrument.refuse_if_dirty(state, "quicktime_atom_tables.py")
    instrument.print_header(tool="quicktime_atom_tables.py", git=state,
                            dirty_overridden=overridden,
                            extra=["scope: source capability candidates; no runtime support claim",
                                   f"source: {args.dump}"])
    result = report(args.dump.read_bytes())
    outputs = [(args.output, serialized(result))]
    if args.summary:
        outputs.append((args.summary, serialized(summary(result))))
    for path, contents in outputs:
        if args.check:
            if not path.is_file() or path.read_text() != contents:
                parser.error(f"stale capability artifact: {path}")
        else:
            with path.open("w" if args.replace else "x") as output:
                output.write(contents)


if __name__ == "__main__":
    main()
