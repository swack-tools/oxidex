#!/usr/bin/env python3
"""Differential oracle for the generated conversion arms (Autogeneration v2).

`conv_codegen.py` compiles each field of a table (`Exif::Main` first) from
ExifTool's own conversion source into a Rust arm
(`src/exiftool_tables/conv/exif_main.rs`). This script proves those arms
against the pinned ExifTool:

1. For every GENERATED field listed in the committed ledger
   (`conv_exif_main_ledger.json`) it builds a probe battery -- integers,
   rational strings as `ReadValue` renders them (`RoundFloat`, `inf`,
   `undef`), space-joined multi-value strings, text, byte strings with NULs
   and non-UTF-8 bytes, every hash key the field's conversions carry, and
   each byte order.
2. `conv_oracle.pl` runs each probe through the pinned tree's OWN pipeline
   -- `FoundTag` (RawConv, data members) then `GetValue` in ValueConv and
   PrintConv modes -- and records Perl's stringified bytes.
3. The capture is committed (`testdata/conv_exif_main_outputs.json`); the
   Rust test `conv::tests::every_generated_arm_matches_the_pinned_perl_capture`
   replays each probe through the arm and requires the same bytes (or an
   explicit decline, counted). `--check` re-runs the pinned Perl and requires
   the committed capture to reproduce byte for byte.

Instrument (AGENTS.md): pinned perl 5.38.2 (`$EXIFTOOL_PERL`), the pinned
tree (`$OXIDEX_PINNED_EXIFTOOL`), asserted with `-ver` against
`.exiftool-version` and the OOXML.docx capability probe before any number;
TZ=UTC.

    python3 tools/exiftool-tables/conv_oracle.py --write
    python3 tools/exiftool-tables/conv_oracle.py --check
"""
import argparse
import json
import os
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(HERE))
import helper_oracle as HO  # noqa: E402  (instrument(), oracle_env())

HARNESS = HERE / "conv_oracle.pl"
TABLES = {
    "Exif::Main": (HERE / "conv_exif_main_ledger.json",
                   HERE / "testdata" / "conv_exif_main_outputs.json"),
}


def S(b):
    if isinstance(b, str):
        b = b.encode("utf-8")
    return {"t": "s", "hex": b.hex()}


def I(v):
    return {"t": "i", "v": str(v)}


def F(v):
    return {"t": "f", "v": v}


INTS = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 16, 17, 24, 25, 31, 32, 65, 100, 255, 256,
        1000, 32768, 65535, 65536, 4294967295, -1, -2, -32768, 2147483647, -2147483648]
FLOATS = ["0.0", "-0.0", "0.5", "1.5", "2.5", "-2.5", "0.1", "3.14159", "1e-05", "1e+20",
          "-1e+20", "29.97", "12.7"]
# `ReadValue` renderings: a rational64 is `RoundFloat($n/$d, 10)` or
# `inf`/`undef` (ExifTool.pm:6107-6120); a multi-count entry is one
# space-joined string (ExifTool.pm:6330).
NUMSTR = ["0", "1", "2.8", "0.0166666667", "0.004", "1.5", "-1.3333333333", "100", "1e-11",
          "1234567890", "3.3333333333", "inf", "undef", "-0", "0.25", "0.3333333333", "4.5",
          "12.5", "-12.5", "0.0005", "5.6", "8", "1.0e+15", "250", "-7",
          "1 2", "2 1", "1 1", "1 2 3 4", "0 0 0 0", "2 3 1", "3 4", "65535 65535",
          "1.5 2.5 3.5 4.5", "18 55 1.8 2.8", "24 70 2.8 2.8", "0 0 undef inf", "24 24 2.8 2.8",
          "35 35 0 0", "1 0 0 0", "2 2 0 0", "0 1 1 2", "1 2 0 1 2 0 1", "256 1", "5 1"]
TEXT = ["", "Canon", "Canon  ", "  x  ", "abc\t", "a: b", "x: y: z", "0230", "0100",
        "a b  c", "abc\n", "2020:01:02 03:04:05", "2020-01-02T03:04:05+01:00",
        "2020-01-02T03:04:05", "2020-01-02 03:04", "1,5", "N", "inf m", "12abc", "0x1A",
        " 12", "1e5", "Off", "on", "ON", "e", "é", "1 2  3", "0 ", "1.2.3.4",
        "01 02 03 04 05 06 07 08", "9 8 7 6 5 4 3 2 1 0 1 2 3 4 5 6"]
BYTES = [b"0230\0\0", b"\0", b"\0\0\0", b"ab\0\0\n", b"\xff\xfe", b"\x01\x02\x03\x04",
         b"\x03\x00\x00\x00", b"a\0b", b"\x80", b"x" * 40, b"1 " * 40, b"A" * 70, b"\x0b"]

IDENTITY = [I(0), I(7), S("2.8"), S("1 2 3"), S("Canon  "), S(b"\xff\xfe"), F("0.5"),
            S("x" * 40)]

U = {"t": "undef"}


def ucs2(text, order="II"):
    return text.encode("utf-16-le" if order == "II" else "utf-16-be")


# Byte strings for the charset conversions (Decode): UCS-2 in both orders
# with and without a BOM, odd lengths, surrogate pairs and lone halves,
# NUL-terminated and not; UTF-8 valid, lax and malformed; Latin-1.
UCS2_PROBES = [ucs2("Title"), ucs2("Title") + b"\0\0", ucs2("caf\u00e9") + b"\0\0",
               ucs2("\u4e2d\u6587"), ucs2("\U0001F600"), b"\x3d\xd8", b"\x00\xde\x3d\xd8",
               b"\xff\xfe" + ucs2("BOM"), b"\xfe\xff" + ucs2("BOM", "MM"), ucs2("odd") + b"x",
               ucs2("Tag1;Tag2") + b"\0\0", b"\0\0", b"\0", b"\0\0" + ucs2("after"),
               ucs2("MM", "MM"), bytes(range(1, 64)), b"\xe9\x00\xff\x00"]
UTF8_PROBES = [b"caf\xc3\xa9", b"\xe2\x82\xac 5", b"\xf0\x9f\x98\x80", b"\xff\xfe", b"\xc3",
               b"caf\xe9", b"\xed\xa0\x80", b"\xc0\x80", b"a\x80b", b"Profile 1",
               b"\x01\x02\x1f\x7f", b"x" * 70, b"\xc3\xa9" * 40, b"tab\there\n"]


def exif_text(ids=(b"ASCII\0\0\0", b"UNICODE\0", b"JIS\0\0\0\0\0", b"\0" * 8, b" " * 8)):
    out = [b"", b"short", b"1234567", b"ASCII  \nHi", b"ASCII \n Hi", b" " * 7 + b"\ntext",
           b"Ascii\0\0\0x", b"XYZ\0\0\0\0\0abc  ", b"UNICODE\nH\0", b"\xff" * 12]
    for i in ids:
        for body in (b"Hello", b"Hello   ", b"Hello  \n", b"a\0junk ", b"", b"   ",
                     b"caf\xe9", b"caf\xc3\xa9", ucs2("Hi"), ucs2("Hi", "MM"),
                     b"\xff\xfe" + ucs2("Hi"), b"\xfe\xff" + ucs2("Hi", "MM"), ucs2("Hi") + b"x",
                     b"\x30\x21\x24\x22", ucs2("pad  "), b"\x3d\xd8\x00\xde"):
            out.append(i + body)
    return out


def sfr(n, m, names, rats, order):
    import struct
    e = "<" if order == "II" else ">"
    return (struct.pack(e + "HH", n, m) + b"".join(x + b"\0" for x in names)
            + b"".join(struct.pack(e + "II", a, b) for a, b in rats))


def guid(last):
    # 6fddc324-4e03-4bfe-b185-3d77768dc9XX as GetGUID reads it (VvvNN)
    import struct
    return (struct.pack("<IHH", 0x6fddc324, 0x4e03, 0x4bfe)
            + bytes.fromhex("b1853d77768dc9") + bytes([last]))


# Field-specific probes: the byte strings each field's conversion actually
# branches on, beyond the shared battery. Keyed by tag name. A tuple entry
# `(bytes, options)` also sets `$$self{OPTIONS}`.
EXTRA = {}
for _n in ("XPTitle", "XPComment", "XPAuthor", "XPKeywords", "XPSubject", "XP_DIP_XML"):
    EXTRA[_n] = UCS2_PROBES + [(ucs2("caf\u00e9"), {"Charset": S("Latin")}),
                               (ucs2("\u4e2d"), {"Charset": S("Latin")})]
for _n in ("DevelopmentTypeDescription", "LocalizedCameraModel", "OriginalRawFileName",
           "PanasonicTitle", "PanasonicTitle2", "CameraCalibrationSig", "ProfileCalibrationSig",
           "AsShotProfileName", "ProfileName", "ProfileCopyright", "PreviewApplicationName",
           "PreviewApplicationVersion", "PreviewSettingsName"):
    EXTRA[_n] = UTF8_PROBES + [(b"caf\xc3\xa9", {"Charset": S("Latin")}),
                               (b"\xe4\xb8\xad", {"Charset": S("Latin")}),
                               (b"\xc3", {"Charset": S("Latin")}),
                               (b"caf\xe9", {"Charset": S("UCS2")})]
EXTRA["LocalizedCameraModel"] = EXTRA["LocalizedCameraModel"] + [
    (b"y" * 30, {"Verbose": I(4)}), (b"z" * 3000, {"Verbose": I(4)})]
EXTRA["UserComment"] = exif_text() + [
    (b"ASCII\0\0\0caf\xe9 ", {"CharsetEXIF": S("Latin")}),
    (b"ASCII\0\0\0caf\xe9 ", {"CharsetEXIF": S("0")}),
    (b"UNICODE\0H\0\xe9\0", {"Charset": S("Latin")})]
# (Validate is not probed here: it makes FoundTag run Validate::ValidateRaw,
# engine validation outside the conversion; helper_oracle probes the port's
# refusal of it.)
EXTRA["CFAPattern"] = [
    b"\x02\x00\x02\x00\x00\x01\x01\x02", b"\x00\x02\x00\x02\x00\x01\x01\x02",
    b"\x02\x00\x02\x00\x00\x01\x01", b"\x03\x00\x03\x00\x01", b"\x01\x00\x01\x00",
    b"\x00\x00\x00\x00", b"\xff\xff\xff\xff\x01", b"\x00\x01\x00\x03\x00\x01\x02",
    b"\x01\x00\x03\x00\x00\x01\x02", b"0112", b"0112\n", b"01", b"7", b"0126", b"01122",
    b"\x01\x02\x03", b"\x06\x00\x06\x00" + bytes(range(36)), b"\x00\x06\x00\x06" + bytes(range(36)),
    b"\x02\x00\x02\x00\x00\x01\x01\x09", b"\x02\x00\x03\x00\x00\x01\x02\x03\x04\x05",
    b"\x01\x00\x02\x00\x00\x01", b"\x02\x00\x01\x00\x00\x01"]
EXTRA["SpatialFrequencyResponse"] = [
    v for o in ("II", "MM") for v in (
        sfr(2, 2, [b"H", b"V"], [(1, 2), (3, 4), (5, 0), (0, 0)], o),
        sfr(1, 3, [b"Col"], [(1, 3), (2, 3), (10, 1)], o), sfr(0, 0, [], [], o) + b"x",
        sfr(2, 0, [b"a", b"b"], [], o), sfr(2, 1, [b"a"], [(1, 1), (2, 2)], o),
        sfr(2, 1, [b"a", b"b", b"c"], [(1, 1), (2, 2)], o),
        sfr(3, 1, [b"", b"x", b""], [(7, 1), (4294967295, 3), (1, 4294967295)], o),
        sfr(2, 2, [b"H", b"V"], [(1, 2)], o), sfr(1, 1, [b"caf\xe9"], [(1, 3)], o))
] + [b"\x01\x00\x01\x00", b"\x01\x00\x01\x00\x00", b"abcde"]
EXTRA["PixelFormat"] = [guid(k) for k in (0x05, 0x08, 0x0d, 0x10, 0x3f, 0x00, 0xff, 0x0a)] + [
    bytes(range(16)), bytes(range(15)), b"\xff" * 16, b"0123456789abcdef"]
EXTRA["Copyright"] = [
    b"a\0b", b"Photographer \0Editor", b"x \0", b"a\0", b"a\n", b"a\n\n", b"a \0 b\0c",
    b"\0", b"abc", b"a\0b\0", b"a  \0\0", b" \0b", b"a\n\0", b"\0\n", b"a\0\n", b"caf\xe9\0x",
    (b"caf\xe9\0x", {"CharsetEXIF": S("Latin")}), (b"caf\xe9", {"CharsetEXIF": S("Latin")}),
    (b"a\0b", {"CharsetEXIF": S("UTF8")}), (b"\x80\0", {"CharsetEXIF": S("Bogus")})]
for _n in ("OpcodeList1", "OpcodeList2", "OpcodeList3"):
    EXTRA[_n] = [b"\0\0\0\x01" + b"\0\0\0\x01" + b"\0" * 12, b"\0\0\0\0", b"\xff" * 40]


def battery():
    return ([I(v) for v in INTS] + [F(v) for v in FLOATS] + [S(v) for v in NUMSTR]
            + [S(v) for v in TEXT] + [S(v) for v in BYTES])


def key_probe(k):
    """A hash key as the walker would hand it over: an integer when it is
    one, else the string."""
    if k.lstrip("-").isdigit() and k == str(int(k)) and -2**63 <= int(k) < 2**63:
        return I(int(k))
    return S(k)


def cases_for(field):
    tid = int(field["id"], 16)
    slots = field["slots"]
    identity = not slots and "Binary" not in field["flags"]
    if identity:
        vals = IDENTITY
    elif set(slots.values()) <= {"hash", "list"}:
        vals = ([key_probe(k) for k in field["hash_keys"]] + [S(k) for k in field["hash_keys"]]
                + [I(v) for v in INTS[:12] + [65535, -1, 4294967295]]
                + [S(v) for v in NUMSTR[:6] + NUMSTR[25:31]] + [S(v) for v in TEXT[:4]]
                + [S(b) for b in BYTES[:3]])
    else:
        vals = battery() + [key_probe(k) for k in field["hash_keys"]]
    extra = EXTRA.get(field["name"], [])
    vals = vals + [S(e) for e in extra if not isinstance(e, tuple)] + ([U] if extra else [])
    out, seen = [], set()
    for v in vals:
        k = json.dumps(v, sort_keys=True)
        if k in seen:
            continue
        seen.add(k)
        out.append({"id": tid, "val": v, "byte_order": "II", "members": {}})
    if not identity:
        mm = vals[:8] + [S(e) for e in extra if not isinstance(e, tuple)]
        seen = set()
        for v in mm:
            k = json.dumps(v, sort_keys=True)
            if k not in seen:
                seen.add(k)
                out.append({"id": tid, "val": v, "byte_order": "MM", "members": {}})
    for e in extra:
        if isinstance(e, tuple):
            for bo in ("II", "MM"):
                out.append({"id": tid, "val": S(e[0]), "byte_order": bo, "members": {},
                            "options": e[1]})
    return out


def run_perl(perl, et_lib, table, payload):
    import subprocess
    proc = subprocess.run([perl, str(HARNESS), str(et_lib), table], input=json.dumps(payload),
                          capture_output=True, text=True, env=HO.oracle_env())
    if proc.returncode != 0:
        sys.exit(f"conv_oracle.pl failed:\n{proc.stderr}")
    return json.loads(proc.stdout)


def build(perl, et_dir, table):
    pv, ver = HO.instrument(perl, et_dir)
    ledger_path, _capture = TABLES[table]
    led = json.loads(ledger_path.read_text(encoding="utf-8"))
    if led["exiftool_version"] != ver:
        sys.exit(f"ledger generated from {led['exiftool_version']}, oracle is {ver}")
    cases = []
    for field in led["generated"]:
        cases += cases_for(field)
    results = run_perl(perl, Path(et_dir) / "lib", table, cases)
    fields = {}
    names = {int(f["id"], 16): f["name"] for f in led["generated"]}
    for c, r in zip(cases, results):
        f = fields.setdefault(f"0x{c['id']:04x}", {"name": names[c["id"]], "cases": []})
        entry = {"val": c["val"], "byte_order": c["byte_order"], "members": c["members"]}
        if c.get("options"):
            entry["options"] = c["options"]
        f["cases"].append({**entry, **r})
    return {
        "capture": {
            "tool": "tools/exiftool-tables/conv_oracle.py", "table": table,
            "perl": perl, "perl_version": pv, "exiftool_version": ver, "tz": "UTC",
            "ledger_rust_sha256": led["rust_sha256"],
            "note": "each case runs twice on fresh ExifTool objects with `members` and "
                    "`options` set and SetByteOrder(byte_order): FoundTag($tagInfo, val) then "
                    "GetValue(key, 'ValueConv') gives `vc`; FoundTag then GetValue(key, "
                    "'PrintConv') gives `pc`, and that run's member changes (`set`, "
                    "`deleted` for a removed one) and Warn requests (`warnings`). Outputs "
                    "are Perl's stringified bytes (hex), `ref` a SCALAR reference's referent, "
                    "`u8` a character string",
        },
        "fields": fields,
    }


def render(capture):
    lines = ["{", '"capture": ' + json.dumps(capture["capture"], sort_keys=True) + ",",
             '"fields": {']
    ids = sorted(capture["fields"])
    for i, fid in enumerate(ids):
        f = capture["fields"][fid]
        lines.append(json.dumps(fid) + ": {" + '"name": ' + json.dumps(f["name"]) + ', "cases": [')
        cs = f["cases"]
        lines += [json.dumps(c, sort_keys=True) + ("," if j < len(cs) - 1 else "")
                  for j, c in enumerate(cs)]
        lines.append("]}" + ("," if i < len(ids) - 1 else ""))
    lines += ["}", "}"]
    text = "\n".join(lines) + "\n"
    assert json.loads(text) == capture
    return text


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    ap.add_argument("--table", default="Exif::Main", choices=sorted(TABLES))
    ap.add_argument("--perl", default=os.environ.get("EXIFTOOL_PERL"))
    ap.add_argument("--exiftool-dir", default=os.environ.get("OXIDEX_PINNED_EXIFTOOL"))
    args = ap.parse_args()
    if not args.perl or not args.exiftool_dir:
        sys.exit("need --perl/--exiftool-dir (or EXIFTOOL_PERL/OXIDEX_PINNED_EXIFTOOL)")
    capture = build(args.perl, args.exiftool_dir, args.table)
    text = render(capture)
    path = TABLES[args.table][1]
    n = sum(len(f["cases"]) for f in capture["fields"].values())
    if args.write:
        path.write_text(text, encoding="utf-8")
        print(f"wrote {path.relative_to(REPO)}: {len(capture['fields'])} fields, {n} cases")
        return 0
    if path.read_text(encoding="utf-8") != text:
        print(f"MISMATCH: re-running the pinned Perl does not reproduce {path.relative_to(REPO)}",
              file=sys.stderr)
        return 1
    print(f"PASS: pinned Perl reproduces {path.relative_to(REPO)} byte for byte "
          f"({len(capture['fields'])} fields, {n} cases)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
