#!/usr/bin/env python3
"""Recover Sony's six existing plain binary tables from dump_tables.pl JSON.

This finite translation registry is not a Perl compiler. Unsupported read-time
fields, expressions and layout changes refuse before opening the output. Names,
IDs, enum data and variant order come from the loaded tables, never from Rust.

RAW_TAG_IDS preserves native key identity alongside each emitted DSL row:
independent fractional keys remain distinct and true variants repeat one ID.
The integer index remains the byte-offset index, not the variant-group identity.
The mandatory independent native verifier also checks PrintInt (documentation
metadata), whose value the dump does not carry.
PROCESS_PROC is checked by its registered name;
changes inside the shared Perl processor remain the engine oracle's contract.
"""
from __future__ import annotations

import argparse
from decimal import Decimal
import json
from pathlib import Path
import re
import sys


class Unsupported(ValueError):
    pass


TABLES = ("CameraSettings", "CameraSettings2", "CameraSettings3",
          "FaceInfo1", "FaceInfo2", "ShotInfo")
CONDITIONS = {
    None: "Cond::Always",
    "$$self{Model} !~ /^(NEX-|DSLR-(A450|A500|A550)$)/":
        'Cond::ModelRe(true, r"^(NEX-|DSLR-(A450|A500|A550)$)")',
    "$$self{Model} !~ /^DSLR-(A450|A500|A550)$/":
        'Cond::ModelRe(true, r"^DSLR-(A450|A500|A550)$")',
    "$$self{Model} !~ /^DSLR-(A450|A500|A550)/":
        'Cond::ModelRe(true, r"^DSLR-(A450|A500|A550)")',
    "$$self{Model} =~ /^DSLR-(A450|A500|A550)$/":
        'Cond::ModelRe(false, r"^DSLR-(A450|A500|A550)$")',
    "($$self{Model} =~ /^NEX-/)": 'Cond::ModelRe(false, r"^NEX-")',
    "($$self{Model} =~ /^NEX-/) and ($$self{LensMount} != 1)":
        'Cond::All(&[Cond::ModelRe(false, r"^NEX-"), Cond::DmCmp(Dm::LensMount, NumCmp::Ne, 1.0_f64)])',
    "\n            $$self{FacesDetected} and\n            $$self{FaceInfoOffset} == 0x48 and\n            $$self{FaceInfoLength} == 0x20\n        ":
        "Cond::All(&[Cond::DmTruthy(Dm::FacesDetected), Cond::DmCmp(Dm::FaceInfoOffset, NumCmp::Eq, 72.0_f64), Cond::DmCmp(Dm::FaceInfoLength, NumCmp::Eq, 32.0_f64)])",
    "\n            $$self{FacesDetected} and\n            $$self{FaceInfoOffset} == 0x5e and\n            $$self{FaceInfoLength} == 0x25\n        ":
        "Cond::All(&[Cond::DmTruthy(Dm::FacesDetected), Cond::DmCmp(Dm::FaceInfoOffset, NumCmp::Eq, 94.0_f64), Cond::DmCmp(Dm::FaceInfoLength, NumCmp::Eq, 37.0_f64)])",
}
VALUE_CONVS = {
    None: "Vc::None",
    "$val * 100": "Vc::Mul(100.0_f64)",
    "$val - 10": "Vc::Add(-10.0_f64)",
    "$val > 128 ? $val - 256 : $val": "Vc::Signed8Above128",
    "$val ? 2 ** (6 - $val/8) : 0": "Vc::ExpTime(6.0_f64, 8.0_f64)",
    "$val ? exp(($val/8-6)*log(2))*100 : $val": "Vc::IsoExp",
    "($val - 128) / 24": "Vc::SubDiv(128.0_f64, 24.0_f64)",
    "($val and $val < 254) ? exp(($val/8-6)*log(2))*100 : $val": "Vc::IsoExpBelow254",
    "2 ** (($val/8 - 1) / 2)": "Vc::Pow2DivSubHalf(8.0_f64, 1.0_f64)",
}
PRINT_CONVS = {
    None: "Pc::None",
    "$self->ConvertDateTime($val)": "Pc::DateTime",
    '$val > 0 ? "+$val" : $val': "Pc::PlusOrVal",
    '$val ? Image::ExifTool::Exif::PrintExposureTime($val) : "Bulb"': "Pc::ExposureTimeOrBulb",
    '$val ? sprintf("%+.1f",$val) : 0': "Pc::Signed1OrZero",
    '$val ? sprintf("%.0f",$val) : "Auto"': "Pc::Fixed0OrAuto",
    "Image::ExifTool::Exif::PrintFNumber($val)": "Pc::FNumber",
    '"$val K"': 'Pc::Suffix(" K")',
    '"$val%"': 'Pc::Suffix("%")',
    'sprintf("%.3d",$val)': "Pc::ZeroPad(3)",
    'sprintf("%.4d",$val)': "Pc::ZeroPad(4)",
    'sprintf("%x.%.2x",$val>>8,$val&0xff)': "Pc::HexDotHex",
    'sprintf("Ver.%.2x.%.3d",$val>>8,$val&0xff)': "Pc::VerHex",
}
RAW_CONVS = {None: "Raw::None"}
for _member in ("FaceInfoLength", "FaceInfoOffset", "FacesDetected", "LensMount", "MetaVersion"):
    RAW_CONVS[f"$$self{{{_member}}} = $val"] = f"Raw::Store(Dm::{_member})"
for _face in range(1, 9):
    RAW_CONVS[f"$$self{{FacesDetected}} < {_face} ? undef : $val"] = f"Raw::DropIfDmLess(Dm::FacesDetected, {_face}.0_f64)"

# Exact native B::Deparse bodies, including their literals. Whitespace changes
# are refused too: a new interpreter spelling needs an independently audited
# entry, not a grammar that can erase changed string or regex contents.
OTHER_BODIES = {
    "{\n    package Image::ExifTool::Sony;\n    use strict;\n    (my($val, $inv) = @_);\n    ($inv or (return int(($val + 0.5))));\n    (return (&Image::ExifTool::IsFloat($val) ? $val : (undef)));\n}": "Other::RoundHalfUp",
    "{\n    package Image::ExifTool::Sony;\n    use strict;\n    (shift());\n}": "Other::Identity",
}
FORMATS = {"int8u": "Fmt::U8", "int8s": "Fmt::I8", "int16u": "Fmt::U16",
           "int16uRev": "Fmt::U16Rev", "int32u": "Fmt::U32", "string": "Fmt::Str"}
FIELDS = {"Name", "Description", "Notes", "Format", "Condition", "RawConv",
          "ValueConv", "PrintConv", "ValueConvInv", "PrintConvInv", "Writable",
          "Mask", "BitShift", "PrintHex", "Priority", "DataMember", "Groups",
          "SeparateTable", "_extra_keys", "_shorthand", "SubDirectory", "Unknown"}


def uint(value, bits=32):
    if isinstance(value, bool) or not isinstance(value, (str, int)) or not re.fullmatch(r"0|[1-9][0-9]*", str(value)):
        raise Unsupported(f"noncanonical uint{bits}: {value!r}")
    n = int(value)
    if n >= 1 << bits:
        raise Unsupported(f"uint{bits} overflow: {value!r}")
    return n


def flag(value):
    n = uint(value)
    if n not in (0, 1):
        raise Unsupported(f"expected flag 0 or 1: {value!r}")
    return n


def tag_index(key):
    """Canonical nonnegative decimal key; integer part is the binary index.

    Reject numeric aliases such as 01, 1.0 and 1.10. A fractional component
    may begin with zero (1.01), but must end in a nonzero digit. Sorting uses
    Decimal; a separate conservative NV-collision check refuses distinct keys
    whose native numeric ordering may tie. No rounded key is ever emitted.
    ProcessBinaryData computes int($index) * $increment + $varSize, truncating
    before FORMAT multiplication (ExifTool.pm 13.59:9957), including U16 tables.
    """
    if not isinstance(key, str):
        raise Unsupported(f"tag ID must be a string: {key!r}")
    match = re.fullmatch(r"(0|[1-9][0-9]*)(?:\.([0-9]*[1-9]))?", key)
    if not match:
        raise Unsupported(f"noncanonical tag ID: {key!r}")
    index = uint(match[1])
    # A decimal just below an integer may round upward in native NV before
    # int($index), even when no second key exposes a numeric-sort collision.
    if int(float(Decimal(key))) != index:
        raise Unsupported(f"native integer offset may round across a boundary: {key!r}")
    return index


def rust_string(value):
    if not isinstance(value, str):
        raise Unsupported(f"expected string: {value!r}")
    result = []
    for char in value:
        if 0xD800 <= ord(char) <= 0xDFFF:
            raise Unsupported("surrogate is not a Unicode scalar")
        if char in ('"', "\\"):
            result.append("\\" + char)
        elif ord(char) < 32 or ord(char) == 127:
            result.append(f"\\u{{{ord(char):x}}}")
        else:
            result.append(char)
    return '"' + ''.join(result) + '"'


def expression(row, field):
    if field not in row:
        return None
    value = row[field]
    if not isinstance(value, dict) or set(value) != {"kind", "expr"} or value["kind"] != "expr" or not isinstance(value["expr"], str):
        raise Unsupported(f"unsupported {field}: {value!r}")
    return value["expr"]


def registered(registry, value, what):
    if not isinstance(value, (str, type(None))) or value not in registry:
        raise Unsupported(f"unregistered {what}: {value!r}")
    return registry[value]


def format_field(value):
    if value is None:
        return "Fmt::Default", 1
    if not isinstance(value, str):
        raise Unsupported("invalid Format")
    match = re.fullmatch(r"([a-zA-Z0-9]+)(?:\[(0|[1-9][0-9]*)\])?", value)
    if not match or match[1] not in FORMATS:
        raise Unsupported(f"unregistered Format: {value!r}")
    count = uint(match[2]) if match[2] is not None else 1
    if count == 0 or (match[1] == "string" and match[2] is None):
        raise Unsupported("zero or unbounded Format count")
    return FORMATS[match[1]], count


_CODE_BASE = {"__name", "__perl", "__opaque", "__deparse"}
_CODE_PROVENANCE = _CODE_BASE | {"resolved", "source_file", "source_sha256", "dependencies"}
_CODE_DEPENDENCY = _CODE_BASE | {"resolved", "source_file", "source_sha256"}
_SHA256 = re.compile(r"[0-9a-f]{64}\Z")


def _shared_binary_processor(proc):
    """Accept the legacy name fact or the authenticated shared-code fact.

    This producer does not interpret ProcessBinaryData itself, but it must not
    collapse an unresolved/rebound callback into its old registered name. The
    full body remains the shared engine-oracle contract; this narrow selector
    authenticates the live CV and its direct Get16u binding.
    """
    if (not isinstance(proc, dict) or proc.get("__name") != "Image::ExifTool::ProcessBinaryData"
            or proc.get("__perl") != "CODE" or proc.get("__opaque") not in (True, 1)
            or not isinstance(proc.get("__deparse"), str)):
        return False
    keys = set(proc)
    if keys == _CODE_BASE:
        return True
    if keys != _CODE_PROVENANCE or proc.get("resolved") is not True:
        return False
    if proc.get("source_file") != "Image/ExifTool.pm" or not isinstance(proc.get("source_sha256"), str) or not _SHA256.fullmatch(proc["source_sha256"]):
        return False
    dependencies = proc.get("dependencies")
    if not isinstance(dependencies, dict) or set(dependencies) != {"Image::ExifTool::Get16u"}:
        return False
    reader = dependencies["Image::ExifTool::Get16u"]
    return (isinstance(reader, dict) and set(reader) == _CODE_DEPENDENCY
            and reader.get("__name") == "Image::ExifTool::Get16u"
            and reader.get("__perl") == "CODE" and reader.get("__opaque") in (True, 1)
            and reader.get("resolved") is True and isinstance(reader.get("__deparse"), str)
            and reader.get("source_file") == "Image/ExifTool.pm"
            and isinstance(reader.get("source_sha256"), str) and _SHA256.fullmatch(reader["source_sha256"]))


def check_meta(name, table):
    if not isinstance(table, dict) or set(table) != {"meta", "tags", "full_name", "tag_count"} or not isinstance(table["tags"], dict) or not table["tags"]:
        raise Unsupported(f"{name}: malformed or empty table")
    if (table["full_name"] != f"Image::ExifTool::Sony::{name}"
            or uint(table["tag_count"]) != len(table["tags"])):
        raise Unsupported(f"{name}: table identity/count changed")
    meta = table["meta"]
    allowed = {"CHECK_PROC", "FIRST_ENTRY", "FORMAT", "GROUPS", "NOTES", "PRIORITY",
               "PROCESS_PROC", "WRITABLE", "WRITE_PROC", "DATAMEMBER", "IS_SUBDIR"}
    if not isinstance(meta, dict) or set(meta) - allowed:
        raise Unsupported(f"{name}: unregistered table metadata")
    if not _shared_binary_processor(meta.get("PROCESS_PROC")):
        raise Unsupported(f"{name}: PROCESS_PROC changed or lacks authenticated shared provenance")
    group = "Camera" if name.startswith("CameraSettings") else "Image"
    if meta.get("GROUPS") != {"0": "MakerNotes", "2": group} or uint(meta.get("FIRST_ENTRY")) != 0:
        raise Unsupported(f"{name}: GROUPS/FIRST_ENTRY changed")
    expected_dm = {"CameraSettings3": ["153"], "ShotInfo": ["2", "48", "50", "52"]}.get(name)
    expected_sub = ["72", "94"] if name == "ShotInfo" else None
    if meta.get("DATAMEMBER") != expected_dm or meta.get("IS_SUBDIR") != expected_sub:
        raise Unsupported(f"{name}: DATAMEMBER/IS_SUBDIR dispatch changed")
    fmt, count = format_field(meta.get("FORMAT"))
    if count != 1 or fmt not in ("Fmt::Default", "Fmt::U8", "Fmt::U16"):
        raise Unsupported(f"{name}: unmodeled table FORMAT")
    return fmt, flag(meta.get("PRIORITY", 1)) == 0


def render(data):
    tables = data["modules"]["Sony"]["tables"]
    maps, bits, map_ids, bit_ids, rendered = [], [], {}, {}, []
    counts = {"tables": 0, "rows": 0, "unknown_omitted": 0, "dump_boundaries": []}

    def intern(pairs, all_pairs, ids, prefix):
        if pairs not in ids:
            ids[pairs] = len(all_pairs)
            all_pairs.append(pairs)
        return f"{prefix}{ids[pairs]}"

    def print_conv(row):
        pc = row.get("PrintConv")
        if isinstance(pc, dict) and pc.get("kind") in ("enum", "enum_partial"):
            if set(pc) != {"kind", "map", "directives"} or not isinstance(pc["map"], dict) or not pc["map"]:
                raise Unsupported("malformed or empty PrintConv map")
            pairs = tuple(sorted(pc["map"].items()))
            for key, text in pairs:
                rust_string(key); rust_string(text)
            map_name = intern(pairs, maps, map_ids, "M")
            directives = pc["directives"]
            if pc["kind"] == "enum" and directives is not None:
                raise Unsupported("plain enum has directives")
            if pc["kind"] == "enum_partial" and (not isinstance(directives, dict) or not directives):
                raise Unsupported("partial enum lacks directives")
            directives = directives or {}
            if set(directives) - {"Notes", "BITMASK", "OTHER"}:
                raise Unsupported("unregistered PrintConv directives")
            if "Notes" in directives:
                rust_string(directives["Notes"])
            other = "Other::None"
            if "OTHER" in directives:
                body = directives["OTHER"]
                if (not isinstance(body, dict) or set(body) != {"__name", "__perl", "__opaque", "__deparse"}
                        or body["__perl"] != "CODE" or body["__opaque"] != 1
                        or body["__name"] != "Image::ExifTool::Sony::__ANON__"):
                    raise Unsupported("unsupported OTHER body")
                other = registered(OTHER_BODIES, body["__deparse"], "OTHER")
            if "BITMASK" in directives:
                bitmap = directives["BITMASK"]
                if not isinstance(bitmap, dict) or not bitmap:
                    raise Unsupported("malformed BITMASK")
                bp = tuple(sorted((uint(k), v) for k, v in bitmap.items()))
                for bit, text in bp:
                    if bit >= 32:
                        raise Unsupported("BITMASK bit exceeds default 32-bit word")
                    rust_string(text)
                bit_name = intern(bp, bits, bit_ids, "B")
                return f"Pc::Bitmask({map_name}, {bit_name}, 32, {other})"
            return f"Pc::Map({map_name}, {other})"
        return registered(PRINT_CONVS, expression(row, "PrintConv"), "PrintConv")

    for table_number, name in enumerate(TABLES):
        table = tables[name]
        table_fmt, table_low = check_meta(name, table)
        rows, raw_ids = [], []
        numeric_keys = {}
        for key in table["tags"]:
            tag_index(key)
            # Native sorts with $a <=> $b. Python's double is used only as a
            # refusal guard, never as the sorting key or emitted identity.
            # This is conservative on Perl builds with wider NV precision.
            nv = float(Decimal(key))
            if nv in numeric_keys:
                raise Unsupported(f"{name}: native numeric sort may tie distinct IDs {numeric_keys[nv]!r} and {key!r}")
            numeric_keys[nv] = key
        for key, group in sorted(table["tags"].items(), key=lambda item: Decimal(item[0])):
            if not isinstance(group, dict):
                raise Unsupported(f"{name}[{key}]: malformed group")
            if "_variants" in group:
                if set(group) != {"_variants"} or not isinstance(group["_variants"], list) or not group["_variants"]:
                    raise Unsupported(f"{name}[{key}]: malformed variants")
                variants = group["_variants"]
            else:
                variants = [group]
            unknown_flags = [flag(row.get("Unknown", 0)) for row in variants if isinstance(row, dict)]
            if len(unknown_flags) != len(variants) or (any(unknown_flags) and not all(unknown_flags)):
                raise Unsupported(f"{name}[{key}]: malformed or mixed known/Unknown alternatives")
            for row in variants:
                try:
                    if set(row) - FIELDS:
                        raise Unsupported(f"unregistered fields: {sorted(set(row) - FIELDS)}")
                    if not isinstance(row.get("Name"), str) or row["Name"] in ("", "0"):
                        raise Unsupported("Name must be a Perl-truthy string")
                    label = rust_string(row["Name"])
                    cond = registered(CONDITIONS, row.get("Condition"), "Condition")
                    fmt, count = format_field(row.get("Format"))
                    if "Format" in row and row["Format"] is None:
                        raise Unsupported("null Format")
                    raw_expr = expression(row, "RawConv")
                    raw = registered(RAW_CONVS, raw_expr, "RawConv")
                    if "DataMember" in row and raw_expr != f"$$self{{{row['DataMember']}}} = $val":
                        raise Unsupported("DataMember/RawConv disagreement")
                    vc = registered(VALUE_CONVS, expression(row, "ValueConv"), "ValueConv")
                    pc = print_conv(row)
                    extras = row.get("_extra_keys", [])
                    if not isinstance(extras, list) or len(set(extras)) != len(extras) or set(extras) - {"PrintConvColumns", "PrintInt", "Shift"}:
                        raise Unsupported("unregistered _extra_keys")
                    if "PrintInt" in extras:
                        if (name, key, row["Name"]) != ("CameraSettings3", "1015", "LensType2"):
                            raise Unsupported("new PrintInt requires native review")
                        counts["dump_boundaries"].append("CameraSettings3[1015] PrintInt: dump carries presence only; independent native verifier checks value 1")
                    if "Shift" in extras and (name, key, row["Name"], row.get("Groups"), pc) != ("ShotInfo", "6", "SonyDateTime", {"2": "Time"}, "Pc::DateTime"):
                        raise Unsupported("new Shift requires review")
                    if "Groups" in row and (name, key, row["Groups"]) != ("ShotInfo", "6", {"2": "Time"}):
                        raise Unsupported("unmodeled Groups override")
                    if "_shorthand" in row and row["_shorthand"] is not True:
                        raise Unsupported("malformed shorthand")
                    mask = uint(row.get("Mask", 0))
                    derived_shift = (mask & -mask).bit_length() - 1 if mask else 0
                    shift = uint(row.get("BitShift", derived_shift))
                    if shift != derived_shift:
                        raise Unsupported("explicit BitShift differs from runtime mask trailing_zeros")
                    if mask and (count != 1 or fmt == "Fmt::Str"):
                        raise Unsupported("mask on non-scalar numeric Format")
                    sub = "None"
                    if "SubDirectory" in row:
                        sd = row["SubDirectory"]
                        allowed_subs = {"Image::ExifTool::Sony::FaceInfo1": 3, "Image::ExifTool::Sony::FaceInfo2": 4}
                        if not isinstance(sd, dict) or set(sd) != {"TagTable"} or sd["TagTable"] not in allowed_subs:
                            raise Unsupported("unregistered SubDirectory")
                        sub = f"Some({allowed_subs[sd['TagTable']]})"
                    low = flag(row["Priority"]) == 0 if "Priority" in row else table_low
                    hexed = bool(flag(row.get("PrintHex", 0)))
                    if flag(row.get("Unknown", 0)):
                        # Condition executes before Unknown filtering. Reject
                        # unmodeled fields/conversions even on an omitted row.
                        counts["unknown_omitted"] += 1
                        continue
                    index = tag_index(key)
                    rows.append(f"    BinTag {{ index: {index}, name: {label}, cond: {cond}, fmt: {fmt}, count: {count}, mask: {mask}, raw: {raw}, vc: {vc}, pc: {pc}, hook: Hook::None, print_hex: {str(hexed).lower()}, low_priority: {str(low).lower()}, subdir: {sub} }},")
                    raw_ids.append(key)
                except (Unsupported, TypeError, KeyError) as error:
                    raise Unsupported(f"{name}[{key}] {row.get('Name', '?')}: {error}") from error
        if not rows:
            raise Unsupported(f"{name}: no emitted rows")
        rendered.append((name, table_fmt, rows, raw_ids))
        counts["tables"] += 1
        counts["rows"] += len(rows)

    version = data["exiftool_version"]
    if not isinstance(version, str) or not re.fullmatch(r"[0-9]+\.[0-9]+", version):
        raise Unsupported("malformed ExifTool version")
    lines = ["//! Sony plain (unenciphered) binary-data tables -- generated, do not hand-edit.",
             "//!", "//! `ShotInfo` (0x3000, with its `FaceInfo1`/`FaceInfo2` sub-directories) and",
             "//! the three `CameraSettings` layouts the A-mount DSLRs write into 0x0114.",
             "//! None of these is enciphered; they go through the same",
             "//! [`super::binary_data`] interpreter as the enciphered blocks because they",
             "//! are the same `ProcessBinaryData` shape. Every row was read out of",
             f"//! ExifTool's own `%Image::ExifTool::Sony::*` hashes in-process ({version}) rather",
             "//! than retyped.", "", "use super::binary_data::{BinTable, BinTag, Cond, Dm, Fmt, Hook, NumCmp, Other, Pc, Raw, Vc};", ""]
    for i, pairs in enumerate(maps):
        text = ", ".join(f"({rust_string(k)}, {rust_string(v)})" for k, v in pairs)
        lines += ["#[rustfmt::skip]", f"static M{i}: &[(&str, &str)] = &[{text}];"]
    for i, pairs in enumerate(bits):
        text = ", ".join(f"({k}u32, {rust_string(v)})" for k, v in pairs)
        lines += ["#[rustfmt::skip]", f"static B{i}: &[(u32, &str)] = &[{text}];"]
    lines.append("")
    for i, (_, _, rows, _) in enumerate(rendered):
        lines += ["#[rustfmt::skip]", f"static T{i}: &[BinTag] = &[", *rows, "];"]
    lines += ["", "/// Native tag IDs, aligned with each table's emitted rows. True variants",
              "/// repeat one ID; independent fractional keys retain distinct identities.",
              "#[rustfmt::skip]", "pub static RAW_TAG_IDS: &[&[&str]] = &["]
    for name, _, _, raw_ids in rendered:
        lines.append(f"    &[{', '.join(rust_string(key) for key in raw_ids)}], // {name}")
    lines.append("];")
    lines += ["", "/// Every table, indexed by the `SubDir`/`Root` table numbers above.", "pub static TABLES: &[BinTable] = &["]
    for i, (name, fmt, _, _) in enumerate(rendered):
        lines += ["    BinTable {", f"        name: {rust_string(name)},", f"        fmt: {fmt},", f"        tags: T{i},", "    },"]
    lines += ["];", "", "/// Table numbers, by ExifTool table name.", "#[allow(dead_code)]", "pub mod idx {"]
    lines += [f"    pub const {name.upper()}: usize = {i};" for i, name in enumerate(TABLES)]
    lines += ["}", ""]
    counts.update(maps=len(maps), bitmaps=len(bits))
    return "\n".join(lines), counts


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise Unsupported(f"duplicate JSON key: {key!r}")
        result[key] = value
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dump", type=Path)
    parser.add_argument("-o", "--output", required=True, type=Path)
    args = parser.parse_args()
    try:
        data = json.loads(args.dump.read_text(), object_pairs_hook=unique_object)
        pin = (Path(__file__).resolve().parents[2] / ".exiftool-version").read_text().strip()
        if data.get("exiftool_version") != pin:
            raise Unsupported(f"dump version {data.get('exiftool_version')!r} != pinned {pin}")
        text, counts = render(data)
        payload = text.encode("utf-8")
        # All validation and encoding finish before any output mutation.
        args.output.write_bytes(payload)
        print("gen_sony_plain_tables: " + json.dumps(counts, sort_keys=True), file=sys.stderr)
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(1, f"gen_sony_plain_tables: {error}\n")


if __name__ == "__main__":
    main()
