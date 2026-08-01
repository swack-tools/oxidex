#!/usr/bin/env python3
"""Classify extracted ExifTool tags by how safely they can be generated.

The point of this script is to stop anyone (human or model) from claiming
coverage they have not earned.  Every tag lands in exactly one tier:

  pure        no conversions at all -- name/format/group only.  Emitting this
              is a transcription, not an interpretation.
  enum        conversions are entirely PrintConv lookup maps.  Still pure data:
              a Rust match arm per key reproduces ExifTool exactly.
  expr        at least one conversion is a Perl expression.  Safe ONLY if every
              such expression has a registered translation (see exprs.py).
  code        at least one conversion is a Perl code ref.  The body is not
              recoverable from the symbol table; these need real porting work.

`pure` + `enum` is the mechanically-safe frontier -- the work that should never
have cost a model call.  Everything else is where human or model effort
actually belongs, and reporting the split honestly is the whole point.
"""

import json
import sys
from collections import Counter, defaultdict

CONV_FIELDS = ("PrintConv", "ValueConv", "RawConv")


def conv_kinds(tag):
    """Yield the classification kind of each conversion present on a tag."""
    for f in CONV_FIELDS:
        c = tag.get(f)
        if isinstance(c, dict) and c.get("kind"):
            yield f, c["kind"]


def classify(tag):
    # A variant tag (arrayref in Perl) is as hard as its hardest branch: the
    # dispatch condition itself is a Perl expression that must be evaluated.
    if "_variants" in tag:
        return "variant"

    kinds = [k for _f, k in conv_kinds(tag)]
    if not kinds:
        return "pure"
    if any(k == "code" for k in kinds):
        return "code"
    if any(k in ("expr", "list", "other") for k in kinds):
        return "expr"
    # enum / enum_partial only
    return "enum"


def main(path):
    with open(path, encoding="utf-8") as fh:
        doc = json.load(fh)

    tiers = Counter()
    per_module = defaultdict(Counter)
    exprs = Counter()
    enum_sizes = []
    subdir_edges = 0

    for mod_name, mod in doc["modules"].items():
        for tbl in mod["tables"].values():
            for tag in tbl["tags"].values():
                tier = classify(tag)
                tiers[tier] += 1
                per_module[mod_name][tier] += 1

                if tag.get("SubDirectory"):
                    subdir_edges += 1
                for _f, c in ((f, tag.get(f)) for f in CONV_FIELDS):
                    if not isinstance(c, dict):
                        continue
                    if c.get("kind") == "expr" and c.get("expr"):
                        exprs[c["expr"].strip()] += 1
                    if c.get("kind") in ("enum", "enum_partial"):
                        enum_sizes.append(len(c.get("map") or {}))

    total = sum(tiers.values())
    safe = tiers["pure"] + tiers["enum"]

    print(f"ExifTool {doc['exiftool_version']} -- {total} tag entries\n")
    print(f"{'tier':<10}{'count':>8}{'share':>9}")
    print("-" * 27)
    for t in ("pure", "enum", "expr", "code", "variant"):
        print(f"{t:<10}{tiers[t]:>8}{tiers[t]/total:>8.1%}")
    print("-" * 27)
    print(f"{'SAFE':<10}{safe:>8}{safe/total:>8.1%}   (pure + enum)\n")

    print(f"enum maps extracted: {len(enum_sizes)}, "
          f"total enum entries: {sum(enum_sizes)}")
    print(f"subdirectory edges (table graph): {subdir_edges}\n")

    print("distinct Perl expressions needing translation: "
          f"{len(exprs)} (covering {sum(exprs.values())} uses)")
    print("\nmost common expressions -- translating the top 20 unlocks "
          f"{sum(c for _e, c in exprs.most_common(20))} tags:")
    for expr, n in exprs.most_common(20):
        flat = " ".join(expr.split())
        if len(flat) > 62:
            flat = flat[:59] + "..."
        print(f"  {n:>5}  {flat}")

    print("\ntop modules by mechanically-safe tags:")
    rows = sorted(
        ((c["pure"] + c["enum"], m, sum(c.values())) for m, c in per_module.items()),
        reverse=True,
    )[:15]
    print(f"  {'module':<16}{'safe':>7}{'total':>8}{'share':>8}")
    for safe_n, mod, tot in rows:
        print(f"  {mod:<16}{safe_n:>7}{tot:>8}{safe_n/tot:>8.0%}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "all_tables.json")
