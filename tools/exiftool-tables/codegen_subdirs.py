#!/usr/bin/env python3
"""Transcribe ExifTool `ProcessBinaryData` sub-directory tables into Rust.

Input is the JSON `dump_tables.pl` produces by loading ExifTool's modules and
reading the tag tables *out of memory* -- not a regex over the `.pm` text.  The
tables are assembled at require-time by loops, hash copies and `%binaryDataAttrs`
splices, so the source text and the hash ExifTool dispatches on are not the same
thing.

The rule this generator exists to enforce: **it hard-errors on any construct it
has not been taught, and says which construct and where.**  It never emits a
field it only half-understands.  A plausible number under a real ExifTool tag
name is worse than a missing tag, because nothing downstream can tell it is
wrong -- and a generator that logs a vague reason and moves on is how 657 tag
instances stayed unwired for weeks behind the note "model-conditional".

Supported per field:

  * `Format`: a scalar int/float format, `string[N]`, `undef[N]`, or an array
    `fmt[N]` of a scalar format.
  * `PrintConv`: absent, or a pure enum map (every key/value a plain scalar).
  * `RawConv`: absent, or one of ExifTool's two count-gate idioms --
    `$$self{X} = $val` (records a data member) and
    `$$self{X} < N ? undef : $val` (suppresses the tag below a count).
  * `Mask`.

Anything else -- a `ValueConv`, a `Hook`, a `Condition`, a nested
`SubDirectory`, a `PrintConv` with `BITMASK`/`OTHER`/code, an arrayref of
model-conditional variants -- raises `Unsupported`, naming the table, the tag
and the offending construct.  Run with `--allow-skip` to downgrade those to a
machine-logged skip line and continue; the log is the deliverable, not a
footnote.

usage:
  dump_tables.pl <exiftool-lib> Panasonic > tables.json
  codegen_subdirs.py tables.json --module Panasonic \
      --table FaceDetInfo --table FaceRecInfo -o out.rs
"""
from __future__ import annotations

import argparse
import json
import re
import sys

# ExifTool format name -> (Rust `Fmt` variant, element size in bytes).
SCALARS = {
    "int8u": ("U8", 1),
    "int8s": ("I8", 1),
    "int16u": ("U16", 2),
    "int16s": ("I16", 2),
    "int16uRev": ("U16Rev", 2),
    "int32u": ("U32", 4),
    "int32s": ("I32", 4),
    "float": ("F32", 4),
    "double": ("F64", 8),
}

SIZED_RE = re.compile(r"^(\w+)\[(\d+)\]$")

# `$$self{Name} = $val` -- the tag records a data member for later gates.
SET_RE = re.compile(r"^\$\$self\{(\w+)\}\s*=\s*\$val$")
# `$$self{Name} < N ? undef : $val` -- ExifTool's count gate.  The space before
# the colon is optional in the wild (`< 1 ? undef: $val` in Canon.pm:6741).
GATE_RE = re.compile(r"^\$\$self\{(\w+)\}\s*<\s*(\d+)\s*\?\s*undef\s*:\s*\$val$")


class Unsupported(Exception):
    """A construct the generator has not been taught. Never guessed at."""

    def __init__(self, table, tag, reason):
        super().__init__(f"{table}[{tag}]: {reason}")
        self.table, self.tag, self.reason = table, tag, reason


def rust_str(s):
    return s.replace("\\", "\\\\").replace('"', '\\"')


def parse_index(key, table):
    """ExifTool tag key -> integer index. Bit-field keys (`12.1`) are refused."""
    if "." in key:
        raise Unsupported(table, key, "bit-field index (ExifTool's `n.m` notation)")
    try:
        return int(str(key), 0)
    except ValueError:
        raise Unsupported(table, key, f"non-numeric index {key!r}") from None


def field_format(tag, table, key):
    """(Fmt variant, count) for a field, or (None, 1) to inherit the table FORMAT."""
    f = tag.get("Format")
    if f is None:
        return None, 1
    if not isinstance(f, str):
        raise Unsupported(table, key, f"Format is not a string: {f!r}")
    m = SIZED_RE.match(f)
    if m:
        base, n = m.group(1), int(m.group(2))
        if base == "string":
            return f"Fmt::Str({n})", 1
        if base == "undef":
            return f"Fmt::Undef({n})", 1
        if base in SCALARS:
            # `int16u[4]` is 4 repeats of int16u, not 4 bytes.
            return f"Fmt::{SCALARS[base][0]}", n
        raise Unsupported(table, key, f"unsupported array element format {base!r}")
    if f in SCALARS:
        return f"Fmt::{SCALARS[f][0]}", 1
    raise Unsupported(table, key, f"unsupported Format {f!r}")


def field_raw_conv(tag, table, key):
    """(set_member, gate) for a RawConv, refusing anything but the two idioms."""
    rc = tag.get("RawConv")
    if rc is None:
        return None, None
    if not isinstance(rc, dict):
        raise Unsupported(table, key, f"RawConv has unexpected shape {type(rc).__name__}")
    if rc.get("kind") != "expr":
        raise Unsupported(table, key, f"RawConv kind={rc.get('kind')!r} is not a plain expression")
    expr = (rc.get("expr") or "").strip()
    m = SET_RE.match(expr)
    if m:
        return m.group(1), None
    m = GATE_RE.match(expr)
    if m:
        return None, (m.group(1), int(m.group(2)))
    raise Unsupported(table, key, f"RawConv expression not in the known vocabulary: {expr!r}")


def field_print_conv(tag, table, key, pool):
    """A `PrintConv` as a Rust const name, or `PrintConv::None`."""
    pc = tag.get("PrintConv")
    if pc is None:
        return "PrintConv::None"
    if not isinstance(pc, dict):
        raise Unsupported(table, key, f"PrintConv has unexpected shape {type(pc).__name__}")
    kind = pc.get("kind")
    if kind != "enum":
        # enum_partial carries BITMASK/OTHER/Notes directives; code/expr are Perl.
        raise Unsupported(table, key, f"PrintConv kind={kind!r} is not a pure enum map")
    entries = []
    for k, v in pc.get("map", {}).items():
        try:
            ik = int(str(k), 0)
        except ValueError:
            raise Unsupported(table, key, f"PrintConv key {k!r} is not an integer") from None
        if not isinstance(v, str):
            raise Unsupported(table, key, f"PrintConv value for {k!r} is not a string")
        entries.append((ik, v))
    if not entries:
        raise Unsupported(table, key, "PrintConv is an empty map")
    entries.sort()
    body = ", ".join(f'({k}, "{rust_str(v)}")' for k, v in entries)
    name = pool.intern(body)
    return f"PrintConv::Map({name})"


class ConstPool:
    """De-duplicates identical enum maps into shared `&[(i64, &str)]` consts."""

    def __init__(self, prefix):
        self.prefix = prefix
        self.by_body = {}
        self.order = []

    def intern(self, body):
        if body not in self.by_body:
            name = f"{self.prefix}_CONV{len(self.order) + 1}"
            self.by_body[body] = name
            self.order.append((name, body))
        return self.by_body[body]

    def emit(self):
        return "\n".join(
            f"const {name}: &[(i64, &str)] = &[{body}];" for name, body in self.order
        )


# Keys that are documentation or writer-side only: present on a field without
# changing what a reader produces.
IGNORED_TAG_KEYS = {
    "Name", "Format", "RawConv", "PrintConv", "Mask", "Notes", "Description",
    "DataMember", "Writable", "Groups", "PrintConvInv", "ValueConvInv",
    "Protected", "Permanent", "SeparateTable", "PrintHex", "Priority",
    "_shorthand", "_extra_keys", "Unknown", "Hidden", "Avoid", "Binary",
    "RelatedTag", "Count", "Flags",
}


def gen_table(module, tname, tbl, pool, skips, allow_skip):
    meta = tbl.get("meta") or {}
    pp = meta.get("PROCESS_PROC")
    pp_name = pp.get("__name", "") if isinstance(pp, dict) else ""
    if not pp_name.endswith("ProcessBinaryData"):
        raise Unsupported(tname, "-", f"PROCESS_PROC is {pp_name or 'absent'}, not ProcessBinaryData")

    # ExifTool's ProcessBinaryData defaults FORMAT to int8u when a table omits it
    # (ExifTool.pm: `$format = $$tagTablePtr{FORMAT} || 'int8u'`).
    fmt_name = meta.get("FORMAT")
    if fmt_name is None:
        fmt_name = "int8u"
    if fmt_name not in SCALARS:
        raise Unsupported(tname, "-", f"table FORMAT {fmt_name!r} is not a scalar format")
    default_fmt, _ = SCALARS[fmt_name]
    first_entry = int(str(meta.get("FIRST_ENTRY", "0")), 0)

    rows = []
    for key in sorted(tbl["tags"], key=lambda k: parse_index(k, tname)):
        tag = tbl["tags"][key]
        try:
            if "_variants" in tag:
                raise Unsupported(tname, key, "arrayref of Condition variants")
            name = tag.get("Name")
            if not isinstance(name, str) or not name:
                raise Unsupported(tname, key, "no Name")
            if tag.get("Unknown"):
                # ExifTool hides these without -U; emitting them would be a diff
                # against the default output, not a gain.
                skips.append((tname, key, name, "Unknown => 1 (ExifTool hides it without -U)"))
                continue
            for k in tag:
                if k not in IGNORED_TAG_KEYS:
                    raise Unsupported(tname, key, f"unhandled tag key {k!r}")
            idx = parse_index(key, tname)
            fmt, count = field_format(tag, tname, key)
            member, gate = field_raw_conv(tag, tname, key)
            pc = field_print_conv(tag, tname, key, pool)
            if count > 1 and pc != "PrintConv::None":
                # ExifTool hands the *joined* array string to a hash PrintConv,
                # so an element-wise lookup would print something ExifTool never
                # does. Refuse rather than pick one of the two readings.
                raise Unsupported(tname, key, "array Format with a hash PrintConv")
            mask = tag.get("Mask")
            mask_s = "None" if mask is None else f"Some({int(str(mask), 0)})"
        except Unsupported as exc:
            if not allow_skip:
                raise
            skips.append((tname, key, tag.get("Name", "?"), exc.reason))
            continue
        rows.append(
            f"    Field {{ index: {idx}, name: \"{rust_str(name)}\", "
            f"format: {'None' if fmt is None else f'Some({fmt})'}, count: {count}, "
            f"set_member: {'None' if member is None else f'Some(\"{member}\")'}, "
            f"gate: {'None' if gate is None else f'Some((\"{gate[0]}\", {gate[1]}))'}, "
            f"mask: {mask_s}, print_conv: {pc} }},"
        )

    ident = re.sub(r"[^A-Za-z0-9]", "_", f"{module}_{tname}").upper()
    body = "\n".join(rows)
    return ident, f"""/// `Image::ExifTool::{module}::{tname}` -- {len(rows)} fields, FORMAT `{fmt_name}`.
///
/// Transcribed from ExifTool's in-memory tag table by
/// `tools/exiftool-tables/codegen_subdirs.py`. Do not edit by hand.
pub(crate) static {ident}: BinaryTable = BinaryTable {{
    name: "{tname}",
    default_format: Fmt::{default_fmt},
    first_entry: {first_entry},
    fields: &[
{body}
    ],
}};"""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("tables_json")
    ap.add_argument("--module", required=True)
    ap.add_argument("--table", action="append", required=True)
    ap.add_argument("--prefix", default=None, help="const-pool prefix (default: MODULE)")
    ap.add_argument("--allow-skip", action="store_true")
    ap.add_argument("-o", "--output", required=True)
    args = ap.parse_args()

    doc = json.load(open(args.tables_json))
    mod = doc["modules"][args.module]
    pool = ConstPool((args.prefix or args.module).upper())
    skips, chunks, idents = [], [], []
    for tname in args.table:
        if tname not in mod["tables"]:
            sys.exit(f"error: {args.module} has no table {tname!r}")
        ident, text = gen_table(
            args.module, tname, mod["tables"][tname], pool, skips, args.allow_skip
        )
        idents.append((tname, ident))
        chunks.append(text)

    header = f"""//! {args.module} MakerNote binary sub-directory tables.
//!
//! GENERATED by `tools/exiftool-tables/codegen_subdirs.py` from ExifTool
//! {doc['exiftool_version']}'s in-memory tag tables. Do not edit by hand.
//!
//! The generator refuses any construct it has not been taught and names it, so
//! a field that is here was reproduced exactly and a field that is missing was
//! reported as missing -- neither is a guess.

use crate::parsers::tiff::makernotes::shared::binary_subdir::{{
    BinaryTable, Field, Fmt, PrintConv,
}};
"""
    out = "\n\n".join([header, pool.emit()] + chunks) + "\n"
    with open(args.output, "w") as fh:
        fh.write(out)

    print(f"wrote {args.output}: {len(idents)} tables", file=sys.stderr)
    for tname, ident in idents:
        print(f"  {tname} -> {ident}", file=sys.stderr)
    if skips:
        print(f"REFUSED {len(skips)} field(s):", file=sys.stderr)
        for tname, key, name, reason in skips:
            print(f"  {tname}[{key}] {name}: {reason}", file=sys.stderr)


if __name__ == "__main__":
    main()
