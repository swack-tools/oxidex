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
# an unrecognised function name (`ConvertDuration`, `CanonEv`, ...), a `tr///`
# range, `length($val)`, string equality on $val, a second `$val`
# interpolation -- fails to parse and `compile_any` returns None. There is no
# partial mode and no fallback rendering: a construct this module cannot
# prove correct is refused, exactly like an unregistered TRANSLATIONS lookup.
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
#   "bytes" -- $val is raw bytes (Decode-UCS2). Same status as "str".
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
    ("QHELPER", r"Image::ExifTool::(?:Exif::PrintExposureTime|Exif::PrintFNumber|GPS::ToDMS)"),
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
    ("NUM", r"0[xX][0-9a-fA-F]+|\d+\.\d+(?:[eE][+-]?\d+)?|\.\d+(?:[eE][+-]?\d+)?|\d+(?:[eE][+-]?\d+)?"),
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
    def __init__(self, toks):
        self.toks = toks
        self.i = 0

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
        while self._at("PLUS") or self._at("MINUS"):
            op = self._eat(self._peek()[0])[1]
            right = self.parse_mul()
            left = _mk_binop(op, left, right)
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
            return ("f64", "{v}")
        if kind == "VALIDX":
            _, idxtext = self._eat("VALIDX")
            idx = re.match(r"\$val\[(\d+)\]", idxtext).group(1)
            return ("f64", "{v" + idx + "}")
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
        raise ExprCompileError(f"unrecognised function {text!r}")

    def _parse_qhelper(self):
        _, text = self._eat("QHELPER")
        self._eat("LPAREN")
        if text.endswith("PrintExposureTime") or text.endswith("PrintFNumber"):
            arg = self.parse_ternary()
            self._eat("RPAREN")
            fn = "print_exposure_time" if "PrintExposureTime" in text else "print_f_number"
            avt, ac = arg
            if avt != "f64":
                raise ExprCompileError(f"{fn} needs a numeric argument")
            return ("string", f"crate::exiftool_tables::exprs::{fn}({ac})")
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

    def _parse_sprintf(self):
        self._eat("LPAREN")
        _, stext = self._eat("STR")
        if stext[0] != '"':
            raise ExprCompileError("sprintf format must be double-quoted")
        fmt = stext[1:-1]
        args = []
        while self._at("COMMA"):
            self._eat("COMMA")
            args.append(self.parse_ternary())
        self._eat("RPAREN")
        return _mk_sprintf(fmt, args)


_SPRINTF_SPEC_RE = re.compile(r"%([-+0]*)(\d*)\.?(\d*)([dfxXg%])")


def _mk_sprintf(fmt, args):
    """Compile a Perl sprintf() call to a Rust format!() call.

    %g is refused outright: Rust's formatter has no significant-digits mode,
    and approximating one is exactly the kind of silent-wrong-number risk
    this module exists to avoid.
    """
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
        if conv == "g":
            raise ExprCompileError("sprintf %g has no exact Rust equivalent")
        if argi >= len(args):
            raise ExprCompileError("sprintf: not enough arguments")
        raw_arg = args[argi]
        argi += 1
        sign = "+" if "+" in flags else ""
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
