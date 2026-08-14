#!/usr/bin/env python3
"""Transcribe the Sony DSLR-A100's four Minolta `ProcessBinaryData` tables
(`CameraInfoA100`, `CameraSettingsA100`, `ISInfoA100`, `CameraSettingsA100`,
`WBInfoA100`) into `minolta_a100_tables.rs`.

Input is the JSON `dump_tables.pl` produces by loading ExifTool's `Minolta`
module and reading these tables out of memory, for the same reason
`codegen_subdirs.py`'s header gives (the tables are assembled at require-time,
so the source text is not guaranteed to be the hash ExifTool dispatches on).

Like `gen_sony_main_extra_tables.py` and `scripts/gen_canon_custom_functions2.pl`,
translation is a HARD-CODED DICTIONARY keyed on the literal (whitespace-
normalized) Perl `ValueConv`/`PrintConv` text -- plus a handful of single-shape
*parameterized* regexes for idioms that repeat with different constants
(`$val / N`, `2 ** (($val-a)/b)`, ...) -- never a general expression parser.
Anything not recognized is a hard error naming the table, the offset and the
offending text, UNLESS it is in ALLOWED_SKIPS: a short, by-hand list of
offsets whose conversion is a genuine bespoke Perl subroutine with no DSL
equivalent (e.g. `WBInfoA100` 4172 `TiffMeteringImage`, which reassembles a
40x30 pixel array into a synthetic 16-bit TIFF). Skipping those is not a
silent guess -- the row is omitted from the output entirely, matching
ExifTool's Sony/Minolta convention that an unsupported field is missing
data, never wrong data.

usage:
  dump_tables.pl <exiftool-lib> Minolta > tables.json
  gen_minolta_a100_tables.py tables.json -o minolta_a100_tables.rs
"""
from __future__ import annotations

import argparse
import json
import re
import sys

# Table name (ExifTool) -> (Rust static name, BinTable Rust name, output idx const)
TABLE_MANIFEST = [
    ("CameraInfoA100", "T0", "CameraInfoA100", "CAMERAINFOA100"),
    ("CameraSettingsA100", "T1", "CameraSettingsA100", "CAMERASETTINGSA100"),
    ("ISInfoA100", "T2", "ISInfoA100", "ISINFOA100"),
    ("WBInfoA100", "T3", "WBInfoA100", "WBINFOA100"),
]

# (table, offset) whose ValueConv/PrintConv is a genuine bespoke Perl
# subroutine with no equivalent in the `sony::binary_data` DSL. Recovered by
# diffing this generator's Unsupported errors against the committed file: a
# hard-erroring construct that also does not appear as a row in
# `minolta_a100_tables.rs` was, by definition, already omitted rather than
# guessed at.
ALLOWED_SKIPS = {
    ("WBInfoA100", "4172"),  # TiffMeteringImage: rebuilds a synthetic TIFF image
}


class Unsupported(Exception):
    def __init__(self, table, offset, name, reason):
        super().__init__(f"{table}[{offset}] {name}: {reason}")


def norm(s: str) -> str:
    return re.sub(r"\s+", " ", s).strip()


def rust_str(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def fnum(x) -> str:
    """A float constant, formatted the way the committed file writes them
    (`N.0_f64` or `N.M_f64`, never bare integers)."""
    f = float(x)
    if f == int(f):
        return f"{int(f)}.0_f64"
    return f"{f}_f64"


# ---------------------------------------------------------------------------
# `Format` -- scalar formats, plus the `fmt[N]` array shorthand.
# ---------------------------------------------------------------------------
SCALAR_FORMAT_DICT = {
    "int8u": "Fmt::U8",
    "int8s": "Fmt::I8",
    "int16u": "Fmt::U16",
    "int16s": "Fmt::I16",
    "int16uRev": "Fmt::U16Rev",
    "string": "Fmt::Str",
}
ARRAY_FORMAT_RE = re.compile(r"^(\w+)\[(\d+)\]$")


def translate_format(table, offset, name, fmt_text):
    """Returns (fmt_rust_or_None, count)."""
    if fmt_text is None:
        return None, 1
    m = ARRAY_FORMAT_RE.match(fmt_text)
    if m:
        base, count = m.group(1), int(m.group(2))
    else:
        base, count = fmt_text, 1
    if base not in SCALAR_FORMAT_DICT:
        raise Unsupported(table, offset, name, f"unregistered Format base: {base!r}")
    return SCALAR_FORMAT_DICT[base], count


# ---------------------------------------------------------------------------
# `ValueConv` -- hard-coded dictionary plus parameterized regexes, all
# doc-commented in `sony/binary_data.rs`'s `enum Vc`.
# ---------------------------------------------------------------------------
VC_EXPR_RULES = [
    (re.compile(r"^\$val \* ([\d.]+)$"), lambda m: f"Vc::Mul({fnum(m.group(1))})"),
    (re.compile(r"^\$val - ([\d.]+)$"), lambda m: f"Vc::Add({fnum(-float(m.group(1)))})"),
    (re.compile(r"^\$val \+ ([\d.]+)$"), lambda m: f"Vc::Add({fnum(m.group(1))})"),
    (re.compile(r"^\$val / ([\d.]+) - ([\d.]+)$"),
     lambda m: f"Vc::DivSub({fnum(m.group(1))}, {fnum(m.group(2))})"),
    (re.compile(r"^\$val / ([\d.]+)$"), lambda m: f"Vc::Div({fnum(m.group(1))})"),
    (re.compile(r"^\$val \? 2 \*\* \(([\d.]+) - \$val/([\d.]+)\) : 0$"),
     lambda m: f"Vc::ExpTime({fnum(m.group(1))}, {fnum(m.group(2))})"),
    (re.compile(r"^\(\$val-([\d.]+)\)/([\d.]+)$"),
     lambda m: f"Vc::SubDiv({fnum(m.group(1))}, {fnum(m.group(2))})"),
    (re.compile(r"^2\s*\*\*\s*\(\(\$val-([\d.]+)\)/([\d.]+)\) \* ([\d.]+)$"),
     lambda m: f"Vc::Pow2SubDivMul({fnum(m.group(1))}, {fnum(m.group(2))}, {fnum(m.group(3))})"),
    (re.compile(r"^2\s*\*\*\s*\(\(\$val-([\d.]+)\)/([\d.]+)\)$"),
     lambda m: f"Vc::Pow2SubDiv({fnum(m.group(1))}, {fnum(m.group(2))})"),
    (re.compile(r"^2\s*\*\*\s*\(\(\$val/([\d.]+) - ([\d.]+)\) / 2\)$"),
     lambda m: f"Vc::Pow2DivSubHalf({fnum(m.group(1))}, {fnum(m.group(2))})"),
]

# ValueConv written as a Perl CODE ref (a `map {...}` over a split list) --
# hard-coded dictionary keyed on the exact deparsed body, same discipline as
# the plain-expr dictionary above.
VC_CODE_DICT = {
    "{ package Image::ExifTool::Minolta; use strict; "
    "join(' ', map({(($_ - 106) / 8);} split(' ', $_[0], 0))); }":
        "Vc::EachSubDiv(106.0_f64, 8.0_f64)",
}


def translate_vc(table, offset, name, vc):
    if vc is None:
        return "Vc::None"
    kind = vc.get("kind")
    if kind == "expr":
        key = norm(vc["expr"])
        for rx, fn in VC_EXPR_RULES:
            m = rx.match(key)
            if m:
                return fn(m)
        raise Unsupported(table, offset, name, f"unregistered ValueConv expr: {key!r}")
    if kind == "code":
        key = norm(vc.get("deparse") or "")
        if key in VC_CODE_DICT:
            return VC_CODE_DICT[key]
        raise Unsupported(table, offset, name, f"unregistered ValueConv code: {key!r}")
    raise Unsupported(table, offset, name, f"unsupported ValueConv kind: {kind!r}")


# ---------------------------------------------------------------------------
# `PrintConv` -- enum maps (optionally with an `OTHER` fallback sub, hard-
# dictionary keyed on its deparsed body), plus expression forms.
# ---------------------------------------------------------------------------
OTHER_CODE_DICT = {
    '{ package Image::ExifTool::Minolta; use strict; (my($val, $inv) = @_); '
    '($inv and (($val =~ /([-+]?\\d+)/), (return $1))); (return (($val < 0) ? '
    '("Front Focus ($val)") : ("Back Focus (+$val)"))); }': "Other::MinoltaFocus",

    '($$$) { package Image::ExifTool::Exif; use strict; (my($val, $inv, $conv) = @_); '
    '($inv and (return $val)); if (($val > 0)) { if (($val > 65520)) { '
    '($val = ($val - 65536)); } else { ($val = "+$val"); } } (return $val); }':
        "Other::ExifParameter",

    '{ package Image::ExifTool::Minolta; use strict; (my($val, $inv) = @_); '
    '($inv and (return (undef))); (my $id = ($val & 65280)); '
    '(my $mb = $Image::ExifTool::Minolta::metabonesID{$id}); if ($mb) { '
    '(ref($mb) or (($id = $mb), ($mb = $Image::ExifTool::Minolta::metabonesID{$id}))); '
    '(require Image::ExifTool::Canon); '
    '(my $lens = $Image::ExifTool::Canon::canonLensTypes{$val - $id}); '
    '($lens and (return ("$lens + $$mb"))); } elsif (($val >= 18688)) { '
    '(require Image::ExifTool::Sigma); '
    '(my $lens = $Image::ExifTool::Sigma::sigmaLensTypes{$val - 18688}); '
    '($lens and (return ("$lens + MC-11 SA-E"))); } (return (undef)); }':
        "Other::MinoltaLens",
}

PC_EXPR_DICT = {
    '"$val s"': 'Pc::Suffix(" s")',
    '$val ? Image::ExifTool::Exif::PrintExposureTime($val) : "Bulb"': "Pc::ExposureTimeOrBulb",
    '$val ? sprintf("%+.1f",$val) : 0': "Pc::Signed1OrZero",
    "Image::ExifTool::Exif::PrintFNumber($val)": "Pc::FNumber",
    "int($val + 0.5)": "Pc::RoundHalfUp",
}
PC_EXPR_RULES = [
    (re.compile(r'^sprintf\("%\.(\d+)f",\$val\)$'), lambda m: f"Pc::Fixed({int(m.group(1))})"),
    # `InfAboveOrMeters(f64)` is the one Pc field the committed file writes
    # without an `_f64` suffix -- matched literally, not via `fnum()`.
    (re.compile(r'^\$val > ([\d.]+) \? "inf" : sprintf\("%\.2f m", \$val\)$'),
     lambda m: f"Pc::InfAboveOrMeters({float(m.group(1))})"),
]


class Pools:
    def __init__(self):
        self.maps: list[tuple] = []

    def intern_map(self, items):
        key = tuple(items)
        for i, existing in enumerate(self.maps):
            if existing == key:
                return f"M{i}"
        self.maps.append(key)
        return f"M{len(self.maps) - 1}"


def translate_pc(table, offset, name, tag, pools):
    pc = tag.get("PrintConv")
    if pc is None:
        return "Pc::None"
    kind = pc["kind"]
    if kind == "enum":
        map_name = pools.intern_map(list(pc["map"].items()))
        return f"Pc::Map({map_name}, Other::None)"
    if kind == "enum_partial":
        directives = pc.get("directives") or {}
        unknown = set(directives.keys()) - {"OTHER", "Notes", "PrintHex", "SeparateTable"}
        if unknown:
            raise Unsupported(table, offset, name, f"enum_partial directives: {unknown!r}")
        if "OTHER" not in directives:
            raise Unsupported(table, offset, name, f"enum_partial with no OTHER: {directives!r}")
        other = directives["OTHER"]
        if other.get("__perl") != "CODE":
            raise Unsupported(table, offset, name, f"OTHER is not CODE: {other!r}")
        key = norm(other.get("__deparse") or "")
        if key not in OTHER_CODE_DICT:
            raise Unsupported(table, offset, name, f"unregistered OTHER code: {key!r}")
        map_name = pools.intern_map(list(pc["map"].items()))
        return f"Pc::Map({map_name}, {OTHER_CODE_DICT[key]})"
    if kind == "expr":
        key = norm(pc["expr"])
        if key in PC_EXPR_DICT:
            return PC_EXPR_DICT[key]
        for rx, fn in PC_EXPR_RULES:
            m = rx.match(key)
            if m:
                return fn(m)
        raise Unsupported(table, offset, name, f"unregistered PrintConv expr: {key!r}")
    raise Unsupported(table, offset, name, f"unsupported PrintConv kind: {kind!r}")


def translate_print_hex(table, offset, name, tag):
    if "PrintHex" not in tag:
        return "false"
    if tag["PrintHex"] == "1":
        return "true"
    raise Unsupported(table, offset, name, f"unregistered PrintHex value: {tag['PrintHex']!r}")


def render_map_decl(name, items):
    body = ", ".join(f"({rust_str(k)}, {rust_str(v)})" for k, v in items)
    return f"#[rustfmt::skip]\nstatic {name}: &[(&str, &str)] = &[{body}];"


def render_row(offset, name, fmt, count, vc, pc, print_hex):
    fmt_text = "Fmt::Default" if fmt is None else fmt
    return (
        f"    BinTag {{ index: {offset}, name: {rust_str(name)}, cond: Cond::Always, "
        f"fmt: {fmt_text}, count: {count}, mask: 0, raw: Raw::None, vc: {vc}, pc: {pc}, "
        f"hook: Hook::None, print_hex: {print_hex}, low_priority: true, subdir: None }},"
    )


HEADER = '''//! Sony DSLR-A100 binary-data tables -- generated, do not hand-edit.
//!
//! The A100 writes a Minolta MakerNote, and ExifTool decodes four of its blocks
//! with tables that exist only for this body: `CameraInfoA100` (0x0010),
//! `ISInfoA100` (0x0018), `WBInfoA100` (0x0020) and `CameraSettingsA100`
//! (0x0114). None of them is enciphered. Every row below was read out of
//! ExifTool's own `%Image::ExifTool::Minolta::*` hashes in-process (13.59)
//! rather than retyped, and is interpreted by
//! [`super::sony::binary_data`](crate::parsers::tiff::makernotes::sony::binary_data).

use crate::parsers::tiff::makernotes::sony::binary_data::{
    BinTable, BinTag, Cond, Fmt, Hook, Other, Pc, Raw, Vc,
};
'''


def generate(data):
    minolta = data["modules"]["Minolta"]["tables"]
    pools = Pools()
    table_blocks = []
    bintable_entries = []
    idx_lines = []
    skipped = []

    for et_name, rust_static, bin_name, idx_const in TABLE_MANIFEST:
        table = minolta[et_name]
        table_fmt_text = table["meta"].get("FORMAT")
        if table_fmt_text is None:
            table_fmt = "Fmt::Default"
        elif table_fmt_text in SCALAR_FORMAT_DICT:
            table_fmt = SCALAR_FORMAT_DICT[table_fmt_text]
        else:
            raise Unsupported(et_name, "<table>", "<table FORMAT>",
                               f"unregistered table FORMAT: {table_fmt_text!r}")

        tags = table["tags"]
        rows = []
        for offset in sorted(tags.keys(), key=lambda k: float(k)):
            tag = tags[offset]
            name = tag["Name"]
            try:
                fmt, count = translate_format(et_name, offset, name, tag.get("Format"))
                vc = translate_vc(et_name, offset, name, tag.get("ValueConv"))
                pc = translate_pc(et_name, offset, name, tag, pools)
                print_hex = translate_print_hex(et_name, offset, name, tag)
            except Unsupported as e:
                if (et_name, offset) in ALLOWED_SKIPS:
                    skipped.append(str(e))
                    continue
                raise
            rows.append(render_row(offset, name, fmt, count, vc, pc, print_hex))

        table_blocks.append(
            f"#[rustfmt::skip]\nstatic {rust_static}: &[BinTag] = &[\n" + "\n".join(rows) + "\n];"
        )
        bintable_entries.append(
            f'    BinTable {{\n        name: {rust_str(bin_name)},\n'
            f"        fmt: {table_fmt},\n        tags: {rust_static},\n    }},"
        )
        idx_lines.append(f"    pub const {idx_const}: usize = {len(idx_lines)};")

    for line in skipped:
        print(f"gen_minolta_a100_tables.py: SKIP (registered) {line}", file=sys.stderr)

    out = [HEADER]
    for i, items in enumerate(pools.maps):
        out.append(render_map_decl(f"M{i}", items))
    out.append("")
    out.extend(table_blocks)
    out.append(
        "\n/// Every table, indexed by the `SubDir`/`Root` table numbers above.\n"
        "pub static TABLES: &[BinTable] = &[\n" + "\n".join(bintable_entries) + "\n];"
    )
    out.append(
        "\n/// Table numbers, by ExifTool table name.\n#[allow(dead_code)]\npub mod idx {\n"
        + "\n".join(idx_lines) + "\n}\n"
    )
    return "\n".join(out)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("json_path")
    ap.add_argument("-o", "--output", required=True)
    args = ap.parse_args()

    with open(args.json_path, encoding="utf-8") as f:
        data = json.load(f)

    try:
        text = generate(data)
    except Unsupported as e:
        print(f"gen_minolta_a100_tables.py: {e}", file=sys.stderr)
        sys.exit(1)

    with open(args.output, "w", encoding="utf-8") as f:
        f.write(text)


if __name__ == "__main__":
    main()
