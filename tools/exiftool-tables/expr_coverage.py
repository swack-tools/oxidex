#!/usr/bin/env python3
"""Step 15 compiler coverage report: run exprs.py's full translation surface
(TRANSLATIONS exact-match + compile()'s grammar) against every ValueConv /
PrintConv / RawConv expression ExifTool declares at the pinned release, and
report translated vs refused -- by USES (tag occurrences), since a handful of
expressions carry thousands of uses and distinct-expression coverage alone
would flatter the result.

This is deliberately independent of codegen.py's own bookkeeping: codegen.py
only ever routes a table's PrintConv through exprs (ValueConv/RawConv are
unconditionally recorded "omitted" -- see codegen.py's own docstring), and
only for tables ExifTool reads via ProcessBinaryData. The census here has no
such restriction: it is every expression in the dump, which is what the Step
15 decision gate's ~69.5%-of-uses figure was measured against, and what this
script re-measures against the actually-shipped compiler.

Instrument (AGENTS.md, "Name the instrument, or the measurement is not
evidence"): the committed `exprs.py` translator, read in-process, against a
`dump_tables.pl` JSON named on the command line -- no ExifTool and no oxidex
binary are involved, so the header below names the source tree and the dump
and nothing else. This script publishes the headline "% of uses translated"
number, and until Step 30 it was the one census tool in this directory with
no header and no dirty-tree refusal at all: a percentage attributed to no
commit. Set `OXIDEX_ALLOW_DIRTY_TREE=1` to measure a dirty tree anyway; the
header records the override.
"""
import collections
import json
import pathlib
import re
import sys

import exprs

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "scripts"))
import instrument  # noqa: E402 -- git/instrument identity header

SLOTS = ("ValueConv", "PrintConv", "RawConv")


def walk_tags(node, out):
    if isinstance(node, dict):
        out.append(node)
    elif isinstance(node, list):
        for v in node:
            walk_tags(v, out)


def census(tables_json_path):
    with open(tables_json_path, encoding="utf-8") as fh:
        d = json.load(fh)
    counter = collections.Counter()
    slot_of = {}
    for _modname, mod in d["modules"].items():
        for _tname, t in (mod.get("tables") or {}).items():
            for _tid, tagnode in (t.get("tags") or {}).items():
                variants = []
                walk_tags(tagnode, variants)
                for tag in variants:
                    for slot in SLOTS:
                        v = tag.get(slot)
                        if isinstance(v, dict) and v.get("kind") == "expr":
                            e = v.get("expr")
                            if isinstance(e, str) and e.strip():
                                counter[e] += 1
                                slot_of.setdefault(e, slot)
    return d.get("exiftool_version"), counter, slot_of


def main():
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <tables.json>", file=sys.stderr)
        raise SystemExit(2)

    # Before the first number, never after it.
    git = instrument.git_state()
    dirty_overridden = instrument.refuse_if_dirty(git, "expr_coverage.py")
    instrument.print_header(
        tool="expr_coverage.py",
        git=git,
        dirty_overridden=dirty_overridden,
        extra=[f"tables:  {sys.argv[1]}",
               "reads:   the committed tools/exiftool-tables/exprs.py translator against "
               "that dump (no ExifTool and no oxidex binary involved)"],
    )

    version, counter, slot_of = census(sys.argv[1])
    total_uses = sum(counter.values())
    total_distinct = len(counter)

    translated_uses = translated_distinct = 0
    from_exact_uses = from_exact_distinct = 0
    from_compile_uses = from_compile_distinct = 0
    by_domain_uses = collections.Counter()
    by_domain_distinct = collections.Counter()
    refused = []
    crashed = []

    for e, n in counter.items():
        try:
            r = exprs.translate_or_compile_any(e)
        except Exception as ex:  # a crash is a refusal that also needs fixing
            crashed.append((n, e, repr(ex)))
            continue
        if not r:
            refused.append((n, e))
            continue
        domain, _rty, _code = r
        translated_uses += n
        translated_distinct += 1
        by_domain_uses[domain] += n
        by_domain_distinct[domain] += 1
        if exprs.normalize(e) in exprs.TRANSLATIONS:
            from_exact_uses += n
            from_exact_distinct += 1
        else:
            from_compile_uses += n
            from_compile_distinct += 1

    print(f"pinned release          {version}")
    print(f"distinct expressions    {total_distinct}")
    print(f"total uses               {total_uses}")
    print()
    print("MEASURED coverage (translate_or_compile_any: TRANSLATIONS + compile(), all domains):")
    print(
        f"  translated   uses {translated_uses:>5}/{total_uses} "
        f"= {100 * translated_uses / total_uses:5.1f}%   "
        f"distinct {translated_distinct:>4}/{total_distinct} "
        f"= {100 * translated_distinct / total_distinct:5.1f}%"
    )
    print(
        f"    of which exact-match (TRANSLATIONS)   uses {from_exact_uses:>5} "
        f"({100 * from_exact_uses / total_uses:5.1f}%)   distinct {from_exact_distinct:>4}"
    )
    print(
        f"    of which grammar-compiled (compile()) uses {from_compile_uses:>5} "
        f"({100 * from_compile_uses / total_uses:5.1f}%)   distinct {from_compile_distinct:>4}"
    )
    print(
        f"  refused      uses {total_uses - translated_uses:>5}/{total_uses} "
        f"= {100 * (total_uses - translated_uses) / total_uses:5.1f}%   "
        f"distinct {total_distinct - translated_distinct:>4}/{total_distinct}"
    )
    if crashed:
        print(f"  CRASHED (counted as refused above) {len(crashed)}")
        for n, e, ex in crashed[:10]:
            print(f"    {n:>5}  {ex}  {re.sub(r'\\s+', ' ', e.strip())[:70]}")
    print()
    print("by value domain (uses / distinct):")
    for dom in ("num", "str", "bytes"):
        print(f"  {dom:<6} uses {by_domain_uses[dom]:>5}   distinct {by_domain_distinct[dom]:>4}")
    print()

    print("top 40 translated expressions by uses:")
    translated_list = [
        (n, e) for e, n in counter.items() if exprs.translate_or_compile_any(e)
    ]
    translated_list.sort(reverse=True)
    for n, e in translated_list[:40]:
        s = re.sub(r"\s+", " ", e.strip())
        dom = exprs.translate_or_compile_any(e)[0]
        src = "exact" if exprs.normalize(e) in exprs.TRANSLATIONS else "compile"
        print(f"  {n:>5}  {dom:<6} {src:<8} {slot_of[e]:<10} {s[:70]}")

    print()
    print("REFUSED expressions ranked by uses (the coverage gap):")
    refused.sort(reverse=True)
    for n, e in refused:
        s = re.sub(r"\s+", " ", e.strip())
        print(f"  {n:>5}  {slot_of[e]:<10} {s[:90]}")


if __name__ == "__main__":
    main()
