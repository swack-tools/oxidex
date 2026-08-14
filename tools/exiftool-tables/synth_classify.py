#!/usr/bin/env python3
"""Classify every codegen.py-emitted ProcessBinaryData table for Step 28's
corpus-synthesis question: can we manufacture a sample for it?

Inputs:
  --binary-tables   src/exiftool_tables/binary_tables.rs (the 613 emitted tables)
  --tables-json     dump_tables.pl output against the PINNED exiftool tree
                     (must be run against the exact .exiftool-version release --
                     see AGENTS.md's "never grade against an unpinned ExifTool")
  --corpus          combined-samples directory (for the file-exists checks the
                     carrier map's "file" entries claim)

Output: JSON classification (one row per of the 613 tables) plus a summary
printed to stderr matching the report's headline numbers.

A table is:
  synthesizable    -- writable (table WRITABLE meta or >=1 tag-level Writable
                       override) AND a carrier (vendor dir or exemplar file)
                       exists in the corpus.
  needs-real-sample -- writable, but no carrier exists in the corpus.
  unwritable        -- not writable regardless of carrier (a read-only
                       structural/computed table; ExifTool itself refuses to
                       write it, so synthesis cannot produce coverage here no
                       matter what sample exists).
Already-reachable tables (the 22) are reported separately, not folded into
these three buckets -- they are not part of the "591 unreachable" gap this
harness measures.
"""
import argparse
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from synth_carriers import CARRIER_MAP, REACHABLE  # noqa: E402


def parse_emitted_tables(rs_path: Path) -> list[tuple[str, str]]:
    text = rs_path.read_text()
    pairs = re.findall(r'module:\s*"([^"]+)",\s*table:\s*"([^"]+)"', text)
    if len(pairs) != 613:
        print(
            f"WARNING: expected 613 emitted tables, parsed {len(pairs)} from {rs_path}",
            file=sys.stderr,
        )
    return pairs


def tag_is_writable(tag_entry: dict, table_writable_default) -> bool:
    """Resolve one tag's writability per ExifTool's own semantics: a
    tag-level `Writable` key overrides the table's WRITABLE default in
    either direction (explicit 0/false = never writable even if the table
    default says otherwise; any truthy value = writable using that format).
    Absent tag-level key falls through to the table default.
    """
    if "_variants" in tag_entry:
        # Conditional layout alternatives -- treat as writable if ANY
        # alternative is (dump_tables.pl represents each as its own
        # tag-entry-shaped dict).
        return any(
            tag_is_writable(v, table_writable_default)
            for v in tag_entry.get("_variants", [])
        )
    if "Writable" in tag_entry:
        w = tag_entry["Writable"]
        if w in (0, "0", False, None, ""):
            return False
        return True
    return bool(table_writable_default)


def classify_table_writable(table_json: dict) -> tuple[bool, str]:
    meta = table_json.get("meta", {})
    table_default = meta.get("WRITABLE")
    # WRITABLE can be a format-name string (e.g. "int16u"), 1, or absent/0.
    table_default_truthy = bool(table_default) and table_default not in ("0",)
    tags = table_json.get("tags", {})
    writable_tags = [
        name for name, entry in tags.items()
        if isinstance(entry, dict) and tag_is_writable(entry, table_default_truthy)
    ]
    if writable_tags:
        return True, f"{len(writable_tags)}/{len(tags)} tags writable"
    if table_default_truthy:
        return True, "table WRITABLE set but no individual tag confirmed (writable via default)"
    return False, "no table WRITABLE and no tag-level Writable override"


def carrier_status(module: str, corpus: Path) -> tuple[str, str]:
    """Returns (status, detail): status in {"available","low-confidence","none"}."""
    entry = CARRIER_MAP.get(module)
    if entry is None:
        return "none", "module not in carrier map (unmapped)"
    kind, target, confidence, note = entry
    if kind == "none":
        return "none", note
    if kind == "vendor_dir":
        d = corpus / target
        if not d.is_dir() or not any(d.iterdir()):
            return "none", f"expected vendor dir {target} missing/empty"
        status = "available" if confidence == "high" else "low-confidence"
        return status, note or f"vendor dir {target}"
    if kind == "file":
        if target.startswith("<"):
            # generic glob description (JPEG's "<any .jpg>")
            return "available", note
        f = corpus / target
        if not f.is_file():
            return "none", f"expected exemplar {target} missing"
        status = "available" if confidence == "high" else "low-confidence"
        return status, note or f"exemplar {target}"
    return "none", "unrecognized carrier kind"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary-tables", type=Path, required=True)
    ap.add_argument("--tables-json", type=Path, required=True)
    ap.add_argument("--corpus", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    args = ap.parse_args()

    emitted = parse_emitted_tables(args.binary_tables)
    print(f"Loading {args.tables_json} ...", file=sys.stderr)
    tables_data = json.loads(args.tables_json.read_text())["modules"]

    rows = []
    counts = {"reachable": 0, "synthesizable": 0, "needs-real-sample": 0, "unwritable": 0, "no-perl-data": 0}
    for module, table in emitted:
        if (module, table) in REACHABLE:
            counts["reachable"] += 1
            rows.append({
                "module": module, "table": table, "class": "reachable",
                "detail": "already wired to find_table() at runtime",
            })
            continue

        mod_json = tables_data.get(module)
        tbl_json = mod_json["tables"].get(table) if mod_json else None
        if tbl_json is None:
            counts["no-perl-data"] += 1
            rows.append({
                "module": module, "table": table, "class": "no-perl-data",
                "detail": f"table not found under {module} in dump_tables.pl output "
                          f"(module load failure, table renamed/removed between versions, "
                          f"or symbol-table walk skipped it)",
            })
            continue

        writable, write_detail = classify_table_writable(tbl_json)
        cstatus, cdetail = carrier_status(module, args.corpus)

        if not writable:
            klass = "unwritable"
        elif cstatus == "none":
            klass = "needs-real-sample"
        else:
            klass = "synthesizable"

        counts[klass if klass in counts else "needs-real-sample"] = counts.get(klass, 0) + 1
        rows.append({
            "module": module, "table": table, "class": klass,
            "writable": writable, "write_detail": write_detail,
            "carrier_status": cstatus, "carrier_detail": cdetail,
        })

    args.out.write_text(json.dumps({"tables": rows, "counts": counts}, indent=2))

    total = len(emitted)
    unreachable = total - counts["reachable"]
    print(f"\n=== Step 28 corpus-synthesis classification ===", file=sys.stderr)
    print(f"emitted tables:        {total}", file=sys.stderr)
    print(f"already reachable:     {counts['reachable']}", file=sys.stderr)
    print(f"unreachable:           {unreachable}", file=sys.stderr)
    print(f"  synthesizable:       {counts.get('synthesizable', 0)}", file=sys.stderr)
    print(f"  needs-real-sample:   {counts.get('needs-real-sample', 0)}", file=sys.stderr)
    print(f"  unwritable:          {counts.get('unwritable', 0)}", file=sys.stderr)
    print(f"  no-perl-data:        {counts.get('no-perl-data', 0)}", file=sys.stderr)
    print(f"\nwrote {args.out}", file=sys.stderr)


if __name__ == "__main__":
    main()
