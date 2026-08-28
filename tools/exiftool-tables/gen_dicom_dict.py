#!/usr/bin/env python3
"""Transcribe ExifTool's DICOM tag dictionary into Rust.

Reads ``%Image::ExifTool::DICOM::Main`` and ``%uid`` from the pinned tree's
``lib/Image/ExifTool/DICOM.pm`` and writes
``src/parsers/specialized/dicom_dict.rs``.

Doctrine (AGENTS.md): never approximate a conversion. This script therefore
parses ONLY the exact entry shapes that exist in the pinned 13.59 table and
aborts loudly on anything else, rather than guessing at Perl it does not
model:

* ``'GGGG,EEEE' => { VR => 'XX', Name => '...' },`` with the two optional
  attributes that appear in 13.59 -- ``Binary => 1`` and the single inline
  ``PrintConv => { 0 => 'Unsigned', 1 => 'Signed' }`` on PixelRepresentation;
* ``'GGGG,EEEE' => 'Name',`` (the three FFFE item/delimiter entries).

Keys keep ExifTool's literal spelling, including the wildcard 'x' digits
('7Fxx,0010', '1010,xxxx', ...): ProcessDICOM matches those by substituting
into the formatted tag string, and the Rust lookup mirrors that, so the keys
must survive verbatim.

That verbatim rule extends to the three keys 13.59 spells with LOWERCASE hex
('0043,106f', '0074,100a', '0074,100c'): ProcessDICOM formats every lookup
key with ``sprintf('%.4X,%.4X')`` (uppercase) against a case-sensitive Perl
hash, so those entries are unreachable in ExifTool itself -- byte-verified:
the pinned oracle reports nothing for element (0043,106F) without ``-u``,
and generic ``DICOM_0043_106F`` with it, never ScannerTableEntry. They are
transcribed as-is, and stay equally dead under the Rust lookup, which
formats keys the same way.

Duplicate keys follow Perl hash-literal semantics: the later entry wins
(13.59 carries five such duplicates, e.g. '0021,1019').

Usage:
    python3 tools/exiftool-tables/gen_dicom_dict.py \
        [--exiftool-dir /tmp/oxidex-exiftool-cache/exiftool] \
        [--out src/parsers/specialized/dicom_dict.rs]
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]

# Key digits: hex in either case plus the wildcard 'x'. Lowercase hex occurs
# in exactly three 13.59 keys, dead in ExifTool itself (see the module
# docstring) but transcribed verbatim.
ENTRY_FULL = re.compile(
    r"^\s*'(?P<key>[0-9A-Fa-fx]{4},[0-9A-Fa-fx]{4})' => \{ "
    r"VR => '(?P<vr>[A-Z]{2})', Name => '(?P<name>[^'\\]+)'"
    r"(?P<extra>, PrintConv => \{ 0 => 'Unsigned', 1 => 'Signed' \}|, Binary => 1)?"
    r" \},?\s*(#.*)?$"
)
ENTRY_BARE = re.compile(
    r"^\s*'(?P<key>[0-9A-Fa-fx]{4},[0-9A-Fa-fx]{4})' => '(?P<name>[^'\\]+)',\s*(#.*)?$"
)
ENTRY_ANY = re.compile(r"^\s*'[0-9A-Fa-fx]{4},[0-9A-Fa-fx]{4}' =>")
UID_LINE = re.compile(r"^\s*'(?P<key>[^'\\]+)' => '(?P<name>[^'\\]+)',\s*(#.*)?$")


def rust_str(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def parse(pm_path: Path):
    lines = pm_path.read_text().split("\n")
    main_start = next(
        i for i, l in enumerate(lines) if l.startswith("%Image::ExifTool::DICOM::Main")
    )
    uid_start = next(i for i, l in enumerate(lines) if l.startswith("%uid"))
    uid_end = next(i for i in range(uid_start, len(lines)) if lines[i].strip() == ");")

    main: dict[str, tuple[str, str | None, bool, bool]] = {}
    for line in lines[main_start + 1 : uid_start]:
        if not ENTRY_ANY.match(line):
            continue  # GROUPS/VARS/NOTES scaffolding, comments, blank lines
        m = ENTRY_FULL.match(line)
        if m:
            extra = m.group("extra") or ""
            main[m.group("key")] = (
                m.group("name"),
                m.group("vr"),
                "Binary" in extra,
                "PrintConv" in extra,
            )
            continue
        m = ENTRY_BARE.match(line)
        if m:
            main[m.group("key")] = (m.group("name"), None, False, False)
            continue
        sys.exit(f"unmodeled Main entry (refusing to guess): {line!r}")

    uid: dict[str, str] = {}
    for line in lines[uid_start + 1 : uid_end]:
        if not line.strip() or line.strip().startswith("#"):
            continue
        m = UID_LINE.match(line)
        if not m:
            sys.exit(f"unmodeled %uid entry (refusing to guess): {line!r}")
        value = m.group("name")
        if value in ("", "0"):
            # ProcessDICOM gates on Perl truthiness ($uid{$val}); a falsy
            # name would silently disable the conversion and the Rust lookup
            # (a plain map) would not reproduce that.
            sys.exit(f"falsy %uid value would change semantics: {line!r}")
        uid[m.group("key")] = value
    return main, uid


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--exiftool-dir", default="/tmp/oxidex-exiftool-cache/exiftool")
    ap.add_argument(
        "--out", default=str(REPO_ROOT / "src/parsers/specialized/dicom_dict.rs")
    )
    args = ap.parse_args()

    exiftool_dir = Path(args.exiftool_dir)
    pin = (REPO_ROOT / ".exiftool-version").read_text().strip()
    cache_pin_file = exiftool_dir.parent / ".exiftool-version"
    if cache_pin_file.exists():
        cache_pin = cache_pin_file.read_text().strip()
        if cache_pin != pin:
            sys.exit(f"version skew: repo pins {pin}, {cache_pin_file} says {cache_pin}")
    pm = exiftool_dir / "lib/Image/ExifTool/DICOM.pm"
    main_tbl, uid = parse(pm)

    out = []
    out.append("//! DICOM tag dictionary and registered-UID names, transcribed from the")
    out.append(f"//! pinned ExifTool {pin} (`lib/Image/ExifTool/DICOM.pm`:")
    out.append("//! `%Image::ExifTool::DICOM::Main` and `%uid`).")
    out.append("//!")
    out.append("//! GENERATED by `tools/exiftool-tables/gen_dicom_dict.py` -- do not edit")
    out.append("//! by hand; re-run the generator against the pinned tree instead.")
    out.append("")
    out.append("/// One `%Image::ExifTool::DICOM::Main` entry.")
    out.append("pub(crate) struct DicomDictEntry {")
    out.append("    pub(crate) name: &'static str,")
    out.append("    /// `VR => '..'`; `None` for the three bare FFFE item/delimiter")
    out.append("    /// entries, which carry a name only.")
    out.append("    pub(crate) vr: Option<[u8; 2]>,")
    out.append("    /// `Binary => 1`")
    out.append("    pub(crate) binary: bool,")
    out.append("    /// The single inline `PrintConv => { 0 => 'Unsigned', 1 => 'Signed' }`")
    out.append("    /// (PixelRepresentation).")
    out.append("    pub(crate) unsigned_signed: bool,")
    out.append("}")
    out.append("")
    out.append("const fn e(name: &'static str, vr: &'static [u8; 2]) -> DicomDictEntry {")
    out.append("    DicomDictEntry {")
    out.append("        name,")
    out.append("        vr: Some(*vr),")
    out.append("        binary: false,")
    out.append("        unsigned_signed: false,")
    out.append("    }")
    out.append("}")
    out.append("")
    out.append("const fn eb(name: &'static str, vr: &'static [u8; 2]) -> DicomDictEntry {")
    out.append("    DicomDictEntry {")
    out.append("        name,")
    out.append("        vr: Some(*vr),")
    out.append("        binary: true,")
    out.append("        unsigned_signed: false,")
    out.append("    }")
    out.append("}")
    out.append("")
    out.append("const fn ep(name: &'static str, vr: &'static [u8; 2]) -> DicomDictEntry {")
    out.append("    DicomDictEntry {")
    out.append("        name,")
    out.append("        vr: Some(*vr),")
    out.append("        binary: false,")
    out.append("        unsigned_signed: true,")
    out.append("    }")
    out.append("}")
    out.append("")
    out.append("const fn bare(name: &'static str) -> DicomDictEntry {")
    out.append("    DicomDictEntry {")
    out.append("        name,")
    out.append("        vr: None,")
    out.append("        binary: false,")
    out.append("        unsigned_signed: false,")
    out.append("    }")
    out.append("}")
    out.append("")
    out.append("/// Sorted by key (ASCII byte order) for binary search. Keys are ExifTool's")
    out.append("/// literal spellings, wildcard `x` digits included -- and including the")
    out.append("/// three LOWERCASE-hex keys ('0043,106f', '0074,100a', '0074,100c'),")
    out.append("/// which are unreachable in ExifTool itself (ProcessDICOM formats every")
    out.append("/// lookup key with uppercase `'%.4X,%.4X'` against a case-sensitive Perl")
    out.append("/// hash) and stay equally unreachable here.")
    out.append("#[rustfmt::skip]")
    out.append("pub(crate) static DICOM_MAIN: &[(&str, DicomDictEntry)] = &[")
    for key in sorted(main_tbl):
        name, vr, binary, us = main_tbl[key]
        if vr is None:
            ctor = f"bare({rust_str(name)})"
        elif us:
            ctor = f"ep({rust_str(name)}, b\"{vr}\")"
        elif binary:
            ctor = f"eb({rust_str(name)}, b\"{vr}\")"
        else:
            ctor = f"e({rust_str(name)}, b\"{vr}\")"
        out.append(f"    ({rust_str(key)}, {ctor}),")
    out.append("];")
    out.append("")
    out.append("/// `%uid`: registered DICOM UIDs, applied as the PrintConv for UI values")
    out.append("/// that appear in this map (`$$tagInfo{PrintConv} = \\%uid if $uid{$val}`).")
    out.append("/// Sorted by key for binary search. Names keep ExifTool's exact bytes,")
    out.append("/// trailing spaces included.")
    out.append("#[rustfmt::skip]")
    out.append("pub(crate) static DICOM_UID: &[(&str, &str)] = &[")
    for key in sorted(uid):
        out.append(f"    ({rust_str(key)}, {rust_str(uid[key])}),")
    out.append("];")
    out.append("")
    out.append("/// Exact-key lookup in the transcribed Main table. Wildcard matching is")
    out.append("/// the caller's job (it mirrors ProcessDICOM's five substitutions).")
    out.append("pub(crate) fn dicom_main_entry(key: &str) -> Option<&'static DicomDictEntry> {")
    out.append("    DICOM_MAIN")
    out.append("        .binary_search_by_key(&key, |&(k, _)| k)")
    out.append("        .ok()")
    out.append("        .map(|index| &DICOM_MAIN[index].1)")
    out.append("}")
    out.append("")
    out.append("/// Registered-UID name, or `None` for an unregistered UID (which ExifTool")
    out.append("/// reports verbatim -- there is no `Unknown (...)` fallback for UIDs).")
    out.append("pub(crate) fn dicom_uid_name(uid: &str) -> Option<&'static str> {")
    out.append("    DICOM_UID")
    out.append("        .binary_search_by_key(&uid, |&(k, _)| k)")
    out.append("        .ok()")
    out.append("        .map(|index| DICOM_UID[index].1)")
    out.append("}")
    out.append("")

    Path(args.out).write_text("\n".join(out))
    print(
        f"wrote {args.out}: {len(main_tbl)} Main entries "
        f"(from {pm}), {len(uid)} UIDs"
    )


if __name__ == "__main__":
    main()
