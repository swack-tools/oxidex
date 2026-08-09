#!/usr/bin/env python3
"""Check generated Rust against ExifTool, by parsing the Rust back out.

The property tested is SOUNDNESS, not completeness:

    every field, every enum entry and every mask present in the generated Rust
    must match ExifTool exactly, and the release it was transcribed from must be
    the one the repo pins.

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
from pathlib import Path

# The repo root, derived from this file's location rather than the working
# directory: CI, the justfile and regen.sh all invoke this script from
# different places, and a cwd-relative pin lookup would silently read nothing.
REPO_ROOT = Path(__file__).resolve().parents[2]
PIN_FILE = REPO_ROOT / ".exiftool-version"

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
    r'count:\s*(?P<count>\d+),\s*'
    r'mask:\s*(?:None'
    r'|Some\(\s*Mask\s*\{\s*bits:\s*(?P<mask_bits>0[xX][0-9a-fA-F_]+|\d+),\s*'
    r'shift:\s*(?P<mask_shift>\d+),?\s*\}\s*,?\s*\)),\s*'
    r'omitted:\s*(?:Omitted::NONE|Omitted\s*\{[^{}]*\}),\s*'
    r'print_conv:\s*(?P<pc>PrintConv::(?:None'
    r'|Expr\(ExprId::\w+\)'
    r'|IntEnum\(&\[.*?\]\)'
    r'|StrEnum\(&\[.*?\]\)))',
    re.S,
)
FIELD_COUNT_RE = re.compile(r'Field\s*\{\s*index:')
VERSION_RE = re.compile(r'pub const EXIFTOOL_VERSION: &str = "([^"]+)";')
# The `,?` before the closing paren matters: rustfmt wraps long tuples onto
# multiple lines and leaves a trailing comma before `)`, and a pattern that
# refuses that comma silently skips every wrapped entry.
INT_PAIR_RE = re.compile(r'\(\s*(-?\d+),\s*"((?:[^"\\]|\\.)*)"\s*,?\s*\)')
STR_PAIR_RE = re.compile(r'\(\s*"((?:[^"\\]|\\.)*)",\s*"((?:[^"\\]|\\.)*)"\s*,?\s*\)')


def _enum_body(src, open_bracket):
    """The text between `[` at open_bracket and its true matching `]`.

    Also returns the number of top-level `(..)` tuples inside it. A regex
    cannot do this job: enum descriptions may contain `])`, which truncated
    the lazy `.*?\\]\\)` capture and silently dropped the rest of that enum
    from verification. This scan honors string literals and escapes.
    """
    i = open_bracket + 1
    depth, paren_depth, tuples, in_str = 1, 0, 0, False
    while i < len(src):
        c = src[i]
        if in_str:
            if c == "\\":
                i += 2
                continue
            if c == '"':
                in_str = False
        elif c == '"':
            in_str = True
        elif c == "(":
            if paren_depth == 0:
                tuples += 1
            paren_depth += 1
        elif c == ")":
            paren_depth -= 1
        elif c == "[":
            depth += 1
        elif c == "]":
            depth -= 1
            if depth == 0:
                return src[open_bracket + 1:i], tuples
        i += 1
    raise SystemExit("unterminated enum body in generated file")


def unescape(s):
    return (s.replace('\\\\', '\x00')
             .replace('\\"', '"').replace('\\n', '\n')
             .replace('\\r', '\r').replace('\\t', '\t')
             .replace('\x00', '\\'))


def parse_rust(path):
    """-> (fields{k->name}, enums{k->{key:val}}, masks{k->(bits,shift)})"""
    with open(path, encoding="utf-8") as fh:
        src = fh.read()

    fields, enums, masks = {}, defaultdict(dict), {}
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

            bits = f.group("mask_bits")
            if bits is not None:
                masks[k] = (int(bits, 0), int(f.group("mask_shift")))

            pc = f.group("pc")
            # Rescan the enum body from the source itself rather than
            # trusting the regex capture -- see _enum_body for why.
            for marker, pair_re, int_keys in (
                ("PrintConv::IntEnum(&[", INT_PAIR_RE, True),
                ("PrintConv::StrEnum(&[", STR_PAIR_RE, False),
            ):
                if not pc.startswith(marker):
                    continue
                body, expected_pairs = _enum_body(
                    src, f.start("pc") + len(marker) - 1
                )
                pairs = pair_re.findall(body)
                # Exact per-enum accounting, the same discipline the field
                # count gets below: a pair the pattern cannot read must fail
                # the run, not silently shrink the verified surface.
                if len(pairs) != expected_pairs:
                    raise SystemExit(
                        f"enum for {k}: parsed {len(pairs)} of "
                        f"{expected_pairs} entries -- the verifier's pair "
                        "pattern is out of date; fix it before trusting a PASS"
                    )
                for kk, vv in pairs:
                    if int_keys:
                        enums[k][kk] = unescape(vv)
                    else:
                        enums[k][unescape(kk)] = unescape(vv)
                break

    # Every Field in the file must have been parsed. Without this, a formatting
    # change that defeats the pattern degrades into a silent partial check.
    if len(fields) != expected:
        raise SystemExit(
            f"parsed {len(fields)} fields but the file contains {expected} "
            "-- the verifier's pattern is out of date; fix it before trusting a PASS"
        )
    return fields, enums, masks


def load_oracle(lib, oracle_pl):
    out = subprocess.run(
        ["perl", oracle_pl, lib],
        capture_output=True, check=True, text=True, encoding="utf-8",
    ).stdout
    names, enums, masks = {}, defaultdict(dict), {}
    for line in out.splitlines():
        p = line.split("\t")
        if len(p) == 4:
            names[(p[0], p[1], p[2])] = p[3]
        elif len(p) == 6 and p[3] == "ENUM":
            enums[(p[0], p[1], p[2])][p[4]] = p[5]
        elif len(p) == 6 and p[3] == "MASK":
            masks[(p[0], p[1], p[2])] = (int(p[4], 0), int(p[5]))
    return names, enums, masks


def oracle_version(lib):
    """The ExifTool release living in `lib`, read the same way the oracle does."""
    return subprocess.run(
        ["perl", f"-I{lib}", "-e",
         "require Image::ExifTool; print $Image::ExifTool::VERSION"],
        capture_output=True, check=True, text=True, encoding="utf-8",
    ).stdout.strip()


def repo_pin():
    """The ExifTool release this repo grades everything against."""
    try:
        pin = PIN_FILE.read_text(encoding="utf-8").strip()
    except OSError as exc:
        raise SystemExit(f"cannot read the ExifTool pin at {PIN_FILE}: {exc}") from exc
    if not pin:
        raise SystemExit(f"{PIN_FILE} is empty -- it must name one ExifTool release")
    return pin


def check_version(generated_rs, lib):
    """Refuse to verify unless artifact, oracle and repo pin are the same release.

    Two distinct skews are possible, and only the second used to be caught:

      stamp vs pin    the committed tables were transcribed from a release the
                      repo no longer grades against. This is the one that hides:
                      `.exiftool-version` is the declared source of truth, but
                      this check used to take its expected release from the
                      artifact's own stamp -- and both callers (the justfile
                      recipe and CI's verify-tables job) picked which ExifTool
                      to fetch from that same stamp. The loop closed on itself,
                      so a table set frozen at 13.30 verified against 13.30 and
                      reported PASS forever while every coverage number in the
                      repo was measured against 13.59. Anchoring to the pin is
                      what makes staleness expressible at all.

      stamp vs lib    you pointed the verifier at the wrong ExifTool. Without
                      this, every field ExifTool has since renamed and every
                      enum value it has inserted counts as a mismatch, which
                      reads as "the transcription is wrong" -- a diagnosis that
                      costs far more to reach from several hundred plausible
                      differences than from one line.
    """
    with open(generated_rs, encoding="utf-8") as fh:
        m = VERSION_RE.search(fh.read())
    if not m:
        raise SystemExit(
            f"{generated_rs} carries no EXIFTOOL_VERSION stamp -- regenerate it "
            "with `just regen-tables`; an unstamped table set cannot be verified"
        )
    stamped, pinned = m.group(1), repo_pin()
    if stamped != pinned:
        raise SystemExit(
            f"ExifTool pin skew: {generated_rs} was transcribed from {stamped}, "
            f"but {PIN_FILE.name} pins {pinned}.\nThe pin is the only source of "
            "truth for which release this repo grades against, so tables from "
            "any other release are stale by definition -- regenerate them with "
            "`just regen-tables`.\nIf you meant to move the whole repo to "
            f"{stamped}, change {PIN_FILE.name} first, then regenerate."
        )
    actual = oracle_version(lib)
    if stamped != actual:
        raise SystemExit(
            f"ExifTool version skew: tables were generated from {stamped}, but "
            f"{lib} is {actual}.\nPoint the verifier at the pinned release "
            f"({pinned}) -- `just verify-tables` fetches it for you."
        )
    return stamped


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

    version = check_version(args.generated_rs, args.exiftool_lib)
    print(f"ExifTool {version}")
    gen_fields, gen_enums, gen_masks = parse_rust(args.generated_rs)
    or_names, or_enums, or_masks = load_oracle(args.exiftool_lib, args.oracle)

    if not gen_fields:
        sys.exit("parsed 0 fields from generated Rust -- verifier is broken, "
                 "not the generator; fix the parser before trusting a PASS")

    name_ok = name_bad = orphan = 0
    enum_ok = enum_bad = 0
    mask_ok = mask_bad = 0
    bad_examples, orphan_examples, enum_examples = [], [], []
    mask_examples = []

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

    # Masks decide what a field's value IS, not merely how it prints, so a
    # wrong one is a wrong number under a real tag name. Checked in both
    # directions: a mask the generator invented and a mask it silently dropped
    # are equally wrong, and only comparing the union catches the second.
    for k in set(gen_masks) | {k for k in or_masks if k in gen_fields}:
        got, want = gen_masks.get(k), or_masks.get(k)
        if got == want:
            mask_ok += 1
        else:
            mask_bad += 1
            if len(mask_examples) < args.show:
                mask_examples.append((k, got, want))

    print(f"fields checked   {name_ok + name_bad}")
    print(f"  match          {name_ok}")
    print(f"  MISMATCH       {name_bad}")
    print(f"  not in oracle  {orphan}")
    print(f"enum entries     {enum_ok + enum_bad}")
    print(f"  match          {enum_ok}")
    print(f"  MISMATCH       {enum_bad}")
    print(f"masked fields    {mask_ok + mask_bad}")
    print(f"  match          {mask_ok}")
    print(f"  MISMATCH       {mask_bad}")

    for k, got, want in bad_examples:
        print(f"  name  {k}: generated {got!r} != exiftool {want!r}")
    for k in orphan_examples:
        print(f"  orphan {k}")
    for k, kk, got, want in enum_examples:
        print(f"  enum  {k} key {kk}: generated {got!r} != exiftool {want!r}")
    for k, got, want in mask_examples:
        print(f"  mask  {k}: generated {got!r} != exiftool {want!r}")

    failed = name_bad + enum_bad + orphan + mask_bad
    print("\nRESULT:", "PASS" if failed == 0 else f"FAIL ({failed} discrepancies)")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
