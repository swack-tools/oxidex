#!/usr/bin/env python3
"""Step 15's differential expression oracle.

An expression `exprs.py` claims to translate (by exact match in TRANSLATIONS
or by `compile()`'s grammar) is not verified because it looks obviously
right. It is verified because this script ran ExifTool's own Perl -- the
pinned release, not whatever `perl`/`exiftool` happen to resolve on PATH --
and the shipped Rust translation over the same probe inputs and diffed the
results. Anything that disagrees is refused and counted here, the same way
codegen.py refuses and counts an untranslatable expression; this script's job
is only to say which of the "translated" set actually deserves that label.

Design of the probe set (see `probes_for`): a small fixed battery (0, a
negative, a fraction, a large value, values right at the census's own
`0x7fffffff`/`655.345`-style boundaries) plus every numeric literal that
appears *inside* the expression text itself, each probed at -1/0/+1 around it
-- because a boundary condition written as `$val > 655.345` is only exercised
by testing near 655.345, not by a generic battery that has no idea that
number is special to this expression.

Instruments named, per AGENTS.md doctrine:
  - Perl side:  /usr/bin/perl5.34 -I <pinned ExifTool 13.59 lib>, capability-
    probed before use (never a bare `perl`/`exiftool` off PATH).
  - Rust side:  a throwaway `src/bin/expr_oracle_harness.rs`, auto-discovered
    by Cargo, built and run via `cargo run` -- i.e. the exact Rust source
    text `exprs.py` emits (for compile()) and the hand-written functions in
    `src/exiftool_tables/exprs.rs` (for the named helpers), not a
    reimplementation of either. The harness file is generated and deleted by
    this script; it is never committed.

Numeric ("f64"/"Option<f64>") results are compared by parsed value (relative
tolerance 1e-9), because Rust's and Perl's float-to-string algorithms are not
contractually identical and a formatting difference is not a translation
bug. Every other result (String) is compared as exact text, because that
text IS the tag value a caller sees.
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

import exprs

REPO_ROOT = Path(__file__).resolve().parents[2]
HARNESS_PATH = REPO_ROOT / "src" / "bin" / "expr_oracle_harness.rs"

# Both oracles run with the process time zone pinned to UTC. ExifTool's
# `ConvertUnixTime($val, 1)` renders LOCAL time plus a `TimeZoneString`
# suffix (ExifTool.pm:6804-6806), and so does the Rust port (chrono::Local,
# which reads TZ through libc exactly as Perl's localtime does): under an
# unpinned zone the two sides would agree only by coincidence of the host,
# and a run on a different host would fail a probe over a fact about the
# host. The pin is recorded in the ledger's instrument block.
ORACLE_TZ = "UTC"
ORACLE_ENV = {**os.environ, "TZ": ORACLE_TZ}
sys.path.insert(0, str(REPO_ROOT / "scripts"))
import instrument  # noqa: E402 -- git/instrument identity header

SLOTS = ("ValueConv", "PrintConv", "RawConv")

# `exprs.compile_composite`'s `@val` placeholders -- see `main()`.
_COMPOSITE_PLACEHOLDER_RE = re.compile(r"\{v\d+\}")


# --- census (same walk as expr_coverage.py / expr_census.py) --------------

def walk_tags(node, out):
    if isinstance(node, dict):
        out.append(node)
    elif isinstance(node, list):
        for v in node:
            walk_tags(v, out)


def walk_code_refs(node, out):
    """Every tag hash reachable from `node`, INCLUDING `_variants`
    alternatives -- but used only to collect CODE refs.

    Deliberately not folded into `walk_tags`. Widening that walk widens the
    *expression* census too, and `_variants` alternatives inside ExifTool's
    Composite tables carry `$val[N]`-indexed conversions -- a different value
    domain (`exprs.compile_composite`, `@val` rather than a lone scalar) that
    this harness has no probe shape for and cannot build Rust for: the
    generated `expr_oracle_harness.rs` fails to compile on the `{v0}`
    placeholders. Six of the eight `CODE_REFS` entries live only in
    `_variants` (Nikon's `AFInfo2V0300`/`V0400` FocusPosition fields), so the
    walk has to reach them; keeping it separate is what stops that reach from
    changing the population the rest of this script has always verified.
    """
    if isinstance(node, dict):
        out.append(node)
        if "_variants" in node:
            walk_code_refs(node["_variants"], out)
    elif isinstance(node, list):
        for v in node:
            walk_code_refs(v, out)


def census(tables_json_path):
    """Every conversion the shipped translator claims to handle, by text.

    Two populations, deliberately merged into one counter because the oracle
    treats them identically -- it evaluates the key as Perl and diffs against
    the Rust:

      kind == "expr"  the conversion IS Perl text (`'$val / 10'`), and the
                      key is that text verbatim.
      kind == "code"  the conversion is a CODE ref (`PrintConv => \\&Sub`),
                      which has no text at all; `exprs.CODE_REFS` maps the
                      deparsed body onto a key that NAMES the sub as a real
                      Perl call (`Image::ExifTool::CanonCustom::ConvertPfn
                      ($val)`). Evaluating that key runs ExifTool's actual
                      subroutine, which is what makes the registry entry
                      checkable at all -- without this the code-ref
                      translations would be the only ones in the file taken
                      on faith.
    """
    d = json.load(open(tables_json_path, encoding="utf-8"))
    counter = {}
    # raw expr -> set of element counts of the fields that carry it (1 for a
    # scalar). A list-domain expression's probe needs the field's count --
    # `sprintf("%4d %4d %4d (%dK)", split(" ",$val))` reads four elements
    # because the field is `int16s[4]`, which the text alone does not say.
    counts_by_expr = {}
    for _modname, mod in d["modules"].items():
        for _tname, t in (mod.get("tables") or {}).items():
            table_format = (t.get("meta") or {}).get("FORMAT")
            for _tid, tagnode in (t.get("tags") or {}).items():
                variants = []
                walk_tags(tagnode, variants)
                for tag in variants:
                    fmt = tag.get("Format") or table_format or ""
                    m = _SIZED_FORMAT_RE.match(str(fmt))
                    count = int(m.group(2)) if m and m.group(1) not in ("string", "undef") else 1
                    for slot in SLOTS:
                        v = tag.get(slot)
                        if isinstance(v, dict) and v.get("kind") == "expr":
                            e = v.get("expr")
                            if isinstance(e, str) and e.strip():
                                counter[e] = counter.get(e, 0) + 1
                                counts_by_expr.setdefault(e, set()).add(count)
                code_nodes = []
                walk_code_refs(tagnode, code_nodes)
                for tag in code_nodes:
                    for slot in SLOTS:
                        v = tag.get(slot)
                        if isinstance(v, dict) and v.get("kind") == "code":
                            named = exprs.code_ref_expr(v.get("deparse"))
                            if named:
                                counter[named] = counter.get(named, 0) + 1
    return d.get("exiftool_version"), counter, counts_by_expr


# --- capability probe ------------------------------------------------------

def capability_probe(perl_bin, et_lib, expect_version):
    """AGENTS.md: a matching -ver is not a working oracle. Confirm the
    interpreter runs, the pinned library loads (not some other ExifTool that
    happens to be findable), reports the exact pinned version, and can
    actually execute a real conversion -- not just parse.

    Also captures the Perl *interpreter's* own version (`$^V`, distinct from
    `$Image::ExifTool::VERSION` above) and returns it, so the ledger this
    run may go on to write can record which Perl produced its judgement.
    codegen.py's `load_oracle_ledger` names this in its abort message when a
    later host's `tables.json` digest does not match -- see docs/FLEET.md's
    2026-08-14 addenda ("regen.sh has a hidden host dependency").
    """
    script = (
        f'use lib "{et_lib}"; use Image::ExifTool; use Image::ExifTool::Exif;\n'
        'print "VERSION\\t$Image::ExifTool::VERSION\\n";\n'
        'print "PERLVER\\t$^V\\n";\n'
        'print "PROBE\\t", Image::ExifTool::Exif::PrintExposureTime(0.125), "\\n";\n'
    )
    r = subprocess.run([perl_bin, "-e", script], capture_output=True, text=True, timeout=30)
    lines = dict(
        line.split("\t", 1) for line in r.stdout.splitlines() if "\t" in line
    )
    ok = (
        r.returncode == 0
        and lines.get("VERSION") == expect_version
        and lines.get("PROBE") == "1/8"
    )
    print(
        f"capability probe: perl={perl_bin} lib={et_lib} "
        f"version={lines.get('VERSION')!r} perl_version={lines.get('PERLVER')!r} "
        f"PrintExposureTime(0.125)={lines.get('PROBE')!r} "
        f"rc={r.returncode} -> {'OK' if ok else 'FAILED'}"
    )
    if not ok:
        print("STDERR:", r.stderr, file=sys.stderr)
        raise SystemExit(
            "oracle capability probe failed -- refusing to trust this Perl/ExifTool "
            "as ground truth (AGENTS.md: a matching -ver is not a working oracle)"
        )
    return lines.get("PERLVER", "<unknown>")


# --- probe sets --------------------------------------------------------

NUM_BASE = [
    0.0, 1.0, -1.0, 0.5, -0.5, 2.0, 3.0, 10.0, 100.0, -128.0, 127.0, 255.0,
    256.0, 65535.0, 65536.0, 1e6, -1e6, 1e-6, 1e18, 2147483647.0, 2147483648.0,
    4294967295.0, 655.345, 655.36,
]
_LIT_RE = re.compile(r"0[xX][0-9a-fA-F]+|\d+\.\d+(?:[eE][+-]?\d+)?|\d+(?:[eE][+-]?\d+)?")
# `int16u[4]` -> (base, count); codegen.py's SIZED_RE, repeated here rather
# than imported so this script keeps depending on exprs.py alone.
_SIZED_FORMAT_RE = re.compile(r"^(\w+)\[(\d+)\]$")
# The element indices and sprintf conversions a list-domain expression reads,
# so a probe can be sized from the text when no carrying field says otherwise.
_LIST_INDEX_RE = re.compile(r"\$[A-Za-z_]\w*\[(\d+)\]")
_LIST_SPEC_RE = re.compile(r"%[-+0]*\d*\.?\d*[dfxXg]")


def list_probes_for(expr, counts):
    """Probe lists for one list-domain expression, at every element count a
    carrying field declares (plus the count the text itself needs, and one
    short and one long list around each): small integers, all-zero, all-max
    of the 16-bit width, negatives (a signed field), the mined literals
    broadcast, and the `(%dK)`-style boundary values. Kept within the
    magnitude of a 32-bit field: the elements come from `split` of an integer
    string on the Perl side, and the IV/NV stringification split perl_int/
    perl_num model only matters past 1e15 -- beyond any format's width."""
    needed = 0
    for m in _LIST_INDEX_RE.finditer(expr):
        needed = max(needed, int(m.group(1)) + 1)
    specs = len(_LIST_SPEC_RE.findall(expr))
    needed = max(needed, specs, 1)
    sizes = set(counts or ()) | {needed}
    sizes.discard(0)
    probes = []
    lits = set()
    for m in _LIT_RE.finditer(expr):
        t = m.group()
        try:
            v = float(int(t, 16)) if t[:2].lower() == "0x" else float(t)
        except ValueError:
            continue
        if abs(v) < 2 ** 31:
            lits.add(v)
    for n in sorted(sizes):
        base = [float(i) for i in range(n)]
        probes.append(base)
        probes.append([1.0] * n)
        probes.append([0.0] * n)
        probes.append([255.0] * n)
        probes.append([65535.0] * n)
        probes.append([-1.0] * n)
        probes.append([float(1000 * (i + 1)) for i in range(n)])
        probes.append([float(1024 * (i + 1)) for i in range(n)])
        probes.append([float(2 ** 31 - 1)] * n)
        probes.append([float(-(2 ** 31))] * n)
        for v in sorted(lits):
            probes.append([v] * n)
            probes.append([v + 1.0] * n)
            probes.append([v - 1.0] * n)
        if n > 1:
            probes.append(base[:-1])  # short list: Perl reads undef -> 0 / ""
        probes.append(base + [float(n)])  # long list: extra element ignored
    # Dedupe, order-preserving.
    seen, out = set(), []
    for p in probes:
        key = tuple(p)
        if key not in seen:
            seen.add(key)
            out.append(p)
    return out


# `PrintAFPointsLeftRight($val, ncol)` / `PrintAFPointsUpDown($val, nrow)`
# (Nikon.pm:13420-13442, pinned 13.59) read `$val` from a real
# ProcessBinaryData field -- always a small non-negative column/row index,
# never wider than the AF grid itself (ncol/nrow max out at 29 in the pinned
# tree). NUM_BASE's generic battery includes 1e18 "to be safe", but for
# *this* pair of functions probing there is actively wrong: it manufactures
# a FAIL that has nothing to do with the translation.
#
# Confirmed by hand against the pinned Perl (13.59, /usr/bin/perl5.34) and
# the shipped Rust (`print_af_points_left_right`, src/exiftool_tables/
# exprs.rs):
#   perl -e 'my $v=1e18; my $c=10.0; my $d=$v-$c;
#            print $d == $v, " ", sprintf("%d", $d), "\n"'
#     -> "1 999999999999999990"
# `$d == $v` is true -- `1e18 - 10` rounds to the exact same IEEE754 double
# as `1e18` itself (its ULP there is 128, well past 10) -- so Perl's own
# `sprintf('%d', ...)` does NOT reproduce that double's true value once it
# exceeds roughly 2^53 (~9.007e15): the same $d prints "1000000000000000000"
# under `%.0f` but "999999999999999990" under `%d`. Rust's
# `format!("{}", v as i64)` (perl_int) gives the mathematically correct
# "1000000000000000000" for the identical double. Both sides are internally
# consistent; only Perl's *own* `%d` formatter loses precision here, and it
# is a fact about `sprintf('%d')` at that magnitude, not about this
# translation or about any real AF-point value ExifTool ever reads. Per
# AGENTS.md's "name the instrument" doctrine and the task's narrow-fix
# preference: bound the probe magnitude for exactly these two functions
# rather than loosen the oracle's comparison generally.
_AF_POINT_PROBES = [float(v) for v in range(-4, 32)] + [40.0, 100.0, -50.0]
NARROW_NUM_DOMAIN_PROBES = {
    f"Image::ExifTool::Nikon::PrintAFPointsLeftRight($val, {n})": _AF_POINT_PROBES
    for n in (19, 21, 29)
}
NARROW_NUM_DOMAIN_PROBES.update({
    f"Image::ExifTool::Nikon::PrintAFPointsUpDown($val, {n})": _AF_POINT_PROBES
    for n in (11, 13, 17)
})


# Boundary probes for the named helpers whose call text carries no literal of
# its own to mine. `ConvertDuration($val)` has nothing for _LIT_RE to find,
# so on NUM_BASE alone its 30 s / 60 s / 3600 s / 86400 s branch points and
# the `$h > 24` day split are exercised only by whatever base values happen
# to land near them. These are UNIONED with the generic battery (and with
# any literals the surrounding expression does carry), never substituted for
# it -- the composed CanonEv sites (`... CanonEv($val-24) ...`) keep their
# mined 24/25/23 probes as well as the stop codes below.
_HELPER_BOUNDARY_PROBES = {
    "ConvertDuration": [
        29.99, 29.995, 30.0, 59.4, 59.5, 60.0, 3599.5, 3600.0, 86399.0,
        86400.0, 86400.5, 90000.0, 172800.0, -30.0, -59.5, -86400.0,
    ],
    "ConvertBitrate": [
        0.001, 99.9, 99.95, 100.0, 999.0, 999.5, 1000.0, 99999.0, 999999.0,
        1e6, 1e9, 999.5e9, 1e12, -1000.0,
    ],
    "PrintFraction": [
        1 / 3, 2 / 3, 0.25, 0.333, 0.3334, 0.5, 0.7, -0.7, 0.999, 1.001, 1.5,
        -1.5, 1.326429536, 1e-3,
    ],
    "CanonEv": [
        8.0, 12.0, 16.0, 20.0, 24.0, 32.0, 36.0, 44.0, 52.0, 56.0, 64.0,
        -12.0, -20.0, -32.0, 12.7, 44.9, 200.0,
    ],
    # Nikon::PrintPC (Nikon.pm:13450): the four sentinel values (0, 0x7f,
    # -128, -127) with their neighbours on both sides, non-integers on both
    # sides of zero for the %+d/%d truncation, and the /4 quotient points.
    "PrintPC": [
        0.0, 1.0, -1.0, 3.0, -3.0, 4.0, -4.0, 6.0, 9.0, 12.0, -12.0, 2.6, -2.6,
        126.0, 127.0, 128.0, -126.0, -127.0, -128.0, -129.0, 1e18,
    ],
    # ConvertUnixTime (ExifTool.pm:6784): the half-to-even second rounding
    # on both sides of zero, the epoch shifts the call sites pass (Mac
    # 631065600, seconds either side), years 1, 1950, 2000, 2038, 2106,
    # 3000, 10000 (the `%4d` year padding and widening), and the 2**53
    # magnitudes gmtime still computes calendar years for. NUM_BASE's 1e18
    # is the glibc gmtime overflow artifact on both sides.
    "ConvertUnixTime": [
        0.4, 0.5, 0.6, 1.5, 2.5, -0.4, -0.5, -1.5, 0.999999, 946684799.9999,
        1e9 + 0.5, 1e9 + 0.49999, 631065600.0, 631065601.0, 631065599.0,
        -631065600.0, -62135596800.0, 253402300800.0, 32503680000.0,
        4294967295.0, 2147483648.0, -2147483648.0, 1e11, 1e12, 2.0 ** 53,
        -(2.0 ** 53),
    ],
    # The FILETIME entry in TRANSLATIONS (`$val/1e7-11644473600`): ticks for
    # 1601-01-01 (the offset itself), 1970-01-01 exactly and half a second
    # either side of it (the float division must carry the remainder into
    # the rounding), a 2019 timestamp, and the far end of a 64-bit field.
    "11644473600": [
        0.0, 116444736000000000.0, 116444736004999999.0, 116444736005000000.0,
        116444736005000001.0, 132000000000000000.0, 1.8e19,
    ],
}


def numeric_probes_for(expr):
    if expr in NARROW_NUM_DOMAIN_PROBES:
        return NARROW_NUM_DOMAIN_PROBES[expr]
    vals = set(NUM_BASE)
    for m in _LIT_RE.finditer(expr):
        t = m.group()
        try:
            v = float(int(t, 16)) if t[:2].lower() == "0x" else float(t)
        except ValueError:
            continue
        vals.update((v, v + 1.0, v - 1.0))
        if v != 0.0:
            vals.add(-v)
    for helper, extra in _HELPER_BOUNDARY_PROBES.items():
        if helper in expr:
            vals.update(extra)
    return sorted(vals)


STR_PROBES_TR = [
    "2024-01-02 10:20:30", "no-dashes-or-space", "", "-T", "a-b",
    "\x00hello\x00world\x00", "  double  space  ", "2024:01:02",
]
STR_PROBES_CDT = [
    "2024:01:02 03:04:05", "2024:01:02 03:04:05-07:00", "2024:01:02 03:04:05Z",
    "", "not a date at all",
]
BYTES_PROBES_SRC = ["Hello", "", "abc", "A", "Test String", "cafe"]
# Raw byte buffers for the bytes-domain shapes that are NOT the UCS2 decode
# (unpack("H*"), ASF::GetGUID): the empty buffer, one byte, the 16-byte GUID
# width with every byte distinct, all-0xff, a real ASF GUID (the one asf.rs's
# own unit test pins), and 15/17/32-byte buffers. The non-16-byte buffers are
# kept ASCII on purpose: GetGUID returns them UNCHANGED and the Perl side of
# this harness prints under `binmode STDOUT, ':utf8'`, which would Latin-1
# upgrade a raw 0x80+ byte where Rust's lossy UTF-8 conversion prints U+FFFD
# -- a disagreement about the harness's own output encoding, not about the
# conversion, and the ASF tables never hand GetGUID anything but 16 bytes.
BYTES_PROBES_RAW = [
    b"", b"A", b"abc", b"\x00\x01\x02", b"Hello World!!!!", b"Hello World 16 b",
    b"Hello World 17 by", bytes(range(16)), b"\xff" * 16,
    bytes.fromhex("c4b0695ff704214b984246cca542d8d3"), bytes(range(32)),
]


def probes_for(domain, raw_expr, counts=None):
    if domain == "num":
        return numeric_probes_for(raw_expr)
    if domain == "list":
        return list_probes_for(raw_expr, counts)
    if domain == "str":
        if "ConvertDateTime" in raw_expr:
            return STR_PROBES_CDT
        return STR_PROBES_TR
    if "UCS2" not in raw_expr:
        return BYTES_PROBES_RAW
    # "bytes" (UCS2 decode): the probe values must already be genuine UCS2 --
    # 2 bytes per character, in whichever order (II=little-endian,
    # MM=big-endian) this specific expression declares -- not raw UTF-8/ASCII
    # text. Encoding plain-text bytes and calling that a UCS2 buffer was the
    # harness's own first bug (found by feeding it to both oracles: Perl
    # decoded 5 single-byte ASCII "Hello" bytes as UCS2 pairs and produced
    # mojibake that happened to differ from Rust's *different* mojibake --
    # neither side was wrong, the probe was).
    little_endian = '"II"' in raw_expr
    enc = "utf-16-le" if little_endian else "utf-16-be"
    return [s.encode(enc) for s in BYTES_PROBES_SRC]


# --- Perl side --------------------------------------------------------

def perl_escape(s):
    return s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n").replace("\x00", "\\0")


def build_perl_script(jobs, et_lib):
    lines = [
        f'use lib "{et_lib}";',
        "use Image::ExifTool;",
        "use Image::ExifTool::Exif;",
        "use Image::ExifTool::GPS;",
        # The modules whose named subs `exprs.CODE_REFS` keys call. Without
        # these the key text still parses and still evaluates -- to an
        # "Undefined subroutine" die, which this harness scores as ERROR and
        # SKIPS, so the entry would read as verified while never having been
        # run once.
        "use Image::ExifTool::CanonCustom;",
        "use Image::ExifTool::Nikon;",
        # `Image::ExifTool::Canon::CanonEv` is a QHELPER in exprs.py's
        # grammar. Without this line every CanonEv probe dies "Undefined
        # subroutine", is scored ERROR, and is SKIPPED -- and an expression
        # whose every probe is skipped is already refused entry to the ledger
        # by the `s[0] > 0` test in main(), so the failure would be loud;
        # but it would read as a translation defect rather than a harness
        # one, which is the wrong bug to send someone chasing.
        "use Image::ExifTool::Canon;",
        # `Image::ExifTool::ASF::GetGUID` is a bytes-domain fixed shape in
        # exprs.py. Same failure mode as CanonEv without this line: every
        # probe dies "Undefined subroutine", scores ERROR, and the
        # expression can never earn a ledger PASS.
        "use Image::ExifTool::ASF;",
        # `Image::ExifTool::ICC_Profile::HexID` is a list-domain QHELPER;
        # without this line its every probe died "Undefined subroutine" and
        # the first slice-4 oracle run scored the expression FAIL for having
        # no comparable probe -- the loud failure the SKIP rule exists for.
        "use Image::ExifTool::ICC_Profile;",
        "my $self = new Image::ExifTool;",
        "binmode STDOUT, ':utf8';",
        # ExifTool itself runs every ValueConv/PrintConv/RawConv string
        # through `eval $conv` from inside ExifTool.pm (13.59:3656-3664) --
        # `eval STRING` resolves barewords in the *calling* code's
        # package, so a conversion that calls a bareword sub ExifTool.pm
        # itself defines (IsInt, IsFloat, ...) only resolves there, not in
        # `main`. Running the oracle's evals from `main` instead was a
        # harness bug that looked like every IsInt($val) probe erroring --
        # it was never a translation defect, it was this script calling the
        # real subroutine from the wrong namespace.
        "package Image::ExifTool;",
    ]
    for job_id, domain, raw, probe in jobs:
        if domain == "num":
            set_val = f"my $val = {probe!r};"
        elif domain == "str":
            set_val = f'my $val = "{perl_escape(probe)}";'
        elif domain == "list":
            # What ReadValue hands a fixed-count field's conversion: the
            # elements joined by a space, as one string (ExifTool.pm:6286 ff.).
            # Integral probes are written as integers so Perl reads them as
            # IVs, exactly as it reads a real record's elements.
            joined = " ".join(str(int(v)) if float(v).is_integer() else repr(v) for v in probe)
            set_val = f'my $val = "{joined}";'
        else:
            set_val = f'my $val = pack("H*", "{probe.hex()}");'  # probe is already bytes
        lines.append("{")
        lines.append(f"  {set_val}")
        lines.append(f"  my $r = eval {{ {raw} }};")
        lines.append(f'  if ($@) {{ print "J{job_id}\\tERROR\\n"; }}')
        lines.append(f'  elsif (!defined $r) {{ print "J{job_id}\\tUNDEF\\n"; }}')
        lines.append(f'  else {{ my $s = $r; $s =~ s/\\n/\\\\n/g; print "J{job_id}\\t$s\\n"; }}')
        lines.append("}")
    return "\n".join(lines)


def run_perl(perl_bin, script_text, timeout):
    # A `-e` argument is subject to the OS argv-length limit; the full probe
    # battery comfortably exceeds it, so the script is always written to a
    # temp file and run as a script, not inlined.
    import tempfile
    with tempfile.NamedTemporaryFile("w", suffix=".pl", delete=False) as fh:
        fh.write(script_text)
        script_path = fh.name
    try:
        r = subprocess.run(
            [perl_bin, script_path],
            capture_output=True, text=True, timeout=timeout, env=ORACLE_ENV,
        )
    finally:
        Path(script_path).unlink(missing_ok=True)
    if r.returncode != 0:
        print("PERL HARNESS STDERR:", r.stderr[-4000:], file=sys.stderr)
        raise SystemExit(f"perl harness exited {r.returncode}")
    out = {}
    for line in r.stdout.splitlines():
        if "\t" in line:
            jid, val = line.split("\t", 1)
            out[jid] = val
    return out


# --- Rust side --------------------------------------------------------

def rust_num_literal(v):
    s = repr(float(v)).replace("e+", "e")
    if "." not in s and "e" not in s and "inf" not in s and "nan" not in s:
        s += ".0"
    return f"({s}_f64)"


def rust_str_literal(s):
    out = []
    for ch in s:
        if ch == "\\":
            out.append("\\\\")
        elif ch == '"':
            out.append('\\"')
        elif ch == "\x00":
            out.append("\\0")
        elif ch == "\n":
            out.append("\\n")
        else:
            out.append(ch)
    return '"' + "".join(out) + '"'


def rust_bytes_literal(b):
    body = ", ".join(f"0x{byte:02x}u8" for byte in b)  # b is already bytes
    return f"(&[{body}][..])"


def rust_list_literal(values):
    body = ", ".join(rust_num_literal(v) for v in values)
    return f"(&[{body}][..])"


def render_probe(domain, probe):
    if domain == "num":
        return rust_num_literal(probe)
    if domain == "str":
        return rust_str_literal(probe)
    if domain == "list":
        return rust_list_literal(probe)
    return rust_bytes_literal(probe)


def build_rust_harness(jobs, by_expr):
    body = []
    for job_id, domain, raw, probe in jobs:
        rust_type, rust_code = by_expr[raw]
        lit = render_probe(domain, probe)
        code = rust_code.replace("{v}", lit)
        # The compiled text uses `crate::exiftool_tables::exprs::...` because
        # that is correct where it actually ships: spliced into
        # binary_tables.rs, part of the `oxidex` lib crate, where `crate`
        # means that crate. This harness is a *different* crate (a `src/bin`
        # binary), where `crate::` means itself -- rewrite the path rather
        # than change the text under test.
        code = code.replace("crate::exiftool_tables::exprs::", "oxidex::exiftool_tables::exprs::")
        # Mirror codegen.py's gen_expr_enum exactly, including its perl_num
        # wrapping for bare numeric results -- the harness exists to test
        # what ships, not a simplified stand-in for it.
        if rust_type == "f64":
            expr_code = f"oxidex::exiftool_tables::exprs::perl_num({code})"
        elif rust_type == "f64_int":
            expr_code = f"oxidex::exiftool_tables::exprs::perl_int({code})"
        elif rust_type == "Option<f64>":
            expr_code = (
                f"match ({code}).map(oxidex::exiftool_tables::exprs::perl_num) "
                '{ Some(s) => s, None => "UNDEF".to_string() }'
            )
        else:  # String
            expr_code = f"({code})"
        body.append(
            f'    out.push_str(&format!("J{job_id}\\t{{}}\\n", '
            f"{{ let __r: String = {expr_code}; __r.replace('\\n', \"\\\\n\") }}));"
        )
    src = (
        "// GENERATED by tools/exiftool-tables/verify_exprs.py -- not committed.\n"
        "use oxidex::exiftool_tables::exprs::*;\n"
        "#[allow(clippy::all, unused_parens, unused_variables)]\n"
        "fn main() {\n"
        "    let mut out = String::new();\n"
        + "\n".join(body)
        + "\n    print!(\"{out}\");\n"
        "}\n"
    )
    return src


def run_rust(jobs, by_expr, timeout):
    src = build_rust_harness(jobs, by_expr)
    HARNESS_PATH.parent.mkdir(parents=True, exist_ok=True)
    HARNESS_PATH.write_text(src, encoding="utf-8")
    try:
        r = subprocess.run(
            ["cargo", "run", "--quiet", "--bin", "expr_oracle_harness"],
            cwd=REPO_ROOT, capture_output=True, text=True, timeout=timeout,
            env=ORACLE_ENV,
        )
        if r.returncode != 0:
            print("RUST HARNESS STDERR:", r.stderr[-6000:], file=sys.stderr)
            raise SystemExit(f"rust harness exited {r.returncode}")
        out = {}
        for line in r.stdout.splitlines():
            if "\t" in line:
                jid, val = line.split("\t", 1)
                out[jid] = val
        return out
    finally:
        HARNESS_PATH.unlink(missing_ok=True)


# --- comparison --------------------------------------------------------

def results_match(rust_type, perl_val, rust_val):
    if perl_val == rust_val:
        return True
    if perl_val in ("UNDEF", "ERROR") or rust_val in ("UNDEF", "ERROR"):
        return False
    if rust_type in ("f64", "f64_int", "Option<f64>"):
        try:
            pf, rf = float(perl_val), float(rust_val)
            return abs(pf - rf) <= max(1e-9, abs(pf) * 1e-9)
        except ValueError:
            return False
    return False


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("tables_json")
    ap.add_argument("--perl", default="/usr/bin/perl5.34")
    ap.add_argument("--et-lib", default="/tmp/oxidex-exiftool-cache/exiftool/lib")
    ap.add_argument("--timeout", type=int, default=180)
    ap.add_argument("--sample-lines", type=int, default=15)
    ap.add_argument(
        "--ledger-out",
        type=Path,
        help="write the oracle-approved expression inventory for codegen; only written on PASS",
    )
    args = ap.parse_args()

    git = instrument.git_state()
    dirty_overridden = instrument.refuse_if_dirty(git, "verify_exprs.py")
    instrument.print_header(
        tool="verify_exprs.py",
        git=git,
        dirty_overridden=dirty_overridden,
        extra=[f"perl:    {args.perl}  et_lib={args.et_lib}",
               f"tables:  {args.tables_json}"],
    )

    version, counter, counts_by_expr = census(args.tables_json)
    perl_version = capability_probe(args.perl, args.et_lib, version)

    # Every expression the shipped translator (TRANSLATIONS + compile())
    # claims to handle, across all three value domains -- this is the whole
    # "translated" surface, not just what codegen.py wires into a binary
    # table's ExprId.
    by_expr = {}       # raw_expr -> (rust_type, rust_code)
    domain_of = {}      # raw_expr -> domain
    # A Composite conversion (`$val[0]`, `$val[1] ? ... : undef`) compiles
    # through `exprs.compile_composite`, whose output carries `{v0}`/`{v1}`/…
    # placeholders for ExifTool's `@val` array rather than this harness's lone
    # `{v}` scalar. It is a different value domain with a different probe
    # shape, it never reaches `binary_tables.rs` (codegen_composite.py owns
    # it), and feeding it to `build_rust_harness` produces a source file that
    # does not compile -- `{v0}` survives into the Rust as an undefined
    # variable and `cargo run` exits 101 before a single probe is compared.
    # Skipped by NAME and COUNTED, not silently: an unverified expression
    # this script does not mention is one a reader would assume it checked.
    composite_domain = []
    for e in counter:
        r = exprs.translate_or_compile_any(e)
        if r:
            domain, rty, code = r
            if _COMPOSITE_PLACEHOLDER_RE.search(code):
                composite_domain.append(e)
                continue
            by_expr[e] = (rty, code)
            domain_of[e] = domain

    jobs = []
    for e in sorted(by_expr):
        for probe in probes_for(domain_of[e], e, counts_by_expr.get(e)):
            jobs.append((len(jobs), domain_of[e], e, probe))

    print(f"pinned release        {version}")
    print(f"translated expressions {len(by_expr)}   probe jobs {len(jobs)}")
    if composite_domain:
        print(f"NOT verified here: {len(composite_domain)} Composite-domain "
              "expression(s) -- `@val`-indexed, a different probe shape "
              "(see the comment in main()); this script covers the scalar "
              "`$val` surface only:")
        for e in sorted(composite_domain):
            print(f"    {re.sub(r'[$]s+', ' ', e.strip())[:88]}")

    perl_script = build_perl_script(jobs, args.et_lib)
    print("running Perl oracle ...")
    perl_out = run_perl(args.perl, perl_script, args.timeout)

    print("running Rust harness (cargo run --bin expr_oracle_harness) ...")
    rust_out = run_rust(jobs, by_expr, args.timeout)

    per_expr = {}  # raw_expr -> [pass, fail, skip]
    fail_examples = []
    skip_examples = []
    for job_id, domain, raw, probe in jobs:
        jid = f"J{job_id}"
        pv = perl_out.get(jid, "<missing>")
        rv = rust_out.get(jid, "<missing>")
        rty = by_expr[raw][0]
        stats = per_expr.setdefault(raw, [0, 0, 0])
        if pv == "ERROR":
            # This probe value isn't meaningful for this expression (e.g. a
            # division that legitimately dies in Perl for this input) --
            # not a translation defect, so it is counted separately, not as
            # a failure.
            stats[2] += 1
            if len(skip_examples) < 20:
                skip_examples.append((raw, probe, pv, rv))
            continue
        if results_match(rty, pv, rv):
            stats[0] += 1
        else:
            stats[1] += 1
            if len(fail_examples) < 40:
                fail_examples.append((raw, probe, pv, rv))

    total_pass = sum(s[0] for s in per_expr.values())
    total_fail = sum(s[1] for s in per_expr.values())
    total_skip = sum(s[2] for s in per_expr.values())
    # An all-errored probe set has established no equivalence.  It must not
    # enter the ledger merely because it has no disagreement; R2 is
    # oracle-first, not absence-of-evidence-first.
    exprs_all_pass = sum(1 for s in per_expr.values() if s[0] > 0 and s[1] == 0)
    exprs_any_fail = sum(1 for s in per_expr.values() if s[1] > 0 or s[0] == 0)

    print()
    print(f"probe-level: PASS {total_pass}  FAIL {total_fail}  SKIP(perl errored) {total_skip}")
    print(f"expression-level: PASS (>=1 match, 0 failing probes) {exprs_all_pass}/{len(by_expr)}"
          f"   FAIL (disagreement or no comparable probe) {exprs_any_fail}/{len(by_expr)}")
    print()
    print(f"sample of {min(args.sample_lines, len(per_expr))} per-expression results:")
    for raw in sorted(per_expr)[: args.sample_lines]:
        p, f, sk = per_expr[raw]
        status = "PASS" if p > 0 and f == 0 else "FAIL"
        s = re.sub(r"\s+", " ", raw.strip())[:70]
        print(f"  {status}  probes(pass={p} fail={f} skip={sk})  {s}")

    if fail_examples:
        print()
        print("FAILING probe examples (raw_expr, probe, perl, rust):")
        for raw, probe, pv, rv in fail_examples:
            s = re.sub(r"\s+", " ", raw.strip())[:70]
            print(f"  expr={s!r} probe={probe!r} perl={pv!r} rust={rv!r}")

    if skip_examples:
        print()
        print(f"sample of Perl-errored (skipped) probes ({total_skip} total):")
        for raw, probe, pv, rv in skip_examples[:10]:
            s = re.sub(r"\s+", " ", raw.strip())[:70]
            print(f"  expr={s!r} probe={probe!r}")

    print()
    if exprs_any_fail:
        print(f"RESULT: FAIL -- {exprs_any_fail} expression(s) disagree with the pinned Perl oracle "
              "or had no comparable probe")
        raise SystemExit(1)
    if args.ledger_out:
        # This artifact is the ORACLE-FIRST hand-off: codegen may enable an
        # expression only when its normalized source appears here, and also
        # verifies the dump digest below.  Shape acceptance alone is never a
        # shipping permission.  The underlying semantics are ExifTool.pm's
        # eval of conversion text at 3656-3664 (pinned 13.59).
        verified = sorted(exprs.normalize(e) for e, s in per_expr.items() if s[0] > 0 and s[1] == 0)
        verified_uses = sum(counter[e] for e, s in per_expr.items() if s[0] > 0 and s[1] == 0)
        artifact = {
            # Schema 2 (was 1): added `perl_version`, the interpreter
            # provenance codegen.py's load_oracle_ledger names in its abort
            # message on a tables_sha256 mismatch. A schema-1 ledger has no
            # such field and is refused outright -- see that function's
            # docstring and docs/FLEET.md's 2026-08-14 addenda.
            "schema": 2,
            "exiftool_version": version,
            "perl_version": perl_version,
            "tables_sha256": hashlib.sha256(Path(args.tables_json).read_bytes()).hexdigest(),
            "instrument": {
                "perl": args.perl,
                "perl_version": perl_version,
                "et_lib": str(args.et_lib),
                "rust": "cargo run --quiet --bin expr_oracle_harness",
                # Both sides ran under this zone (see ORACLE_ENV); a ledger
                # that does not say so cannot be reproduced on another host.
                "tz": ORACLE_TZ,
            },
            "probe_counts": {"pass": total_pass, "fail": total_fail, "skip": total_skip},
            "expression_counts": {
                "verified": len(verified),
                "translated": len(by_expr),
                "total": len(counter),
            },
            "use_counts": {"verified": verified_uses, "total": sum(counter.values())},
            "verified_expressions": verified,
        }
        args.ledger_out.parent.mkdir(parents=True, exist_ok=True)
        args.ledger_out.write_text(json.dumps(artifact, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"wrote oracle ledger  {args.ledger_out}")
    print("RESULT: PASS -- every translated expression agreed with the pinned Perl oracle "
          f"on every probe ({total_pass} probe comparisons, {total_skip} skipped as "
          "inapplicable-to-probe Perl errors)")


if __name__ == "__main__":
    main()
