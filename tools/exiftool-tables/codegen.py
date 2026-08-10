#!/usr/bin/env python3
"""Generate Rust binary-table definitions from extracted ExifTool tables.

Scope is deliberately narrow: ExifTool's ProcessBinaryData tables -- the ones
carrying FORMAT/FIRST_ENTRY and a field per byte offset.  They are where the
maker-note coverage gap lives, because they are the part `exiftool -listx`
cannot express.  `-listx` gives names; these give layout, and layout is what you
need to actually read bytes out of a MakerNote.  Membership is decided by
PROCESS_PROC alone: a table ExifTool reads some other way (deciphered first,
or keyed by sequence rather than offset) is not a flat byte record, and
emitting it as one yields plausible integers at meaningless offsets.

Every skipped tag is counted and reported, and `REPORT` is asserted to cover
every counter the code keeps.  A generator that silently drops what it cannot
handle produces a coverage number that is a lie, and the whole argument for
doing this mechanically rests on the numbers being trustworthy.  The same rule
governs the semantics it cannot reproduce: `Mask` is transcribed, and an
omitted `ValueConv`/`RawConv`/`Condition` is recorded in the emitted schema so
that a caller can tell the raw value from the reported one.
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

SIZED_RE = re.compile(r"^(\w+)\[(\d+)\]$")


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


# PrintConv directives that change only how an unmatched value is displayed,
# never which string a matched key maps to. Dropping these leaves every emitted
# entry exactly right, so they cost nothing but are still counted.
#
# BITMASK and OTHER are different in kind: both add a fallback that produces a
# rendering for values the plain map does not contain (DecodeBits for the
# former, a Perl sub for the latter), so an enum carrying one is genuinely
# partial. The exact-match entries stay -- they are verified against ExifTool
# and are what it would print -- but they are counted apart from complete
# enums, because reporting a partial transcription as a whole one is how the
# coverage number stops meaning anything.
BENIGN_PC_DIRECTIVES = {"Notes", "PrintHex", "PrintSort", "SeparateTable"}


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
        # `enum_partial` means dump_tables.pl saw directive keys alongside the
        # map; `directives` is a dict of them (null when there are none).
        # Record which, so the accounting says what was lost rather than
        # scoring the result as a complete enum.
        directives = sorted(
            d for d in (pc.get("directives") or {})
            if d not in BENIGN_PC_DIRECTIVES
        )
        partial = bool(directives)
        for d in directives:
            stats["pc_directives_dropped"][d] += 1
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
            stats["enum_int_partial" if partial else "enum_int"] += 1
            pairs.sort()
            body = ", ".join(f'({k}, "{rust_str(v)}")' for k, v in pairs)
            return f"PrintConv::IntEnum(&[{body}])"
        stats["enum_str_partial" if partial else "enum_str"] += 1
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


def mask_for(tag, stats):
    """Return the Rust `mask:` member for a tag, honouring ExifTool's BitShift.

    ExifTool reduces a masked field to `($val & Mask) >> BitShift` before any
    conversion runs, so a table that declares a Mask is describing a slice of
    the word, not the word. Ignoring the key -- which this generator used to do,
    with no counter and no slot in the emitted schema -- reported the whole word
    under the real tag name and then looked the enum up with it, so a field like
    `DjVu::Info` Orientation (Mask 0x7, enum keyed 1/2/5/6) usually matched
    nothing and printed a raw number instead.

    BitShift is taken from the table when stated and derived from the lowest set
    bit otherwise, exactly as ExifTool does; see the note in dump_tables.pl for
    why deriving it unconditionally is wrong.
    """
    raw = tag.get("Mask")
    if raw is None:
        return "None"
    try:
        mask = int(str(raw), 0)
    except (TypeError, ValueError):
        stats["tag_mask_unreadable"] += 1
        return None
    if mask == 0:
        # ExifTool guards both the BitShift derivation and the application with
        # `if $mask`, so a zero mask is no mask -- not a dropped field.
        return "None"
    if mask < 0 or mask > 0xFFFF_FFFF_FFFF_FFFF:
        stats["tag_mask_unreadable"] += 1
        return None

    declared = tag.get("BitShift")
    if declared is None:
        shift = (mask & -mask).bit_length() - 1
    else:
        try:
            shift = int(str(declared), 0)
        except (TypeError, ValueError):
            stats["tag_mask_unreadable"] += 1
            return None
        if shift < 0 or shift > 63:
            stats["tag_mask_unreadable"] += 1
            return None
    stats["tag_masked"] += 1
    return f"Some(Mask {{ bits: {mask:#x}, shift: {shift} }})"


def omitted_for(tag, stats):
    """Flag the semantics ExifTool applies that this schema does not reproduce.

    `ValueConv`, `RawConv` and `Condition` are Perl, so the mechanical pass
    cannot run them. It used to drop them without a counter and without a slot
    in the emitted schema, which made the omission unknowable downstream:
    `DecodedField::apply_print_conv_to_raw` documented a precondition ("only
    after verifying that the field has no intervening ValueConv") that no caller
    could actually check, and a field whose PrintConv is keyed on post-ValueConv
    values then rendered a confident wrong string from the raw one.

    Refusing these fields outright is not an option -- around a third of the
    emitted set carries one, including tables parsers already read for layout --
    so the omission is recorded instead. A caller that sees `value_conv` set
    knows the raw value is not the reported value, which is the whole point.
    """
    flags = []
    for key, member in (
        ("ValueConv", "value_conv"),
        ("RawConv", "raw_conv"),
        ("Condition", "condition"),
    ):
        if tag.get(key) is not None:
            flags.append(member)
            stats[f"omitted_{member}"] += 1
    if not flags:
        return "Omitted::NONE"
    return "Omitted { " + ", ".join(
        f"{m}: {'true' if m in flags else 'false'}"
        for m in ("value_conv", "raw_conv", "condition")
    ) + " }"


def gen_table(mod_name, tbl_name, tbl, stats):
    meta = tbl.get("meta") or {}
    # PROCESS_PROC is the only honest signal for "this is a flat byte record".
    # Checking it only when FORMAT is absent -- which this generator used to do
    # -- let 46 tables through that ExifTool does not read with
    # ProcessBinaryData: 31 Sony tables deciphered before parsing, 7 read with
    # ProcessSerialData (Canon::AFInfo's keys are sequence numbers, not
    # offsets), and a handful of bespoke parsers. Emitted as flat records they
    # yield plausible integers at offsets that mean nothing, under real ExifTool
    # tag names. codegen_subdirs.py hard-errors on this same construct.
    if not is_binary_table(meta):
        stats["table_not_binary"] += 1
        return None
    fmt_name = meta.get("FORMAT")
    if not isinstance(fmt_name, str):
        # ExifTool's ProcessBinaryData does `$$tagTablePtr{FORMAT} || 'int8u'`.
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

        # A per-field Format overrides the table FORMAT. `count` is the number
        # of repetitions of that format (ExifTool's `format[N]` array syntax);
        # 1 for every scalar field.
        f = tag.get("Format")
        fmt_expr = "None"
        count = 1
        if isinstance(f, str):
            m = SIZED_RE.match(f)
            if m:
                base, n = m.group(1), int(m.group(2))
                if base == "string":
                    fmt_expr = f"Some(Fmt::Str({n}))"
                elif base == "undef":
                    fmt_expr = f"Some(Fmt::Undef({n}))"
                elif base in SCALAR_FORMATS:
                    # An array of a scalar format, e.g. `int16u[4]`: n repeats
                    # of the element format, not n bytes.
                    fmt_expr = f"Some(Fmt::{SCALAR_FORMATS[base][0]})"
                    count = n
                else:
                    stats["tag_fmt_unsupported"] += 1
                    continue
            elif f in SCALAR_FORMATS:
                fmt_expr = f"Some(Fmt::{SCALAR_FORMATS[f][0]})"
            else:
                # Variable-length or expression-sized format -- not mechanical.
                stats["tag_fmt_unsupported"] += 1
                continue

        mask = mask_for(tag, stats)
        if mask is None:
            # A Mask this schema cannot express means the field's value is not
            # the word at that offset. Omit it rather than report the word.
            continue

        pc = conv_for(tag, stats)
        if sub is not None:
            # A fractional key names a slice of the word at int(key); `Mask` is
            # what says which bits. The runtime decodes the ones that declare
            # one and refuses the rest, so the split is the honest measure of
            # how much of ExifTool's bit-field notation this schema reaches.
            # Both halves are emitted either way -- this counts, it does not
            # gate.
            stats["tag_fractional_masked" if mask != "None" else "tag_fractional_bare"] += 1
        sub_s = "None" if sub is None else f"Some({sub})"
        rows.append(
            f'    Field {{ index: {idx}, sub: {sub_s}, name: "{rust_str(name)}", '
            f"format: {fmt_expr}, count: {count}, mask: {mask}, "
            f"omitted: {omitted_for(tag, stats)}, print_conv: {pc} }},"
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

__VERSION_BLOCK__

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

/// ExifTool's `Mask`/`BitShift` pair: the field is a slice of the word.
///
/// `ProcessBinaryData` reduces the value to `(val & bits) >> shift` before any
/// conversion runs, so a `PrintConv` on a masked field is keyed on the reduced
/// value, never on the whole word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mask {
    pub bits: u64,
    /// Taken from the table's `BitShift` when it states one, otherwise the
    /// lowest set bit of `bits` -- the same rule ExifTool applies.
    pub shift: u32,
}

impl Mask {
    /// Reduce a raw word to the field's value.
    #[must_use]
    pub const fn apply(self, val: i64) -> i64 {
        ((val as u64 & self.bits) >> self.shift) as i64
    }
}

/// Semantics ExifTool applies to a field that this schema does not reproduce.
///
/// These are Perl, so the mechanical transcription cannot run them. Recording
/// which were dropped is what lets a caller tell "the raw value is the value"
/// from "the raw value is an input to something you still have to do". Any flag
/// set means the decoded value is NOT what ExifTool would report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Omitted {
    /// A `ValueConv` stood between the raw bytes and the value. A `PrintConv`
    /// on such a field is keyed on the converted value, so applying it to the
    /// raw one renders a confident wrong string.
    pub value_conv: bool,
    /// A `RawConv` ran first. ExifTool's common idioms (`$val ? $val : undef`)
    /// suppress the tag entirely for some inputs.
    pub raw_conv: bool,
    /// The field is gated on a `Condition`; ExifTool may not report it at all.
    pub condition: bool,
}

impl Omitted {
    pub const NONE: Self = Self {
        value_conv: false,
        raw_conv: false,
        condition: false,
    };

    /// True when anything was dropped, i.e. the raw value stands alone.
    #[must_use]
    pub const fn any(self) -> bool {
        self.value_conv || self.raw_conv || self.condition
    }
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
    /// Number of repetitions of `format` (ExifTool's `format[N]` array
    /// syntax), e.g. 4 for `int16u[4]`. 1 for scalar fields.
    pub count: usize,
    /// Present when the field is a slice of the word at `index`.
    pub mask: Option<Mask>,
    /// What ExifTool does to this field that the transcription does not.
    pub omitted: Omitted,
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
    ///
    /// ExifTool's `ProcessBinaryData` computes `$entry = int($index) *
    /// $increment` (ExifTool.pm) -- the tag index scales from the start of
    /// the data block unconditionally. `FIRST_ENTRY` never enters that
    /// arithmetic; it only bounds the `Unknown > 1` auto-scan range, so
    /// subtracting it here shifted every field of a `FIRST_ENTRY 1` table
    /// one format-width early.
    #[must_use]
    pub fn byte_offset(&self, field: &Field) -> i64 {
        field.index * i64::from(self.default_format.size())
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


# Every counter the generator keeps, and the heading it prints under. The run
# summary is the only evidence anyone sees that the transcription under-claims
# rather than guesses, so the grouping matters as much as the numbers: an enum
# that lost its BITMASK fallback is not a failure, but it is not a complete
# enum either, and filing it under the same heading as one is what turned a
# partial transcription into a reported success.
REPORT = (
    ("transcribed", (
        ("tables emitted", "table_emitted"),
        ("tags emitted", "tag_emitted"),
        ("int enums", "enum_int"),
        ("string enums", "enum_str"),
        ("exprs translated", "expr_translated"),
        ("masked fields", "tag_masked"),
        ("bit fields (frac + Mask)", "tag_fractional_masked"),
    )),
    ("partial -- exact matches kept, fallback dropped", (
        ("int enums", "enum_int_partial"),
        ("string enums", "enum_str_partial"),
    )),
    ("emitted with semantics recorded but not applied", (
        ("ValueConv", "omitted_value_conv"),
        ("RawConv", "omitted_raw_conv"),
        ("Condition", "omitted_condition"),
        ("bit fields, no Mask", "tag_fractional_bare"),
    )),
    ("refused, not approximated", (
        ("exprs unsupported", "expr_unsupported"),
        ("other PrintConv", "conv_dropped"),
        ("variant tags", "tag_variant_skipped"),
        ("Unknown tags", "tag_unknown_skipped"),
        ("unsupported format", "tag_fmt_unsupported"),
        ("unreadable Mask", "tag_mask_unreadable"),
        ("unreadable index", "tag_bad_index"),
        ("unnamed tags", "tag_no_name"),
        ("tables not ProcessBinaryData", "table_not_binary"),
        ("tables bad FORMAT", "table_bad_format"),
    )),
)


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
    stats["pc_directives_dropped"] = Counter()
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

    # Stamp the release these tables came from. ExifTool renames fields and
    # inserts enum values between releases, so verifying against a different
    # one reports hundreds of differences that read as generator bugs rather
    # than as "wrong ExifTool". dump_tables.pl already recorded the version;
    # carrying it into the artifact is what lets verify.py say which it is.
    version = str(doc.get("exiftool_version") or "").strip()
    if not version:
        raise SystemExit(
            "tables JSON has no exiftool_version -- regenerate it with "
            "dump_tables.pl; an unstamped table set cannot be verified"
        )
    version_block = (
        "/// The ExifTool release these tables were transcribed from.\n"
        "///\n"
        "/// `tools/exiftool-tables/verify.py` refuses to compare against any\n"
        "/// other release. Field names and enum values move between versions,\n"
        "/// so a skewed check produces spurious mismatches that look like\n"
        "/// transcription errors. Regenerate with `just regen-tables <version>`.\n"
        f'pub const EXIFTOOL_VERSION: &str = "{version}";'
    )

    with open(args.out, "w", encoding="utf-8") as fh:
        fh.write(PRELUDE.replace("__VERSION_BLOCK__", version_block))
        fh.write(gen_expr_enum(used))
        fh.write(joined)
        fh.write(index)

    ue = stats.pop("unsupported_exprs")
    pcd = stats.pop("pc_directives_dropped")
    print(f"wrote {args.out}")
    printed = set()
    for heading, rows in REPORT:
        if heading:
            print(f"  --- {heading} ---")
        for label, key in rows:
            printed.add(key)
            print(f"  {label:<30}{stats[key]}")

    # The completeness argument for this whole pipeline rests on the refusal
    # count being trustworthy, and four counters were incremented but never
    # printed -- a run reported 2301 refusals against a true 2393 plus 11
    # dropped tables. Asserting that the report covers every key the code
    # touches is what makes the next added counter impossible to forget.
    missed = sorted(set(stats) - printed)
    if missed:
        raise SystemExit(
            "these counters were recorded but not reported: "
            + ", ".join(missed)
            + " -- add them to REPORT; an unreported refusal is a coverage lie"
        )

    if pcd:
        print("\n  PrintConv directives dropped from partial enums:")
        for d, n in pcd.most_common():
            print(f"    {n:>4}  {d}")
    if ue:
        print("\n  top unsupported expressions (translate these next):")
        for e, n in ue.most_common(10):
            flat = e if len(e) <= 58 else e[:55] + "..."
            print(f"    {n:>4}  {flat}")


if __name__ == "__main__":
    main()
