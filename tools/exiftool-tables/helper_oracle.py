#!/usr/bin/env python3
"""Differential oracle for the Autogeneration v2 helper library.

`src/exiftool_tables/helpers.rs` ports ExifTool helper subs (the ones table
expressions call: `ConvertDateTime`, `Exif::ConvertFraction`, `GPS::ToDMS`,
...) once each, over Perl scalars and a `Session`. A port is admitted by two
proofs, both made here against the PINNED tree and interpreter only:

1. **Source identity.** The sub's text in the pinned tree, comments and
   whitespace folded (the same fold #805/#818 use to select a ConvertUnixTime
   or AF-point port), must hash to the sha256 recorded in `HELPERS` below --
   and in the Rust `PORTS` table, which the cargo test cross-checks against
   the capture. A tree whose sub differs selects nothing: the port is then
   unproven for that tree, never assumed.
2. **Behaviour.** `helper_oracle.pl` calls the pinned sub itself on every
   probe (0, negatives, undef, huge, non-numeric strings, rationals, the
   helper's own boundary values, and each option branch the port claims or
   refuses) and records Perl's stringified result as bytes. The Rust test
   `helpers::tests::every_port_matches_the_pinned_perl_capture` replays each
   probe through the port and requires the SAME bytes -- or an explicit
   refusal, which is counted and must be one the port documents. A port
   that returns a different answer fails the build.

The capture is committed (`testdata/helper_oracle_outputs.json`) so cargo
can check it without Perl; `--check` re-runs the pinned Perl and requires the
committed capture to reproduce byte for byte, which is how the capture
itself is proven rather than trusted.

Instrument (AGENTS.md): pinned perl 5.38.2 (`$EXIFTOOL_PERL`), the pinned
ExifTool tree (`$OXIDEX_PINNED_EXIFTOOL`), asserted with `-ver` against
`.exiftool-version` and the OOXML.docx capability probe (-> DOCX) before any
number is produced; TZ=UTC for both sides.

    python3 tools/exiftool-tables/helper_oracle.py --write   # regenerate
    python3 tools/exiftool-tables/helper_oracle.py --check   # prove it
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

# The fold #805/#818 select ConvertUnixTime and AF-point ports with.
from exprs import sub_source
import codegen_charsets
import conv_codegen

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
HARNESS = HERE / "helper_oracle.pl"
CAPTURE = HERE / "testdata" / "helper_oracle_outputs.json"
PINNED_PERL_VERSION = "v5.38.2"

# Exif::Main conversion rows kept on the hand residual path. A residual is
# admitted only when dump_tables.pl projects this exact source body hash, and
# every probe below is executed by the pinned table through FoundTag/GetValue.
RESIDUAL_PORTS = {
    "0x8298": "038cd9fc244cc07f43a2047668344ca425ffa7c0010a1540f91e22ed01ddc9cd",
    "0x9287": "e6a9f51b8f8ab554eeaa6e18c266064605a5b859f2785bfd23cad0887f351d7c",
    "0xa462": "c0bc35f4a1d0bdd77a22eb4038e87ba9ed0d614d79aa28aec2df1424540ff953",
    "0xc740": "45bb086b3a8fc85c1f18f55858f8abb4a600ce2dfb48b7d4af61096b7e1eea1f",
    "0xc741": "45bb086b3a8fc85c1f18f55858f8abb4a600ce2dfb48b7d4af61096b7e1eea1f",
    "0xc74e": "45bb086b3a8fc85c1f18f55858f8abb4a600ce2dfb48b7d4af61096b7e1eea1f",
    "0xc763": "0bf58b0a75d26b1c3f205392918e8c50e13f114864d7b6251f4b27c783ad12c5",
}

# ---------------------------------------------------------------------------
# The registry. Order and use counts are the spike's (PR #817,
# tools/exiftool-tables/spike/COVERAGE.md section 5, Frame B, 13.59 dump
# sha256 536386691b0d...): the build order comes from that curve, not from
# preference. The folded source's sha256 is not repeated here: the capture
# records what the pinned tree carries, and the Rust `PORTS` table must name
# the same digest (cargo test), so a port is tied to the exact sub it was
# proven against.
# ---------------------------------------------------------------------------
PORTED = "ported"
REFUSED = "refused"

# Subs Decode reaches beyond its own body ("<module>::<sub>"); their folded
# sources are captured beside Decode's and named by the Rust
# `helpers::DECODE_DEPENDENCIES`.
DECODE_DEPS = ["Image/ExifTool/Charset.pm::Decompose", "Image/ExifTool/Charset.pm::Recompose",
               "Image/ExifTool/Charset.pm::LoadCharset"]

HELPERS = [
    # rank, spike name, qualified Perl sub, module file, spike uses, status, note
    dict(rank=1, spike="ET->ConvertDateTime", perl="Image::ExifTool::ConvertDateTime",
         module="Image/ExifTool.pm", uses=403, status=PORTED,
         note="identity unless OPTIONS DateFormat or GlobalTimeShift is Perl-true; "
              "those branches (strftime, ShiftTime) refuse"),
    dict(rank=2, spike="Exif::ConvertFraction", perl="Image::ExifTool::Exif::ConvertFraction",
         module="Image/ExifTool/Exif.pm", uses=191, status=PORTED, note="complete"),
    dict(rank=3, spike="GPS::ToDMS", perl="Image::ExifTool::GPS::ToDMS",
         module="Image/ExifTool/GPS.pm", uses=179, status=PORTED,
         note="complete except doPrintConv '1' with a Perl-true OPTIONS CoordFormat "
              "(user sprintf format), which refuses"),
    dict(rank=4, spike="Exif::PrintExposureTime", perl="Image::ExifTool::Exif::PrintExposureTime",
         module="Image/ExifTool/Exif.pm", uses=168, status=PORTED,
         note="complete; numeric core is the existing core::formatters port"),
    dict(rank=5, spike="ConvertUnixTime", perl="Image::ExifTool::ConvertUnixTime",
         module="Image/ExifTool.pm", uses=157, status=PORTED,
         note="complete for an effective $dec of 0 (no $dec argument, SystemTimeRes "
              "Perl-false) incl. KeepUTCTime; sub-second $dec refuses"),
    dict(rank=6, spike="ET->InverseDateTime", perl="Image::ExifTool::InverseDateTime",
         module="Image/ExifTool/Writer.pl", uses=90, status=REFUSED,
         note="write-side inverse; reads DateFormat/StrictDate and calls "
              "POSIX::strptime-style parsing and Time::Local -- not ported"),
    dict(rank=7, spike="ConvertDuration", perl="Image::ExifTool::ConvertDuration",
         module="Image/ExifTool.pm", uses=89, status=PORTED,
         note="complete; numeric core is the existing core::formatters port"),
    dict(rank=8, spike="IsFloat", perl="Image::ExifTool::IsFloat",
         module="Image/ExifTool.pm", uses=75, status=PORTED,
         note="complete, including the in-place `tr/,/./` of its argument"),
    dict(rank=9, spike="ET->Decode", perl="Image::ExifTool::Decode",
         module="Image/ExifTool.pm", uses=64, status=PORTED,
         deps=DECODE_DEPS,
         note="complete over byte strings (MemberVal::Bytes) for every %csType charset, "
              "source and destination, incl. BOMs, 'Unknown' byte-order guessing, UTF-16 "
              "surrogates, Perl's lax UTF-8 and its malformations, and the DecodeWarn/"
              "WarnBadUTF8/WrongByteOrder/EncodingError members and Warn requests; a "
              "2-/4-byte charset needing GetByteOrder() with no Session byte order refuses"),
    dict(rank=10, spike="ConvertBitrate", perl="Image::ExifTool::ConvertBitrate",
         module="Image/ExifTool.pm", uses=41, status=PORTED,
         note="complete; numeric core is the existing core::formatters port"),
    dict(rank=11, spike="Exif::PrintFraction", perl="Image::ExifTool::Exif::PrintFraction",
         module="Image/ExifTool/Exif.pm", uses=40, status=PORTED, note="complete"),
    dict(rank=12, spike="GPS::ToDegrees", perl="Image::ExifTool::GPS::ToDegrees",
         module="Image/ExifTool/GPS.pm", uses=40, status=PORTED, note="complete"),
    dict(rank=13, spike="Canon::CanonEv", perl="Image::ExifTool::Canon::CanonEv",
         module="Image/ExifTool/Canon.pm", uses=37, status=PORTED,
         note="complete; numeric core is the existing exprs::canon_ev port"),
    dict(rank=14, spike="Canon::CanonEvInv", perl="Image::ExifTool::Canon::CanonEvInv",
         module="Image/ExifTool/Canon.pm", uses=37, status=PORTED, note="complete"),
    dict(rank=15, spike="GetUnixTime", perl="Image::ExifTool::GetUnixTime",
         module="Image/ExifTool.pm", uses=34, status=PORTED,
         note="UTC branch (no $isLocal, '2', or an explicit Z/+-HH:MM zone) for years "
              "1000-9999 with in-range fields; local time and Time::Local's croak/"
              "two-digit-year window refuse"),
    dict(rank=16, spike="Exif::PrintFNumber", perl="Image::ExifTool::Exif::PrintFNumber",
         module="Image/ExifTool/Exif.pm", uses=32, status=PORTED, note="complete"),
    dict(rank=17, spike="ET->ValidateImage", perl="Image::ExifTool::ValidateImage",
         module="Image/ExifTool.pm", uses=31, status=REFUSED,
         note="mutates its image argument and issues Warn gated on REQ_TAG_LOOKUP; "
              "needs the session warning sink (next slice)"),
    dict(rank=18, spike="ET->Warn", perl="Image::ExifTool::Warn",
         module="Image/ExifTool.pm", uses=27, status=REFUSED,
         note="engine side effect (warning list, IgnoreMinorErrors); needs the session "
              "warning sink (next slice)"),
    dict(rank=19, spike="IsInt", perl="Image::ExifTool::IsInt",
         module="Image/ExifTool.pm", uses=27, status=PORTED, note="complete"),
    dict(rank=20, spike="XMP::ConvertXMPDate", perl="Image::ExifTool::XMP::ConvertXMPDate",
         module="Image/ExifTool/XMP.pm", uses=27, status=PORTED,
         note="complete in scalar context (every table call site); the list-context "
              "($val, 1) return is not modelled"),
    dict(rank=21, spike="ET->Encode", perl="Image::ExifTool::Encode",
         module="Image/ExifTool.pm", uses=26, status=PORTED,
         deps=["Image/ExifTool.pm::Decode"] + DECODE_DEPS,
         note="complete: Decode from the Charset option (see Decode)"),
    dict(rank=22, spike="Samsung::Crypt", perl="Image::ExifTool::Samsung::Crypt",
         module="Image/ExifTool/Samsung.pm", uses=25, status=REFUSED,
         note="reads the array-valued EncryptionKey member, $tagInfo and the "
              "module-level %formatMinMax; not modelled on Session yet"),
    # A partial port in exprs.rs completed here because its only option,
    # ByteUnit, is on Session (spike rank > 40; not counted toward the top 22).
    dict(rank=None, spike="ConvertFileSize", perl="Image::ExifTool::ConvertFileSize",
         module="Image/ExifTool.pm", uses=None, status=PORTED,
         note="complete: SI and Binary ByteUnit branches, with or without $et"),
    # The helpers Exif::Main's refused conversions call (#838's REFUSED
    # table). `deps` are the engine subs each port reproduces inline.
    dict(rank=None, spike="Exif::ConvertExifText", perl="Image::ExifTool::Exif::ConvertExifText",
         module="Image/ExifTool/Exif.pm", uses=None, status=PORTED,
         deps=["Image/ExifTool.pm::Options", "Image/ExifTool.pm::Decode"],
         note="complete except a Perl-true Validate option (its extra warnings), which "
              "refuses; Decode as ported"),
    dict(rank=None, spike="Exif::DecodeCFAPattern", perl="Image::ExifTool::Exif::DecodeCFAPattern",
         module="Image/ExifTool/Exif.pm", uses=None, status=PORTED,
         deps=["Image/ExifTool.pm::GetByteOrder"],
         note="complete; GetByteOrder with no Session byte order refuses"),
    dict(rank=None, spike="Exif::PrintCFAPattern", perl="Image::ExifTool::Exif::PrintCFAPattern",
         module="Image/ExifTool/Exif.pm", uses=None, status=PORTED,
         note="complete for canonical-integer fields (every DecodeCFAPattern result); "
              "any other field refuses"),
    dict(rank=None, spike="Exif::PrintSFR", perl="Image::ExifTool::Exif::PrintSFR",
         module="Image/ExifTool/Exif.pm", uses=None, status=PORTED,
         deps=["Image/ExifTool.pm::Get16u", "Image/ExifTool.pm::Get32u",
               "Image/ExifTool.pm::DoUnpackStd", "Image/ExifTool.pm::GetRational64u",
               "Image/ExifTool.pm::RoundFloat"],
         note="complete; GetByteOrder with no Session byte order refuses"),
    dict(rank=None, spike="ASF::GetGUID", perl="Image::ExifTool::ASF::GetGUID",
         module="Image/ExifTool/ASF.pm", uses=None, status=PORTED, note="complete"),
    dict(rank=None, spike="ET->Printable", perl="Image::ExifTool::Printable",
         module="Image/ExifTool.pm", uses=None, status=PORTED,
         note="complete for a plain scalar (a SCALAR reference is never a conversion's "
              "$val); a non-integer $maxLen or Verbose refuses"),
]


def helper_source(module_text, sub):
    """The folded text of `sub`'s DEFINITION. `exprs.sub_source` (the #805/#818
    fold) takes the first `^sub <name>` line through the next `^}`, which is
    exact for the multi-line subs it was written for but wrong for two shapes
    this library meets: a forward declaration (`sub ConvertFileSize($;$);`
    near the top of ExifTool.pm would match first and drag in 70 KB of
    unrelated code) and a one-line sub (`sub IsInt($) { ... }` has no `^}`
    of its own). So: skip declaration lines, take a one-line sub as its own
    line, and otherwise defer to `exprs.sub_source` on the text from the
    definition on -- the same bytes it would fold for any sub it handles."""
    for m in re.finditer(rf"^sub {re.escape(sub)}\b(\s*\([^)]*\))?[ \t]*(.*)$",
                         module_text, re.M):
        rest = m.group(2)
        if rest.startswith(";"):
            continue
        line = m.group(0)
        if "{" in line and line.count("{") == line.count("}"):
            body = re.sub(r"(^|\s)#.*$", r"\1", line)
            return re.sub(r"\s+", " ", body).strip()
        return sub_source(module_text[m.start():], sub)
    return None


def folded(et_lib, module, sub):
    """(folded source, sha256) of `sub` in `module` under `et_lib`."""
    text = (Path(et_lib) / module).read_text(encoding="latin-1")
    src = helper_source(text, sub)
    return src, (hashlib.sha256(src.encode("latin-1")).hexdigest() if src is not None else None)


def pinned_sources(et_lib):
    """{perl name: (folded source, sha256)} for every HELPERS entry, read from
    the tree at `et_lib` (the directory holding `Image/`)."""
    return {h["perl"]: folded(et_lib, h["module"], h["perl"].rsplit("::", 1)[1])
            for h in HELPERS}


def dependency_sources(et_lib):
    """{"<module>::<sub>": sha256} for every dependency a HELPERS entry names."""
    out = {}
    for h in HELPERS:
        for dep in h.get("deps", []):
            module, sub = dep.rsplit("::", 1)
            out[dep] = folded(et_lib, module, sub)[1]
    return out


# ---------------------------------------------------------------------------
# Probe set
# ---------------------------------------------------------------------------

def S(s):
    return {"t": "s", "hex": s.encode("latin-1").hex()}


def I(v):
    return {"t": "i", "v": str(v)}


def F(v):
    return {"t": "f", "v": v}


U = {"t": "undef"}

INTS = [0, 1, -1, 2, 3, 12, 20, 30, 31, 32, 59, 60, 100, 999, 1000, 1999, 2000,
        2047, 2048, 9999, 10000, 10239, 10240, 3599, 3600, 86399, 86400, 90061,
        1234567890, 2147483647, -2147483648, 9007199254740992, 9007199254740993,
        9223372036854775807, -9223372036854775808]
FLOATS = ["0.0", "-0.0", "0.5", "-0.5", "0.25", "0.25001", "0.2500099", "0.3",
          "0.333333333", "0.6666667", "1.5", "2.5", "-2.5", "12.7", "29.97",
          "1e-05", "1e-30", "123456.789", "1e15", "1e18", "1e19", "1e20", "1e300",
          "-1e300", "3.0", "0.1", "0.05", "1.00001", "4.99995", "-12.33", "59.999"]
STRINGS = ["", "0", "0.0", "00", "0E0", "abc", "12abc", " 12", "12 ", "1,5", "-1,5",
           "1e5", "1E-3", ".5", "5.", "+3", "-", ".", "inf", "Inf", "nan", "-inf",
           "0x1A", "3/4", "-3/4", "+3/4", "0/0", "5/0", "-5/0", "1/3", "10/20",
           "1 2 3", "12.5\n", "N", "undef", "7/2 junk", "x 9/3 y", "1,5e3", ",5", "1,",
           "5\n", "1 ", "+.5e-3", "1e", "18446744073709551616", "5/00", "0/00", "-0/7",
           "-0", " -00", "-0.0", "-0abc"]
BATTERY = [I(v) for v in INTS] + [F(v) for v in FLOATS] + [S(s) for s in STRINGS] + [U]

DATES = ["2020:01:02 03:04:05", "", "0000:00:00 00:00:00", "2020:01:02 03:04:05+01:00",
         "2020:01:02 03:04:05Z", "2020:01:02 03:04:05.123", "abc"]


def truthiness_cases():
    vals = [U, S(""), S("0"), S("0.0"), S("00"), S("0E0"), S(" 0"), S("0 "), S("\n"),
            S("0\n"), S("a"), S("-0"), S("+0"), S(".0"), S("0x0"), S("00.0"),
            I(0), I(1), I(-1), F("0.0"), F("-0.0"), F("0.1"), F("nan"), F("inf")]
    return [{"truthy": v} for v in vals]


def Bx(b):
    """A probe byte string given as `bytes`."""
    return {"t": "s", "hex": bytes(b).hex()}


def utf8(cps):
    """Perl's `pack('C0U*', @cps)` for code points up to 0x7FFFFFFF."""
    return b"".join(chr(c).encode("utf-8", "surrogatepass") if c < 0x110000
                    else _utf8_long(c) for c in cps)


def _utf8_long(c):
    n, lead = (4, 0xf0) if c < 0x200000 else (5, 0xf8) if c < 0x4000000 else (6, 0xfc)
    out = [lead | (c >> (6 * (n - 1)))]
    out += [0x80 | ((c >> (6 * k)) & 0x3f) for k in range(n - 2, -1, -1)]
    return bytes(out)


# `%csType` names (the probe set exercises each as a source and destination;
# test_helper_oracle holds this list to the generated CS_TYPE) and names it
# does not hold (the Unsupported-character-set path).
CHARSETS = ["ASCII", "Arabic", "Baltic", "Cyrillic", "DOSCyrillic", "DOSLatin1",
            "DOSLatinUS", "Greek", "Hebrew", "JIS", "Latin", "Latin2", "MacArabic",
            "MacChineseCN", "MacChineseTW", "MacCroatian", "MacCyrillic", "MacGreek",
            "MacHebrew", "MacIceland", "MacJapanese", "MacKorean", "MacLatin2", "MacRSymbol",
            "MacRoman", "MacRomanian", "MacThai", "MacTurkish", "PDFDoc", "ShiftJIS", "Symbol",
            "Thai", "Turkish", "UCS2", "UCS4", "UTF16", "UTF8", "Unicode", "Vietnam"]
NOT_CHARSETS = ["Bogus", "utf8", "RSymbol", "latin1"]
# 2-/4-byte fixed-width sets: their byte order argument matters.
FIXED_MULTI = ["JIS", "Symbol", "UCS2", "UCS4", "UTF16", "Unicode"]

# Byte strings every source charset decodes: ASCII, NUL placement, every
# 1-byte value, UTF-8 (valid, lax and malformed), UCS-2/UTF-16 in both
# orders with and without BOMs, odd lengths, surrogates, UCS-4, JIS and
# Shift-JIS-style double bytes (valid, truncated, bad trail byte).
DECODE_VALUES = [
    b"A", b"Hello, World", b"a\0b", b"\0", b"\0abc", b"\xe9", b"caf\xe9", b"\x80",
    b"\x9f", b"\xa0", b"\xff", b"\x80\x81", bytes(range(1, 256)), b"\xe9\0\xe9",
    b"caf\xc3\xa9", b"\xe2\x82\xac", b"\xf0\x9f\x98\x80", b"\xed\xa0\x80",
    b"\xf4\x90\x80\x80", b"\xfe\x82\x80\x80\x80\x80\x80", b"\xef\xbf\xbf",
    b"\xc3", b"\xc3\x41", b"\xe2\x82", b"\xe2\x82\x41", b"\xc0\x80", b"\xc3\xc3",
    b"\xff" + b"\x80" * 11 + b"\x81", b"\xff\x80\x87" + b"\xbf" * 10,
    b"H\0i\0", b"\0H\0i", b"\xff\xfeH\0i\0", b"\xfe\xff\0H\0i", b"\xff\xfe",
    b"\xfe\xff", b"H\0i", b"H", b"\x3d\xd8\x00\xde", b"\xd8\x3d\xde\x00", b"\x3d\xd8",
    b"\x00\xde\x3d\xd8", b"\xff\xfe\x3d\xd8\x00\xde", b"\xfe\xff\xd8\x3d\xde\x00",
    b"\xd8\x3d\xd8\x3d\xde\x00", b"A\0\0\0B\0\0\0", b"\0\0\0A\0\0\0B",
    b"\0\0\xfe\xff\0\0\0A", b"\xff\xfe\0\0A\0\0\0", b"\xff\xff\xff\xff",
    b"\0\x11\0\0", b"\x7f\xff\xff\xff", b"A\0\0\0B\0", b"\x30\x21", b"\x21\x30",
    b"\x24\x22\x24\x24", b"\x7f\x7f", b"\x30\x21\x7f\x7f\x7f\x7f",
    b"\x21\x30\x7f\x7f\x7f\x7f\x22\x24", b"\x82\xa0", b"\x82", b"\x82\x20",
    b"\x88\x9f", b"\xa6", b"A\x82\xa0B", b"\x81", b"\x81\x81\x82", b"\xa1\xa1\xb0\xa1",
    b"\xa1", b"\xa1\x20", b"\x80\x80\xfd\xfe\xff",
]

# Code point strings every destination is asked to encode, as UTF-8.
RECOMPOSE_VALUES = [
    [0x48, 0x65, 0x6c, 0x6c, 0x6f], list(range(1, 256)), [0xe9, 0, 0xe9], [0x61, 0, 0x62],
    [0x4e2d, 0x6587], [0x3042, 0x3044], [0x1f600], [0x10ffff], [0x10fffe, 0x10000],
    [0xffff], [0xd800], [0xdc00, 0xd800], [0x20ac, 0x2122, 0x152], [0x2018, 0x201c],
    [0x391, 0x3b1, 0x410, 0x5d0, 0x627, 0xe01], [0x80, 0x9f, 0xa0], [0x7f, 0x80],
    [0x2c7, 0x2d8, 0x2dd], [0x3000, 0xff01, 0xff61], [0x200000], [0x7fffffff],
]


def decode_cases(add):
    """Probes for Decode and Encode (added to `cases()`)."""
    import codegen_charsets
    D, E = "Image::ExifTool::Decode", "Image::ExifTool::Encode"
    II = {"byte_order": "II"}
    MM = {"byte_order": "MM"}

    # Decode's own branches: from/to defaulting, eq, empty/undef/numeric
    # values, the ASCII shortcut (no NUL truncation), the unsupported path
    # (once per set), and each Charset option.
    vals = [U, S(""), S("0"), I(5), F("1.5"), S("abc"), S("a\0b"), S("\xe9"),
            S("a\xe9\0\xe9"), S("\xc3\xa9\0x")]
    pairs = [("Latin", "UTF8"), ("UTF8", "UTF8"), ("Latin", "Latin"), ("PDFDoc", "UTF8"),
             ("UCS2", "UTF8"), ("UTF8", "Latin"), ("Bogus", "UTF8"), ("UTF8", "Bogus"),
             ("Latin", "MacJapanese"), ("MacJapanese", "UTF8"), ("UTF8", "UCS2"),
             ("Latin", "ASCII"), ("ASCII", "Latin"), (None, None), ("", "Latin"),
             ("0", None), (None, "Latin"), ("Latin", "0"), ("Bogus", "Bogus2")]
    for opts in [None, {"Charset": S("Latin")}, {"Charset": S("UTF8")}, {"Charset": U},
                 {"Charset": S("Bogus")}, {"Charset": S("UCS2")}]:
        for f, t in pairs:
            for v in vals:
                fa = U if f is None else S(f)
                ta = U if t is None else S(t)
                add(D, [v, fa, U, ta, U], opts, extra=II)
    # members already set: the warning / flag is not repeated
    add(D, [S("x"), S("Bogus")], None, extra={**II, "members": {"DecodeWarnBogus": I(1)}})
    add(D, [S("x"), S("Bogus")], None, extra={**II, "members": {"DecodeWarnBogus": S("0")}})
    add(D, [S("x"), S("UTF8"), U, S("Bogus")], None,
        extra={**II, "members": {"DecodeWarnBogus": I(1)}})
    add(D, [S("\xc3"), S("UTF8"), U, S("Latin")], None,
        extra={**II, "members": {"WarnBadUTF8": I(1)}})
    add(D, [S("\xe4\xb8\xad"), S("UTF8"), U, S("Latin")], None,
        extra={**II, "members": {"EncodingError": I(1)}})
    add(D, [S("\xe4\xb8\xad\xc3"), S("UTF8"), U, S("Latin")], None, extra=II)
    add(D, [S("H\0i\0"), S("UCS2"), S("Unknown")], None,
        extra={**MM, "members": {"WrongByteOrder": I(0)}})
    # GetByteOrder with no byte order on the Session: refused on the Rust side
    for args in ([S("H\0i\0"), S("UCS2")], [S("H\0i\0"), S("UCS2"), S("Unknown")],
                 [S("Hi"), S("UTF8"), U, S("UCS2")], [S("H\0i\0"), S("UCS2"), S("II")],
                 [S("Hi"), S("UTF8"), U, S("UCS2"), S("MM")], [S("caf\xe9"), S("Latin")]):
        add(D, args)

    # Decompose: every source charset (to the UTF8 default) over DECODE_VALUES;
    # the fixed multi-byte sets under every byte-order argument and Session order.
    for cs in CHARSETS + NOT_CHARSETS:
        for v in DECODE_VALUES:
            add(D, [Bx(v), S(cs)], None, extra=II)
    for cs in FIXED_MULTI:
        for v in DECODE_VALUES:
            add(D, [Bx(v), S(cs)], None, extra=MM)
            for order, bo in (("II", MM), ("MM", II), ("Unknown", II), ("Unknown", MM),
                              ("x", MM), ("0", MM)):
                add(D, [Bx(v), S(cs), S(order)], None, extra=bo)

    # Every generated table entry, decoded.
    _, tables, scalars = codegen_charsets.generated_tables()
    for cs, (ty, entries) in sorted(tables.items()):
        if ty & 0x600:
            for order in ("MM", "II"):
                units = sorted(entries)
                data = b"".join(k.to_bytes(2, "big" if order == "MM" else "little")
                                for k in units)
                add(D, [Bx(data), S(cs), S(order)], None, extra=II)
        elif ty & 0x800:
            data = bytearray()
            for lead in sorted(entries):
                e = entries[lead]
                if isinstance(e, dict):
                    for trail in sorted(e):
                        data += bytes([lead, trail])
                else:
                    data.append(lead)
            add(D, [Bx(data), S(cs)], None, extra=II)
            # every lead byte with a trail byte it lacks, and at the end
            bad = bytearray()
            for lead in sorted(entries):
                e = entries[lead]
                if isinstance(e, dict):
                    miss = next((b for b in range(256) if b not in e), None)
                    if miss is not None:
                        bad += bytes([lead, miss])
                    bad.append(lead)
            add(D, [Bx(bad), S(cs)], None, extra=II)

    # Recompose: every destination (from UTF8) over RECOMPOSE_VALUES, and the
    # multi-byte destinations under every byte-order argument.
    for cs in CHARSETS + NOT_CHARSETS:
        for cps in RECOMPOSE_VALUES:
            add(D, [Bx(utf8(cps)), S("UTF8"), U, S(cs)], None, extra=II)
    for cs in FIXED_MULTI:
        for cps in RECOMPOSE_VALUES:
            add(D, [Bx(utf8(cps)), S("UTF8"), U, S(cs)], None, extra=MM)
            for order in ("II", "MM", "x", "Unknown"):
                add(D, [Bx(utf8(cps)), S("UTF8"), U, S(cs), S(order)], None, extra=MM)
    # code points past U+10FFFF and 0x7FFFFFFF, via UCS4 and UTF-8
    for src, order in ((b"\x7f\xff\xff\xff\0\0\0A", "MM"), (b"\xff\xff\xff\xff", "MM"),
                       (b"\0\0\x11\0\0\x10\xff\xff", "MM"), (b"\0\x01\xf6\x00", "MM")):
        for cs in ("UTF8", "UCS2", "UTF16", "UCS4", "Latin", "JIS"):
            add(D, [Bx(src), S("UCS4"), S(order), S(cs), S("MM")], None, extra=II)
    for src in (b"\xff\x80\x87" + b"\xbf" * 10, b"\xff" + b"\x80" * 5 + b"\x81" + b"\x80" * 6):
        for cs in ("UTF8", "UCS2", "UTF16", "UCS4", "Latin"):
            add(D, [Bx(src), S("UTF8"), U, S(cs), S("MM")], None, extra=II)
    # Every inverse entry of every destination table, encoded.
    for cs, vals_ in sorted(scalars.items()):
        ty = tables[cs][0]
        if not ty & 0x001 or ty & 0x802:
            continue
        cps = [u for u, _ in sorted(vals_, key=lambda x: x[1])]
        add(D, [Bx(utf8(cps)), S("UTF8"), U, S(cs), S("MM")], None, extra=II)

    # A deterministic set of short random byte strings (UTF-8-shaped and
    # not), for the UTF-8 decoder: malformations, and perl's DFA fast path
    # walking from its reject state (a class-1 lead -- C0, C1, ED, F5-FF --
    # followed by another lead byte).
    import random
    rng = random.Random(1359)
    frags = [b"\x80", b"\x8f", b"\x9f", b"\xa0", b"\xbf", b"\xc0", b"\xc1", b"\xc2",
             b"\xdf", b"\xe0", b"\xed", b"\xef", b"\xf0", b"\xf4", b"\xf5", b"\xf7",
             b"\xf8", b"\xfc", b"\xfd", b"\xfe", b"\xff", b"A", b"\0", b"\xc3\xa9",
             b"\xe2\x82\xac", b"\x80\x80\x80", b"\xf0\x9f\x98\x80", b"\xef\xbf\xbf"]
    for _ in range(600):
        v = b"".join(rng.choice(frags) for _ in range(rng.randint(1, 8)))
        add(D, [Bx(v), S("UTF8"), U, S(rng.choice(["Latin", "UCS2", "UTF16"])), S("MM")],
            None, extra=II)

    # Encode: from the Charset option into each destination.
    for opts in [None, {"Charset": S("Latin")}, {"Charset": S("UCS2")}, {"Charset": U}]:
        for v in [U, S(""), S("abc"), S("caf\xc3\xa9"), S("caf\xe9"), S("\xe4\xb8\xad"),
                  S("H\0i\0")]:
            for t in [U, S("UTF8"), S("Latin"), S("UCS2"), S("UTF16"), S("Bogus"),
                      S("MacJapanese")]:
                for order in ([], [S("MM")], [S("II")]):
                    add(E, [v, t] + order, opts, extra=II)
    add(E, [S("abc"), S("UCS2")])


def exif_helper_cases(add):
    """Probes for the Exif::Main helpers (ConvertExifText, DecodeCFAPattern,
    PrintCFAPattern, PrintSFR, ASF::GetGUID, Printable)."""
    II, MM = {"byte_order": "II"}, {"byte_order": "MM"}
    CET = "Image::ExifTool::Exif::ConvertExifText"
    texts = [b"", b"abc", b"1234567", b"ASCII\0\0\0Hello", b"ASCII\0\0\0Hello   ",
             b"ASCII\0\0\0Hello  \n", b"ASCII\0\0\0Hello \n\n", b"ASCII\0\0\0a\0junk ",
             b"ASCII   Hi", b"ASCII  \nHi", b"ASCII \n Hi", b"ASCII\0\0\0", b"ASCII\0\0\0   ",
             b"\0" * 8 + b"text", b" " * 8 + b"text  ", b"\0" + b" " * 7 + b"t", b" " + b"\0" * 7 + b"t",
             b" " * 7 + b"\ntext", b" " * 6 + b"\n\ntext", b"Ascii\0\0\0x", b"ASCIIX\0\0x",
             b"ASCII\0\0\0caf\xe9", b"ASCII\0\0\0caf\xc3\xa9", b"\0" * 8, b"\0" * 20,
             b"UNICODE\0H\0i\0", b"UNICODE\0\0H\0i", b"UNICODE\0\xff\xfeH\0i\0",
             b"UNICODE\0\xfe\xff\0H\0i", b"UNICODE\0H\0i", b"UNICODE\0", b"UNICODE H\0 \0 \0",
             b"UNICODE\0\x3d\xd8\x00\xde", b"UNICODE\0\xe9\0\0\0x\0", b"Unicode\0H\0i\0",
             b"UNICODE\nH\0", b"JIS\0\0\0\0\0\x30\x21", b"JIS     \x24\x22\x24\x24", b"JIS\0\0\0\0x\x30",
             b"JIS\0\0\0\0\0", b"XYZ\0\0\0\0\0abc  ", b"\xff" * 12, b"12345678 9 ", b"ASCII\0\0\0\x80 "]
    flexes = [[I(1), S("UserComment")], [I(1)], [S("1"), U], [U, S("Tag")], [I(0), S("")],
              [S("Other"), S("Tag")], []]
    for v in texts:
        for fl in flexes:
            for bo in (II, MM):
                add(CET, [Bx(v)] + fl, None, extra=bo)
    for v in (U, I(5), F("1.5"), S("ASCII\0\0\0x")):
        add(CET, [v, I(1), S("UserComment")], None, extra=II)
    for opts in ({"CharsetEXIF": S("Latin")}, {"CharsetEXIF": S("UTF8")}, {"CharsetEXIF": S("0")},
                 {"Charset": S("Latin")}, {"Validate": I(1)}, {"Validate": S("0")}):
        for v in (b"ASCII\0\0\0caf\xe9 ", b"UNICODE\0H\0\xe9\0", b"XYZ\0\0\0\0\0a", b"ASCII\0\0\0a"):
            for fl in ([I(1), S("UserComment")], [S("1"), U], [I(2)]):
                add(CET, [Bx(v)] + fl, opts, extra=II)
    for mem in ({"WrongByteOrder": I(1)}, {"WrongByteOrder": U}):
        for v in (b"abc", b"ASCII\0\0\0x", b"UNICODE\0\0H\0i"):
            add(CET, [Bx(v), I(1), S("UserComment")], None, extra={**II, "members": mem})
    # no Session byte order: Decode's 'Unknown' needs GetByteOrder
    add(CET, [Bx(b"UNICODE\0H\0i\0"), I(1), S("UserComment")])
    add(CET, [Bx(b"ASCII\0\0\0Hi"), I(1), S("UserComment")])

    DCP = "Image::ExifTool::Exif::DecodeCFAPattern"
    cfas = [b"\x02\x00\x02\x00\x00\x01\x01\x02", b"\x00\x02\x00\x02\x00\x01\x01\x02",
            b"\x02\x00\x02\x00\x00\x01\x01", b"\x03\x00\x03\x00\x01", b"\x01\x00\x01\x00",
            b"\x00\x00\x00\x00", b"\xff\xff\xff\xff\x01", b"\x00\x01\x00\x03\x00\x01\x02",
            b"\x01\x00\x03\x00\x00\x01\x02", b"0112", b"0112\n", b"01", b"7", b"0126", b"01122",
            b"", b"\x01\x02\x03", b"0 1 2", b"\x06\x00\x06\x00" + bytes(range(36)),
            b"\x00\x06\x00\x06" + bytes(range(36)), b"\x00\x02\x00\x02" + b"\x00\x01\x01\x02\x09"]
    for v in cfas:
        for bo in (II, MM):
            add(DCP, [Bx(v)], None, extra=bo)
    for v in (U, I(12), I(789), I(1234), F("0.5")):
        add(DCP, [v], None, extra=II)
    add(DCP, [Bx(b"\x02\x00\x02\x00\x00\x01\x01\x02")])
    add(DCP, [Bx(b"0112")])

    PCP = "Image::ExifTool::Exif::PrintCFAPattern"
    for v in ["2 2 0 1 1 2", "2 2 0 1 1", "0 2", "2 0 1 2", "1", "", "1 2", "3 2 0 1 2 1 2 0",
              "2 3 0 1 2 3 4 5 6", "2 2 -1 -7 -8 7", "-1 2 0", "2 -2 0 1 1 2", "1 1", "1 1 9",
              "2 1 0 1", "abc def", "00 2 0", "4 4 " + " ".join(["1"] * 16), "65535 65535 1",
              "1 -1 3", "-1 -1 2", "  2   2  0 1 1 2  ", "2\t2\n0 1 1 2", "3 1 0 1 2",
              "2 2 0 1 1 2 extra", "1 1 5", "1 1 6", "1 1 -8", "1 3 0 1 2", "3 3 0 1 2 3 4 5 6 0 1"]:
        add(PCP, [S(v)])
    for v in (U, I(5)):
        add(PCP, [v])

    SFR = "Image::ExifTool::Exif::PrintSFR"

    def sfr(n, m, names, rats, order):
        e = "<" if order == "II" else ">"
        import struct
        return (struct.pack(e + "HH", n, m) + b"".join(x + b"\0" for x in names)
                + b"".join(struct.pack(e + "II", a, b) for a, b in rats))
    for order, bo in (("II", II), ("MM", MM)):
        vals = [sfr(2, 2, [b"H", b"V"], [(1, 2), (3, 4), (5, 0), (0, 0)], order),
                sfr(1, 3, [b"Col"], [(1, 3), (2, 3), (10, 1)], order),
                sfr(0, 0, [], [], order) + b"x", sfr(2, 0, [b"a", b"b"], [], order),
                sfr(2, 1, [b"a"], [(1, 1), (2, 2)], order),
                sfr(2, 1, [b"a", b"b", b"c"], [(1, 1), (2, 2)], order),
                sfr(3, 1, [b"", b"x", b""], [(7, 1), (4294967295, 3), (1, 4294967295)], order),
                sfr(2, 2, [b"H", b"V"], [(1, 2)], order), b"\x01\x00\x01\x00",
                b"\x01\x00\x01\x00\x00", b"abcde", b"\x00\x00\x00\x00\x00\x00",
                sfr(1, 1, [b"caf\xe9"], [(1, 3)], order)]
        for v in vals:
            add(SFR, [Bx(v)], None, extra=bo)
    for v in (U, S(""), S("abcd"), I(12345)):
        add(SFR, [v], None, extra=II)
    add(SFR, [Bx(sfr(1, 1, [b"a"], [(1, 3)], "II"))])

    GG = "Image::ExifTool::ASF::GetGUID"
    for v in [bytes(range(16)), bytes(range(15)), bytes(range(17)), b"\xff" * 16, b"",
              bytes.fromhex("24c3dd6f034efe4bb1853d77768dc90c"), b"0123456789abcdef"]:
        add(GG, [Bx(v)])
    for v in (U, I(5), S("0123456789abcde\n")):
        add(GG, [v])

    PR = "Image::ExifTool::Printable"
    pvals = [U, S(""), S("abc"), Bx(b"a\0b\x01c\x1f\x7f\xff d"), S("x" * 100), S("y" * 25),
             S("z" * 61), S("w" * 2100), I(5), F("1.5"), Bx(b"caf\xc3\xa9")]
    for v in pvals:
        for ml in ([], [U], [I(0)], [I(10)], [I(30)], [S("abc")], [F("25.5")], [I(-5)]):
            for opts in (None, {"Verbose": I(4)}, {"Verbose": I(5)}, {"Verbose": S("x")},
                         {"Verbose": F("4.5")}):
                add(PR, [v] + ml, opts)


def cases():
    out = []

    def add(helper, args, options=None, with_session=False, extra=None):
        c = {"helper": helper, "args": args}
        if options:
            c["options"] = options
        if with_session:
            c["with_session"] = 1
        if extra:
            c.update(extra)
        out.append(c)

    one_arg = ["Image::ExifTool::Exif::ConvertFraction",
               "Image::ExifTool::Exif::PrintExposureTime",
               "Image::ExifTool::ConvertDuration", "Image::ExifTool::IsFloat",
               "Image::ExifTool::ConvertBitrate", "Image::ExifTool::Exif::PrintFraction",
               "Image::ExifTool::Canon::CanonEv", "Image::ExifTool::Canon::CanonEvInv",
               "Image::ExifTool::Exif::PrintFNumber", "Image::ExifTool::IsInt"]
    for h in one_arg:
        for a in BATTERY:
            add(h, [a])

    # ConvertDateTime: the identity branch and each option that leaves it.
    cdt_opts = [None, {"DateFormat": S("0")}, {"DateFormat": S("")},
                {"DateFormat": U}, {"DateFormat": S("%Y")},
                {"GlobalTimeShift": S("0")}, {"GlobalTimeShift": S("1")},
                {"StrictDate": I(1)}]
    for o in cdt_opts:
        for d in [S(x) for x in DATES] + [U, I(0), F("1.5")]:
            add("Image::ExifTool::ConvertDateTime", [d], o)

    # GPS::ToDMS
    coords = [F("12.5"), F("-12.5"), F("45.999999999"), F("72.99999999"),
              F("59.9999999"), F("0.0"), I(0), I(-1), F("1e-9"), F("-0.00001"),
              S(""), U, S("abc"), S("12.5 N"), F("123.456789"), F("179.9999999999"),
              F("1e18"), S("-45.5"), S("-12.5 N"), S("-3/4")]
    for o in [None, {"CoordFormat": S("%.6f")}, {"CoordFormat": S("0")}]:
        for v in coords:
            for dpc in [None, U, I(0), I(1), S("2"), S("3"), S("x")]:
                for ref in [None, U, S(""), S("N"), S("E"), S("S"), S("W")]:
                    args = [v]
                    if dpc is not None or ref is not None:
                        args.append(dpc if dpc is not None else U)
                    if ref is not None:
                        args.append(ref)
                    add("Image::ExifTool::GPS::ToDMS", args, o)

    # ConvertUnixTime
    for o in [None, {"KeepUTCTime": I(1)}, {"SystemTimeRes": I(3)}, {"SystemTimeRes": I(0)}]:
        for a in BATTERY:
            for extra in ([], [I(1)], [I(1), U], [I(0), I(2)]):
                add("Image::ExifTool::ConvertUnixTime", [a] + extra, o)

    # GetUnixTime
    ts = ["2020:01:02 03:04:05", "2020-01-02 03:04:05", "2020:01:02 03:04:05Z",
          "2020:01:02 03:04:05+01:00", "2020:01:02 03:04:05-05:30",
          "2020:01:02 03:04:05.123", "2020:01:02 03:04:05.5+01:00",
          "0000:00:00 00:00:00", "1970:01:01 00:00:00", "2020:02:30 00:00:00",
          "0070:01:01 00:00:00", "2020:13:01 00:00:00", "2020:01:02 24:00:00",
          "abc", "", "2020:01:02  03:04:05 junk", "9999:12:31 23:59:59",
          "1000:01:01 00:00:00", "1969:12:31 23:59:59", "2020:01:02 03:04:05z",
          "2020:01:02 03:04:05 +01:00", "2020:01:02 03:04:05+1:5", "2000:02:29 12:00:00",
          "1900:02:29 12:00:00", "2020:1:2 3:4:5", "2020:01:02 03:04:60",
          "2020:01:02 03:04:05\n", "12020:01:01 00:00:00"]
    for t in ts:
        for loc in ([], [U], [I(0)], [I(1)], [I(2)], [S("2")], [F("2.0")]):
            add("Image::ExifTool::GetUnixTime", [S(t)] + loc)

    # XMP::ConvertXMPDate
    xs = ["2020-01-02T03:04:05", "2020-01-02T03:04", "2020-01-02 03:04:05+01:00",
          "2020-01-02T03:04:05Z", "2020-01-02T03:04:05.123-05:00",
          "2020-01-02T03:04:05 x y", "2020-01", "2020", "2020-01-02", "20-01-02",
          "2020-01-02T03:04:05\n", "abc", "2020-01-02T03:04:05  +01:00", "",
          "2020-01-02T03:04:05+01:00\n", "2020-1-2", "2020-01-02-03", "2020-01-02T03:04:5"]
    for x in xs:
        for uns in ([], [I(0)], [I(1)], [U]):
            add("Image::ExifTool::XMP::ConvertXMPDate", [S(x)] + uns)
    add("Image::ExifTool::XMP::ConvertXMPDate", [U])

    # GPS::ToDegrees
    gs = ["12 30 15.5 N", "12.5 S", "12,30,15 W", '41 deg 53\' 23.12" N, 12 deg 29\' 30.10" E',
          "-12.5", "inf", "undef", "abc", "1e+5", "1e5", "3.5.6", ".5", "12 South",
          "12 south", "12 30 S ", "12 W\n", "N 12 30", "12S", "", "0", "-0 30 0 S",
          "12 30 15 45", "41.5 N, 12.5 E", "41.5 S, 12.5 W", "infinity", "+.5 -1.5",
          "1.5e-3", "12.E+2"]
    for g in gs:
        for extra in ([], [I(1)], [I(0)], [I(1), S("lat")], [I(1), S("lon")],
                      [I(0), S("lat")], [U, S("lon")], [I(1), S("x")]):
            add("Image::ExifTool::GPS::ToDegrees", [S(g)] + extra)
    for a in BATTERY:
        add("Image::ExifTool::GPS::ToDegrees", [a])

    # ConvertFileSize: no $et, and $et with each ByteUnit.
    for a in BATTERY + [I(2097151), I(2097152), I(10485759), I(10485760),
                        I(2147483647), I(2147483648), I(10737418239), I(10737418240),
                        I(1999999999), I(2000000000), I(9999999999), I(10000000000)]:
        add("Image::ExifTool::ConvertFileSize", [a])
        for bu in [None, {"ByteUnit": S("SI")}, {"ByteUnit": S("Binary")},
                   {"ByteUnit": S("binary")}, {"ByteUnit": U}]:
            add("Image::ExifTool::ConvertFileSize", [a], bu, with_session=True)

    decode_cases(add)
    exif_helper_cases(add)
    return out


def residual_cases():
    def case(tag_id, value, **extra):
        return {"residual_id": tag_id, "input": value, **extra}

    opcode = (2).to_bytes(4, "big") + (1).to_bytes(4, "big") + (1).to_bytes(4, "big") \
        + (0).to_bytes(4, "big") + (0).to_bytes(4, "big") \
        + (99).to_bytes(4, "big") + (1).to_bytes(4, "big") \
        + (0).to_bytes(4, "big") + (2).to_bytes(4, "big") + b"\xaa\xbb"
    truncated = (2).to_bytes(4, "big") + (14).to_bytes(4, "big") \
        + (1).to_bytes(4, "big") + (0).to_bytes(4, "big") + (0).to_bytes(4, "big")
    composite = b"".join((0).to_bytes(4, "big") + (1).to_bytes(4, "big") for _ in range(7)) \
        + (2).to_bytes(2, "big") + (3).to_bytes(2, "big") \
        + (1).to_bytes(4, "big") + (2).to_bytes(4, "big")
    out = [
        case("0x8298", Bx(b"Photographer \0Editor \0")),
        case("0x8298", Bx(b"Photographer \0Editor ")),
        case("0x9287", S("3 0 1 4 2 99 9")),
        case("0xa462", Bx(composite), byte_order="MM"),
        case("0xa462", Bx(composite[:57]), byte_order="MM"),
    ]
    for tag_id in ("0xc740", "0xc741", "0xc74e"):
        out.extend((case(tag_id, Bx(opcode)), case(tag_id, Bx(truncated))))
    for raw in (
        [1, 2, 3, 4, 0, 0, 0, 0],
        [1, 2, 3, 4, 5, 6, 7],
        [1, 2, 3, 0x84, 0x15, 0x09, 0x26, 0],
        [1, 2, 3, 0x84, 0x87, 0x05, 0x04, 0xa8],
        [0, 0, 0, 0x81, 0x87, 0x05, 0x04, 0x8b],
        [0, 0, 0, 0x80, 1, 1, 0x7a, 0],
        [0, 0, 0, 0x80, 0x01, 0x00, 0x1e, 0x80],
    ):
        out.append(case("0xc763", S(" ".join(map(str, raw)))))
    return out


# Options whose `Image::ExifTool->new` value Session::new must reproduce.
OPTION_DEFAULTS = ["ByteUnit", "Charset", "CharsetEXIF", "CharsetFileName", "CharsetID3",
                   "CharsetIPTC", "CharsetPhotoshop", "CharsetQuickTime", "CharsetRIFF",
                   "CoordFormat", "DateFormat", "GlobalTimeShift", "KeepUTCTime",
                   "StrictDate", "SystemTimeRes", "Validate", "Verbose"]

# Keys a case carries besides helper/args (copied into the capture).
CASE_KEYS = ("options", "with_session", "byte_order", "members")


# ---------------------------------------------------------------------------
# Instrument
# ---------------------------------------------------------------------------

def instrument(perl, et_dir):
    """Assert the pinned interpreter and tree before producing any number."""
    pinned = (REPO / ".exiftool-version").read_text().strip()
    env = oracle_env()
    pv = subprocess.run([perl, "-e", "print $^V"], capture_output=True, text=True,
                        env=env, check=True).stdout.strip()
    if pv != PINNED_PERL_VERSION:
        sys.exit(f"perl {perl} is {pv}, not the pinned {PINNED_PERL_VERSION}")
    exiftool = Path(et_dir) / "exiftool"
    ver = subprocess.run([perl, str(exiftool), "-ver"], capture_output=True, text=True,
                         env=env, check=True).stdout.strip()
    if ver != pinned:
        sys.exit(f"ExifTool at {et_dir} reports {ver!r}, pinned is {pinned!r}")
    docx = Path(et_dir) / "t" / "images" / "OOXML.docx"
    ft = subprocess.run([perl, str(exiftool), "-s3", "-FileType", str(docx)],
                        capture_output=True, text=True, env=env, check=True).stdout.strip()
    if ft != "DOCX":
        sys.exit(f"capability probe: OOXML.docx -> {ft!r}, not DOCX (degraded perl)")
    print(f"=== instrument: helper_oracle ===\n"
          f"perl      {perl} ({pv})\nexiftool  {exiftool} -ver {ver} (pinned {pinned}); "
          f"OOXML.docx -> {ft}\nTZ        UTC")
    return pv, ver


def oracle_env():
    env = {k: v for k, v in os.environ.items()
           if k not in ("PERL5LIB", "PERLLIB", "PERL5OPT")}
    env["TZ"] = "UTC"
    return env


def run_perl(perl, et_lib, payload):
    proc = subprocess.run([perl, str(HARNESS), str(et_lib)], input=json.dumps(payload),
                          capture_output=True, text=True, env=oracle_env())
    if proc.returncode != 0:
        sys.exit(f"helper_oracle.pl failed:\n{proc.stderr}")
    return json.loads(proc.stdout)


def pinned_residual_sources(perl, et_lib):
    proc = subprocess.run(
        [perl, str(HERE / "dump_tables.pl"), "--reader-only", str(et_lib), "Exif"],
        capture_output=True, text=True, env=oracle_env(), check=True,
    )
    doc = json.loads(proc.stdout)
    tags = doc["modules"]["Exif"]["tables"]["Main"]["tags"]
    selected = {}
    for tag_id, admitted in RESIDUAL_PORTS.items():
        tag = tags[str(int(tag_id, 16))]
        body = conv_codegen.refusal_source_body(tag)
        digest = hashlib.sha256(body.encode()).hexdigest()
        if digest != admitted:
            sys.exit(
                f"Exif::Main residual {tag_id} source {digest} is not admitted; "
                f"expected {admitted}"
            )
        selected[tag_id] = {"source_body": body, "source_sha256": digest, "cases": []}
    return selected


def build(perl, et_dir):
    pv, ver = instrument(perl, et_dir)
    et_lib = Path(et_dir) / "lib"
    sources = pinned_sources(et_lib)
    missing = [h["perl"] for h in HELPERS if sources[h["perl"]][0] is None]
    if missing:
        sys.exit(f"subs absent from the pinned tree: {missing}")
    cs = cases()
    results = run_perl(perl, et_lib, cs)
    tr_cases = truthiness_cases()
    tr_results = run_perl(perl, et_lib, tr_cases)
    [defaults] = run_perl(perl, et_lib, [{"option_defaults": OPTION_DEFAULTS}])
    deps = dependency_sources(et_lib)
    missing = [d for d, digest in deps.items() if digest is None]
    if missing:
        sys.exit(f"dependency subs absent from the pinned tree: {missing}")
    helpers = {}
    residuals = pinned_residual_sources(perl, et_lib)
    r_cases = residual_cases()
    r_results = run_perl(perl, et_lib, r_cases)
    for case, result in zip(r_cases, r_results):
        entry = {"input": case["input"]}
        if "byte_order" in case:
            entry["byte_order"] = case["byte_order"]
        entry.update(result)
        residuals[case["residual_id"]]["cases"].append(entry)
    for h in HELPERS:
        src, digest = sources[h["perl"]]
        helpers[h["perl"]] = {
            "module": h["module"], "status": h["status"], "spike_rank": h["rank"],
            "spike_uses": h["uses"], "note": h["note"], "sub_source": src,
            "source_sha256": digest, "cases": []}
        if h.get("deps"):
            helpers[h["perl"]]["dependencies"] = {d: deps[d] for d in h["deps"]}
    for c, r in zip(cs, results):
        entry = {"args": c["args"]}
        for k in CASE_KEYS:
            if k in c:
                entry[k] = c[k]
        entry.update(r)
        helpers[c["helper"]]["cases"].append(entry)
    return {
        "capture": {
            "tool": "tools/exiftool-tables/helper_oracle.py",
            "perl": perl, "perl_version": pv, "exiftool_version": ver, "tz": "UTC",
            "note": "each case is the pinned sub called directly on `args` (prototypes "
                    "bypassed), `options` set in $$et{OPTIONS} and mirrored into "
                    "%static_vars as ExifTool::Init does; `out` is Perl's stringified "
                    "return value as hex bytes, `die` means the call died",
        },
        "truthiness": [dict(value=c["truthy"], **r) for c, r in zip(tr_cases, tr_results)],
        "option_defaults": dict(zip(OPTION_DEFAULTS, defaults["out"])),
        "charset_sources": codegen_charsets.charset_sources(et_lib),
        "perl_sources": codegen_charsets.perl_sources(codegen_charsets.perl_core(perl, oracle_env())),
        "helpers": helpers,
        "residuals": residuals,
    }


def render(capture):
    """Deterministic text: the envelope indented, each case on one line, so
    a re-capture diffs case by case and the file stays reviewable."""
    lines = ["{", '"capture": ' + json.dumps(capture["capture"], sort_keys=True) + ",",
             '"charset_sources": ' + json.dumps(capture["charset_sources"], sort_keys=True)
             + ",",
             '"option_defaults": ' + json.dumps(capture["option_defaults"], sort_keys=True)
             + ",",
             '"perl_sources": ' + json.dumps(capture["perl_sources"], sort_keys=True) + ",",
             '"helpers": {']
    names = sorted(capture["helpers"])
    for i, name in enumerate(names):
        h = dict(capture["helpers"][name])
        cases = h.pop("cases")
        head = json.dumps(h, sort_keys=True)[:-1]
        lines.append(json.dumps(name) + ": " + head + ', "cases": [')
        lines += [json.dumps(c, sort_keys=True) + ("," if j < len(cases) - 1 else "")
                  for j, c in enumerate(cases)]
        lines.append("]}" + ("," if i < len(names) - 1 else ""))
    lines.append("},")
    lines.append('"residuals": ' + json.dumps(capture["residuals"], sort_keys=True) + ",")
    lines.append('"truthiness": [')
    t = capture["truthiness"]
    lines += [json.dumps(x, sort_keys=True) + ("," if j < len(t) - 1 else "")
              for j, x in enumerate(t)]
    lines += ["]", "}"]
    text = "\n".join(lines) + "\n"
    assert json.loads(text) == capture
    return text


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    ap.add_argument("--perl", default=os.environ.get("EXIFTOOL_PERL"))
    ap.add_argument("--exiftool-dir", default=os.environ.get("OXIDEX_PINNED_EXIFTOOL"))
    args = ap.parse_args()
    if not args.perl or not args.exiftool_dir:
        sys.exit("need --perl/--exiftool-dir (or EXIFTOOL_PERL/OXIDEX_PINNED_EXIFTOOL)")
    capture = build(args.perl, args.exiftool_dir)
    text = render(capture)
    n = sum(len(h["cases"]) for h in capture["helpers"].values())
    rn = sum(len(r["cases"]) for r in capture["residuals"].values())
    if args.write:
        CAPTURE.write_text(text, encoding="utf-8")
        print(f"wrote {CAPTURE.relative_to(REPO)}: {n} helper cases, {rn} residual cases, "
              f"{len(capture['truthiness'])} truthiness cases")
        return 0
    committed = CAPTURE.read_text(encoding="utf-8")
    if committed != text:
        print(f"MISMATCH: re-running the pinned Perl does not reproduce "
              f"{CAPTURE.relative_to(REPO)}", file=sys.stderr)
        return 1
    tables = codegen_charsets.generate(args.perl, args.exiftool_dir, oracle_env(),
                                       capture["capture"]["exiftool_version"])
    if codegen_charsets.OUT.read_text(encoding="utf-8") != tables:
        print(f"MISMATCH: the pinned tree does not regenerate "
              f"{codegen_charsets.OUT.relative_to(REPO)}", file=sys.stderr)
        return 1
    print(f"PASS: pinned Perl reproduces {CAPTURE.relative_to(REPO)} byte for byte "
          f"({n} helper cases, {rn} residual cases, "
          f"{len(capture['truthiness'])} truthiness cases), and the "
          f"pinned tree regenerates {codegen_charsets.OUT.relative_to(REPO)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
