#!/usr/bin/env python3
"""Transcribe `Sony::Main` scalars that `sony/main_table.rs` does not
hand-implement into `sony/main_extra_tables.rs`.

Input is the JSON `dump_tables.pl` produces by loading ExifTool's `Sony`
module and reading `%Image::ExifTool::Sony::Main` out of memory -- not a
regex over the `.pm` text, for the same reason `codegen_subdirs.py`'s header
gives: the table is assembled at require-time and the source text is not
guaranteed to be the hash ExifTool dispatches on.

Unlike `codegen_subdirs.py`, this is NOT a general `ProcessBinaryData`
transcriber. `Sony::Main` entries are ordinary IFD scalars read through the
bespoke `sony::main_extra` interpreter (see that module's doc comment), whose
`MCond`/`Raw`/`Vc`/`Pc` vocabulary was hand-matched, tag by tag, against this
exact set of `Condition`/`RawConv`/`ValueConv`/`PrintConv` Perl strings. So,
like `scripts/gen_canon_custom_functions2.pl`, translation here is a
HARD-CODED DICTIONARY keyed on the literal (whitespace-normalized) Perl
expression text -- not a general expression parser -- and any expression not
in the dictionary is a hard error naming the tag and the offending text. A
generator that guessed at an unregistered expression could emit a
plausible-but-wrong value under a real ExifTool tag name, which is exactly
the failure this whole methodology exists to refuse.

MANIFEST is the list of `Sony::Main` tag ids this file covers. It is NOT
derived by diffing against `main_table.rs`'s own dispatch (that Rust file
predates any generator and is not itself transcribed by one) -- it was
recovered by reading the committed `main_extra_tables.rs`'s own `TAGS` array
back out, the same way `regen-all.sh`'s `gen_subdir` table lists were
recovered for `codegen_subdirs.py`. Adding a tag to this file means adding
its id to MANIFEST here, by hand, after confirming `main_table.rs` still has
no entry for it.

usage:
  dump_tables.pl <exiftool-lib> Sony > tables.json
  gen_sony_main_extra_tables.py tables.json -o main_extra_tables.rs
"""
from __future__ import annotations

import argparse
import json
import re
import sys

# ---------------------------------------------------------------------------
# The manifest: every Sony::Main tag id `main_extra_tables.rs` covers, in the
# order the generated file lists them (ascending numeric id -- ExifTool's own
# hash iteration order is unspecified, so this is a deliberate, reproducible
# choice, not a transcription of anything Perl exposes).
# ---------------------------------------------------------------------------
MANIFEST = [
    0x1000, 0x1001, 0x1002, 0x2004, 0x2005, 0x2006, 0x2007, 0x201D, 0x2020,
    0x2022, 0x2026, 0x2027, 0x2028, 0x2029, 0x202B, 0x202C, 0x202D, 0x2031,
    0x2032, 0x2033, 0x2034, 0x2035, 0x2036, 0x2037, 0x2039, 0x204A, 0x205C,
    0xB041, 0xB042, 0xB043, 0xB04E, 0xB050,
]


class Unsupported(Exception):
    def __init__(self, tag_id, name, reason):
        super().__init__(f"0x{tag_id:x} {name}: {reason}")


def norm(s: str) -> str:
    return re.sub(r"\s+", " ", s).strip()


def rust_str(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


# ---------------------------------------------------------------------------
# `Format` -- only reinterprets the entry when present.
# ---------------------------------------------------------------------------
FORMAT_DICT = {
    "int8u": "Fmt::U8",
    "int16u": "Fmt::U16",
}

# ---------------------------------------------------------------------------
# `Condition` -- hard-coded dictionary keyed on the literal, whitespace-
# normalized Perl text. Every key here was copied verbatim (via `norm`) from
# `%Image::ExifTool::Sony::Main` as dumped by `dump_tables.pl`.
# ---------------------------------------------------------------------------
COND_DICT = {
    '$format eq "undef"': 'MCond::EntryFormat("undef")',
    '$format eq "int16u"': 'MCond::EntryFormat("int16u")',
    '$$self{Model} =~ /^(NEX-|ILCE-|ILME-|ZV-|DSC-(RX10M4|RX100M6|RX100M7|RX100M5A|HX95|HX99|RX0M2|RX1RM3))/':
        'MCond::ModelRe(false, r"^(NEX-|ILCE-|ILME-|ZV-|DSC-(RX10M4|RX100M6|RX100M7|RX100M5A|HX95|HX99|RX0M2|RX1RM3))")',
    '$$self{Model} !~ /^(ILCA-|DSC-|ZV-)/':
        'MCond::ModelRe(true, r"^(ILCA-|DSC-|ZV-)")',
    '$$self{Model} =~ /^ILCA-(68|77M2)/':
        'MCond::ModelRe(false, r"^ILCA-(68|77M2)")',
    '$$self{Model} =~ /^(ILCE-(5100|6000|7M2))/':
        'MCond::ModelRe(false, r"^(ILCE-(5100|6000|7M2))")',
    '$$self{Model} =~ /^ILCE-7RM2/':
        'MCond::ModelRe(false, r"^ILCE-7RM2")',
    '$$self{Model} =~ /^(DSC-RX1RM3)\\b/':
        'MCond::ModelRe(false, r"^(DSC-RX1RM3)\\b")',
    '$$self{Model} =~ /^(DSC-|Stellar)/':
        'MCond::ModelRe(false, r"^(DSC-|Stellar)")',
    '$$self{TagB042} and $$self{TagB042} != 0':
        'MCond::All(&[MCond::DmTruthy(Dm::TagB042), MCond::DmCmp(Dm::TagB042, NumCmp::Ne, 0.0_f64)])',
    '$$self{MetaVersion} and $$self{MetaVersion} eq "DC7303320222000"':
        'MCond::All(&[MCond::DmTruthy(Dm::MetaVersion), MCond::DmStrCmp(Dm::MetaVersion, true, "DC7303320222000")])',
    'not $$self{MetaVersion} or $$self{MetaVersion} ne "DC7303320222000"':
        'MCond::Any(&[MCond::DmFalsy(Dm::MetaVersion), MCond::DmStrCmp(Dm::MetaVersion, false, "DC7303320222000")])',
    "($$self{TagB042} = Get16u($valPt, 0)) and (not $$self{MetaVersion} or $$self{MetaVersion} ne 'DC7303320222000')":
        'MCond::All(&[MCond::StoreU16(Dm::TagB042), MCond::Any(&[MCond::DmFalsy(Dm::MetaVersion), MCond::DmStrCmp(Dm::MetaVersion, false, "DC7303320222000")])])',
}

# ---------------------------------------------------------------------------
# `RawConv` -- hard-coded dictionary, same discipline.
# ---------------------------------------------------------------------------
RAW_DICT = {
    "$val == 65535 ? undef : $val": "Raw::DropIfEq(65535.0_f64)",
}

# ---------------------------------------------------------------------------
# `ValueConv` -- hard-coded dictionary, same discipline.
# ---------------------------------------------------------------------------
VC_DICT = {
    r"$val=~s/(\d{2})(\d{2})(\d{2})(\d{2})/$4$3$2$1/; $val=~s/^0//; $val":
        "Vc::SerialNumberSwap",
}

# ---------------------------------------------------------------------------
# `PrintConv` expressions -- hard-coded dictionary, plus one parameterized
# form (`sprintf("%.Nd",$val)` -> `Pc::ZeroPad(N)`, Perl's zero-pad-to-N-
# digits idiom) since it is a single regular shape, not a distinct construct
# per N.
# ---------------------------------------------------------------------------
PC_EXPR_DICT = {
    '$val > 0 ? "+$val" : $val': "Pc::PlusOrVal",
    '$val ? sprintf("%+.1f",$val) : 0': "Pc::Signed1OrZero",
    "my @a = split ' ', $val; return $a[2] ? sprintf('%3dx%3d', $a[0], $a[1]) : 'n/a';":
        "Pc::FocusFrameSize",
    'my @v=split(" ",$val); $_/=1000 foreach @v; sprintf("%.2f %.2f",$v[0],$v[1])':
        "Pc::WbShiftPrecise",
}
ZERO_PAD_RE = re.compile(r'^sprintf\("%\.(\d+)d",\$val\)$')


def translate_cond(tag_id, name, cond_text):
    if cond_text is None:
        return "MCond::Always"
    key = norm(cond_text)
    if key not in COND_DICT:
        raise Unsupported(tag_id, name, f"unregistered Condition: {key!r}")
    return COND_DICT[key]


def translate_fmt(tag_id, name, fmt_text):
    if fmt_text is None:
        return None
    if fmt_text not in FORMAT_DICT:
        raise Unsupported(tag_id, name, f"unregistered Format: {fmt_text!r}")
    return FORMAT_DICT[fmt_text]


def translate_raw(tag_id, name, raw):
    if raw is None:
        return "Raw::None"
    if raw.get("kind") != "expr":
        raise Unsupported(tag_id, name, f"non-expr RawConv: {raw!r}")
    key = norm(raw["expr"])
    if key not in RAW_DICT:
        raise Unsupported(tag_id, name, f"unregistered RawConv: {key!r}")
    return RAW_DICT[key]


def translate_vc(tag_id, name, vc):
    if vc is None:
        return "Vc::None"
    if vc.get("kind") != "expr":
        raise Unsupported(tag_id, name, f"non-expr ValueConv: {vc!r}")
    key = norm(vc["expr"])
    if key not in VC_DICT:
        raise Unsupported(tag_id, name, f"unregistered ValueConv: {key!r}")
    return VC_DICT[key]


def translate_pc(tag_id, name, tag, pools):
    pc = tag.get("PrintConv")
    if pc is None:
        return "Pc::None"
    kind = pc["kind"]
    if kind == "enum":
        map_name = pools.intern_map(list(pc["map"].items()))
        return f"Pc::Map({map_name}, Other::None)"
    if kind == "enum_partial":
        directives = pc.get("directives") or {}
        if list(directives.keys()) != ["BITMASK"]:
            raise Unsupported(
                tag_id, name, f"enum_partial directives other than BITMASK: {directives!r}"
            )
        bitmask = directives["BITMASK"]
        per_word = tag.get("BitsPerWord")
        if per_word is None:
            raise Unsupported(tag_id, name, "BITMASK PrintConv with no BitsPerWord")
        map_name = pools.intern_map(list(pc["map"].items()))
        bits_items = sorted(((int(k), v) for k, v in bitmask.items()), key=lambda kv: kv[0])
        bit_name = pools.intern_bits(bits_items)
        return f"Pc::Bitmask({map_name}, {bit_name}, {int(per_word)}, Other::None)"
    if kind == "expr":
        key = norm(pc["expr"])
        if key in PC_EXPR_DICT:
            return PC_EXPR_DICT[key]
        m = ZERO_PAD_RE.match(key)
        if m:
            return f"Pc::ZeroPad({int(m.group(1))})"
        raise Unsupported(tag_id, name, f"unregistered PrintConv expr: {key!r}")
    raise Unsupported(tag_id, name, f"unsupported PrintConv kind: {kind!r}")


def translate_priority(tag_id, name, tag):
    if "Priority" not in tag:
        return "false"
    val = tag["Priority"]
    if val == "0":
        return "true"
    raise Unsupported(tag_id, name, f"unregistered Priority value: {val!r}")


def translate_print_hex(tag_id, name, tag):
    if "PrintHex" not in tag:
        return "false"
    val = tag["PrintHex"]
    if val == "1":
        return "true"
    raise Unsupported(tag_id, name, f"unregistered PrintHex value: {val!r}")


class Pools:
    """The two shared static-array pools (`M*` maps, `B*` bitmasks), each
    assigned names in first-appearance order while walking MANIFEST -- the
    same order the committed file declares them in."""

    def __init__(self):
        self.maps: list[tuple] = []  # ordered list of (&str, &str) tuples
        self.bits: list[tuple] = []  # ordered list of (u32, &str) tuples

    def intern_map(self, items):
        key = tuple(items)
        for i, existing in enumerate(self.maps):
            if existing == key:
                return f"M{i}"
        self.maps.append(key)
        return f"M{len(self.maps) - 1}"

    def intern_bits(self, items):
        key = tuple(items)
        for i, existing in enumerate(self.bits):
            if existing == key:
                return f"B{i}"
        self.bits.append(key)
        return f"B{len(self.bits) - 1}"


def render_map_decl(name, items):
    body = ", ".join(f"({rust_str(k)}, {rust_str(v)})" for k, v in items)
    return f"#[rustfmt::skip]\nstatic {name}: &[(&str, &str)] = &[{body}];"


def render_bits_decl(name, items):
    body = ", ".join(f"({k}u32, {rust_str(v)})" for k, v in items)
    return f"#[rustfmt::skip]\nstatic {name}: &[(u32, &str)] = &[{body}];"


def render_row(tag_id, name, cond, fmt, raw, vc, pc, print_hex, low_priority):
    fmt_text = "None" if fmt is None else f"Some({fmt})"
    return (
        f"    MainExtraTag {{ id: 0x{tag_id:x}, name: {rust_str(name)}, "
        f"cond: {cond}, fmt: {fmt_text}, raw: {raw}, vc: {vc}, pc: {pc}, "
        f"print_hex: {print_hex}, low_priority: {low_priority} }},"
    )


HEADER = '''//! `Sony::Main` scalars main_table.rs does not implement -- generated, do not
//! hand-edit.
//!
//! Read out of ExifTool's own `%Image::ExifTool::Sony::Main` hash in-process
//! (13.59) rather than retyped, and interpreted by [`super::main_extra`]. A tag
//! whose Condition or conversion is not one of the forms that module implements
//! is omitted entirely rather than emitted with a guessed value.

use super::binary_data::{Dm, Fmt, NumCmp, Other, Pc, Raw, Vc};
use super::main_extra::{MCond, MainExtraTag};
'''


def generate(data):
    main = data["modules"]["Sony"]["tables"]["Main"]["tags"]
    pools = Pools()
    rows = []
    for tag_id in MANIFEST:
        key = str(tag_id)
        if key not in main:
            raise Unsupported(tag_id, "?", "id not present in Sony::Main")
        entry = main[key]
        variants = entry["_variants"] if "_variants" in entry else [entry]
        for tag in variants:
            name = tag["Name"]
            cond = translate_cond(tag_id, name, tag.get("Condition"))
            fmt = translate_fmt(tag_id, name, tag.get("Format"))
            raw = translate_raw(tag_id, name, tag.get("RawConv"))
            vc = translate_vc(tag_id, name, tag.get("ValueConv"))
            pc = translate_pc(tag_id, name, tag, pools)
            print_hex = translate_print_hex(tag_id, name, tag)
            low_priority = translate_priority(tag_id, name, tag)
            rows.append(render_row(tag_id, name, cond, fmt, raw, vc, pc, print_hex, low_priority))

    out = [HEADER]
    for i, items in enumerate(pools.maps):
        out.append(render_map_decl(f"M{i}", items))
    for i, items in enumerate(pools.bits):
        out.append(render_bits_decl(f"B{i}", items))
    out.append(
        "\n/// Every row, in ExifTool's own order so a Condition list resolves the same way.\n"
        "#[rustfmt::skip]\npub static TAGS: &[MainExtraTag] = &["
    )
    out.extend(rows)
    out.append("];\n")
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
        print(f"gen_sony_main_extra_tables.py: {e}", file=sys.stderr)
        sys.exit(1)

    with open(args.output, "w", encoding="utf-8") as f:
        f.write(text)


if __name__ == "__main__":
    main()
