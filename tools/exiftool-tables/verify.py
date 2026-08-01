#!/usr/bin/env python3
"""Check generated Rust against ExifTool, by parsing the Rust back out.

The property tested is SOUNDNESS, not completeness:

    every field and every enum entry present in the generated Rust must match
    ExifTool exactly.

Completeness is a separate question, already answered by codegen.py's own
skip accounting. Splitting the two matters. A generator that silently drops
hard cases scores well on "does everything I emitted match?" and badly on
"did I emit everything?", and only reporting both keeps the coverage number
honest. Conflating them is how a project ends up claiming 58% parity while
extracting 48.8%.

The Rust is parsed rather than trusted from the intermediate JSON: the JSON is
the codegen's *input*, so comparing against it would test nothing about the
codegen. Reading back what was actually written catches escaping bugs, integer
overflow in enum keys, sort-order mistakes that break binary_search, and
truncation -- the failures that compile perfectly.
"""

import argparse
import re
import subprocess
import sys
from collections import defaultdict

# Whitespace-tolerant on purpose: the generated file is run through rustfmt
# before it is committed, which wraps every `Field { .. }` across several lines.
# The original single-line patterns silently matched nothing after that change,
# and a verifier that parses zero fields reports no mismatches -- it looked like
# a pass. `parse_rust` now also asserts it accounted for every `Field {` in the
# file, so under-parsing fails loudly instead of quietly.
TABLE_RE = re.compile(
    r'pub static \w+: BinaryTable = BinaryTable \{\s*'
    r'module:\s*"(?P<module>[^"]*)",\s*'
    r'table:\s*"(?P<table>[^"]*)",',
)
FIELD_RE = re.compile(
    r'Field\s*\{\s*'
    r'index:\s*(?P<index>-?\d+),\s*'
    r'sub:\s*(?P<sub>None|Some\(\d+\)),\s*'
    r'name:\s*"(?P<name>(?:[^"\\]|\\.)*)",\s*'
    r'format:\s*(?P<fmt>None|Some\(Fmt::\w+(?:\(\d+\))?\)),\s*'
    r'print_conv:\s*(?P<pc>PrintConv::(?:None'
    r'|Expr\(ExprId::\w+\)'
    r'|IntEnum\(&\[.*?\]\)'
    r'|StrEnum\(&\[.*?\]\)))',
    re.S,
)
FIELD_COUNT_RE = re.compile(r'Field\s*\{\s*index:')
INT_ENUM_RE = re.compile(r'PrintConv::IntEnum\(&\[(.*)\]\)$', re.S)
STR_ENUM_RE = re.compile(r'PrintConv::StrEnum\(&\[(.*)\]\)$', re.S)
INT_PAIR_RE = re.compile(r'\(\s*(-?\d+),\s*"((?:[^"\\]|\\.)*)"\s*\)')
STR_PAIR_RE = re.compile(r'\(\s*"((?:[^"\\]|\\.)*)",\s*"((?:[^"\\]|\\.)*)"\s*\)')


def unescape(s):
    return (s.replace('\\\\', '\x00')
             .replace('\\"', '"').replace('\\n', '\n')
             .replace('\\r', '\r').replace('\\t', '\t')
             .replace('\x00', '\\'))


def parse_rust(path):
    """-> (fields{(mod,tbl,idx)->name}, enums{(mod,tbl,idx)->{key:val}})"""
    with open(path, encoding="utf-8") as fh:
        src = fh.read()

    fields, enums = {}, defaultdict(dict)
    expected = len(FIELD_COUNT_RE.findall(src))
    bounds = [(m.start(), m.group("module"), m.group("table"))
              for m in TABLE_RE.finditer(src)]
    bounds.append((len(src), None, None))

    for i in range(len(bounds) - 1):
        start, mod, tbl = bounds[i]
        end = bounds[i + 1][0]
        for f in FIELD_RE.finditer(src, start, end):
            # Sub-indexed bit-fields share a byte offset; the oracle keys them
            # by ExifTool's original "12.1" string, so rebuild that form.
            sub = f.group("sub")
            idx = f.group("index")
            key = idx if sub == "None" else f"{idx}.{sub[5:-1]}"
            k = (mod, tbl, key)
            fields[k] = unescape(f.group("name"))

            pc = f.group("pc")
            m = INT_ENUM_RE.search(pc)
            if m:
                for kk, vv in INT_PAIR_RE.findall(m.group(1)):
                    enums[k][kk] = unescape(vv)
                continue
            m = STR_ENUM_RE.search(pc)
            if m:
                for kk, vv in STR_PAIR_RE.findall(m.group(1)):
                    enums[k][unescape(kk)] = unescape(vv)

    # Every Field in the file must have been parsed. Without this, a formatting
    # change that defeats the pattern degrades into a silent partial check.
    if len(fields) != expected:
        raise SystemExit(
            f"parsed {len(fields)} fields but the file contains {expected} "
            "-- the verifier's pattern is out of date; fix it before trusting a PASS"
        )
    return fields, enums


def load_oracle(lib, oracle_pl):
    out = subprocess.run(
        ["perl", oracle_pl, lib],
        capture_output=True, check=True, text=True, encoding="utf-8",
    ).stdout
    names, enums = {}, defaultdict(dict)
    for line in out.splitlines():
        p = line.split("\t")
        if len(p) == 4:
            names[(p[0], p[1], p[2])] = p[3]
        elif len(p) == 6 and p[3] == "ENUM":
            enums[(p[0], p[1], p[2])][p[4]] = p[5]
    return names, enums


def norm_key(k):
    """ExifTool enum keys may be decimal or hex; compare numerically."""
    try:
        return str(int(str(k), 0))
    except ValueError:
        return str(k)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("generated_rs")
    ap.add_argument("exiftool_lib")
    ap.add_argument("--oracle", default="oracle.pl")
    ap.add_argument("--show", type=int, default=10)
    args = ap.parse_args()

    gen_fields, gen_enums = parse_rust(args.generated_rs)
    or_names, or_enums = load_oracle(args.exiftool_lib, args.oracle)

    if not gen_fields:
        sys.exit("parsed 0 fields from generated Rust -- verifier is broken, "
                 "not the generator; fix the parser before trusting a PASS")

    name_ok = name_bad = orphan = 0
    enum_ok = enum_bad = 0
    bad_examples, orphan_examples, enum_examples = [], [], []

    for k, name in gen_fields.items():
        truth = or_names.get(k)
        if truth is None:
            orphan += 1
            if len(orphan_examples) < args.show:
                orphan_examples.append(k)
            continue
        if truth == name:
            name_ok += 1
        else:
            name_bad += 1
            if len(bad_examples) < args.show:
                bad_examples.append((k, name, truth))

    for k, m in gen_enums.items():
        truth = {norm_key(a): b for a, b in or_enums.get(k, {}).items()}
        for kk, vv in m.items():
            t = truth.get(norm_key(kk))
            if t == vv:
                enum_ok += 1
            else:
                enum_bad += 1
                if len(enum_examples) < args.show:
                    enum_examples.append((k, kk, vv, t))

    print(f"fields checked   {name_ok + name_bad}")
    print(f"  match          {name_ok}")
    print(f"  MISMATCH       {name_bad}")
    print(f"  not in oracle  {orphan}")
    print(f"enum entries     {enum_ok + enum_bad}")
    print(f"  match          {enum_ok}")
    print(f"  MISMATCH       {enum_bad}")

    for k, got, want in bad_examples:
        print(f"  name  {k}: generated {got!r} != exiftool {want!r}")
    for k in orphan_examples:
        print(f"  orphan {k}")
    for k, kk, got, want in enum_examples:
        print(f"  enum  {k} key {kk}: generated {got!r} != exiftool {want!r}")

    failed = name_bad + enum_bad + orphan
    print("\nRESULT:", "PASS" if failed == 0 else f"FAIL ({failed} discrepancies)")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
