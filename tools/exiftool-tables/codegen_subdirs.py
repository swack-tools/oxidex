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
    """ExifTool tag key -> integer index.

    A key like `0.2` is not a fractional offset: `ProcessBinaryData` computes the
    entry as `int($index) * $increment`, so the fraction only lets the hash hold
    several tags at one offset. Which bits each of them takes is `Mask`, not the
    fraction -- so a table with `0.1`/`0.2` and no masks would be two whole reads
    of the same word, which is what ExifTool does.
    """
    text = str(key)
    if "." in text:
        whole, _, frac = text.partition(".")
        if not frac.isdigit():
            raise Unsupported(table, key, f"non-numeric bit-field key {key!r}")
        text = whole
    try:
        return int(text, 0)
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


def field_priority(tag, table_priority, table, key):
    """True when ExifTool would give this field priority 0.

    `FoundTag` reads a tag's priority at ExifTool.pm:9469-9473 -- the tag's own
    `Priority`, else the table's `PRIORITY`, else 0 when the tag is `Avoid` --
    and a 0 there means the field never displaces a value already reported
    under the same name (see `shared::tag_priority`). Dropping it silently
    prints a sub-directory's copy of a tag over the `Main` table's copy.

    Only 0 is modelled. ExifTool also uses -1, 1 and 2, but none of those
    appears in a `ProcessBinaryData` table, and treating an unmodelled value as
    "normal" would be a guess at which of two real values prints.
    """
    priority = tag.get("Priority")
    if priority is None:
        if table_priority is not None:
            return table_priority == 0
        return bool(tag.get("Avoid"))
    priority = int(str(priority), 0)
    if priority != 0:
        raise Unsupported(table, key, f"Priority => {priority} is not modelled")
    return True


def normalize_deparse(text):
    """A Perl sub body reduced to a stable key.

    `B::Deparse` prefixes every body with the package and pragma context, which
    is noise for identification, and indents to taste. Everything else is kept:
    the point of keying on the body is that an upstream edit changes the key and
    the generator stops, instead of a stale translation living on under a real
    tag name.
    """
    lines = [
        line.strip()
        for line in text.splitlines()
        if line.strip() and not re.match(r"^\s*(package\s|use\s+strict|use\s+warnings|no\s+)", line)
    ]
    return " ".join(lines)


# Translations for `PrintConv => { ..., OTHER => sub {...} }`, keyed on the
# normalized body of the sub itself.
#
# `OTHER` is the fallback ExifTool runs for a value the hash does not list, and
# it is an anonymous closure -- there is no name to look it up by and no
# expression string to register, which is why `dump_tables.pl` deparses it. Each
# entry below names a Rust function that reproduces that exact body, and the
# generator refuses any `OTHER` it does not find here. The read direction only:
# the `$inv` branch is ExifTool's writer.
OTHER_CONVS = {
    # FujiFilm.pm:1089 -- AFAreaPointSize: the number itself.
    "{ (return $_[0]); }": "identity",
    # FujiFilm.pm:1097-1108 -- AFAreaZoneSize: "<w> x <h>" out of one byte.
    "{ (my($val, $inv) = @_); my($w, $h); if ($inv) { (my($w, $h) = ($val =~ /(\\d+)/g)); "
    "(($w and $h) or (return 0)); (return ((($h << 5) & 240) | ($w & 15))); } "
    '(($w, $h) = (($val & 15), ($val >> 5))); (return ("$w x $h")); }': "zone_size",
    # FujiFilm.pm:1131-1135 -- AF-CSetting: a preset the table does not list.
    "{ (my($val, $inv) = @_); ($inv and (return (($val =~ /(0x\\w+)/) ? hex($1) : (undef)))); "
    "(return sprintf('Set 6 (custom 0x%.3x)', $val)); }": "custom_afc_set",
    # FujiFilm.pm:1180-1186 -- DriveSpeed: frames per second.
    '{ (my($val, $inv) = @_); ($inv or (return ("$val fps"))); ($val =~ s/ ?fps$//); '
    "(return $val); }": "fps",
}

# Translations for `ValueConv`, in the two shapes ExifTool writes it.
#
# A `ValueConv` is a real computation, not a lookup, so there is no way to carry
# it as data: it has to be ported. Both registries key on ExifTool's own text --
# an expression verbatim, a code ref by its deparsed body -- so a translation is
# bound to the exact source it was written against and an upstream edit stops the
# generator instead of leaving a stale number behind a real tag name.
#
# Scalar expressions apply per element. A list conversion sees the whole element
# list, which is the only way to express a conversion that treats each slot of an
# array differently.
SCALAR_VALUE_CONVS = {
    # Pentax.pm:5744, :5750 -- CompositionAdjustX/Y, steps in the opposite sense.
    "-$val": "negate",
    # Pentax.pm:5734, :5740, :5760 -- RollAngle, PitchAngle,
    # CompositionAdjustRotation: half-degree steps, opposite sense.
    "-$val / 2": "negate_half",
}
LIST_VALUE_CONVS = {
    # Pentax.pm:837-840 -- `%kelvinWB`, shared by all 17 KelvinWB_* tags.
    "{ (my @a = split(' ', (shift()), 0)); ((53190 - $a[0]) . ' ' . $a[1] . ' ' "
    ". ($a[2] / 8192) . ' ' . ($a[3] / 8192)); }": "kelvin_wb",
}


def field_value_conv(tag, table, key, conv_prefix):
    """A `ValueConv` as a Rust expression, or `ValueConv::None`."""
    vc = tag.get("ValueConv")
    if vc is None:
        return "ValueConv::None"
    if not isinstance(vc, dict):
        raise Unsupported(table, key, f"ValueConv has unexpected shape {type(vc).__name__}")
    kind = vc.get("kind")
    if kind == "expr":
        expr = (vc.get("expr") or "").strip()
        fn = SCALAR_VALUE_CONVS.get(expr)
        if fn is None:
            raise Unsupported(table, key, f"ValueConv expression not in SCALAR_VALUE_CONVS: {expr!r}")
        return f"ValueConv::Each({conv_prefix}::{fn})"
    if kind == "code":
        source = vc.get("deparse")
        if not source:
            raise Unsupported(table, key, "ValueConv is a sub with no recoverable body")
        fn = LIST_VALUE_CONVS.get(normalize_deparse(source))
        if fn is None:
            raise Unsupported(
                table, key, "ValueConv body is not in LIST_VALUE_CONVS: " + normalize_deparse(source)
            )
        return f"ValueConv::List({conv_prefix}::{fn})"
    raise Unsupported(table, key, f"ValueConv kind={kind!r} is neither an expression nor a sub")


# `PrintConv` hash keys that direct ExifTool rather than naming a value, and
# that a reader can carry without changing what it prints.
BENIGN_PC_DIRECTIVES = {
    # Documentation only: how `BuildTagLookup` renders the keys, and in what
    # order. Neither reaches the value ExifTool reports for a file.
    "PrintHex",
    "PrintSort",
    "Notes",
    "SeparateTable",
}


def field_print_conv(tag, table, key, pool, conv_prefix):
    """A `PrintConv` as a Rust expression, or `PrintConv::None`."""
    pc = tag.get("PrintConv")
    if pc is None:
        return "PrintConv::None"
    if not isinstance(pc, dict):
        raise Unsupported(table, key, f"PrintConv has unexpected shape {type(pc).__name__}")
    kind = pc.get("kind")
    other = None
    if kind == "enum_partial":
        directives = pc.get("directives") or {}
        unknown = set(directives) - BENIGN_PC_DIRECTIVES - {"OTHER"}
        if unknown:
            raise Unsupported(
                table, key, f"PrintConv carries directive(s) {sorted(unknown)!r}"
            )
        if "OTHER" in directives:
            spec = directives["OTHER"]
            source = spec.get("__deparse") if isinstance(spec, dict) else None
            if not source:
                raise Unsupported(table, key, "PrintConv OTHER is a sub with no recoverable body")
            fn = OTHER_CONVS.get(normalize_deparse(source))
            if fn is None:
                raise Unsupported(
                    table,
                    key,
                    "PrintConv OTHER body is not in OTHER_CONVS: "
                    + normalize_deparse(source),
                )
            other = f"{conv_prefix}::{fn}"
    elif kind != "enum":
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
    if other is not None:
        return f"PrintConv::MapOr({name}, {other})"
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


# Every tag key this generator knows about. A key outside this set stops the
# table rather than being skipped, so an ExifTool edit that adds one cannot pass
# silently as "no change".
#
# The first group is read above -- `Name`, `Format`/`Count`, `RawConv`,
# `PrintConv`, `ValueConv`, `Mask`, and `Priority`/`Avoid` (by `field_priority`)
# all change what a reader produces. `Priority` in particular sat in the second
# group until it was found printing a sub-directory's `LensType` over the one
# `%Pentax::Main` reports; it is not documentation.
#
# The second group is documentation or writer-side only: present on a field
# without changing what a reader produces.
KNOWN_TAG_KEYS = {
    "Name", "Format", "Count", "RawConv", "PrintConv", "ValueConv", "Mask",
    "Priority", "Avoid",
    "Notes", "Description", "DataMember", "Writable", "Groups", "PrintConvInv",
    "ValueConvInv", "Protected", "Permanent", "SeparateTable", "PrintHex",
    "_shorthand", "_extra_keys", "Unknown", "Hidden", "Binary",
    "RelatedTag", "Flags",
}


def gen_table(module, tname, tbl, pool, skips, allow_skip, conv_prefix):
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
    # The table's default priority, used for any tag without its own `Priority`
    # (`$priority = $$tbl{PRIORITY}`, ExifTool.pm:9471). No `ProcessBinaryData`
    # table declares one today, so this is here to keep a future one from
    # arriving as a silent behaviour change.
    table_priority = meta.get("PRIORITY")
    if table_priority is not None:
        table_priority = int(str(table_priority), 0)

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
                if k not in KNOWN_TAG_KEYS:
                    raise Unsupported(tname, key, f"unhandled tag key {k!r}")
            idx = parse_index(key, tname)
            fmt, count = field_format(tag, tname, key)
            member, gate = field_raw_conv(tag, tname, key)
            low_priority = field_priority(tag, table_priority, tname, key)
            pc = field_print_conv(tag, tname, key, pool, conv_prefix)
            vc = field_value_conv(tag, tname, key, conv_prefix)
            if vc != "ValueConv::None" and pc != "PrintConv::None":
                # ExifTool runs ValueConv then PrintConv; nothing here does both,
                # and guessing the composition would be inventing an output.
                raise Unsupported(tname, key, "ValueConv combined with a PrintConv")
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
            f"mask: {mask_s}, value_conv: {vc}, print_conv: {pc}, "
            f"low_priority: {'true' if low_priority else 'false'} }},"
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
    ap.add_argument(
        "--other-conv-mod",
        default="super::print_conv",
        help="Rust path holding the hand-written OTHER PrintConv fallbacks",
    )
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
            args.module,
            tname,
            mod["tables"][tname],
            pool,
            skips,
            args.allow_skip,
            args.other_conv_mod,
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
    BinaryTable, Field, Fmt, PrintConv, ValueConv,
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
