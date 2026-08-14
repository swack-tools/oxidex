#!/usr/bin/env python3
"""Generate the declarative half of ExifTool's Composite tag definitions.

Composite tags are ExifTool's derivation layer: ImageSize, Megapixels,
Aperture, ShutterSpeed, FocalLength35efl, DOF, HyperfocalDistance and friends
are not read from the file at all. They are computed from tags that have
already been extracted.

That makes them the cheapest coverage in the project. In a 190-file corpus they
account for the ten most-missed tag names outright -- ~500 missing instances --
and every input they need is already being parsed correctly. No new format
work, no byte offsets, no per-camera quirks. One engine, and every format
gains at once.

What is emitted here is the dependency graph -- name, group, Require/Desire/
Inhibit lists -- plus, as of Step 29, whatever computation exprs.py's
`compile_composite` can prove correct: a ValueConv (and, if present,
PrintConv) that is pure arithmetic over `$val[N]` (ExifTool's `@val` array)
or the single-input `$val` alias ExifTool.pm:3611-3612 gives it. That closed
grammar is emitted to `generated_compute.rs`. Everything outside it is
hand-ported by a human into `src/composite/compute.rs` and looked up by
name; a composite with no arm in EITHER file is left with no computation and
simply never fires -- reported, not guessed, by this generator's own "no
registered computation" triage line. That keeps the same rule as the
binary-table generator: the dependency graph is transcribed, the semantics
are ported (by hand or by the closed-grammar compiler) deliberately, and
nothing is guessed.
"""

import argparse
import json
import os
import re

import exprs

# Composites whose inputs ExifTool populates from internal parser state rather
# than from a named tag. We cannot see that state, so we refuse them outright
# instead of emitting a definition that could half-fire on the wrong inputs.
#
# RawImageCroppedSize used to live here, but it no longer belongs: it is not
# internal Perl state, it is a named FujiFilm RAF tag (0x0111,
# FujiFilm.pm:1289) that src/parsers/raw/raf_parser.rs emits as
# `RAF:RawImageCroppedSize`. Filtering it out of Composite:ImageSize's Desire
# list here silently dropped Exif.pm:4747-4766's `return $val[4] if $val[4]`
# branch -- see Step 8 of OVERHAUL_OXIDEX_PLAN.md.
INTERNAL_STATE = set()


def rust_str(s):
    return (s.replace("\\", "\\\\").replace('"', '\\"')
             .replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t"))


def priority(tag):
    """ExifTool's effective priority for a Composite tag, as FoundTag computes it.

    ExifTool.pm:9347-9351 (line numbers throughout are from the release named
    by `.exiftool-version`, which is the only release this generator reads):

        my $priority = $$tagInfo{Priority};
        unless (defined $priority) {
            $priority = $$tbl{PRIORITY};
            $priority = 0 if not defined $priority and $$tagInfo{Avoid};
        }

    `%Image::ExifTool::Composite` declares no table-level `PRIORITY`
    (ExifTool.pm:2256-2262), so an undeclared Composite falls through to the
    "normal default" of 1 at ExifTool.pm:9440. This is what decides whether the
    Composite claims the *bare* tag key from a same-named extracted tag, and so
    whether another Composite's unqualified dependency binds it.

    `Priority` beats `Avoid` when both are present: GPS.pm:371-372 sets
    `Avoid => 1, Priority => 1` on GPSLatitude precisely because "Avoid sets
    default Priority to 0", and the explicit 1 is what takes effect.
    """
    p = tag.get("Priority")
    if p is not None:
        try:
            return int(p)
        except (TypeError, ValueError):
            pass
    elif tag.get("Avoid"):
        return 0
    return 1


# Cross-reference for the "no registered computation" triage line: a static
# regex scan of compute.rs's `match (module, name) { ... }` arms, not an
# import of compute.rs (it is hand-written Rust, not something this Python
# generator can execute). This is deliberately the SAME technique used to
# read `Require`/`Desire`/`Inhibit` off the Perl side -- read the real thing,
# do not re-derive it by hand -- just aimed at a Rust source file instead of
# a Perl one. A pattern this regex fails to recognise (reformatted arms,
# guard clauses) undercounts "implemented" rather than overcounts it, which
# is the safe direction: it would falsely flag an implemented composite as
# unimplemented, not silently hide a real gap.
_COMPUTE_ARM_RE = re.compile(
    r'^\s*\(\s*"([A-Za-z0-9_]+)"\s*,\s*("[A-Za-z0-9_]+"(?:\s*\|\s*"[A-Za-z0-9_]+")*)\)\s*=>',
    re.M,
)


def implemented_pairs(compute_rs_path):
    """(module, name) pairs with a hand-written arm in compute.rs's big
    `match (module, name)`, expanding `"A" | "B" | "C"` alternation arms."""
    try:
        with open(compute_rs_path, encoding="utf-8") as fh:
            text = fh.read()
    except OSError:
        return set()
    pairs = set()
    for m in _COMPUTE_ARM_RE.finditer(text):
        mod = m.group(1)
        for name in re.findall(r'"([A-Za-z0-9_]+)"', m.group(2)):
            pairs.add((mod, name))
    return pairs


def _expr_text(node):
    """Pull the Perl source text out of dump_tables.pl's `{"expr": ..., "kind":
    "expr"}` wrapper, or None if this slot is absent, a hash/list PrintConv, or
    an opaque compiled closure (`kind` other than `"expr"`)."""
    if not isinstance(node, dict):
        return None
    if node.get("kind") != "expr":
        return None
    e = node.get("expr")
    return e if isinstance(e, str) else None


def try_compile_generated(mod_name, name, tag, req, des):
    """Attempt to auto-derive a compute.rs-style arm for a composite that has
    no hand-written implementation, via exprs.compile_composite() over its
    ValueConv and (if present) exprs.translate_or_compile() over its
    PrintConv -- the "pure-@val expression composite" path Step 29 adds
    alongside the hand-written one. Returns Rust source text for one `match`
    arm, or None if anything here falls outside the closed grammar (refused
    and counted by the caller, exactly like an unregistered compute.rs pair).
    """
    vc_expr = _expr_text(tag.get("ValueConv"))
    compiled = exprs.compile_composite(vc_expr)
    if compiled is None:
        return None
    rust_type, value_code, indices = compiled
    if rust_type not in ("f64", "f64_int"):
        # String/Option<f64> ValueConv results are real ExifTool shapes but
        # have no candidate in the pinned tables today; refuse rather than
        # guess how a future one should be wired into Computed.
        return None

    # Sanity check: every index the compiled ValueConv actually reads must be
    # one this composite's own Require/Desire declares -- otherwise codegen
    # and the compiled expression have silently drifted apart (a
    # Require/Desire this generator filtered for INTERNAL_STATE, or an
    # exprs.py bug), and emitting the arm anyway would read past what
    # `apply()` actually populates in `Inputs`.
    declared = {i for i, _ in req} | {i for i, _ in des}
    if not set(indices) <= declared:
        return None

    print_code = None
    print_rust_type = None
    pc_node = tag.get("PrintConv")
    if pc_node is not None:
        pc_expr = _expr_text(pc_node)
        translated = exprs.translate_or_compile(pc_expr)
        if translated is None:
            return None
        print_rust_type, print_code = translated
        if print_rust_type not in ("f64", "f64_int", "String"):
            return None

    lines = [f"let v{idx} = f(get(i, {idx}))?;" for idx in indices]
    value_expr = value_code
    for idx in indices:
        value_expr = value_expr.replace("{v" + str(idx) + "}", f"v{idx}")
    # `{ ... }`, not a bare assignment: the compiled expression is already
    # fully parenthesised by exprs.py's own convention, and a bare `let
    # value: f64 = (...)  ;` trips rustc's unnecessary-parens lint on the
    # outermost pair. The block form is warning-free without post-processing
    # the generated text to strip parens it may or may not have.
    lines.append(f"let value: f64 = {{ {value_expr} }};")

    stringify = "perl_int" if rust_type == "f64_int" else "perl_num"
    value_str = f"crate::exiftool_tables::exprs::{stringify}(value)"

    if print_code is None:
        print_str = value_str
    else:
        print_expr = print_code.replace("{v}", "value")
        if print_rust_type == "String":
            print_str = print_expr
        else:
            pstringify = "perl_int" if print_rust_type == "f64_int" else "perl_num"
            print_str = f"crate::exiftool_tables::exprs::{pstringify}({print_expr})"

    lines.append(
        f"Some(Computed {{ value: {value_str}, print: {print_str} }})"
    )
    body = "\n            ".join(lines)
    return (
        f'        ("{rust_str(mod_name)}", "{rust_str(name)}") => {{\n'
        f"            {body}\n"
        f"        }}"
    )


def dep_list(d):
    """ExifTool keys Require/Desire by position: {0 => 'ImageWidth', ...}.

    Position matters -- the Perl conversions index $val[0], $val[1] -- so the
    list is emitted in numeric key order, not hash order.
    """
    if d is None:
        return []
    if isinstance(d, str):
        return [(0, d)]
    if isinstance(d, dict):
        out = []
        for k, v in d.items():
            if not isinstance(v, str):
                continue
            try:
                out.append((int(k), v))
            except ValueError:
                continue
        return sorted(out)
    return []


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("tables_json")
    ap.add_argument("-o", "--out", required=True)
    ap.add_argument(
        "--generated-out",
        help="Where to write the auto-derived $val[N]-expression arms "
        "(default: generated_compute.rs next to --out).",
    )
    args = ap.parse_args()

    with open(args.tables_json, encoding="utf-8") as fh:
        doc = json.load(fh)

    compute_rs = os.path.join(os.path.dirname(args.out), "compute.rs")
    implemented = implemented_pairs(compute_rs)

    rows = []
    seen = set()
    seen_had_inhibit = {}
    skipped_internal = 0
    dropped_internal = 0
    emitted_pairs = []  # (module, name), one per row, in emission order
    generated_rows = []  # Rust match arms auto-derived by compile_composite()
    generated_pairs = set()  # (module, name) already covered by a generated arm

    for mod_name in sorted(doc["modules"]):
        tables = doc["modules"][mod_name]["tables"]
        tbl = tables.get("Composite")
        if not tbl:
            continue
        for tag_name in sorted(tbl["tags"]):
            tag = tbl["tags"][tag_name]
            name = tag.get("Name")
            if not isinstance(name, str) or not name:
                continue
            req = dep_list(tag.get("Require"))
            des = dep_list(tag.get("Desire"))
            if not req and not des:
                continue
            # A REQUIRED input we cannot see means the composite can never fire
            # correctly, so refuse it outright.
            if any(d in INTERNAL_STATE for _i, d in req):
                skipped_internal += 1
                continue
            # A merely DESIRED one is different in kind: the composite still
            # computes from its required inputs, and ExifTool itself treats the
            # input as optional. Only that entry is dropped, so the emitted
            # definition claims no input this project cannot supply.
            #
            # Refusing the whole composite here instead -- which this generator
            # did until the rule was made precise -- silently deleted
            # `Exif::ImageSize`, whose ValueConv opens `return $val[4] if
            # $val[4]` on RawImageCroppedSize but otherwise derives the size
            # from ImageWidth/ImageHeight. That cost ImageSize on every file,
            # and Megapixels with it (it requires ImageSize), to avoid
            # disagreeing with ExifTool on raw files that carry a crop. The
            # drop went unnoticed because the committed tables predated the
            # rule; nothing regenerated them, so nothing ever applied it.
            kept_des = [(i, d) for i, d in des if d not in INTERNAL_STATE]
            dropped_internal += len(des) - len(kept_des)
            des = kept_des
            inh = dep_list(tag.get("Inhibit"))
            kept_inh = [(i, d) for i, d in inh if d not in INTERNAL_STATE]
            dropped_internal += len(inh) - len(kept_inh)
            inh = kept_inh
            # First definition wins, matching ExifTool's module load order for
            # the common tags; later modules override only for their own
            # files, which we do not model -- UNLESS `Inhibit` is what tells
            # the two apart. ExifTool ships exactly this shape twice: Exif.pm
            # defines both `LensID` (Require LensType) and `LensID-2`
            # (Desire-only, `Inhibit => {4 => 'Composite:LensID'}` --
            # Exif.pm:5362-5385), and XMP.pm's own `LensID` carries the same
            # `Inhibit => {6 => 'Composite:LensID'}` (XMP.pm:2789-2801). Both
            # are real, load-bearing alternates -- LensID-2/XMP:LensID are
            # ExifTool's fallback when the numeric LensType-based primary
            # can't fire -- and deduping them away by Name alone (as this
            # generator did before) silently dropped both from the table:
            # sorted-by-tagID iteration visits "LensID" before "LensID-2"
            # within Exif, so the plain first-definition-wins rule kept only
            # the primary and threw away the fallback ExifTool actually
            # ships. Every OTHER same-(module,Name) collision in the pinned
            # tables (Kodak WB_RGBLevels/WB_RGBLevels2, QuickTime
            # GPSAltitude/GPSAltitude2 and its GPSLatitude/GPSLongitude
            # siblings, RIFF Duration/Duration2) resolves purely through
            # different Require'd inputs and declares no Inhibit at all, so
            # this leaves their existing first-wins behavior untouched.
            key = (mod_name, name)
            if key in seen and not inh and not seen_had_inhibit.get(key):
                continue
            seen.add(key)
            if inh:
                seen_had_inhibit[key] = True

            groups = tag.get("Groups") or {}
            g2 = groups.get("2", "") if isinstance(groups, dict) else ""
            r = ", ".join(f'({i}, "{rust_str(d)}")' for i, d in req)
            s = ", ".join(f'({i}, "{rust_str(d)}")' for i, d in des)
            h = ", ".join(f'({i}, "{rust_str(d)}")' for i, d in inh)
            rows.append(
                f'    Composite {{ name: "{rust_str(name)}", module: "{rust_str(mod_name)}", '
                f'group2: "{rust_str(g2)}", priority: {priority(tag)}, '
                f"require: &[{r}], desire: &[{s}], inhibit: &[{h}] }},"
            )
            emitted_pairs.append((mod_name, name))

            pair = (mod_name, name)
            if pair not in implemented and pair not in generated_pairs:
                arm = try_compile_generated(mod_name, name, tag, req, des)
                if arm is not None:
                    generated_rows.append(arm)
                    generated_pairs.add(pair)

    body = "\n".join(rows)
    with open(args.out, "w", encoding="utf-8") as fh:
        fh.write(f'''//! ExifTool Composite tag definitions, generated from ExifTool's Perl tables.
//!
//! DO NOT EDIT. Regenerate with `just regen-tables`.
//!
//! Composite tags are derived, not read: `Megapixels` comes from `ImageSize`,
//! which comes from `ImageWidth`/`ImageHeight`. Only the dependency graph is
//! generated here. The arithmetic lives in `super::compute`, is written by
//! hand, and is looked up by name -- a composite with no implementation simply
//! never fires, rather than producing an approximation.

/// One Composite tag: a name and the tags it is derived from.
#[derive(Clone, Copy, Debug)]
pub struct Composite {{
    pub name: &'static str,
    /// ExifTool module that defined it, kept for provenance.
    pub module: &'static str,
    pub group2: &'static str,
    /// ExifTool's effective tag priority, which decides whether this Composite
    /// claims the *bare* tag key from a same-named extracted tag.
    ///
    /// `FoundTag` keeps the higher-priority tag under the unsuffixed key and
    /// pushes the loser to `Name (1)` (ExifTool.pm:9442-9464); an unqualified
    /// `Require`/`Desire` reads only the bare key (ExifTool.pm:4008). An
    /// ordinarily-extracted tag counts as 1, so a Composite left at the default
    /// 1 wins the name and a `Priority => 0` Composite -- `Canon:ISO`'s "let
    /// EXIF:ISO take priority" (Canon.pm:9781-9782) -- yields to it.
    pub priority: i8,
    /// All of these indexed inputs must be present or the tag does not fire.
    pub require: &'static [(usize, &'static str)],
    /// Indexed optional inputs; absent positions are passed through as `None`.
    pub desire: &'static [(usize, &'static str)],
    /// If ANY of these indexed dependencies resolves to a value, this
    /// Composite does not fire at all this pass -- ExifTool's
    /// `BuildCompositeTags` (ExifTool.pm:4070-4079) treats an `Inhibit`
    /// entry as the mirror image of `Require`: present means refuse,
    /// absent is simply not an input. Used by exactly two ExifTool
    /// composites, both yielding to a same-named primary that can produce
    /// a better answer when it fires: Exif.pm's `LensID-2` (the
    /// LensModel/Lens text fallback) inhibits on `Composite:LensID` (the
    /// LensType-based primary), and XMP.pm's own `LensID` (the
    /// XMP-aux:LensID numeric fallback) inhibits on the same target.
    pub inhibit: &'static [(usize, &'static str)],
}}

/// Every Composite definition ExifTool declares ({len(rows)} total).
pub static COMPOSITES: &[Composite] = &[
{body}
];
''')

    generated_out = args.generated_out or os.path.join(
        os.path.dirname(args.out), "generated_compute.rs"
    )
    gen_body = "\n".join(generated_rows)
    with open(generated_out, "w", encoding="utf-8") as fh:
        fh.write(f'''//! Composite conversions compiled from ExifTool's Perl ValueConv/PrintConv
//! text by codegen_composite.py's `$val[N]` grammar compiler
//! (`tools/exiftool-tables/exprs.py`'s `compile_composite`).
//!
//! DO NOT EDIT. Regenerate with `just regen-tables`.
//!
//! This is the automatic sibling of `compute.rs`: a Composite whose
//! ValueConv (and, if present, PrintConv) is pure arithmetic over `$val[N]`
//! (ExifTool's `@val` array) -- or, for a single-input composite, the bare
//! `$val` ExifTool.pm:3611-3612 aliases to `$val[0]` -- compiles here with
//! zero hand-written code. Anything outside that closed grammar is refused
//! and counted by this generator's own "no registered computation" triage
//! line instead of being approximated, the same rule `compute.rs`'s
//! hand-written match follows. `compute::compute` only ever consults this
//! file for a `(module, name)` pair it has no arm of its own for.
#![allow(unused_parens)] // exprs.py's compiled arithmetic is fully
// parenthesised by construction (see exprs.py's own _mk_binop et al.); that
// is correct and deliberate, not something worth post-processing away, the
// same call binary_tables.rs already makes for the same reason.
use super::compute::{{Computed, Inputs, f, get}};

pub(super) fn compute_generated(module: &str, name: &str, i: Inputs) -> Option<Computed> {{
    match (module, name) {{
{gen_body}
        _ => None,
    }}
}}
''')

    # Triage line for Step 29 (R6): a Composite that made it into COMPOSITES
    # but has no arm in EITHER compute.rs's hand-written match or this
    # generator's own $val[N]-compiled fallback never fires, and until now
    # nothing said so -- it was just absent, the exact silent-drop class
    # this whole overhaul exists to close. This is a STATIC fact about the
    # generated/hand-written files agreeing (or not) with each other, not a
    # runtime measurement, so it belongs in this generator's existing
    # report rather than a new channel.
    covered = implemented | generated_pairs
    unimplemented = [p for p in emitted_pairs if p not in covered]

    print(f"wrote {args.out}")
    print(f"wrote {generated_out}")
    print(f"  composites emitted        {len(rows)}")
    print(f"  skipped (internal)        {skipped_internal}")
    print(f"  desires dropped           {dropped_internal}")
    print(f"  auto-derived ($val[N])    {len(generated_pairs)}")
    if generated_pairs:
        for mod, name in sorted(generated_pairs):
            print(f"    {mod}::{name}")
    print(f"  no registered computation (never fire)   {len(unimplemented)}")
    if unimplemented:
        for mod, name in unimplemented:
            print(f"    {mod}::{name}")


if __name__ == "__main__":
    main()
