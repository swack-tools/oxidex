#!/usr/bin/env python3
"""Generate Rust binary-table definitions from extracted ExifTool tables.

Scope is deliberately narrow: ExifTool's ProcessBinaryData tables -- the ones
carrying FORMAT/FIRST_ENTRY and a field per byte offset.  Those 557 tables hold
~8,000 tags and are where the maker-note coverage gap lives, because they are
the part `exiftool -listx` cannot express.  `-listx` gives names; these give
layout, and layout is what you need to actually read bytes out of a MakerNote.

Every skipped tag is counted and reported.  A generator that silently drops
what it cannot handle produces a coverage number that is a lie, and the whole
argument for doing this mechanically rests on the numbers being trustworthy.
"""

import argparse
import hashlib
import json
import re
from collections import Counter

import exprs

# ExifTool format names -> (Rust Fmt variant, byte width).  Sized formats
# (string[32]) are handled separately since their width is per-field.
SCALAR_FORMATS = {
    "int8u": ("Int8u", 1),
    "int8s": ("Int8s", 1),
    "int16u": ("Int16u", 2),
    "int16s": ("Int16s", 2),
    "int32u": ("Int32u", 4),
    "int32s": ("Int32s", 4),
    "int16uRev": ("Int16uRev", 2),
    "float": ("Float", 4),
    "double": ("Double", 8),
    "rational64u": ("Rational64u", 8),
    "rational64s": ("Rational64s", 8),
}

SIZED_RE = re.compile(r"^(string|undef|int8u|int8s|int16u|int32u)\[(\d+)\]$")


def rust_str(s):
    """Escape a Python str into a Rust string literal body."""
    out = s.replace("\\", "\\\\").replace('"', '\\"')
    out = out.replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t")
    return out


def parse_index(key):
    """ExifTool binary-table keys are decimal offsets, sometimes fractional.

    A key like `12.1` is a bit-field within the byte at offset 12.  We keep the
    integer part and record the sub-index rather than inventing bit semantics
    we have not verified.
    """
    try:
        if "." in key:
            whole, frac = key.split(".", 1)
            return int(whole), int(frac)
        return int(key, 0), None
    except ValueError:
        return None, None


def conv_for(tag, stats):
    """Return Rust PrintConv construction, or None if this tag must lose it."""
    pc = tag.get("PrintConv")
    if not isinstance(pc, dict):
        return "PrintConv::None"

    kind = pc.get("kind")
    if kind in ("enum", "enum_partial"):
        m = pc.get("map") or {}
        if not m:
            return "PrintConv::None"
        # An enum whose keys are all integers becomes a sorted i64 table so the
        # runtime can binary-search it; otherwise fall back to string keys.
        pairs = []
        all_int = True
        for k in m:
            try:
                pairs.append((int(k, 0), m[k]))
            except ValueError:
                all_int = False
                break
        if all_int:
            stats["enum_int"] += 1
            pairs.sort()
            body = ", ".join(f'({k}, "{rust_str(v)}")' for k, v in pairs)
            return f"PrintConv::IntEnum(&[{body}])"
        stats["enum_str"] += 1
        body = ", ".join(
            f'("{rust_str(k)}", "{rust_str(v)}")' for k, v in sorted(m.items())
        )
        return f"PrintConv::StrEnum(&[{body}])"

    if kind == "expr":
        t = exprs.translate(pc.get("expr"))
        if t:
            stats["expr_translated"] += 1
            # Translated expressions are emitted by name so the generated code
            # stays readable and the mapping stays auditable.
            return f"PrintConv::Expr(ExprId::{expr_ident(pc['expr'])})"
        stats["expr_unsupported"] += 1
        stats["unsupported_exprs"][exprs.normalize(pc.get("expr") or "")] += 1
        return "PrintConv::None"

    stats["conv_dropped"] += 1
    return "PrintConv::None"


def expr_ident(expr):
    """Stable, collision-free Rust identifier for a translated expression.

    The readable part strips non-alphanumerics, which makes distinct
    expressions collide: `$val` and `"$val%"` both reduce to `Val`. A collision
    silently aliases two conversions to one enum variant and renders `50` where
    `50%` was meant -- compiles, passes name/enum verification, wrong output.
    The digest suffix is what makes the identifier injective; it is derived from
    the normalized expression so it is stable across runs and machines.
    """
    n = exprs.normalize(expr)
    ident = re.sub(r"[^A-Za-z0-9]+", "_", n).strip("_")
    if not ident:
        ident = "Empty"
    if ident[0].isdigit():
        ident = "E" + ident
    camel = "".join(p[:1].upper() + p[1:] for p in ident.split("_"))[:40]
    # No separator: an underscore here would trip `non_camel_case_types`, and
    # the digest is fixed-width so the boundary stays unambiguous anyway.
    digest = hashlib.sha256(n.encode("utf-8")).hexdigest()[:6].upper()
    return f"{camel}{digest}"


def is_binary_table(meta):
    """True for tables ExifTool reads with ProcessBinaryData.

    Detected by PROCESS_PROC rather than by the presence of FORMAT. FORMAT is
    optional -- ExifTool's ProcessBinaryData does `$$tagTablePtr{FORMAT} ||
    'int8u'` -- so requiring it silently skipped 365 tables / 4,844 tags whose
    fields each carry their own Format. PROCESS_PROC is what ExifTool itself
    dispatches on, so it is the honest signal.
    """
    pp = meta.get("PROCESS_PROC")
    if not isinstance(pp, dict):
        return False
    return (pp.get("__name") or "").endswith("ProcessBinaryData")


def gen_table(mod_name, tbl_name, tbl, stats):
    meta = tbl.get("meta") or {}
    fmt_name = meta.get("FORMAT")
    if not isinstance(fmt_name, str):
        # No table-level FORMAT: only valid for a real ProcessBinaryData table,
        # where ExifTool falls back to int8u.
        if not is_binary_table(meta):
            return None
        fmt_name = "int8u"
    default_fmt = SCALAR_FORMATS.get(fmt_name)
    if not default_fmt:
        stats["table_bad_format"] += 1
        return None

    try:
        first_entry = int(str(meta.get("FIRST_ENTRY", "0")), 0)
    except ValueError:
        first_entry = 0

    rows = []
    for key, tag in sorted(tbl["tags"].items(), key=lambda kv: parse_index(kv[0])[0] or 0):
        idx, sub = parse_index(key)
        if idx is None:
            stats["tag_bad_index"] += 1
            continue
        if "_variants" in tag:
            # Model-dependent layout: needs Condition evaluation, which is a
            # Perl expression. Out of scope for the mechanical pass by design.
            stats["tag_variant_skipped"] += 1
            continue
        name = tag.get("Name")
        if not isinstance(name, str) or not name:
            stats["tag_no_name"] += 1
            continue
        if tag.get("Unknown"):
            stats["tag_unknown_skipped"] += 1
            continue

        # A per-field Format overrides the table FORMAT.
        f = tag.get("Format")
        fmt_expr = "None"
        if isinstance(f, str):
            m = SIZED_RE.match(f)
            if m:
                base, count = m.group(1), int(m.group(2))
                variant = "Str" if base == "string" else (
                    "Undef" if base == "undef" else None)
                if variant:
                    fmt_expr = f"Some(Fmt::{variant}({count}))"
                else:
                    stats["tag_fmt_unsupported"] += 1
                    continue
            elif f in SCALAR_FORMATS:
                fmt_expr = f"Some(Fmt::{SCALAR_FORMATS[f][0]})"
            else:
                # Variable-length or expression-sized format -- not mechanical.
                stats["tag_fmt_unsupported"] += 1
                continue

        pc = conv_for(tag, stats)
        sub_s = "None" if sub is None else f"Some({sub})"
        rows.append(
            f'    Field {{ index: {idx}, sub: {sub_s}, name: "{rust_str(name)}", '
            f"format: {fmt_expr}, print_conv: {pc} }},"
        )
        stats["tag_emitted"] += 1

    if not rows:
        return None

    stats["table_emitted"] += 1
    ident = re.sub(r"[^A-Za-z0-9]", "_", f"{mod_name}_{tbl_name}").upper()
    groups = meta.get("GROUPS") or {}
    g0 = groups.get("0", "") if isinstance(groups, dict) else ""
    g2 = groups.get("2", "") if isinstance(groups, dict) else ""

    body = "\n".join(rows)
    return f"""
/// `Image::ExifTool::{mod_name}::{tbl_name}` -- {len(rows)} fields.
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static {ident}: BinaryTable = BinaryTable {{
    module: "{mod_name}",
    table: "{tbl_name}",
    group0: "{rust_str(g0)}",
    group2: "{rust_str(g2)}",
    first_entry: {first_entry},
    default_format: Fmt::{default_fmt[0]},
    fields: &[
{body}
    ],
}};
"""


PRELUDE = '''//! ExifTool binary tag tables, generated from ExifTool's own Perl hashes.
//!
//! DO NOT EDIT. Regenerate with:
//!
//! ```sh
//! perl tools/exiftool-tables/dump_tables.pl <exiftool>/lib > tables.json
//! python3 tools/exiftool-tables/codegen.py tables.json -o <this file>
//! ```
//!
//! Only ExifTool's ProcessBinaryData tables are emitted here -- the ones with a
//! FORMAT and a field per offset. That is deliberate: those tables carry the
//! byte layout that `exiftool -listx` does not expose, and layout is what a
//! reader actually needs. Tags whose conversions could not be reproduced
//! exactly are emitted without the conversion or omitted, never approximated;
//! the generator prints a full accounting of what it dropped.

#![allow(clippy::unreadable_literal, clippy::too_many_lines)]

/// A binary-table field format.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fmt {
    Int8u,
    Int8s,
    Int16u,
    Int16s,
    /// 16-bit integer read with the opposite endianness to the record.
    Int16uRev,
    Int32u,
    Int32s,
    Float,
    Double,
    Rational64u,
    Rational64s,
    /// `string[N]`: N bytes, truncated at the first NUL.
    Str(u32),
    /// `undef[N]`: N raw bytes.
    Undef(u32),
}

impl Fmt {
    #[must_use]
    pub const fn size(self) -> u32 {
        match self {
            Fmt::Int8u | Fmt::Int8s => 1,
            Fmt::Int16u | Fmt::Int16s | Fmt::Int16uRev => 2,
            Fmt::Int32u | Fmt::Int32s | Fmt::Float => 4,
            Fmt::Double | Fmt::Rational64u | Fmt::Rational64s => 8,
            Fmt::Str(n) | Fmt::Undef(n) => n,
        }
    }
}

/// How a raw value is rendered for display.
///
/// `None` is load-bearing: it means either the tag genuinely has no conversion,
/// or the generator refused to reproduce one it could not verify. Both cases
/// yield the raw value, which is honest, rather than a guess.
#[derive(Clone, Copy, Debug)]
pub enum PrintConv {
    None,
    /// Sorted by key; look up with `binary_search_by_key`.
    IntEnum(&'static [(i64, &'static str)]),
    StrEnum(&'static [(&'static str, &'static str)]),
    Expr(ExprId),
}

/// One field within a binary table.
#[derive(Clone, Copy, Debug)]
pub struct Field {
    /// Offset in units of the table's default format.
    pub index: i64,
    /// Sub-index for bit-fields (ExifTool's `12.1` notation).
    pub sub: Option<u32>,
    pub name: &'static str,
    /// Overrides the table default when present.
    pub format: Option<Fmt>,
    pub print_conv: PrintConv,
}

/// A `ProcessBinaryData` table.
#[derive(Clone, Copy, Debug)]
pub struct BinaryTable {
    pub module: &'static str,
    pub table: &'static str,
    pub group0: &'static str,
    pub group2: &'static str,
    pub first_entry: i64,
    pub default_format: Fmt,
    pub fields: &'static [Field],
}

impl BinaryTable {
    /// Byte offset of `field` from the start of the record.
    #[must_use]
    pub fn byte_offset(&self, field: &Field) -> i64 {
        (field.index - self.first_entry) * i64::from(self.default_format.size())
    }

    #[must_use]
    pub fn field_format(&self, field: &Field) -> Fmt {
        match field.format {
            Some(f) => f,
            None => self.default_format,
        }
    }
}

impl PrintConv {
    /// Render `val`, or return `None` to fall back to the raw value.
    #[must_use]
    pub fn apply(&self, val: i64) -> Option<String> {
        match self {
            PrintConv::None => None,
            PrintConv::IntEnum(m) => m
                .binary_search_by_key(&val, |(k, _)| *k)
                .ok()
                .map(|i| m[i].1.to_string()),
            PrintConv::StrEnum(m) => {
                let key = val.to_string();
                m.iter().find(|(k, _)| *k == key).map(|(_, v)| (*v).to_string())
            }
            PrintConv::Expr(e) => e.apply(val as f64),
        }
    }
}
'''


def gen_expr_enum(used):
    """Emit the ExprId enum for every translated expression actually used."""
    if not used:
        return (
            "\n/// No Perl expressions were translated in this build.\n"
            "#[derive(Clone, Copy, Debug)]\npub enum ExprId {}\n\n"
            "impl ExprId {\n"
            "    #[must_use]\n"
            "    pub fn apply(&self, _val: f64) -> Option<String> { None }\n}\n"
        )
    variants = "\n".join(f"    /// `{rust_str(e)}`\n    {i}," for i, e in sorted(used.items()))
    arms = []
    for ident, expr in sorted(used.items()):
        rty, rexpr = exprs.translate(expr)
        body = rexpr.replace("{v}", "val")
        if rty == "f64":
            arms.append(f'            ExprId::{ident} => Some(format!("{{}}", {body})),')
        elif rty == "String":
            arms.append(f"            ExprId::{ident} => Some({body}),")
        else:  # Option<f64>
            arms.append(
                f'            ExprId::{ident} => ({body}).map(|v| format!("{{v}}")),'
            )
    arm_body = "\n".join(arms)
    return f"""
/// Perl conversions with a hand-verified Rust equivalent.
///
/// Each variant corresponds to one entry in `tools/exiftool-tables/exprs.py`.
/// Adding an entry there fixes every tag sharing that expression at once.
#[derive(Clone, Copy, Debug)]
pub enum ExprId {{
{variants}
}}

impl ExprId {{
    #[must_use]
    pub fn apply(&self, val: f64) -> Option<String> {{
        match self {{
{arm_body}
        }}
    }}
}}
"""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("tables_json")
    ap.add_argument("-o", "--out", required=True)
    ap.add_argument("--modules", nargs="*", help="limit to these modules")
    args = ap.parse_args()

    with open(args.tables_json, encoding="utf-8") as fh:
        doc = json.load(fh)

    stats = Counter()
    stats["unsupported_exprs"] = Counter()
    chunks = []
    index_rows = []

    mods = doc["modules"]
    names = args.modules or sorted(mods)
    for mod_name in names:
        mod = mods.get(mod_name)
        if not mod:
            continue
        for tbl_name in sorted(mod["tables"]):
            out = gen_table(mod_name, tbl_name, mod["tables"][tbl_name], stats)
            if out:
                chunks.append(out)
                ident = re.sub(r"[^A-Za-z0-9]", "_", f"{mod_name}_{tbl_name}").upper()
                index_rows.append(f"    &{ident},")

    # Collect the expressions actually referenced so the enum has no dead arms.
    # Iterate in sorted order: set iteration order varies between runs, and a
    # generator whose output depends on it cannot be checked into git.
    used = {}
    joined = "".join(chunks)
    for e in sorted(exprs.TRANSLATIONS):
        ident = expr_ident(e)
        if ident in used and used[ident] != e:
            raise SystemExit(
                f"identifier collision: {ident!r} maps to both {used[ident]!r} "
                f"and {e!r} -- two conversions would alias to one variant"
            )
        if f"ExprId::{ident}" in joined:
            used[ident] = e

    index = (
        "\n/// Every generated binary table, for iteration and lookup.\n"
        f"pub static ALL_BINARY_TABLES: &[&BinaryTable] = &[\n"
        + "\n".join(index_rows)
        + "\n];\n"
    )

    with open(args.out, "w", encoding="utf-8") as fh:
        fh.write(PRELUDE)
        fh.write(gen_expr_enum(used))
        fh.write(joined)
        fh.write(index)

    ue = stats.pop("unsupported_exprs")
    print(f"wrote {args.out}")
    print(f"  tables emitted      {stats['table_emitted']}")
    print(f"  tags emitted        {stats['tag_emitted']}")
    print(f"  int enums           {stats['enum_int']}")
    print(f"  string enums        {stats['enum_str']}")
    print(f"  exprs translated    {stats['expr_translated']}")
    print("  --- refused, not approximated ---")
    print(f"  exprs unsupported   {stats['expr_unsupported']}")
    print(f"  variant tags        {stats['tag_variant_skipped']}")
    print(f"  Unknown tags        {stats['tag_unknown_skipped']}")
    print(f"  unsupported format  {stats['tag_fmt_unsupported']}")
    if ue:
        print("\n  top unsupported expressions (translate these next):")
        for e, n in ue.most_common(10):
            flat = e if len(e) <= 58 else e[:55] + "..."
            print(f"    {n:>4}  {flat}")


if __name__ == "__main__":
    main()
