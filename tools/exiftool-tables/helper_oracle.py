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

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
HARNESS = HERE / "helper_oracle.pl"
CAPTURE = HERE / "testdata" / "helper_oracle_outputs.json"
PINNED_PERL_VERSION = "v5.38.2"

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
         module="Image/ExifTool.pm", uses=64, status=REFUSED,
         note="takes a BYTE string (UCS2/Latin input), which MemberVal::Str (UTF-8 text) "
              "cannot carry, and needs Charset::Decompose/Recompose and %csType; exprs.rs "
              "keeps its UCS2-only partial for the v1 path. Exif::Main's top blocker (20 "
              "read-side uses) -- next slice, with a byte-string MemberVal"),
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
         module="Image/ExifTool.pm", uses=26, status=REFUSED,
         note="write-side charset encode; needs Charset.pm (as Decode)"),
    dict(rank=22, spike="Samsung::Crypt", perl="Image::ExifTool::Samsung::Crypt",
         module="Image/ExifTool/Samsung.pm", uses=25, status=REFUSED,
         note="reads the array-valued EncryptionKey member, $tagInfo and the "
              "module-level %formatMinMax; not modelled on Session yet"),
    # A partial port in exprs.rs completed here because its only option,
    # ByteUnit, is on Session (spike rank > 40; not counted toward the top 22).
    dict(rank=None, spike="ConvertFileSize", perl="Image::ExifTool::ConvertFileSize",
         module="Image/ExifTool.pm", uses=None, status=PORTED,
         note="complete: SI and Binary ByteUnit branches, with or without $et"),
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


def pinned_sources(et_lib):
    """{perl name: (folded source, sha256)} for every HELPERS entry, read from
    the tree at `et_lib` (the directory holding `Image/`)."""
    out = {}
    for h in HELPERS:
        text = (Path(et_lib) / h["module"]).read_text(encoding="latin-1")
        src = helper_source(text, h["perl"].rsplit("::", 1)[1])
        out[h["perl"]] = (src, hashlib.sha256(src.encode("latin-1")).hexdigest()
                          if src is not None else None)
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
           "5\n", "1 ", "+.5e-3", "1e", "18446744073709551616", "5/00", "0/00", "-0/7"]
BATTERY = [I(v) for v in INTS] + [F(v) for v in FLOATS] + [S(s) for s in STRINGS] + [U]

DATES = ["2020:01:02 03:04:05", "", "0000:00:00 00:00:00", "2020:01:02 03:04:05+01:00",
         "2020:01:02 03:04:05Z", "2020:01:02 03:04:05.123", "abc"]


def truthiness_cases():
    vals = [U, S(""), S("0"), S("0.0"), S("00"), S("0E0"), S(" 0"), S("0 "), S("\n"),
            S("0\n"), S("a"), S("-0"), S("+0"), S(".0"), S("0x0"), S("00.0"),
            I(0), I(1), I(-1), F("0.0"), F("-0.0"), F("0.1"), F("nan"), F("inf")]
    return [{"truthy": v} for v in vals]


def cases():
    out = []

    def add(helper, args, options=None, with_session=False):
        c = {"helper": helper, "args": args}
        if options:
            c["options"] = options
        if with_session:
            c["with_session"] = 1
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
    return out


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
    helpers = {}
    for h in HELPERS:
        src, digest = sources[h["perl"]]
        helpers[h["perl"]] = {
            "module": h["module"], "status": h["status"], "spike_rank": h["rank"],
            "spike_uses": h["uses"], "note": h["note"], "sub_source": src,
            "source_sha256": digest, "cases": []}
    for c, r in zip(cs, results):
        entry = {"args": c["args"]}
        for k in ("options", "with_session"):
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
        "helpers": helpers,
    }


def render(capture):
    """Deterministic text: the envelope indented, each case on one line, so
    a re-capture diffs case by case and the file stays reviewable."""
    lines = ["{", '"capture": ' + json.dumps(capture["capture"], sort_keys=True) + ",",
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
    if args.write:
        CAPTURE.write_text(text, encoding="utf-8")
        print(f"wrote {CAPTURE.relative_to(REPO)}: {n} helper cases, "
              f"{len(capture['truthiness'])} truthiness cases")
        return 0
    committed = CAPTURE.read_text(encoding="utf-8")
    if committed != text:
        print(f"MISMATCH: re-running the pinned Perl does not reproduce "
              f"{CAPTURE.relative_to(REPO)}", file=sys.stderr)
        return 1
    print(f"PASS: pinned Perl reproduces {CAPTURE.relative_to(REPO)} byte for byte "
          f"({n} helper cases, {len(capture['truthiness'])} truthiness cases)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
