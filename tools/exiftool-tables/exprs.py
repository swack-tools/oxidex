#!/usr/bin/env python3
"""Registry of hand-verified Perl-expression -> Rust translations.

This file is the safety boundary of the whole generator.  ExifTool conversions
are arbitrary Perl; most are trivial arithmetic, but a handful do real work.
The rule enforced here is absolute:

    An expression is translated only if it appears in TRANSLATIONS by exact
    (whitespace-normalised) match.  Anything else is UNSUPPORTED, and an
    unsupported conversion means the tag is emitted WITHOUT that conversion, or
    skipped entirely -- never approximated.

The reason for the strictness is that the failure mode is silent.  A wrong
`PrintConv` does not crash; it prints a confident, plausible, wrong number
under a genuine ExifTool tag name, into an archival pipeline, and nothing
downstream can tell.  A missing tag is loud and recoverable.  So the generator
is built to under-claim.

Adding a translation is cheap and permanent: one entry here fixes every tag
that shares the expression, forever, across all 146 modules.  That is the
compounding this project needs -- contrast one model call fixing one tag once.
The analyzer prints expressions ranked by usage; work the top of that list.
"""

import re

# Rust expression templates.  `{v}` is the input value as f64.
# Only conversions that are pure functions of $val belong here -- anything
# touching $self, other tags, or ExifTool state is deliberately absent.
TRANSLATIONS = {
    # --- identity / trivial ---------------------------------------------
    "$val":                  ("f64", "{v}"),
    "$val + 1":              ("f64", "{v} + 1.0"),
    "$val - 1":              ("f64", "{v} - 1.0"),
    "-$val":                 ("f64", "-{v}"),
    "$val / 10":             ("f64", "{v} / 10.0"),
    "$val / 100":            ("f64", "{v} / 100.0"),
    "$val / 1000":           ("f64", "{v} / 1000.0"),
    "$val * 2":              ("f64", "{v} * 2.0"),
    "$val / 2":              ("f64", "{v} / 2.0"),
    "$val * 100":            ("f64", "{v} * 100.0"),
    "$val / 8":              ("f64", "{v} / 8.0"),
    "$val / 32":             ("f64", "{v} / 32.0"),
    "2 ** ($val / 3)":       ("f64", "2f64.powf({v} / 3.0)"),
    "2 ** (-$val / 3)":      ("f64", "2f64.powf(-{v} / 3.0)"),
    "2 ** ($val / 6)":       ("f64", "2f64.powf({v} / 6.0)"),

    # --- formatted numbers ----------------------------------------------
    'sprintf("%.1f",$val)':  ("String", 'format!("{:.1}", {v})'),
    'sprintf("%.2f",$val)':  ("String", 'format!("{:.2}", {v})'),
    'sprintf("%.0f",$val)':  ("String", 'format!("{:.0}", {v})'),
    'sprintf("%.3f",$val)':  ("String", 'format!("{:.3}", {v})'),
    'sprintf("%.1f mm",$val)': ("String", 'format!("{:.1} mm", {v})'),
    'sprintf("%.1fmm",$val/10)': ("String", 'format!("{:.1}mm", {v} / 10.0)'),

    # --- units --------------------------------------------------------------
    # `perl_num` rather than a bare `{}`: Perl's `"$val"` interpolation goes
    # through `%.15g`, which switches to scientific notation outside roughly
    # [1e-4, 1e15); Rust's `Display` for f64 never does. See
    # src/exiftool_tables/exprs.rs::perl_num for the incident that found this.
    '"$val mm"':
        ("String", 'format!("{} mm", crate::exiftool_tables::exprs::perl_num({v}))'),
    '"$val m"':
        ("String", 'format!("{} m", crate::exiftool_tables::exprs::perl_num({v}))'),
    '"$val C"':
        ("String", 'format!("{} C", crate::exiftool_tables::exprs::perl_num({v}))'),
    '"$val s"':
        ("String", 'format!("{} s", crate::exiftool_tables::exprs::perl_num({v}))'),
    '"$val%"':
        ("String", 'format!("{}%", crate::exiftool_tables::exprs::perl_num({v}))'),

    # --- conditionals ------------------------------------------------------
    # `$val ? $val : undef` suppresses the tag entirely when zero, which is
    # why the Rust side returns Option rather than a sentinel.
    "$val ? $val : undef":   ("Option<f64>",
                              "if {v} != 0.0 { Some({v}) } else { None }"),
    # The same conversion in Perl's other two spellings. `||` and `or` on a
    # number are the identical truthiness test (`!= 0`), so all three keys map
    # to one piece of Rust; they are separate entries only because this table
    # matches text, never meaning. Honest scope note: at the pinned release
    # every use of these two spellings is a RawConv, and codegen.py does not
    # emit RawConv (it records `omitted_raw_conv`), so these raise the census
    # and the oracle coverage but not, yet, the generated tables.
    "$val || undef":         ("Option<f64>",
                              "if {v} != 0.0 { Some({v}) } else { None }"),
    "$val or undef":         ("Option<f64>",
                              "if {v} != 0.0 { Some({v}) } else { None }"),
    '$val ? sprintf("%+.2f", $val) : 0':
        ("String",
         'if {v} != 0.0 { format!("{:+.2}", {v}) } else { "0".to_string() }'),
    '$val ? sprintf("%+.1f",$val) : 0':
        ("String",
         'if {v} != 0.0 { format!("{:+.1}", {v}) } else { "0".to_string() }'),
    '$val > 655.345 ? "inf" : "$val m"':
        ("String",
         'if {v} > 655.345 { "inf".to_string() } else '
         '{ format!("{} m", crate::exiftool_tables::exprs::perl_num({v})) }'),

    # --- named ExifTool helper subs reached through a Perl CODE ref ---------
    # These keys are not text lifted out of a tag table: a `PrintConv =>
    # \&SomeSub` is a code REF, so `dump_tables.pl` records its deparsed body
    # and there is no expression string to match on. CODE_REFS below maps each
    # such deparsed body -- by exact match, same doctrine as this table --
    # onto the key here that NAMES the sub it is. Writing the key as a real
    # Perl call is what makes it verifiable: `verify_exprs.py` evaluates the
    # key text against the pinned tree's own subroutine, so the entry is
    # checked against the actual ExifTool implementation rather than against
    # a transcription of it.
    "Image::ExifTool::CanonCustom::ConvertPfn($val)":
        ("String", "crate::exiftool_tables::exprs::convert_pfn({v})"),
    "Image::ExifTool::ConvertFileSize($val)":
        ("String", "crate::exiftool_tables::exprs::convert_file_size({v})"),
    # `$ncol`/`$nrow` is a literal at every pinned call site, so each distinct
    # literal is its own entry rather than a parameter: a template with a hole
    # in it could be filled with a number no ExifTool table actually passes.
    "Image::ExifTool::Nikon::PrintAFPointsLeftRight($val, 19)":
        ("String",
         "crate::exiftool_tables::exprs::print_af_points_left_right({v}, 19.0)"),
    "Image::ExifTool::Nikon::PrintAFPointsLeftRight($val, 21)":
        ("String",
         "crate::exiftool_tables::exprs::print_af_points_left_right({v}, 21.0)"),
    "Image::ExifTool::Nikon::PrintAFPointsLeftRight($val, 29)":
        ("String",
         "crate::exiftool_tables::exprs::print_af_points_left_right({v}, 29.0)"),
    "Image::ExifTool::Nikon::PrintAFPointsUpDown($val, 11)":
        ("String",
         "crate::exiftool_tables::exprs::print_af_points_up_down({v}, 11.0)"),
    "Image::ExifTool::Nikon::PrintAFPointsUpDown($val, 13)":
        ("String",
         "crate::exiftool_tables::exprs::print_af_points_up_down({v}, 13.0)"),
    "Image::ExifTool::Nikon::PrintAFPointsUpDown($val, 17)":
        ("String",
         "crate::exiftool_tables::exprs::print_af_points_up_down({v}, 17.0)"),

    # --- multi-statement idiom, hand-verified rather than grammar-parsed:
    # a Perl statement-modifier guard followed by a helper call is a
    # different syntactic shape than a plain expression, and generalising a
    # statement-sequence parser for one idiom is not worth the risk. ---
    "return 'Full' if $val > 0.99; Image::ExifTool::Exif::PrintExposureTime($val);":
        ("String",
         'if {v} > 0.99 { "Full".to_string() } '
         'else { crate::exiftool_tables::exprs::print_exposure_time({v}) }'),
    # Perl's list-operator call form (no parentheses): one exact spelling in
    # the census (CanonVRD::DR4, five PrintConvs). `_parse_sprintf` requires
    # the parenthesised form, and a one-line entry beats teaching the grammar
    # a second call syntax for one spelling. A bare `%g` is `%.6g`.
    'sprintf "%g", $val':
        ("String", "crate::exiftool_tables::exprs::perl_g_spec({v}, 6, false)"),
    # Windows FILETIME (100 ns ticks since 1601-01-01) to a local time
    # (ExifTool's `$val/1e7-11644473600` idiom, ten tables): a FLOAT
    # division, so the tick remainder survives into ConvertUnixTime's own
    # half-to-even second rounding, then the 1601->1970 offset. Two
    # statements, hand-verified, same reason as the entry above.
    "$val=$val/1e7-11644473600; ConvertUnixTime($val,1)":
        ("String",
         "crate::exiftool_tables::exprs::convert_unix_time("
         "{v} / 1e7 - 11644473600.0, true)"),
}


# =============================================================================
# CODE_REFS -- the sibling of TRANSLATIONS for a `PrintConv => \&NamedSub`.
#
# ExifTool writes a conversion two ways: as a string of Perl (`'$val / 10'`),
# which `dump_tables.pl` records verbatim and TRANSLATIONS/compile() handle,
# and as a reference to a named subroutine (`\&ConvertPfn`), which has no
# expression text at all. `dump_tables.pl` records the latter as
# `{"kind": "code", "deparse": "<B::Deparse output>"}`, and codegen.py used to
# drop every one of them -- counted as `conv_dropped`, but with the field still
# EMITTED carrying `PrintConv::None`, i.e. a raw number where ExifTool prints a
# string. That is the one failure mode `AGENTS.md` rates worse than a missing
# tag, so the two halves of this file's answer to it are: recognise the code
# refs that are pure functions of `$val` (here), and make codegen.py flag the
# rest as an explicit `Omitted { print_conv: true }` refusal so the field is
# WITHHELD instead of reported wrong.
#
# The doctrine is TRANSLATIONS' doctrine, unchanged. A deparsed body is
# recognised only by exact (whitespace-normalised) match, and the value it maps
# to is a key in TRANSLATIONS -- so a code ref cannot reach any Rust that has
# not already been written down, reviewed and oracle-checked as an ordinary
# translation. Matching on the deparse rather than on the sub's name is what
# makes this safe across releases: if ExifTool rewrites `ConvertPfn`'s body,
# the key stops matching and the conversion goes back to being refused, rather
# than silently continuing to run the old translation under the new sub.
#
# Deliberately NOT here, and why (each is a real entry in the pinned tree that
# this file refuses):
#
#   Nikon::FormatString          not a pure function of $val -- its first
#                                branch is keyed on $et->Options
#                                ('LimitLongValues') (Nikon.pm:13530).
#   Nikon::PrintAFPoints         string-domain: they consume the SPACE-
#     (Nikon.pm:13307-13329),    SEPARATED HEX TEXT a ValueConv already
#   Nikon::PrintAFPointsGrid     produced, and PrintConv::Expr's shipped
#     (Nikon.pm:13378-13395),    surface is `apply(f64) -> Option<String>`.
#   Sony::PrintLensSpec          PrintLensSpec additionally reads the
#     (Sony.pm:11179-11213),     module-level @lensFeatures table, and
#   Pentax::AFPointNamesK3III    AFPointNamesK3III takes a third argument
#     (Pentax.pm:6758-6769)      ExifTool's caller supplies, not $val.
#
# See codegen.py's `conv_for` for the counters, and
# `docs/reference/binary-data-engine.md` for the per-field census.
CODE_REFS = {
    # Image::ExifTool::CanonCustom::ConvertPfn -- CanonCustom.pm:2624-2628,
    # reached from `%convPFn` (CanonCustom.pm:36) by all 29 fields of
    # CanonCustom::PersonalFuncs.
    "($) { package Image::ExifTool::CanonCustom; use strict; "
    "(my($val) = (shift())); "
    "(return ($val ? (($val == 1) ? 'On' : (\"On ($val)\")) : 'Off')); }":
        "Image::ExifTool::CanonCustom::ConvertPfn($val)",

    # Image::ExifTool::ConvertFileSize -- ExifTool.pm:6851-6871, reached from
    # Palm.pm:121-124 (MOBI UncompressedTextLength). The deparse carries BOTH
    # of the sub's branches; the translation reproduces the SI one, which is
    # the only one reachable without an API option this crate never sets
    # (`ByteUnit` defaults to 'SI', ExifTool.pm:1115).
    "($;$) { package Image::ExifTool; use strict; (my($val, $et) = @_); "
    "if (($et and ($et->{'OPTIONS'}{'ByteUnit'} eq 'Binary'))) { "
    "(($val < 2048) and (return (\"$val bytes\"))); "
    "(($val < 10240) and (return sprintf('%.1f KiB', ($val / 1024)))); "
    "(($val < 2097152) and (return sprintf('%.0f KiB', ($val / 1024)))); "
    "(($val < 10485760) and (return sprintf('%.1f MiB', ($val / 1048576)))); "
    "(($val < 2147483648) and (return sprintf('%.0f MiB', ($val / 1048576)))); "
    "(($val < 10737418240) and (return sprintf('%.1f GiB', ($val / 1073741824)))); "
    "(return sprintf('%.0f GiB', ($val / 1073741824))); } "
    "else { (($val < 2000) and (return (\"$val bytes\"))); "
    "(($val < 10000) and (return sprintf('%.1f kB', ($val / 1000)))); "
    "(($val < 2000000) and (return sprintf('%.0f kB', ($val / 1000)))); "
    "(($val < 10000000) and (return sprintf('%.1f MB', ($val / 1000000)))); "
    "(($val < 2000000000) and (return sprintf('%.0f MB', ($val / 1000000)))); "
    "(($val < 10000000000) and (return sprintf('%.1f GB', ($val / 1000000000)))); "
    "(return sprintf('%.0f GB', ($val / 1000000000))); } }":
        "Image::ExifTool::ConvertFileSize($val)",
}

# The `PrintAFPointsLeftRight`/`PrintAFPointsUpDown` bodies differ from each
# other only in the literal column/row count, so they are built rather than
# spelled out -- the deparse text is still matched in full and exactly, and the
# TRANSLATIONS key each maps to still has to exist by name below.
for _sub, _ns in (("PrintAFPointsLeftRight", (19, 21, 29)),
                  ("PrintAFPointsUpDown", (11, 13, 17))):
    for _n in _ns:
        CODE_REFS[
            "{ package Image::ExifTool::Nikon; use strict; "
            "(my($val) = @_); "
            f"{_sub}($val, {_n}); }}"
        ] = f"Image::ExifTool::Nikon::{_sub}($val, {_n})"

for _deparse, _key in CODE_REFS.items():
    if _key not in TRANSLATIONS:
        raise AssertionError(
            f"CODE_REFS maps a deparsed body onto {_key!r}, which is not in "
            "TRANSLATIONS -- a code ref must never reach Rust that has not "
            "been written down as an ordinary, oracle-checked translation"
        )


def code_ref_expr(deparse):
    """The TRANSLATIONS key naming the ExifTool sub `deparse` is, or None.

    None is the normal outcome for the overwhelming majority of code refs and
    the caller must handle it by REFUSING the conversion -- explicitly, with a
    counter and an `Omitted` flag -- never by falling back to the raw value
    silently.
    """
    if not isinstance(deparse, str):
        return None
    return CODE_REFS.get(normalize(deparse))


def normalize(expr):
    """Collapse whitespace so formatting differences don't defeat lookup.

    Deliberately conservative: it does NOT strip parentheses, reorder terms or
    canonicalise numbers.  Two expressions that differ by anything other than
    whitespace are treated as different expressions, because proving them
    equivalent is exactly the kind of reasoning that produces silent errors.
    """
    return re.sub(r"\s+", " ", expr.strip())


def translate(expr):
    """Return (rust_type, rust_expr) or None if unsupported.

    None is a normal, expected outcome and the caller must handle it by
    dropping the conversion -- not by falling back to something plausible.
    """
    if expr is None:
        return None
    return TRANSLATIONS.get(normalize(expr))


def coverage(expr_counter):
    """Report how many tag-uses the current registry covers."""
    covered = sum(n for e, n in expr_counter.items() if translate(e))
    total = sum(expr_counter.values())
    return covered, total


# =============================================================================
# compile() -- a grammar-driven compiler over the Step 15 decision gate's
# closed grammar, for everything TRANSLATIONS does not already cover by exact
# match.
#
# Same doctrine as TRANSLATIONS above, enforced by construction instead of by
# lookup: a Perl snippet is compiled only if every one of its constructs is
# one this module recognises and can reproduce exactly. Anything else --
# an unrecognised function name (`GPS::ToDegrees`, `PrintHex`, ...), a
# `tr///` range, `length($val)`, string equality on $val, a second `$val`
# interpolation, a `$$self{...}` read -- fails to parse and `compile_any`
# returns None. There is no partial mode and no fallback rendering: a
# construct this module cannot prove correct is refused, exactly like an
# unregistered TRANSLATIONS lookup.
#
# Named helpers the grammar DOES know, and how each is typed:
#   string-valued: Exif::PrintExposureTime, Exif::PrintFNumber,
#                  Exif::PrintFraction, GPS::ToDMS, Nikon::PrintPC
#                  (fully-qualified, QHELPER), ConvertDuration, ConvertBitrate
#                  (bare names -- subs of package Image::ExifTool, see
#                  _parse_ident_call), and ConvertUnixTime in both spellings
#                  (`$toLocal` only as the literal 1; the local rendering
#                  depends on the process time zone exactly as ExifTool's
#                  does, and verify_exprs.py pins TZ=UTC on both sides).
#   number-valued: Canon::CanonEv -- the one helper that returns an f64 and
#                  therefore composes under exp/log/arithmetic like `$val`.
# Every one of them compiles to a call into src/exiftool_tables/exprs.rs and
# is differentially checked by verify_exprs.py against the pinned Perl sub
# itself; none is trusted on the strength of its transcription.
#
# Three value domains fall out of the grammar rather than being declared up
# front:
#   "num"   -- $val is the field's numeric value (f64). This is the only
#              domain codegen.py's binary-table ExprId enum can host today
#              (PrintConv::Expr dispatches on a numeric raw value), so it is
#              the only domain translate_or_compile() exposes to codegen.
#   "str"   -- $val is a string (tr///, ConvertDateTime). Not wired into the
#              numeric ExprId enum; verified and reported for the coverage
#              census, ready for whichever future step gives string-typed
#              tag values a compiled-expression path.
#   "bytes" -- $val is raw bytes (Decode-UCS2, unpack("H*"), ASF::GetGUID).
#              Same status as "str".
#   "list"  -- $val is a fixed-count field's elements (`int16u[4]`), the
#              space-joined list ReadValue hands the conversion; the Rust
#              side sees `&[f64]`. See _compile_list. Perl's `.` (string
#              concatenation, `_mk_concat`) is in the scalar grammar too.
#
# ConvertDateTime is translated as the identity function deliberately, not as
# an approximation of the general case: `Image::ExifTool::ConvertDateTime`
# reformats its input only when `$self->Options('DateFormat')` is set (a `-d`
# CLI flag neither oxidex nor its comparison harnesses ever pass), and
# returns $date unchanged otherwise (Image/ExifTool.pm:6574-6578,
# 6621-6622, pinned 13.59). verify_exprs.py's oracle probes this
# under the same no-DateFormat conditions oxidex actually runs under.
# =============================================================================


class ExprCompileError(Exception):
    """Internal: this Perl construct is outside the closed grammar."""


# --- lexer -------------------------------------------------------------

_TOKEN_SPEC = [
    ("QHELPER", r"Image::ExifTool::(?:Exif::PrintExposureTime|Exif::PrintFNumber|Exif::PrintFraction|Canon::CanonEv|Nikon::PrintPC|GPS::ToDMS|ConvertUnixTime|ICC_Profile::HexID)"),
    ("SELFCONVERTDATETIME", r"\$self->ConvertDateTime"),
    ("SELFDECODE", r"\$self->Decode"),
    ("SELFTOK", r"\$self"),
    # Composite ValueConv/PrintConv/RawConv index into ExifTool's `@val`
    # array ($val[0], $val[1], ...) rather than using the scalar `$val` an
    # ordinary tag's conversion sees -- must be tried before VAL below, or
    # "$val[0]" would lex as VAL followed by unconsumed "[0]" and refuse.
    # Only used by compile_composite(); the scalar grammar never produces
    # this token because ordinary (non-Composite) conversions never contain
    # "$val[".
    ("VALIDX", r"\$val\[(\d+)\]"),
    ("VAL", r"\$val"),
    # List-domain element and whole-list references (`$v[1]`, `@v`), only
    # meaningful when _Parser was given a bound list name -- see
    # _compile_list. Listed after VAL so `$val` / `$val[N]` keep their own
    # tokens; a name is never "val".
    ("LISTIDX", r"\$([A-Za-z_]\w*)\[(\d+)\]"),
    ("LISTALL", r"@([A-Za-z_]\w*)"),
    ("NUM", r"0[xX][0-9a-fA-F]+|\d+\.\d+(?:[eE][+-]?\d+)?|\.\d+(?:[eE][+-]?\d+)?|\d+(?:[eE][+-]?\d+)?"),
    # Perl's string concatenation. After NUM so `.5` stays a number; the
    # lookahead keeps `1.` (never seen) from lexing as NUM DOT either way.
    ("DOT", r"\.(?!\d)"),
    ("STR", r'"(?:[^"\\]|\\.)*"' + r"|'(?:[^'\\]|\\.)*'"),
    ("IDENT", r"[A-Za-z_][A-Za-z_0-9]*"),
    ("POW", r"\*\*"),
    ("EQ", r"=="),
    ("NE", r"!="),
    ("GE", r">="),
    ("LE", r"<="),
    ("SHR", r">>"),
    ("GT", r">"),
    ("LT", r"<"),
    ("PLUS", r"\+"),
    ("MINUS", r"-"),
    ("STAR", r"\*"),
    ("SLASH", r"/"),
    ("PCT", r"%"),
    ("AMP", r"&"),
    ("PIPE", r"\|"),
    ("LPAREN", r"\("),
    ("RPAREN", r"\)"),
    ("QMARK", r"\?"),
    ("COLON", r":"),
    ("COMMA", r","),
    ("WS", r"\s+"),
]
_MASTER_RE = re.compile("|".join(f"(?P<{n}>{p})" for n, p in _TOKEN_SPEC))


def _tokenize(s):
    toks = []
    pos = 0
    while pos < len(s):
        m = _MASTER_RE.match(s, pos)
        if not m:
            raise ExprCompileError(f"lex error at {pos}: {s[pos:pos + 10]!r}")
        kind = m.lastgroup
        if kind != "WS":
            toks.append((kind, m.group()))
        pos = m.end()
    toks.append(("EOF", ""))
    return toks


# --- string-literal helpers ---------------------------------------------

def _rust_str_lit(s):
    """A plain (non-format!) Rust `String` literal -- no brace doubling."""
    body = s.replace("\\", "\\\\").replace('"', '\\"')
    return '"' + body + '".to_string()'


def _rust_fmt_esc(s):
    """Text destined for inside a `format!("...")` template: braces double."""
    return (
        s.replace("\\", "\\\\")
        .replace('"', '\\"')
        .replace("{", "{{")
        .replace("}", "}}")
    )


def _unescape_dq(body):
    """Minimal Perl double-quote escapes actually seen in the census."""
    out = []
    i = 0
    while i < len(body):
        c = body[i]
        if c == "\\" and i + 1 < len(body):
            nxt = body[i + 1]
            if nxt in ('"', "\\"):
                out.append(nxt)
                i += 2
                continue
            if nxt == "n":
                out.append("\n")
                i += 2
                continue
            if nxt == "t":
                out.append("\t")
                i += 2
                continue
        out.append(c)
        i += 1
    return "".join(out)


# --- $val-arithmetic parser ---------------------------------------------
#
# Parses and *emits* in one pass: every parse_* method returns a
# (vtype, rust_code) pair, vtype in {"f64", "bool", "string", "undef"}.
# rust_code always uses the literal placeholder text "{v}" wherever $val
# belongs, matching TRANSLATIONS' own convention -- codegen.py substitutes it
# with the real Rust variable name.

_MATH_FN = {"abs": "abs", "log": "ln", "exp": "exp", "sqrt": "sqrt"}
# "int" is handled separately in _mk_func1 -- it needs a distinct output
# vtype ("f64_int"), not just a different method name.


class _Parser:
    def __init__(self, toks, list_name=None, list_var=None):
        self.toks = toks
        self.i = 0
        # List-domain context (see _compile_list): `list_name` is the Perl
        # array bound by `my @NAME = split ...` (or "" for the anonymous
        # `split(" ",$val)` form), `list_var` the Rust `&[f64]` expression
        # that holds it. Outside the list domain both are None and the
        # LISTIDX/LISTALL tokens are refused.
        self.list_name = list_name
        self.list_var = list_var

    def _peek(self):
        return self.toks[self.i]

    def _at(self, kind):
        return self.toks[self.i][0] == kind

    def _eat(self, kind):
        if self.toks[self.i][0] != kind:
            raise ExprCompileError(f"expected {kind}, got {self.toks[self.i]}")
        t = self.toks[self.i]
        self.i += 1
        return t

    def parse_top(self):
        v = self.parse_ternary()
        if not self._at("EOF"):
            raise ExprCompileError(f"trailing tokens at {self._peek()}")
        return v

    def parse_ternary(self):
        cond = self.parse_and()
        if self._at("QMARK"):
            self._eat("QMARK")
            a = self.parse_ternary()
            self._eat("COLON")
            b = self.parse_ternary()
            return _mk_ternary(cond, a, b)
        return cond

    def parse_and(self):
        left = self.parse_cmp()
        while self._at("IDENT") and self._peek()[1] == "and":
            self._eat("IDENT")
            right = self.parse_cmp()
            left = _mk_and(left, right)
        return left

    def parse_cmp(self):
        left = self.parse_add()
        for kind, op in (
            ("EQ", "=="), ("NE", "!="), ("GE", ">="), ("LE", "<="), ("GT", ">"), ("LT", "<"),
        ):
            if self._at(kind):
                self._eat(kind)
                right = self.parse_add()
                return _mk_cmp(op, left, right)
        return left

    def parse_add(self):
        left = self.parse_mul()
        # Perl's `.` shares the additive precedence level with + and -
        # (perlop), left-associative like them.
        while self._at("PLUS") or self._at("MINUS") or self._at("DOT"):
            kind, op = self._eat(self._peek()[0])
            right = self.parse_mul()
            left = _mk_concat(left, right) if kind == "DOT" else _mk_binop(op, left, right)
        return left

    def parse_mul(self):
        left = self.parse_unary()
        while (self._at("STAR") or self._at("SLASH") or self._at("PCT")
               or self._at("SHR") or self._at("AMP") or self._at("PIPE")):
            op = self._eat(self._peek()[0])[1]
            right = self.parse_unary()
            left = _mk_binop(op, left, right)
        return left

    def parse_unary(self):
        if self._at("MINUS"):
            self._eat("MINUS")
            return _mk_neg(self.parse_unary())
        return self.parse_pow()

    def parse_pow(self):
        left = self.parse_atom()
        if self._at("POW"):
            self._eat("POW")
            right = self.parse_unary()
            return _mk_pow(left, right)
        return left

    def parse_atom(self):
        kind, text = self._peek()
        if kind == "NUM":
            self._eat("NUM")
            return _mk_num(text)
        if kind == "VAL":
            self._eat("VAL")
            if self.list_var is not None:
                # In the list domain `$val` is the space-joined list, which
                # only `split` (consumed by _compile_list / _parse_sprintf)
                # and the list-valued helpers read; arithmetic on it is a
                # Perl numification of "1 2 3" -- never what a table means.
                raise ExprCompileError("bare $val in a list-domain expression")
            return ("f64", "{v}")
        if kind == "VALIDX":
            _, idxtext = self._eat("VALIDX")
            idx = re.match(r"\$val\[(\d+)\]", idxtext).group(1)
            return ("f64", "{v" + idx + "}")
        if kind == "LISTIDX":
            _, idxtext = self._eat("LISTIDX")
            name, idx = re.match(r"\$([A-Za-z_]\w*)\[(\d+)\]", idxtext).groups()
            if self.list_var is None or name != self.list_name:
                raise ExprCompileError(f"element of an unbound array @{name}")
            # Perl reads a missing element as undef: 0 in arithmetic, "" in
            # a string (with a warning). list_get is the arithmetic case;
            # _compile_list handles interpolation itself.
            return ("f64", f"crate::exiftool_tables::exprs::list_get({self.list_var}, {idx})")
        if kind == "LISTALL":
            raise ExprCompileError("@array outside a sprintf argument list")
        if kind == "LPAREN":
            self._eat("LPAREN")
            v = self.parse_ternary()
            self._eat("RPAREN")
            return v
        if kind == "STR":
            self._eat("STR")
            return _mk_str_literal(text)
        if kind == "QHELPER":
            return self._parse_qhelper()
        if kind == "IDENT":
            return self._parse_ident_call(text)
        raise ExprCompileError(f"unexpected token {kind} {text!r}")

    def _parse_ident_call(self, text):
        self._eat("IDENT")
        if text == "undef":
            return ("undef", None)
        if text in ("abs", "int", "log", "exp", "sqrt"):
            self._eat("LPAREN")
            arg = self.parse_ternary()
            self._eat("RPAREN")
            return _mk_func1(text, arg)
        if text in ("IsInt", "IsFloat"):
            self._eat("LPAREN")
            arg = self.parse_ternary()
            self._eat("RPAREN")
            return _mk_predicate(text, arg)
        if text == "sprintf":
            return self._parse_sprintf()
        if text == "ConvertUnixTime":
            # ExifTool.pm:6784-6810, a sub of package Image::ExifTool like the
            # two below, so the tables call it bare as well as fully
            # qualified; both spellings share one argument parser.
            self._eat("LPAREN")
            return self._parse_convert_unix_time_args()
        if text in ("ConvertDuration", "ConvertBitrate"):
            # ExifTool.pm:6877-6895 / :6902-6913. Both are subs of package
            # Image::ExifTool, which is why a tag table can call them by bare
            # name (ExifTool evals every conversion string from inside that
            # package, ExifTool.pm:3656-3664) and why verify_exprs.py's own
            # `package Image::ExifTool;` line resolves them identically. Both
            # are pure functions of one numeric argument (the leading IsFloat
            # guard is always true for a numeric field) and return a string.
            # The argument is parsed as a full expression, not just `$val`,
            # so `ConvertDuration(int($val + 0.5))` (a real pinned call site)
            # compiles too -- hence _NUMERIC_VTYPES rather than "f64" alone,
            # since int() yields the "f64_int" vtype.
            self._eat("LPAREN")
            arg = self.parse_ternary()
            self._eat("RPAREN")
            avt, ac = _as_f64(arg)
            if avt not in _NUMERIC_VTYPES:
                raise ExprCompileError(f"{text} needs a numeric argument")
            fn = "convert_duration" if text == "ConvertDuration" else "convert_bitrate"
            return ("string", f"crate::exiftool_tables::exprs::{fn}({ac})")
        raise ExprCompileError(f"unrecognised function {text!r}")

    def _parse_qhelper(self):
        _, text = self._eat("QHELPER")
        self._eat("LPAREN")
        if text.endswith("ConvertUnixTime"):
            return self._parse_convert_unix_time_args()
        if text.endswith("ICC_Profile::HexID"):
            # ICC_Profile.pm HexID: `split(' ', $val)`, then `0` if every
            # element starts with 0, else each element as `%.2x`
            # concatenated. A list-domain helper: its argument is the
            # space-joined list itself, so it is only in grammar when
            # _compile_list has bound one (`$val` = the whole list here).
            if self.list_var is None:
                raise ExprCompileError("HexID takes the whole list; only in the list domain")
            self._eat("VAL")
            self._eat("RPAREN")
            return ("string", f"crate::exiftool_tables::exprs::icc_hex_id({self.list_var})")
        if text.endswith("PrintExposureTime") or text.endswith("PrintFNumber"):
            arg = self.parse_ternary()
            self._eat("RPAREN")
            fn = "print_exposure_time" if "PrintExposureTime" in text else "print_f_number"
            avt, ac = arg
            if avt != "f64":
                raise ExprCompileError(f"{fn} needs a numeric argument")
            return ("string", f"crate::exiftool_tables::exprs::{fn}({ac})")
        if text.endswith("Exif::PrintFraction"):
            # Exif.pm:5516-5535 -- a pure function of $val (the `defined $val`
            # guard is always true for a numeric field). String result.
            arg = self.parse_ternary()
            self._eat("RPAREN")
            avt, ac = _as_f64(arg)
            if avt not in _NUMERIC_VTYPES:
                raise ExprCompileError("PrintFraction needs a numeric argument")
            return ("string", f"crate::exiftool_tables::exprs::print_fraction({ac})")
        if text.endswith("Canon::CanonEv"):
            # Canon.pm:10650-10670 -- a pure numeric function of $val, and the
            # only QHELPER that returns a NUMBER rather than a string. That is
            # what lets it compose: every pinned call site wraps it in further
            # arithmetic (`exp(4*log(2)*(1-CanonEv($val-24)))`,
            # `exp(CanonEv($val)*log(2)/2)`, `-CanonEv($val*4)*log(2)`), so a
            # string-typed result would refuse all 35 of them at the next
            # operator. Returns the "f64" vtype so _mk_func1/_mk_neg/_mk_binop
            # accept it exactly as they accept a `$val` subexpression.
            arg = self.parse_ternary()
            self._eat("RPAREN")
            avt, ac = _as_f64(arg)
            if avt not in _NUMERIC_VTYPES:
                raise ExprCompileError("CanonEv needs a numeric argument")
            return ("f64", f"crate::exiftool_tables::exprs::canon_ev({ac})")
        if text.endswith("Nikon::PrintPC"):
            # Nikon.pm:13450-13460 -- PrintPC($val [, $norm [, $fmt [, $div]]]).
            # A pure function of $val once the trailing arguments are fixed,
            # and at every pinned call site they are LITERALS: $norm in
            # {"None", "No Sharpening", undef, absent}, $fmt in {"%.2f", "%d",
            # absent (= '%+d')}, $div in {4, absent (= 1)}. Only those
            # literals are accepted -- the PrintAFPointsLeftRight doctrine: a
            # template with a hole in it could be filled with an argument no
            # ExifTool table actually passes, so each pinned literal is
            # spelled out and anything else is refused.
            arg = self.parse_ternary()
            norm, fmt, div = "None", "PlusD", "1.0"
            if self._at("COMMA"):  # $norm
                self._eat("COMMA")
                if self._at("IDENT") and self._peek()[1] == "undef":
                    self._eat("IDENT")
                else:
                    _, s = self._eat("STR")
                    lit = s[1:-1]
                    if lit not in ("None", "No Sharpening"):
                        raise ExprCompileError(
                            f"PrintPC norm literal {lit!r} is not a pinned call site")
                    norm = f'Some("{lit}")'
                if self._at("COMMA"):  # $fmt
                    self._eat("COMMA")
                    _, s = self._eat("STR")
                    fmt = {"%.2f": "F2", "%d": "D", "%+d": "PlusD"}.get(s[1:-1])
                    if fmt is None:
                        raise ExprCompileError(
                            f"PrintPC fmt literal {s!r} is not a pinned call site")
                    if self._at("COMMA"):  # $div
                        self._eat("COMMA")
                        _, n = self._eat("NUM")
                        if n != "4":
                            raise ExprCompileError(
                                "only a PrintPC divisor of 4 is a pinned call site")
                        div = "4.0"
            self._eat("RPAREN")
            avt, ac = _as_f64(arg)
            if avt not in _NUMERIC_VTYPES:
                raise ExprCompileError("PrintPC needs a numeric argument")
            return ("string",
                    f"crate::exiftool_tables::exprs::nikon_print_pc({ac}, {norm}, "
                    f"crate::exiftool_tables::exprs::PcFmt::{fmt}, {div})")
        # GPS::ToDMS($self, $val, 1, ["N"|"E"])
        self._eat("SELFTOK")
        self._eat("COMMA")
        arg = self.parse_ternary()
        self._eat("COMMA")
        _, prec_text = self._eat("NUM")
        if prec_text != "1":
            raise ExprCompileError("only GPS::ToDMS precision 1 (doPrintConv=1) is in-grammar")
        ref = None
        if self._at("COMMA"):
            self._eat("COMMA")
            _, stext = self._eat("STR")
            if stext[0] != '"':
                raise ExprCompileError("GPS::ToDMS ref must be a double-quoted literal")
            ref = stext[1:-1]
            if ref not in ("N", "E"):
                raise ExprCompileError(f"unsupported GPS::ToDMS ref {ref!r}")
        self._eat("RPAREN")
        avt, ac = arg
        if avt != "f64":
            raise ExprCompileError("GPS::ToDMS needs a numeric argument")
        ref_rust = "None" if ref is None else f"Some('{ref}')"
        code = f"crate::exiftool_tables::exprs::gps_to_dms({ac}, {ref_rust})"
        return ("string", code)

    def _parse_convert_unix_time_args(self):
        # ExifTool.pm:6784-6810 -- ConvertUnixTime($time [, $toLocal [, $dec]]).
        # Reached from both spellings with the LPAREN already consumed. The
        # first argument is a full numeric expression: `$val + 631065600`
        # (the Mac/QuickTime epoch shift) at 58 pinned call sites, bare
        # `$val` at 21, and `$val/1e7-11644473600` behind the FILETIME entry
        # in TRANSLATIONS. `$toLocal` is accepted only as the literal `1`,
        # the sole spelling in the pinned tables; `$dec` never appears there.
        # A non-literal second argument -- `$self->Options("QuickTimeUTC")
        # || $$self{FileType} eq "CR3"` is the one in the census -- is
        # refused, not defaulted: it reads option state the generated table
        # does not have. The local rendering depends on the process time
        # zone exactly as ExifTool's does; verify_exprs.py pins TZ=UTC on
        # both sides of the oracle.
        arg = self.parse_ternary()
        to_local = "false"
        if self._at("COMMA"):
            self._eat("COMMA")
            _, n = self._eat("NUM")
            if n != "1":
                raise ExprCompileError("only ConvertUnixTime(..., 1) is a pinned call site")
            to_local = "true"
        self._eat("RPAREN")
        avt, ac = _as_f64(arg)
        if avt not in _NUMERIC_VTYPES:
            raise ExprCompileError("ConvertUnixTime needs a numeric argument")
        return ("string", f"crate::exiftool_tables::exprs::convert_unix_time({ac}, {to_local})")

    def _parse_sprintf(self):
        self._eat("LPAREN")
        fmt = self._parse_sprintf_format()
        args = []
        while self._at("COMMA"):
            self._eat("COMMA")
            if self.list_var is not None and (self._at("LISTALL") or self._is_split_source()):
                # `sprintf(FMT, @v)` / `sprintf(FMT, split(" ",$val))`: the
                # list supplies the arguments positionally. Bind exactly as
                # many elements as the format has conversions; a shorter
                # list reads undef -> 0 through list_get, which is what
                # Perl's %d/%f make of a missing argument (with a warning).
                if self._at("LISTALL"):
                    _, atext = self._eat("LISTALL")
                    if atext[1:] != self.list_name:
                        raise ExprCompileError(f"unbound array {atext}")
                else:
                    self._parse_split_source()
                n = _sprintf_arg_count(fmt)
                args.extend(
                    ("f64", f"crate::exiftool_tables::exprs::list_get({self.list_var}, {i})")
                    for i in range(n)
                )
                continue
            args.append(self.parse_ternary())
        self._eat("RPAREN")
        return _mk_sprintf(fmt, args)

    def _parse_sprintf_format(self):
        """A sprintf format: one double-quoted literal, or literals joined
        with `.` where a literal may carry Perl's `x N` repetition (`"%3d
        %4d %6d" . " %3d %4d %6d" x 10`, Sony MeterInfo). `x` binds tighter
        than `.` (perlop), so the repetition applies to the literal just
        read. Folded to one string at compile time -- there is nothing to
        evaluate at runtime."""
        _, stext = self._eat("STR")
        if stext[0] != '"':
            raise ExprCompileError("sprintf format must be double-quoted")
        fmt = stext[1:-1]
        fmt = self._maybe_repeat(fmt)
        while self._at("DOT"):
            self._eat("DOT")
            _, stext = self._eat("STR")
            if stext[0] != '"':
                raise ExprCompileError("sprintf format pieces must be double-quoted")
            fmt += self._maybe_repeat(stext[1:-1])
        return fmt

    def _maybe_repeat(self, piece):
        if self._at("IDENT") and self._peek()[1] == "x":
            self._eat("IDENT")
            _, n = self._eat("NUM")
            if not n.isdigit() or int(n) > 64:
                raise ExprCompileError("sprintf format repetition must be a small integer literal")
            return piece * int(n)
        return piece

    def _is_split_source(self):
        return self._at("IDENT") and self._peek()[1] == "split"

    def _parse_split_source(self):
        """Consume `split(" ",$val)` / `split " ",$val` / `split(' ',$val)` /
        `split ' ', $val` -- the four spellings the tables use for "the list".
        The separator must be a single space: Perl's `split " "` is the
        awk-style whitespace split ReadValue's space-joined list expects."""
        self._eat("IDENT")  # split
        paren = self._at("LPAREN")
        if paren:
            self._eat("LPAREN")
        _, sep = self._eat("STR")
        if sep[1:-1] != " ":
            raise ExprCompileError("only split on a single space is in grammar")
        self._eat("COMMA")
        self._eat("VAL")
        if paren:
            self._eat("RPAREN")


_SPRINTF_SPEC_RE = re.compile(r"%([-+0]*)(\d*)\.?(\d*)([dfxXg%])")


def _mk_sprintf(fmt, args):
    """Compile a Perl sprintf() call to a Rust format!() call.

    %g used to be refused outright because Rust's formatter has no
    significant-digits mode and approximating one is exactly the
    silent-wrong-number risk this module exists to avoid. It now routes
    through src/exiftool_tables/exprs.rs::perl_g_spec -- a real C-style %g
    over core::formatters::numeric_precision::perl_g, exponent form and
    all -- and verify_exprs.py checks every %g form against the pinned Perl
    like any other spec. Width and the -/0 flags with %g remain refused: no
    pinned call site uses them, and an unreproduced flag is refused, not
    guessed at.
    """
    # Every `%` must start a conversion this function models. Before this
    # guard, `%c` / `%s` / `%e` fell through the spec regex and were emitted
    # as literal text -- `sprintf("%d.%d%c",@a)` became `format!("{}.{}%c")`,
    # a silently wrong PrintConv. The oracle would have refused it a ledger
    # entry, but the grammar should say no itself, not lean on the oracle.
    # `%%` is a literal percent, not a conversion: strip those pairs first
    # (the first version of this guard read the second `%` of `%%` as a
    # conversion and refused every `sprintf("%.0f%%", ...)` in the tables).
    if re.search(r"%(?![-+0]*\d*\.?\d*[dfxXg])", fmt.replace("%%", "")):
        raise ExprCompileError("sprintf conversion outside the modelled set")
    parts = []
    rust_args = []
    pos = 0
    argi = 0
    while pos < len(fmt):
        m = _SPRINTF_SPEC_RE.search(fmt, pos)
        if not m:
            parts.append(_rust_fmt_esc(fmt[pos:]))
            pos = len(fmt)
            break
        parts.append(_rust_fmt_esc(fmt[pos:m.start()]))
        flags, width, prec, conv = m.groups()
        if conv == "%":
            parts.append("%")
            pos = m.end()
            continue
        if argi >= len(args):
            raise ExprCompileError("sprintf: not enough arguments")
        raw_arg = args[argi]
        argi += 1
        sign = "+" if "+" in flags else ""
        if conv == "g":
            # C's %g: `prec` significant digits, %e form below 1e-4 or at/above
            # `prec` digits, trailing zeros trimmed. This used to be refused on
            # the grounds that Rust's formatter has no significant-digits mode
            # -- true of format!, but exprs.rs::perl_g_spec (over
            # numeric_precision::perl_g, the crate's general %g) has one, and
            # verify_exprs.py checks it against the pinned Perl like every
            # other spec. Width and the -/0 flags stay out of grammar: no
            # pinned call site uses them with %g, and a flag this module does
            # not reproduce is refused, not approximated. Emitted as a
            # pre-rendered String argument behind a bare `{}` placeholder --
            # the same shape the zero-padded %d branch below uses -- because
            # the digit selection happens in perl_g_spec, not in format!.
            if width or "-" in flags or "0" in flags:
                raise ExprCompileError("sprintf %g with a width or -/0 flag is out of grammar")
            avt, ac = _as_f64(raw_arg)
            if avt not in _NUMERIC_VTYPES:
                raise ExprCompileError("sprintf argument must be numeric")
            p = int(prec) if prec else 6  # a bare %g is %.6g
            plus = "true" if "+" in flags else "false"
            parts.append("{}")
            rust_args.append(f"crate::exiftool_tables::exprs::perl_g_spec({ac}, {p}, {plus})")
            pos = m.end()
            continue
        if conv in ("x", "X"):
            # _as_bits(), not _as_f64(): a %x/%X argument that is already a
            # u64bits chain (e.g. `$val >> 8` in `sprintf("%x.%.2x",
            # $val>>8, $val&0xff)`) must go straight to u64 without visiting
            # f64 in between, for the same reason _as_f64's own docstring
            # gives -- a large shift result loses its low bits to f64's
            # 53-bit mantissa. Routing every sprintf arg through the shared
            # _as_f64() normalisation first (as an earlier version of this
            # function did) reintroduced exactly that rounding one level up;
            # verify_exprs.py's oracle caught it as the low hex digits of
            # `$val >> 8` disagreeing with Perl at a negative $val.
            hexspec = conv
            zero_width = prec or (width if ("0" in flags and width) else "")
            w = zero_width or width or ""
            zero = "0" if zero_width else ""
            parts.append("{:" + zero + w + hexspec + "}")
            rust_args.append(_as_bits(raw_arg))
            pos = m.end()
            continue
        avt, ac = _as_f64(raw_arg)
        if avt not in _NUMERIC_VTYPES:
            raise ExprCompileError("sprintf argument must be numeric")
        if conv == "f":
            p = prec or "6"
            parts.append("{:" + sign + "." + p + "}")
            rust_args.append(ac)
        elif conv == "d":
            zero_width = prec or (width if ("0" in flags and width) else "")
            if zero_width:
                # Zero-pad the *magnitude* to zero_width digits and prepend
                # any sign separately: Rust's `{:0N}` counts the sign
                # character inside N (`format!("{:010}", -1000000)` is
                # "-001000000", nine padding digits), while Perl's
                # precision/zero-flag digit count does not
                # (`sprintf("%.10d", -1000000)` is "-0001000000", ten).
                # verify_exprs.py's oracle caught this on sprintf("%.10d",
                # $val) at a negative $val, off by exactly one digit.
                block = (
                    "{ let __v = ((" + ac + ") as i64); if __v < 0 "
                    '{ format!("-{:0' + zero_width + '}", __v.unsigned_abs()) } '
                    'else { format!("{:0' + zero_width + '}", __v as u64) } }'
                )
                parts.append("{}")
                rust_args.append(block)
            else:
                w = width or ""
                parts.append("{:" + sign + w + "}")
                rust_args.append(f"(({ac}) as i64)")
        pos = m.end()
    if argi != len(args):
        raise ExprCompileError("sprintf: unused arguments")
    fmt_str = "".join(parts)
    if rust_args:
        code = 'format!("' + fmt_str + '", ' + ", ".join(rust_args) + ")"
    else:
        code = '"' + fmt_str + '".to_string()'
    return ("string", code)


def _mk_num(text):
    if text[:2].lower() == "0x":
        v = float(int(text, 16))
    else:
        v = float(text)
    # An explicit `_f64` suffix, always -- not only when a bare literal would
    # otherwise be ambiguous. A literal like `24` inside `sqrt(24*24+36*36)`
    # has no `$val` anywhere nearby to anchor its type by context, and
    # method-call resolution (`.sqrt()`) happens before Rust can look outward
    # for one; an unsuffixed literal there is a genuine "can't call method on
    # ambiguous numeric type" compile error waiting for whichever expression
    # first combines a builtin call with an all-constant argument.
    if v == int(v) and abs(v) < 1e15:
        return ("f64", f"{int(v)}.0_f64")
    return ("f64", repr(v) + "_f64")


# Vtypes flow through the parser: "f64" (Perl NV -- a plain float,
# stringifies through %.15g / perl_num), "f64_int" (Perl IV -- the result of
# int(), stringifies as exact decimal digits with no scientific notation
# regardless of magnitude, via perl_int), and "u64bits" (mid-chain result of
# >>/&/| -- see below). Perl's own type system distinguishes IV from NV at
# every operation; this compiler only tracks the one distinction
# verify_exprs.py's oracle actually found a bug from (`$val ? int($val +
# 0.5) : "n/a"` at very large $val stringifies through IV rules, not NV
# ones) and demotes to "f64" the moment a value touches any further
# arithmetic -- which is where Perl itself would generally promote to NV too
# (division always does; +-* sometimes don't, but by then the numbers
# involved are no longer close enough to $val's original magnitude for the
# distinction to matter for any real binary-table field).
_NUMERIC_VTYPES = ("f64", "f64_int")


def _stringify(vtype, code):
    """Rust text that renders a numeric vtype's value as Perl would
    stringify it -- perl_int for an IV (int()'s result), perl_num
    (%.15g) for everything else."""
    fn = "perl_int" if vtype == "f64_int" else "perl_num"
    return f"crate::exiftool_tables::exprs::{fn}({code})"


def _as_bits(node):
    """Coerce a numeric node to a Rust u64 bit-pattern expression, for a
    >>/&/| operand. Perl's bitwise operators work on the machine's native
    64-bit representation: a negative operand is not sign-extended on shift,
    it is reinterpreted as the bit pattern of an unsigned value first.
    `as i64 as u64` does exactly that (a value-preserving f64->i64
    truncation, then a same-size bit-pattern reinterpretation, not a range
    clamp) -- verify_exprs.py's oracle caught `$val >> 2` at $val = -2
    disagreeing under plain signed-shift semantics (Rust: -1, arithmetic /
    sign-extending; Perl: 4611686018427387903, logical)."""
    vt, code = node
    if vt == "u64bits":
        return code
    if vt in _NUMERIC_VTYPES:
        return f"(({code}) as i64 as u64)"
    raise ExprCompileError("bitwise operator needs a numeric operand")


def _as_f64(node):
    """Demote a u64bits chain result back to f64 the moment it touches
    anything other than another >>/&/|. Chaining bitwise ops must stay in
    u64 the whole way through, never round-tripping through f64 in between:
    `($val >> 3) & 0x3` at $val = -3 produces an intermediate shift result of
    0x1FFFFFFFFFFFFFFF, which does not fit f64's 53-bit mantissa exactly, and
    rounding it before the `& 0x3` corrupts the very low bits the mask
    reads -- verify_exprs.py's oracle caught exactly this losing the low
    bits of that AND. Real binary-table field values never approach that
    magnitude (they are bounded by their declared FORMAT width), so this
    only ever mattered for the oracle's deliberately adversarial probes --
    but "only synthetic inputs are affected" is not a reason to leave a
    provable wrong answer sitting in the compiler."""
    vt, code = node
    if vt == "u64bits":
        return ("f64", f"(({code}) as f64)")
    return node


def _mk_neg(v):
    vt, code = _as_f64(v)
    if vt not in _NUMERIC_VTYPES:
        raise ExprCompileError("unary minus needs a numeric operand")
    return ("f64", f"(-({code}))")


def _mk_binop(op, left, right):
    if op in (">>", "&", "|"):
        lb, rb = _as_bits(left), _as_bits(right)
        return ("u64bits", f"({lb} {op} {rb})")
    lt, lc = _as_f64(left)
    rt, rc = _as_f64(right)
    if lt not in _NUMERIC_VTYPES or rt not in _NUMERIC_VTYPES:
        raise ExprCompileError(f"binary {op!r} needs numeric operands")
    return ("f64", f"(({lc}) {op} ({rc}))")


def _sprintf_arg_count(fmt):
    """How many arguments a (folded) sprintf format consumes: every
    conversion except the literal `%%`."""
    return sum(1 for m in _SPRINTF_SPEC_RE.finditer(fmt) if m.group(4) != "%")


def _mk_concat(left, right):
    """Perl's `.`: both sides stringified the way Perl stringifies them --
    a string as itself, an NV through %.15g (perl_num), an IV through its
    exact digits (perl_int), and a bitwise-operator result (a UV in Perl:
    `($val >> 8) . "."` in ICC_Profile.pm's ProfileVersion) as unsigned
    digits straight off the u64. A bool or undef operand is refused: Perl's
    "1"/"" spellings of those are never what a table means by them."""
    parts = []
    for node in (left, right):
        vt, code = node
        if vt == "string":
            parts.append(code)
        elif vt == "u64bits":
            parts.append(f"({code}).to_string()")
        elif vt in _NUMERIC_VTYPES:
            if _ARITH_RESULT_RE.match(code):
                # Perl performs `+ - *` (and exact `/`) in INTEGER arithmetic
                # whenever both operands are integral, and stringifies the IV
                # result with exact digits; this compiler's f64 model prints
                # every plain result through %.15g. The two agree below 1e15
                # and part company above it -- verify_exprs.py caught
                # `$val * 1e6 . " microseconds"` printing `2.147483647e+15`
                # where Perl prints `2147483647000000` at $val = 2^31-1.
                # `_NUMERIC_VTYPES`' docstring discloses that limit for bare
                # results; a concatenation makes it reachable at 32-bit
                # magnitudes, so an arithmetic result is refused here rather
                # than approximated. `$val` itself, int(...), a helper call
                # and a bitwise result all stringify exactly and pass.
                raise ExprCompileError("concatenation of an arithmetic result (IV arithmetic not modelled)")
            parts.append(_stringify(vt, code))
        else:
            raise ExprCompileError(f"concatenation of a {vt} operand")
    return ("string", f'format!("{{}}{{}}", {parts[0]}, {parts[1]})')


# The shape _mk_binop / _mk_pow / _mk_neg emit for an arithmetic result.
_ARITH_RESULT_RE = re.compile(r"^\(\(.*\) [-+*/] \(.*\)\)$|^\(.*\)\.powf\(|^\(-\(")


def _mk_pow(left, right):
    lt, lc = _as_f64(left)
    rt, rc = _as_f64(right)
    if lt not in _NUMERIC_VTYPES or rt not in _NUMERIC_VTYPES:
        raise ExprCompileError("** needs numeric operands")
    return ("f64", f"({lc}).powf({rc})")


def _mk_func1(name, arg):
    vt, code = _as_f64(arg)
    if vt not in _NUMERIC_VTYPES:
        raise ExprCompileError(f"{name}() needs a numeric argument")
    if name == "int":
        # Perl's int() returns an integer (IV), not a float (NV) -- see
        # _NUMERIC_VTYPES' docstring above.
        return ("f64_int", f"({code}).trunc()")
    return ("f64", f"({code}).{_MATH_FN[name]}()")


def _mk_predicate(name, arg):
    vt, code = _as_f64(arg)
    if vt not in _NUMERIC_VTYPES:
        raise ExprCompileError(f"{name}() needs a numeric argument")
    if name == "IsInt":
        # Image::ExifTool::IsInt is `$_[0] =~ /^[+-]?\d+$/` -- a *string*
        # pattern on the value's own %.15g stringification, not a
        # mathematical "has no fractional part" test. The two agree for any
        # realistic binary-table field, but not at the same magnitude
        # perl_num switches to scientific notation: IsInt(1e18) is false in
        # Perl (its stringification is "1e+18", which the regex rejects),
        # not true -- verify_exprs.py's oracle caught `IsInt($val) ? "$val
        # C" : $val` disagreeing at $val = 1e18. `perl_num`'s own scientific
        # threshold is magnitude >= 1e15, so that is IsInt's threshold too.
        return (
            "bool",
            f"(({code}).fract() == 0.0 && ({code}).abs() < 1e15)",
        )
    return ("bool", f"(({code}).is_finite())")


def _as_bool(node):
    vt, code = _as_f64(node)
    if vt == "bool":
        return code
    if vt in _NUMERIC_VTYPES:
        return f"(({code}) != 0.0)"
    raise ExprCompileError("condition needs a boolean or numeric operand")


def _mk_cmp(op, left, right):
    lt, lc = _as_f64(left)
    rt, rc = _as_f64(right)
    if lt not in _NUMERIC_VTYPES or rt not in _NUMERIC_VTYPES:
        raise ExprCompileError(f"comparison {op!r} needs numeric operands")
    return ("bool", f"(({lc}) {op} ({rc}))")


def _mk_and(left, right):
    return ("bool", f"(({_as_bool(left)}) && ({_as_bool(right)}))")


def _mk_str_literal(text):
    quote = text[0]
    body = text[1:-1]
    if quote == "'":
        s = body.replace("\\'", "'").replace("\\\\", "\\")
        return ("string", _rust_str_lit(s))
    if "$val" not in body:
        if "$" in body or "@" in body:
            raise ExprCompileError("unsupported interpolation")
        return ("string", _rust_str_lit(_unescape_dq(body)))
    if body.count("$val") != 1:
        raise ExprCompileError("multiple $val interpolations are unsupported")
    if re.search(r"\$(?!val\b)|@", body):
        raise ExprCompileError("unsupported additional interpolation")
    pre, post = body.split("$val", 1)
    pre = _rust_fmt_esc(_unescape_dq(pre))
    post = _rust_fmt_esc(_unescape_dq(post))
    # perl_num, not a bare {v}: Perl's "$val" interpolation goes through
    # %.15g (scientific notation outside ~[1e-4, 1e15)), which Rust's
    # Display for f64 never does -- see exprs.rs::perl_num.
    code = (
        'format!("' + pre + "{}" + post
        + '", crate::exiftool_tables::exprs::perl_num({v}))'
    )
    return ("string", code)


def _mk_ternary(cond, a, b):
    cond_code = _as_bool(cond)
    at, ac = _as_f64(a)
    bt, bc = _as_f64(b)
    if at in _NUMERIC_VTYPES and bt in _NUMERIC_VTYPES:
        # Both branches are already valid f64 Rust expressions regardless of
        # which numeric vtype each is (f64_int is still an f64 at the Rust
        # level, only its *stringification* rule differs) -- an int()'d
        # result on one arm and a plain arithmetic result on the other still
        # unify as one Rust f64 value with no coercion needed here. Only
        # demote to plain "f64" if either side is: the branch not taken at
        # runtime is exactly the case Perl's own IV/NV distinction would
        # still track (this compiler does not), so this is a deliberate,
        # narrow, documented under-claim rather than a silent one.
        out_vtype = "f64_int" if (at == "f64_int" and bt == "f64_int") else "f64"
        return (out_vtype, f"(if {cond_code} {{ {ac} }} else {{ {bc} }})")
    if at == "undef" and bt in _NUMERIC_VTYPES:
        return ("f64_option", f"(if {cond_code} {{ None }} else {{ Some({bc}) }})")
    if at in _NUMERIC_VTYPES and bt == "undef":
        return ("f64_option", f"(if {cond_code} {{ Some({ac}) }} else {{ None }})")
    if at == "string" and bt == "string":
        return ("string", f"(if {cond_code} {{ {ac} }} else {{ {bc} }})")
    # _stringify, not a bare format!("{}", ...): Perl auto-stringifies the
    # numeric branch (through %.15g for a float, or exact digits for an
    # int()'d value) when the other branch is a string, and Rust's Display
    # for f64 matches neither rule on its own -- see exprs.rs::perl_num and
    # ::perl_int, and _NUMERIC_VTYPES' docstring above for why there are two.
    if at == "string" and bt in _NUMERIC_VTYPES:
        return (
            "string",
            f"(if {cond_code} {{ {ac} }} else {{ {_stringify(bt, bc)} }})",
        )
    if at in _NUMERIC_VTYPES and bt == "string":
        return (
            "string",
            f"(if {cond_code} {{ {_stringify(at, ac)} }} else {{ {bc} }})",
        )
    raise ExprCompileError(f"unsupported ternary branch combination {at}/{bt}")


# --- whole-expression string/bytes-domain patterns ----------------------
# These are checked before the numeric tokenizer runs: their syntax ($self->,
# tr/.../, fully-qualified Decode) is not part of the $val-arithmetic
# grammar, and each is a fixed, hand-verified shape rather than a general
# parse.

_CONVERTDATETIME_RE = re.compile(r"^\$self->ConvertDateTime\(\$val\)$")
_DECODE_UCS2_RE = re.compile(r'^\$self->Decode\(\$val,\s*"UCS2",\s*"(II|MM)"\)$')
_TR_RE = re.compile(
    r"^\$val\s*=~\s*tr/((?:[^/\\]|\\.)*)/((?:[^/\\]|\\.)*)/(d)?\s*;\s*\$val$"
)
# Two bytes-domain helpers: Perl's own `unpack("H*", $val)` and
# `Image::ExifTool::ASF::GetGUID($val)` (ASF.pm:525-533), the latter in both
# spellings the tables use -- the `require Image::ExifTool::ASF;` prefix is a
# load directive with no value and is dropped, not modelled.
_UNPACK_HEX_RE = re.compile(r'^unpack\("H\*",\s*\$val\)$')
_GETGUID_RE = re.compile(
    r"^(?:require Image::ExifTool::ASF;\s*)?Image::ExifTool::ASF::GetGUID\(\$val\)$"
)
# Three more bytes-domain spellings of "hex of the bytes" (Sony Tag9050,
# ITC::Item, Nintendo/QuickTime SchemeInfo): two hex pairs joined by a
# space, uppercase hex, and hex behind a "0x" prefix.
_UNPACK_H2_PAIRS_RE = re.compile(r'^join " ", unpack "H2H2", \$val$')
_UC_UNPACK_HEX_RE = re.compile(r'^uc unpack "H\*", \$val$')
_0X_UNPACK_HEX_RE = re.compile(r'^"0x" \. unpack\("H\*",\s*\$val\)$')

# --- the list domain -------------------------------------------------------
# A fixed-count field (`int16u[4]`, `int32u[33]`, `int8u[16]` ...) reaches its
# ValueConv/PrintConv as ONE scalar: ReadValue joins the elements with a
# space (ExifTool.pm:6286 ff.), and the conversion re-splits it. Every pinned
# shape is one of: a `sprintf` fed positionally from the list, an interpolated
# string of indexed elements, a whole-list join, or a short statement sequence
# (`my @v = split ...; $_ *= 15 foreach @v; "$v[1] $v[0] ..."`) ending in one
# of those. The Rust side sees `&[f64]` -- the elements as numbers, which is
# what the runtime's `DecodedValue::Array` decodes -- and a missing element
# reads as Perl's undef would: 0 in arithmetic, "" in a string.
_SPLIT_SRC = r"split\s*(?:\(\s*)?(?:\" \"|' ')\s*,\s*\$val\s*\)?"
_LIST_HINT_RE = re.compile(_SPLIT_SRC + r"|@[A-Za-z_]\w*|ICC_Profile::HexID")
_LIST_BIND_RE = re.compile(r"^my\s+@([A-Za-z_]\w*)\s*=\s*" + _SPLIT_SRC + r"\s*;\s*")
_LIST_HEX_RE = re.compile(r'^unpack\s+"H\*",\s*pack\s+"C\*",\s*' + _SPLIT_SRC + r"$")
_FOREACH_OP_RE = re.compile(r"^\$_\s*([*/+-])=\s*(.+?)\s+foreach\s+@([A-Za-z_]\w*)$")
_FOREACH_FMT_RE = re.compile(r"^\$_\s*=\s*sprintf\('%\.(\d+)f',\s*\$_\)\s+foreach\s+@([A-Za-z_]\w*)$")
_ELEM_OPASSIGN_RE = re.compile(r"^\$([A-Za-z_]\w*)\[(\d+)\]\s*([*/+&|-])=\s*(.+)$")
_ELEM_ASSIGN_RE = re.compile(r"^\$([A-Za-z_]\w*)\[(\d+)\]\s*=\s*(.+)$")
_INTERP_PIECE_RE = re.compile(r"\$([A-Za-z_]\w*)\[(\d+)\]|@([A-Za-z_]\w*)")


def _list_interpolation(body, name, list_var, is_str):
    """A double-quoted result string of a list-domain sequence: `"@a"`,
    `"$v[1] $v[0] $v[3] $v[2]"`, `"$a[1].$a[0].$a[3].$a[2]"`. Anything else
    interpolated is refused."""
    if "$val" in body:
        raise ExprCompileError("$val inside a list-domain string")
    parts, args, pos = [], [], 0
    for m in _INTERP_PIECE_RE.finditer(body):
        parts.append(_rust_fmt_esc(_unescape_dq(body[pos:m.start()])))
        pos = m.end()
        if m.group(3) is not None:
            if m.group(3) != name:
                raise ExprCompileError(f"unbound array @{m.group(3)}")
            parts.append("{}")
            args.append('__s.join(" ")' if is_str
                        else f"crate::exiftool_tables::exprs::list_join({list_var})")
        else:
            if m.group(1) != name:
                raise ExprCompileError(f"unbound array @{m.group(1)}")
            idx = int(m.group(2))
            parts.append("{}")
            args.append(f"__s.get({idx}).cloned().unwrap_or_default()" if is_str
                        else f"crate::exiftool_tables::exprs::list_elem_str({list_var}, {idx})")
    parts.append(_rust_fmt_esc(_unescape_dq(body[pos:])))
    tail = "".join(parts)
    if re.search(r"\$|@", tail):
        raise ExprCompileError("unsupported interpolation in a list-domain string")
    if not args:
        raise ExprCompileError("list-domain string interpolates nothing")
    return ("string", 'format!("' + tail + '", ' + ", ".join(args) + ")")


def _compile_list(s):
    """Compile one list-domain conversion (see the block comment above), or
    return None. `{v}` in the result is the `&[f64]` slice."""
    if _LIST_HEX_RE.match(s):
        return ("list", "String", "crate::exiftool_tables::exprs::list_hex({v})")
    m = _LIST_BIND_RE.match(s)
    name, rest = (m.group(1), s[m.end():]) if m else ("", s)
    if "#" in rest:
        # normalize() folded the newline that ended a `# comment`, so the
        # comment's extent is unknowable here; refuse rather than guess.
        return None
    stmts = [t.strip() for t in rest.split(";") if t.strip()]
    if not stmts:
        return None
    result = stmts[-1]
    if result.startswith("return "):
        result = result[len("return "):].strip()
    body = stmts[:-1]
    if body and not name:
        return None
    lines, is_str, mutates = [], False, False
    for st in body:
        m1 = _FOREACH_OP_RE.match(st)
        if m1 and m1.group(3) == name:
            vt, code = _as_f64(_Parser(_tokenize(m1.group(2)), name, "&__l").parse_top())
            if vt not in _NUMERIC_VTYPES:
                return None
            lines.append(f"for __x in __l.iter_mut() {{ *__x = *__x {m1.group(1)} ({code}); }}")
            mutates = True
            continue
        m2 = _FOREACH_FMT_RE.match(st)
        if m2 and m2.group(2) == name and not is_str:
            prec = int(m2.group(1))
            lines.append(
                "let __s: Vec<String> = __l.iter().map(|x| format!(\"{:."
                + str(prec) + "}\", x)).collect();"
            )
            is_str = True
            continue
        m3 = _ELEM_OPASSIGN_RE.match(st)
        if m3 and m3.group(1) == name and not is_str:
            idx, op = int(m3.group(2)), m3.group(3)
            node = _Parser(_tokenize(m3.group(4)), name, "&__l").parse_top()
            cur = f"crate::exiftool_tables::exprs::list_get(&__l, {idx})"
            if op in ("&", "|"):
                lb, rb = _as_bits(("f64", cur)), _as_bits(node)
                value = f"(({lb} {op} {rb}) as f64)"
            else:
                vt, code = _as_f64(node)
                if vt not in _NUMERIC_VTYPES:
                    return None
                value = f"({cur} {op} ({code}))"
            lines.append(
                f"{{ let __v = {value}; crate::exiftool_tables::exprs::list_set(&mut __l, {idx}, __v); }}"
            )
            mutates = True
            continue
        m4 = _ELEM_ASSIGN_RE.match(st)
        if m4 and m4.group(1) == name and not is_str:
            idx = int(m4.group(2))
            vt, code = _as_f64(_Parser(_tokenize(m4.group(3)), name, "&__l").parse_top())
            if vt not in _NUMERIC_VTYPES:
                return None
            lines.append(
                f"{{ let __v = {code}; crate::exiftool_tables::exprs::list_set(&mut __l, {idx}, __v); }}"
            )
            mutates = True
            continue
        return None
    list_var = "&__l" if body else "{v}"
    if len(result) >= 2 and result[0] == '"' and result[-1] == '"' and '"' not in result[1:-1]:
        vt, code = _list_interpolation(result[1:-1], name, list_var, is_str)
    else:
        if is_str:
            return None
        toks = _tokenize(result)
        if len(toks) == 2 and toks[0][0] == "LISTIDX":
            # A bare `$v[i]` as the whole result: Perl returns the element,
            # or undef past the end -- and an undef ValueConv result
            # suppresses the tag, which is not the same as 0 (verify_exprs.py
            # caught `my @c = split " ", $val; $c[1]` on a one-element list
            # printing 0 where Perl has UNDEF). Option, like `$val ? $val :
            # undef` in the scalar grammar.
            nm, idx = re.match(r"\$([A-Za-z_]\w*)\[(\d+)\]", toks[0][1]).groups()
            if nm != name:
                return None
            vt, code = "f64_option", f"crate::exiftool_tables::exprs::list_elem({list_var}, {idx})"
        else:
            vt, code = _Parser(toks, name, list_var).parse_top()
            vt, code = _as_f64((vt, code))
    if vt == "string":
        rty = "String"
    elif vt in _NUMERIC_VTYPES:
        rty = "f64"
    elif vt == "f64_option":
        rty = "Option<f64>"
    else:
        return None
    if body:
        decl = "let mut __l" if mutates else "let __l"
        code = f"{{ {decl}: Vec<f64> = ({{v}}).to_vec(); {' '.join(lines)} {code} }}"
    return ("list", rty, code)


def _tr_unescape_class(cls):
    """Perl tr/// character class -> literal chars, or raise if it looks like
    a range (`a-z`) -- none of the census's classes are ranges, and expanding
    one correctly (including reversed/edge cases) is not worth doing on
    spec."""
    out = []
    i = 0
    while i < len(cls):
        c = cls[i]
        if c == "\\" and i + 1 < len(cls):
            nxt = cls[i + 1]
            table = {"0": "\0", "n": "\n", "t": "\t", "/": "/", "\\": "\\"}
            if nxt not in table:
                raise ExprCompileError(f"unsupported tr/// escape \\{nxt}")
            out.append(table[nxt])
            i += 2
            continue
        if c == "-" and 0 < i < len(cls) - 1:
            raise ExprCompileError("tr/// character ranges are unsupported")
        out.append(c)
        i += 1
    return "".join(out)


def _rust_char_class_lit(chars):
    """Rust string literal holding a raw tr/// character class."""
    out = []
    for c in chars:
        if c == "\\":
            out.append("\\\\")
        elif c == '"':
            out.append('\\"')
        elif c == "\0":
            out.append("\\0")
        elif c == "\n":
            out.append("\\n")
        elif c == "\t":
            out.append("\\t")
        else:
            out.append(c)
    return '"' + "".join(out) + '"'


def compile_any(expr):
    """Compile `expr` against the closed grammar.

    Returns (domain, rust_type, rust_code) or None. `domain` is "num"
    ($val: f64), "str" ($val: &str) or "bytes" ($val: &[u8]).  `rust_code`
    uses the "{v}" placeholder, exactly like TRANSLATIONS.  Memoised: the
    codegen pass and the coverage census both call this once per distinct
    expression across an entire run, and re-parsing on every lookup would be
    wasted work for no benefit (the result is pure).
    """
    if expr is None:
        return None
    s = normalize(expr)
    if s in _COMPILE_CACHE:
        return _COMPILE_CACHE[s]
    result = _compile_uncached(s)
    _COMPILE_CACHE[s] = result
    return result


_COMPILE_CACHE = {}


def _compile_uncached(s):
    # `VALIDX` exists solely for compile_composite(): ordinary tag
    # conversions receive one scalar `$val`, while Composite conversions
    # receive `@val` (ExifTool.pm:3611-3612).  The shared lexer must know
    # both spellings, but accepting `$val[N]` here would leak a composite
    # placeholder (`{vN}`) into the scalar harness/codegen path.  Refuse it
    # before parsing; only compile_composite() may turn that syntax into a
    # positional input.  This is a soundness gate, not a convenience check.
    # See Image/ExifTool.pm:3611-3612 (pinned 13.59).
    if "$val[" in s:
        return None
    if _CONVERTDATETIME_RE.match(s):
        # Identity: ConvertDateTime only reformats when DateFormat is set;
        # Image/ExifTool.pm:6574-6578,6621-6622 (pinned 13.59). No path that
        # reaches this generator sets it.
        return ("str", "String", "{v}.to_string()")

    m = _DECODE_UCS2_RE.match(s)
    if m:
        le = "true" if m.group(1) == "II" else "false"
        code = f"crate::exiftool_tables::exprs::decode_ucs2({{v}}, {le})"
        return ("bytes", "String", code)

    if _UNPACK_HEX_RE.match(s):
        # Lowercase hex of every byte, no separators. Bytes domain ONLY:
        # codegen refuses the domain mismatch on a `string`-format field (a
        # Rust String's bytes are not the field's raw bytes once a lossy
        # decode has run), so the census's 18 uses on `string` fields stay
        # refused until the bytes path reaches them; the undef/undef[N]
        # uses (Canon LensSerialNumber, ImageUniqueID, ...) compile.
        return ("bytes", "String", "crate::exiftool_tables::exprs::unpack_hex({v})")

    if _GETGUID_RE.match(s):
        return ("bytes", "String", "crate::exiftool_tables::exprs::asf_get_guid({v})")

    if _UNPACK_H2_PAIRS_RE.match(s):
        return ("bytes", "String", "crate::exiftool_tables::exprs::unpack_h2_pairs({v})")
    if _UC_UNPACK_HEX_RE.match(s):
        return ("bytes", "String", "crate::exiftool_tables::exprs::unpack_hex({v}).to_uppercase()")
    if _0X_UNPACK_HEX_RE.match(s):
        return ("bytes", "String", 'format!("0x{}", crate::exiftool_tables::exprs::unpack_hex({v}))')

    if _LIST_HINT_RE.search(s):
        try:
            return _compile_list(s)
        except ExprCompileError:
            return None
        except (IndexError, RecursionError):
            return None

    m = _TR_RE.match(s)
    if m:
        try:
            frm = _tr_unescape_class(m.group(1))
            to = _tr_unescape_class(m.group(2))
        except ExprCompileError:
            return None
        delete = "true" if m.group(3) else "false"
        code = (
            "crate::exiftool_tables::exprs::tr_translate({v}, "
            f"{_rust_char_class_lit(frm)}, {_rust_char_class_lit(to)}, {delete})"
        )
        return ("str", "String", code)

    try:
        toks = _tokenize(s)
        vt, code = _Parser(toks).parse_top()
    except ExprCompileError:
        return None
    except (IndexError, RecursionError):
        # A malformed/adversarial token stream should refuse, not crash the
        # generator; RecursionError guards absurdly deep parenthesisation.
        return None

    # A bare bitwise expression (e.g. a whole PrintConv that is just
    # `$val & 0xff`) never gets demoted by _as_f64 -- nothing consumed it.
    vt, code = _as_f64((vt, code))

    if vt == "f64":
        return ("num", "f64", code)
    if vt == "f64_int":
        return ("num", "f64_int", code)
    if vt == "string":
        return ("num", "String", code)
    if vt == "f64_option":
        return ("num", "Option<f64>", code)
    # A bare "bool" or "undef" as the whole expression's result has no
    # corresponding PrintConv output shape -- refuse rather than guess one.
    return None


def compile(expr):  # noqa: A001 - matches translate()'s naming, deliberately
    """Numeric-domain-only compile(): the shape codegen.py's ExprId enum can
    host today. Returns (rust_type, rust_code) or None."""
    c = compile_any(expr)
    if c and c[0] == "num":
        return (c[1], c[2])
    return None


def translate_or_compile(expr):
    """TRANSLATIONS first (hand-verified, zero ambiguity), then the grammar
    compiler. Same (rust_type, rust_code) shape as translate() alone, so
    every existing call site can adopt this by name with no other change."""
    t = translate(expr)
    if t:
        return t
    return compile(expr)


def translate_or_compile_any(expr):
    """The full-system view used for coverage reporting: TRANSLATIONS union
    compile() across all three value domains, not just the numeric one
    codegen.py's ExprId enum can host. Every existing TRANSLATIONS entry
    happens to be numeric-domain, so this is translate_or_compile()'s domain
    tag plus compile_any()'s reach into "str"/"bytes"."""
    t = translate(expr)
    if t:
        return ("num", t[0], t[1])
    return compile_any(expr)


# =============================================================================
# compile_composite() -- the $val[N]-indexed sibling of compile(), for
# Composite ValueConv/PrintConv/RawConv text (codegen_composite.py's use,
# not codegen.py's). ExifTool's Composite conversions see the whole `@val`
# array, referenced as `$val[0]`, `$val[1]`, ... -- a different shape from
# the single scalar `$val` an ordinary tag's conversion sees, which is what
# compile()/translate() above assume throughout (VAL always renders as the
# lone placeholder "{v}").
#
# A bare `$val` (no brackets) is legal inside a Composite conversion too --
# not a mistake, and not the same thing as an ordinary tag's $val: ExifTool
# aliases it to $val[0] for exactly this case (ExifTool.pm:3611-3612's `$val
# = ref $conv eq 'CODE' ? \@val : $val[0];`, evaluated before every
# Composite RawConv/ValueConv/PrintConv `eval`). FLIR.pm:1313's
# `ValueConv => '14387.6515/$val'` on a single-Require composite is exactly
# this shape, not a scalar-tag conversion that wandered into this file by
# accident.
#
# Reuses every existing grammar rule (arithmetic, ternary, sprintf, ...)
# unchanged: the only new surface is the VALIDX token above and this
# pre-normalisation pass, so a construct this compiler already refuses for
# the scalar grammar (a regex match, a `split`, a `$self->` call, a second
# distinct interpolation) is refused here too, for the same reason.
_BARE_VAL_RE = re.compile(r"\$val(?!\[)")
_VPLACEHOLDER_RE = re.compile(r"\{v(\d+)\}")
_COMPOSITE_COMPILE_CACHE = {}


def compile_composite(expr):
    """Compile a Composite conversion expression over `$val[N]` (and bare
    `$val`, aliased to `$val[0]`) into Rust.

    Returns `(rust_type, rust_code, indices_used)` -- `rust_code` uses
    "{v0}", "{v1}", ... placeholders, one per referenced index, and
    `indices_used` is the sorted list of ints actually referenced (so a
    caller building a fixed-arity Inputs slice knows which positions the
    generated code actually reads). Returns None if `expr` is missing or
    falls outside the closed grammar, exactly like compile() -- refused and
    counted, never approximated.
    """
    if expr is None:
        return None
    s = normalize(expr)
    s = _BARE_VAL_RE.sub("$val[0]", s)
    if s in _COMPOSITE_COMPILE_CACHE:
        return _COMPOSITE_COMPILE_CACHE[s]
    result = _compile_composite_uncached(s)
    _COMPOSITE_COMPILE_CACHE[s] = result
    return result


def _compile_composite_uncached(s):
    try:
        toks = _tokenize(s)
        vt, code = _Parser(toks).parse_top()
    except ExprCompileError:
        return None
    except (IndexError, RecursionError):
        return None

    vt, code = _as_f64((vt, code))
    if vt == "f64":
        rust_type = "f64"
    elif vt == "f64_int":
        rust_type = "f64_int"
    elif vt == "string":
        rust_type = "String"
    elif vt == "f64_option":
        rust_type = "Option<f64>"
    else:
        # A bare "bool" or "undef" as the whole expression has no
        # corresponding ValueConv/PrintConv output shape.
        return None

    indices = sorted({int(m) for m in _VPLACEHOLDER_RE.findall(code)})
    if not indices:
        # A composite conversion that never actually reads $val[N] anywhere
        # (a bare numeric/string literal) is not a real translation of a
        # Composite -- ExifTool composites always derive from their inputs.
        return None
    return (rust_type, code, indices)


def known_num_domain_exprs():
    """TRANSLATIONS union every compile()-covered num-domain expression seen
    so far in this process, keyed by normalized expression text. codegen.py
    uses this instead of iterating TRANSLATIONS alone once compile() is in
    the loop, or every grammar-covered ExprId reference in the generated
    source would be a variant with no enum arm."""
    out = dict(TRANSLATIONS)
    for e, result in _COMPILE_CACHE.items():
        if result and result[0] == "num" and e not in out:
            out[e] = (result[1], result[2])
    return out


def known_exprs():
    """Every translation compiler has accepted in this process, by text.

    Unlike :func:`known_num_domain_exprs`, this includes the string and byte
    domains.  R2's ValueConv carriage and the PrintConv string path need this
    complete set to build an ExprId for a conversion the differential oracle
    has approved.  The caller must still consult that oracle's ledger before
    shipping an entry; this function is only an enum-construction inventory.
    """
    out = dict(TRANSLATIONS)
    for e, result in _COMPILE_CACHE.items():
        if result and e not in out:
            out[e] = (result[1], result[2])
    return out
