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

    # --- units ------------------------------------------------------------
    '"$val mm"':             ("String", 'format!("{} mm", {v})'),
    '"$val m"':              ("String", 'format!("{} m", {v})'),
    '"$val C"':              ("String", 'format!("{} C", {v})'),
    '"$val s"':              ("String", 'format!("{} s", {v})'),
    '"$val%"':               ("String", 'format!("{}%", {v})'),

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
         'if {v} > 655.345 { "inf".to_string() } else { format!("{} m", {v}) }'),
}


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
