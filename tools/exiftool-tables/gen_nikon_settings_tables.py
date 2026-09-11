#!/usr/bin/env python3
"""Recover NikonSettings::Main's existing Rust table from dump_tables.pl JSON.

This is a finite translation registry for settings.rs, not a Perl compiler.
Names, IDs, ordered alternatives, masks and enum maps come from loaded Perl.
New read-time constructs fail before the output is opened. Unknown tags are
counted and omitted, matching default ExifTool output. Write-only inverses and
Notes do not affect this read-only table.

The existing interpreter does not export AFAreaMode to other directories.
Its exact existing RawConv store is an explicitly reported projection below;
recovering this producer does not resolve cross-directory state propagation.
It also applies the one existing BracketProgram mask that the native custom
processor does not apply. Preserve that declaration but report the discrepancy;
new masks need runtime review instead of inheriting that interpretation.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys


class Unsupported(ValueError):
    pass


# Literal expressions intentionally retain whitespace inside strings/regexes.
# No blanket normalization can erase a changed quoted value.
CONDITIONS = {
    None: "Always",
    r"$$self{Model} =~ /^NIKON D6\b/i": "ModelD6",
    r"$$self{Model} =~ /^NIKON Z (7|7_2)\b/i": "ModelZ7",
    r"$$self{Model} =~ /^NIKON Z (5|50|6|6_2|7|7_2|fc)\b/i": "ModelZSeries",
    r"$$self{Model} =~ /^NIKON Z [67]\b/": "ModelZ6Or7",
    "$$self{HDMIBitDepth}  == 2": "HdmiBitDepthIs2",
    "$$self{CmdDialsReverseRotExposureComp} and $$self{CmdDialsReverseRotExposureComp} == 1": "CmdDialsReverseRotExposureCompIs1",
    "$$self{CmdDialsChangeMainSubExposure} and $$self{CmdDialsChangeMainSubExposure} == 1": "CmdDialsChangeMainSubExposureIs(1)",
    "$$self{CmdDialsChangeMainSubExposure} and $$self{CmdDialsChangeMainSubExposure} == 2": "CmdDialsChangeMainSubExposureIs(2)",
    "$$self{BracketSet} < 4": "BracketSetLt4",
    "$$self{BracketSet} and $$self{BracketSet} == 4": "BracketSetIs(4)",
    "$$self{BracketSet} and $$self{BracketSet} == 5": "BracketSetIs(5)",
    "$$self{BracketSet} < 4 and $$self{BracketProgram} ne 19": "BracketSetLt4AndProgramNe(19)",
    "$$self{BracketSet} == 4 and $$self{BracketProgram} ne 5": "BracketSetEqAndProgramNe(4, 5)",
    "$$self{BracketSet} == 5 and $$self{BracketProgram} ne 10": "BracketSetEqAndProgramNe(5, 10)",
    "$$self{PlaybackFlickUp} and $$self{PlaybackFlickUp} == 1": "PlaybackFlickUpIs1",
    "$$self{PlaybackFlickDown} and $$self{PlaybackFlickDown} == 1": "PlaybackFlickDownIs1",
}

STORES = {
    "HDMIBitDepth": "HdmiBitDepth", "HDMIOutputHDR": "HdmiOutputHdr",
    "BracketSet": "BracketSet", "BracketProgram": "BracketProgram",
    "PlaybackFlickUp": "PlaybackFlickUp", "PlaybackFlickDown": "PlaybackFlickDown",
}

# Key on both stages: accepting PrintConv while dropping ValueConv is wrong.
CONVERSIONS = {
    (None, None): "Raw",
    ("$val - 6", None): "Offset(-6)",
    (None, "$val-6"): "Offset(-6)",
    ("10 - $val", None): "Negate(10)",
    ("15 - $val", '"$val fps"'): "Fps(15)",
    ("11 - $val", '"$val fps"'): "Fps(11)",
    ("6 - $val", '"$val fps"'): "Fps(6)",
    ("($val - 7) / 6", '$val ? sprintf("%+.2f", $val) : 0'): "FineTune",
}
FIELDS = {"Name", "Condition", "Mask", "PrintConv", "ValueConv", "RawConv",
          "Unknown", "Notes", "Description", "PrintConvInv", "ValueConvInv"}


def uint(value, bits):
    if isinstance(value, bool) or not isinstance(value, (str, int)):
        raise Unsupported(f"expected uint{bits}, got {value!r}")
    if not re.fullmatch(r"0|[1-9][0-9]*", str(value)):
        raise Unsupported(f"noncanonical uint{bits}: {value!r}")
    n = int(value)
    if n >= 1 << bits:
        raise Unsupported(f"uint{bits} overflow: {value!r}")
    return n


def rust_string(value):
    if not isinstance(value, str):
        raise Unsupported(f"expected string, got {value!r}")
    escaped = []
    for char in value:
        if 0xD800 <= ord(char) <= 0xDFFF:
            raise Unsupported("surrogate is not a Unicode scalar")
        if char in ('"', "\\"):
            escaped.append("\\" + char)
        elif ord(char) < 32 or ord(char) == 127:
            escaped.append(f"\\u{{{ord(char):x}}}")
        else:
            escaped.append(char)
    return '"' + "".join(escaped) + '"'


def expression(row, field):
    if field not in row:
        return None
    value = row[field]
    if (not isinstance(value, dict) or set(value) != {"kind", "expr"}
            or value["kind"] != "expr" or not isinstance(value["expr"], str)):
        raise Unsupported(f"unsupported {field}: {value!r}")
    return value["expr"]


def render(data):
    table = data["modules"]["NikonSettings"]["tables"]["Main"]
    meta = table["meta"]
    if (set(meta) - {"GROUPS", "NOTES", "PROCESS_PROC"}
            or meta.get("GROUPS") != {"0": "MakerNotes", "2": "Camera"}
            or meta.get("PROCESS_PROC", {}).get("__name") !=
            "Image::ExifTool::NikonSettings::ProcessNikonSettings"):
        raise Unsupported("NikonSettings::Main table contract changed")
    maps, map_ids, rows = [], {}, []
    counts = {"rows": 0, "unknown_omitted": 0, "state_projections": [], "mask_residuals": []}
    for key, group in sorted(table["tags"].items()):
        tag_id = uint(key, 16)
        if "_variants" in group and (set(group) != {"_variants"}
                                     or not isinstance(group["_variants"], list)
                                     or not group["_variants"]):
            raise Unsupported(f"NikonSettings::Main[{key}]: malformed variants")
        variants = group.get("_variants", [group])
        if not all(isinstance(row, dict) for row in variants):
            raise Unsupported(f"NikonSettings::Main[{key}]: malformed row")
        unknown_flags = [row.get("Unknown", 0) for row in variants]
        if any(flag not in (0, 1, "0", "1") for flag in unknown_flags):
            raise Unsupported(f"NikonSettings::Main[{key}]: invalid Unknown flag")
        omitted = [flag in (1, "1") for flag in unknown_flags]
        # GetTagInfo stops at the first matching variant, even if Unknown
        # suppresses it. Removing such a variant would expose a later fallback.
        if any(omitted) and not all(omitted):
            raise Unsupported(f"NikonSettings::Main[{key}]: mixed known/Unknown alternatives need a runtime veto")
        for row in variants:
            try:
                # GetTagInfo evaluates Condition before filtering Unknown.
                # Even an omitted row must not hide a new stateful expression.
                condition = row.get("Condition")
                if not isinstance(condition, (str, type(None))) or condition not in CONDITIONS:
                    raise Unsupported(f"unregistered Condition: {condition!r}")
                unknown = row.get("Unknown", 0)
                if unknown not in (0, 1, "0", "1"):
                    raise Unsupported(f"unexpected Unknown flag: {unknown!r}")
                if unknown in (1, "1"):
                    counts["unknown_omitted"] += 1
                    continue
                extra = set(row) - FIELDS
                if extra:
                    raise Unsupported(f"unregistered fields: {sorted(extra)}")
                if not isinstance(row.get("Name"), str) or not row["Name"]:
                    raise Unsupported("tag Name must be a nonempty string")
                name = rust_string(row["Name"])
                mask = uint(row.get("Mask", 0), 32)
                if mask:
                    if (key, row["Name"], condition, mask) != (
                            "266", "BracketProgram", "$$self{BracketSet} and $$self{BracketSet} == 5", 15):
                        raise Unsupported("new mask requires custom-processor runtime review")
                    counts["mask_residuals"].append("Main[266] BracketProgram: Rust applies Mask 15; native custom processor does not")
                raw = expression(row, "RawConv")
                dm = "None"
                if raw is not None:
                    stores = {f"$$self{{{k}}} = $val": v for k, v in STORES.items()}
                    if raw in stores:
                        dm = stores[raw]
                    elif (key, row["Name"], raw) == ("366", "AFAreaMode", "$$self{AFAreaMode} = $val"):
                        counts["state_projections"].append("Main[366] AFAreaMode: existing Dm::None; cross-directory store not propagated")
                    else:
                        raise Unsupported(f"unregistered RawConv: {raw!r}")
                value = expression(row, "ValueConv")
                pc = row.get("PrintConv")
                if isinstance(pc, dict) and pc.get("kind") == "enum":
                    if (value is not None or set(pc) != {"kind", "map", "directives"}
                            or pc["directives"] is not None or not isinstance(pc["map"], dict)
                            or not pc["map"]):
                        raise Unsupported("enum with additional conversion, directives or empty map")
                    pairs = tuple(sorted((uint(k, 32), v) for k, v in pc["map"].items()))
                    for _, text in pairs:
                        rust_string(text)
                    if pairs not in map_ids:
                        map_ids[pairs] = len(maps)
                        maps.append(pairs)
                    conv = f"Map(PC_{map_ids[pairs]})"
                else:
                    pair = (value, expression(row, "PrintConv"))
                    if pair not in CONVERSIONS:
                        raise Unsupported(f"unregistered ValueConv/PrintConv pair: {pair!r}")
                    conv = CONVERSIONS[pair]
                rows.append(f"    E {{ id: 0x{tag_id:04x}, name: {name}, cond: Cond::{CONDITIONS[condition]}, mask: 0x{mask:x}, conv: Conv::{conv}, dm: Dm::{dm} }},")
                counts["rows"] += 1
            except (Unsupported, KeyError, TypeError) as error:
                raise Unsupported(f"NikonSettings::Main[{key}] {row.get('Name', '?')}: {error}") from error
    if not rows:
        raise Unsupported("NikonSettings::Main produced no rows")
    lines = [
        "//! `Image::ExifTool::NikonSettings::Main` -- generated, do not hand-edit.",
        "//!", "//! Every row here was walked out of ExifTool's own `%Image::ExifTool::",
        "//! NikonSettings::Main` hash rather than retyped, so the names, PrintConv",
        "//! tables, Conditions and Masks are the ones ExifTool itself applies.",
        "//! Tags flagged `Unknown` are omitted because ExifTool does not report them",
        "//! without `-u`.", "", "use super::settings::{Cond, Conv, Dm, SettingsTag as E};", "",
    ]
    for i, pairs in enumerate(maps):
        entries = ", ".join(f"({k}, {rust_string(v)})" for k, v in pairs)
        lines += ["#[rustfmt::skip]", f"const PC_{i}: &[(u32, &str)] = &[{entries}];"]
    lines += ["", "/// Directory order is irrelevant here; lookup is by tag id, and the first",
              "/// row whose `Cond` holds wins, exactly as ExifTool picks a Condition variant.",
              "#[rustfmt::skip]", "pub(super) const SETTINGS_TAGS: &[E] = &[", *rows, "];", ""]
    counts["maps"] = len(maps)
    return "\n".join(lines), counts


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dump", type=Path)
    parser.add_argument("-o", "--output", required=True, type=Path)
    args = parser.parse_args()
    try:
        data = json.loads(args.dump.read_text())
        pin = (Path(__file__).resolve().parents[2] / ".exiftool-version").read_text().strip()
        if data.get("exiftool_version") != pin:
            raise Unsupported(f"dump version {data.get('exiftool_version')!r} != pinned {pin}")
        text, counts = render(data)
        # Encode before opening: malformed Unicode cannot truncate an old file.
        payload = text.encode("utf-8")
        args.output.write_bytes(payload)
        print("gen_nikon_settings_tables: " + json.dumps(counts, sort_keys=True), file=sys.stderr)
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(1, f"gen_nikon_settings_tables: {error}\n")


if __name__ == "__main__":
    main()
