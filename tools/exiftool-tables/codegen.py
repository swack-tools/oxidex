#!/usr/bin/env python3
"""Generate Rust binary-table definitions from extracted ExifTool tables.

Scope is deliberately narrow: ExifTool's ProcessBinaryData tables -- the ones
carrying FORMAT/FIRST_ENTRY and a field per byte offset.  They are where the
maker-note coverage gap lives, because they are the part `exiftool -listx`
cannot express.  `-listx` gives names; these give layout, and layout is what you
need to actually read bytes out of a MakerNote.  Membership is decided by
PROCESS_PROC alone: a table ExifTool reads some other way (deciphered first,
or keyed by sequence rather than offset) is not a flat byte record, and
emitting it as one yields plausible integers at meaningless offsets.

Every skipped tag is counted and reported, and `REPORT` is asserted to cover
every counter the code keeps.  A generator that silently drops what it cannot
handle produces a coverage number that is a lie, and the whole argument for
doing this mechanically rests on the numbers being trustworthy.  The same rule
governs the semantics it cannot reproduce: `Mask` is transcribed, and an
omitted `ValueConv`/`RawConv`/`Condition` is recorded in the emitted schema so
that a caller can tell the raw value from the reported one.
"""

import argparse
import hashlib
import json
import re
from collections import Counter

import conds
import exprs

# ExifTool format names -> (Rust Fmt variant, byte width).  Sized formats
# (string[32]) are handled separately since their width is per-field.
SCALAR_FORMATS = {
    "int8u": ("Int8u", 1),
    "int8s": ("Int8s", 1),
    "int16u": ("Int16u", 2),
    "int16s": ("Int16s", 2),
    "int32u": ("Int32u", 4),
    "int32s": ("Int32s", 4),
    "int16uRev": ("Int16uRev", 2),
    "float": ("Float", 4),
    "double": ("Double", 8),
    "rational64u": ("Rational64u", 8),
    "rational64s": ("Rational64s", 8),
}

SIZED_RE = re.compile(r"^(\w+)\[(\d+)\]$")


def rust_str(s):
    """Escape a Python str into a Rust string literal body."""
    out = s.replace("\\", "\\\\").replace('"', '\\"')
    out = out.replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t")
    return out


def parse_index(key):
    """ExifTool binary-table keys are decimal offsets, sometimes fractional.

    A key like `12.1` is a bit-field within the byte at offset 12.  We keep the
    integer part and record the sub-index rather than inventing bit semantics
    we have not verified.
    """
    try:
        if "." in key:
            whole, frac = key.split(".", 1)
            return int(whole), int(frac)
        return int(key, 0), None
    except ValueError:
        return None, None


# PrintConv directives that change only how an unmatched value is displayed,
# never which string a matched key maps to. Dropping these leaves every emitted
# entry exactly right, so they cost nothing but are still counted.
#
# BITMASK and OTHER are different in kind: both add a fallback that produces a
# rendering for values the plain map does not contain (DecodeBits for the
# former, a Perl sub for the latter), so an enum carrying one is genuinely
# partial. The exact-match entries stay -- they are verified against ExifTool
# and are what it would print -- but they are counted apart from complete
# enums, because reporting a partial transcription as a whole one is how the
# coverage number stops meaning anything.
BENIGN_PC_DIRECTIVES = {"Notes", "PrintHex", "PrintSort", "SeparateTable"}


def conv_for(tag, stats):
    """Return Rust PrintConv construction, or None if this tag must lose it."""
    pc = tag.get("PrintConv")
    if not isinstance(pc, dict):
        return "PrintConv::None"

    kind = pc.get("kind")
    if kind in ("enum", "enum_partial"):
        m = pc.get("map") or {}
        if not m:
            return "PrintConv::None"
        # `enum_partial` means dump_tables.pl saw directive keys alongside the
        # map; `directives` is a dict of them (null when there are none).
        # Record which, so the accounting says what was lost rather than
        # scoring the result as a complete enum.
        directives = sorted(
            d for d in (pc.get("directives") or {})
            if d not in BENIGN_PC_DIRECTIVES
        )
        partial = bool(directives)
        for d in directives:
            stats["pc_directives_dropped"][d] += 1
        # An enum whose keys are all integers becomes a sorted i64 table so the
        # runtime can binary-search it; otherwise fall back to string keys.
        pairs = []
        all_int = True
        for k in m:
            try:
                pairs.append((int(k, 0), m[k]))
            except ValueError:
                all_int = False
                break
        if all_int:
            stats["enum_int_partial" if partial else "enum_int"] += 1
            pairs.sort()
            body = ", ".join(f'({k}, "{rust_str(v)}")' for k, v in pairs)
            return f"PrintConv::IntEnum(&[{body}])"
        stats["enum_str_partial" if partial else "enum_str"] += 1
        body = ", ".join(
            f'("{rust_str(k)}", "{rust_str(v)}")' for k, v in sorted(m.items())
        )
        return f"PrintConv::StrEnum(&[{body}])"

    if kind == "expr":
        raw = pc.get("expr")
        t = exprs.translate_or_compile(raw)
        if t:
            # translate_or_compile() tries the hand-verified TRANSLATIONS
            # exact-match table first, then Step 15's grammar compiler
            # (exprs.compile()) -- counted separately so a coverage report
            # can tell curated translations from mechanically-derived ones.
            if exprs.normalize(raw) in exprs.TRANSLATIONS:
                stats["expr_translated"] += 1
            else:
                stats["expr_compiled"] += 1
            # Translated expressions are emitted by name so the generated code
            # stays readable and the mapping stays auditable.
            return f"PrintConv::Expr(ExprId::{expr_ident(raw)})"
        stats["expr_unsupported"] += 1
        stats["unsupported_exprs"][exprs.normalize(raw or "")] += 1
        return "PrintConv::None"

    stats["conv_dropped"] += 1
    return "PrintConv::None"


def expr_ident(expr):
    """Stable, collision-free Rust identifier for a translated expression.

    The readable part strips non-alphanumerics, which makes distinct
    expressions collide: `$val` and `"$val%"` both reduce to `Val`. A collision
    silently aliases two conversions to one enum variant and renders `50` where
    `50%` was meant -- compiles, passes name/enum verification, wrong output.
    The digest suffix is what makes the identifier injective; it is derived from
    the normalized expression so it is stable across runs and machines.
    """
    n = exprs.normalize(expr)
    ident = re.sub(r"[^A-Za-z0-9]+", "_", n).strip("_")
    if not ident:
        ident = "Empty"
    if ident[0].isdigit():
        ident = "E" + ident
    camel = "".join(p[:1].upper() + p[1:] for p in ident.split("_"))[:40]
    # No separator: an underscore here would trip `non_camel_case_types`, and
    # the digest is fixed-width so the boundary stays unambiguous anyway.
    digest = hashlib.sha256(n.encode("utf-8")).hexdigest()[:6].upper()
    return f"{camel}{digest}"


def is_binary_table(meta):
    """True for tables ExifTool reads with ProcessBinaryData.

    Detected by PROCESS_PROC rather than by the presence of FORMAT. FORMAT is
    optional -- ExifTool's ProcessBinaryData does `$$tagTablePtr{FORMAT} ||
    'int8u'` -- so requiring it silently skipped 365 tables / 4,844 tags whose
    fields each carry their own Format. PROCESS_PROC is what ExifTool itself
    dispatches on, so it is the honest signal.
    """
    pp = meta.get("PROCESS_PROC")
    if not isinstance(pp, dict):
        return False
    return (pp.get("__name") or "").endswith("ProcessBinaryData")


def mask_for(tag, stats):
    """Return the Rust `mask:` member for a tag, honouring ExifTool's BitShift.

    ExifTool reduces a masked field to `($val & Mask) >> BitShift` before any
    conversion runs, so a table that declares a Mask is describing a slice of
    the word, not the word. Ignoring the key -- which this generator used to do,
    with no counter and no slot in the emitted schema -- reported the whole word
    under the real tag name and then looked the enum up with it, so a field like
    `DjVu::Info` Orientation (Mask 0x7, enum keyed 1/2/5/6) usually matched
    nothing and printed a raw number instead.

    BitShift is taken from the table when stated and derived from the lowest set
    bit otherwise, exactly as ExifTool does; see the note in dump_tables.pl for
    why deriving it unconditionally is wrong.
    """
    raw = tag.get("Mask")
    if raw is None:
        return "None"
    try:
        mask = int(str(raw), 0)
    except (TypeError, ValueError):
        stats["tag_mask_unreadable"] += 1
        return None
    if mask == 0:
        # ExifTool guards both the BitShift derivation and the application with
        # `if $mask`, so a zero mask is no mask -- not a dropped field.
        return "None"
    if mask < 0 or mask > 0xFFFF_FFFF_FFFF_FFFF:
        stats["tag_mask_unreadable"] += 1
        return None

    declared = tag.get("BitShift")
    if declared is None:
        shift = (mask & -mask).bit_length() - 1
    else:
        try:
            shift = int(str(declared), 0)
        except (TypeError, ValueError):
            stats["tag_mask_unreadable"] += 1
            return None
        if shift < 0 or shift > 63:
            stats["tag_mask_unreadable"] += 1
            return None
    stats["tag_masked"] += 1
    return f"Some(Mask {{ bits: {mask:#x}, shift: {shift} }})"


def omitted_for(tag, stats, condition_resolved=False):
    """Flag the semantics ExifTool applies that this schema does not reproduce.

    `condition_resolved` is set by `compile_variant_group`'s call through
    `gen_field_literal`: a `_variants` alternative's `Condition` is exactly
    what picked this `Field` in the first place (`conds.py` compiled it, and
    `Cond::eval`/`first_match` at runtime apply it before this `Field` is ever
    reached) -- by the time a caller holds the winning alternative, there is
    no unresolved `Condition` left to flag `omitted.condition` over. Setting
    it anyway (as the single-entry path correctly still does for its own,
    uncompiled, `Condition`) would make `DecodedField::emit` refuse a field
    Step 23 just finished proving it can resolve, silently erasing the whole
    point of compiling it.

    `ValueConv`, `RawConv` and `Condition` are Perl, so the mechanical pass
    cannot run them. It used to drop them without a counter and without a slot
    in the emitted schema, which made the omission unknowable downstream:
    `DecodedField::apply_print_conv_to_raw` documented a precondition ("only
    after verifying that the field has no intervening ValueConv") that no caller
    could actually check, and a field whose PrintConv is keyed on post-ValueConv
    values then rendered a confident wrong string from the raw one.

    Refusing these fields outright is not an option -- around a third of the
    emitted set carries one, including tables parsers already read for layout --
    so the omission is recorded instead. A caller that sees `value_conv` set
    knows the raw value is not the reported value, which is the whole point.

    `Hook` and `SubDirectory` are the same kind of breach and were, until now,
    not even read from the dump: a field carrying either was emitted as an
    ordinary scalar with no record that a semantic was dropped. `Hook` is a
    Perl closure that can rewrite the format or byte order of *later* fields
    mid-table (ExifTool.pm's ProcessBinaryData `#[...]` Hook mechanism) --
    unrunnable here, same as RawConv. `SubDirectory` means the bytes are not
    this field's value at all but the entry point to a nested table -- readable
    as a scalar, but the scalar is not what ExifTool reports for the tag.
    """
    flags = []
    for key, member in (
        ("ValueConv", "value_conv"),
        ("RawConv", "raw_conv"),
        ("Condition", "condition"),
        ("Hook", "hook"),
        ("SubDirectory", "subdirectory"),
    ):
        if member == "condition" and condition_resolved:
            continue
        if tag.get(key) is not None:
            flags.append(member)
            stats[f"omitted_{member}"] += 1
    if not flags:
        return "Omitted::NONE"
    return "Omitted { " + ", ".join(
        f"{m}: {'true' if m in flags else 'false'}"
        for m in ("value_conv", "raw_conv", "condition", "hook", "subdirectory")
    ) + " }"


def _merge_stats(dst, src):
    """`dst.update(src)` done by hand: `Counter.update` adds values via `+=`,
    which breaks for the two keys (`unsupported_exprs`,
    `pc_directives_dropped`) whose value is itself a nested `Counter` rather
    than an int -- `0 + Counter(...)` has no `__radd__`. Nested Counters
    merge key-wise; everything else adds as a plain int count.
    """
    for key, value in src.items():
        if isinstance(value, Counter):
            dst.setdefault(key, Counter())
            dst[key].update(value)
        else:
            dst[key] += value


def gen_field_literal(
    tag, idx, sub, stats, var_sound_until, record_offset_hazard=True, condition_resolved=False
):
    """Build one `Field {...}` Rust literal for `tag` at offset `idx`/`sub`.

    Shared by the plain (one-entry) field path and `compile_variant_group`'s
    per-alternative path below -- the two must apply identical Format/Mask/
    PrintConv/Omitted rules, or a variant alternative's semantics would
    silently diverge from a non-variant tag's at the same offset for no
    ExifTool-side reason.

    Always returns a `(field_src, updated_var_sound_until)` pair -- `field_src`
    is `None` on refusal (the various `stats` counters record why, same keys
    the single-field path has always used), but `updated_var_sound_until`
    must still propagate even then: a refused `var_*` field is exactly what
    *establishes* the hazard boundary in the first place (see the `var_`
    branch below), so a caller that discarded the pair on refusal would lose
    the one piece of state a var_* refusal exists to produce -- silently
    reverting every later field in the table to the unsound static-offset
    formula with no `offsets_sound_until` guard at all. `record_offset_hazard`
    is false for a variant alternative: the group compiler checks the
    *group's* offset against the hazard boundary once, after every
    alternative has run (see `compile_variant_group`), rather than once per
    alternative competing for the same offset -- `tag_offset_unsound` counts
    affected offsets, and a variant group is one offset no matter how many
    alternatives it lists.
    """
    name = tag.get("Name")
    if not isinstance(name, str) or not name:
        stats["tag_no_name"] += 1
        return None, var_sound_until
    if tag.get("Unknown"):
        stats["tag_unknown_skipped"] += 1
        return None, var_sound_until

    # A per-field Format overrides the table FORMAT. `count` is the number
    # of repetitions of that format (ExifTool's `format[N]` array syntax);
    # 1 for every scalar field.
    f = tag.get("Format")
    fmt_expr = "None"
    count = 1
    if isinstance(f, str):
        m = SIZED_RE.match(f)
        if m:
            base, n = m.group(1), int(m.group(2))
            if base == "string":
                fmt_expr = f"Some(Fmt::Str({n}))"
            elif base == "undef":
                fmt_expr = f"Some(Fmt::Undef({n}))"
            elif base in SCALAR_FORMATS:
                # An array of a scalar format, e.g. `int16u[4]`: n repeats
                # of the element format, not n bytes.
                fmt_expr = f"Some(Fmt::{SCALAR_FORMATS[base][0]})"
                count = n
            else:
                stats["tag_fmt_unsupported"] += 1
                return None, var_sound_until
        elif f in SCALAR_FORMATS:
            fmt_expr = f"Some(Fmt::{SCALAR_FORMATS[f][0]})"
        else:
            # Variable-length or expression-sized format -- not mechanical.
            if f.startswith("var_"):
                stats["tag_var_format"] += 1
                if var_sound_until is None:
                    # Only the first one anchors the table's soundness
                    # boundary; a second var_* field past it is already
                    # inside the region the first one made unsound.
                    var_sound_until = idx
            stats["tag_fmt_unsupported"] += 1
            return None, var_sound_until

    mask = mask_for(tag, stats)
    if mask is None:
        # A Mask this schema cannot express means the field's value is not
        # the word at that offset. Omit it rather than report the word.
        return None, var_sound_until

    pc = conv_for(tag, stats)
    if sub is not None:
        # A fractional key names a slice of the word at int(key); `Mask` is
        # what says which bits. The runtime decodes the ones that declare
        # one and refuses the rest, so the split is the honest measure of
        # how much of ExifTool's bit-field notation this schema reaches.
        # Both halves are emitted either way -- this counts, it does not
        # gate.
        stats["tag_fractional_masked" if mask != "None" else "tag_fractional_bare"] += 1
    if record_offset_hazard and var_sound_until is not None and idx > var_sound_until:
        stats["tag_offset_unsound"] += 1
    sub_s = "None" if sub is None else f"Some({sub})"
    field_src = (
        f'Field {{ index: {idx}, sub: {sub_s}, name: "{rust_str(name)}", '
        f"format: {fmt_expr}, count: {count}, mask: {mask}, "
        f"omitted: {omitted_for(tag, stats, condition_resolved)}, print_conv: {pc} }}"
    )
    return field_src, var_sound_until


def compile_variant_group(tag, idx, sub, stats, var_sound_until):
    """Compile a `_variants` array (`dump_tables.pl`'s arrayref-of-alternatives
    shape) into a `VariantGroup`, per Step 23.

    All-or-nothing, same doctrine as `exprs.py`'s TRANSLATIONS lookup: this
    refuses the WHOLE group -- not just the alternative that failed -- the
    moment any alternative's `Condition` falls outside `conds.py`'s closed
    grammar, or any alternative itself hits one of the ordinary per-field
    refusal reasons (`Unknown`, no `Name`, an unsupported `Format`/`Mask`).
    Partially compiling an array (keeping the alternatives that work, silently
    dropping the rest) would change *first-match-wins* order: dropping the
    alternative that would have won for some models lets a later, wrong,
    alternative win instead under that model, which is a wrong value under a
    real tag name -- exactly what `AGENTS.md` forbids. See `conds.py`'s
    module docstring for the same argument in more detail.

    Trial-compiles into a throwaway `Counter` (`mask_for`/`conv_for`/
    `omitted_for` all write into whatever `stats` they are given) and only
    merges it into the real `stats` -- plus increments `tag_variant_emitted`
    -- once every alternative has compiled; a refused group instead
    increments exactly one of the `tag_variant_*_unsupported` counters once
    for the whole array, leaving no partial-credit residue (an `enum_int` or
    `expr_compiled` bump for an alternative that never actually shipped)
    in the coverage report.

    Returns `(group_src, updated_var_sound_until)` or `None`.
    """
    variants = tag["_variants"]
    trial_stats = Counter()
    # `conv_for` writes into these two nested Counters (diagnostic listings,
    # not part of the coverage arithmetic REPORT checks), so the trial copy
    # needs its own, or the first write inside a refused-and-discarded trial
    # would crash on a bare `int` where a `Counter` was expected.
    trial_stats["unsupported_exprs"] = Counter()
    trial_stats["pc_directives_dropped"] = Counter()
    alt_srcs = []
    local_var_sound_until = var_sound_until
    for v in variants:
        if not isinstance(v, dict) or "_variants" in v:
            # Nested variants: never observed in the pinned corpus's
            # binary-table population -- refuse rather than guess at a shape
            # nothing exercises or verifies.
            stats["tag_variant_cond_unsupported"] += 1
            return None
        cond_src = conds.compile_cond(v.get("Condition"))
        if cond_src is None:
            stats["tag_variant_cond_unsupported"] += 1
            return None
        field_src, local_var_sound_until = gen_field_literal(
            v,
            idx,
            sub,
            trial_stats,
            local_var_sound_until,
            record_offset_hazard=False,
            condition_resolved=True,
        )
        if field_src is None:
            stats["tag_variant_field_unsupported"] += 1
            return None
        alt_srcs.append(f"({cond_src}, {field_src})")

    _merge_stats(stats, trial_stats)
    stats["tag_variant_emitted"] += 1
    if local_var_sound_until is not None and idx > local_var_sound_until:
        # One offset, counted once, regardless of how many alternatives
        # compete for it -- see gen_field_literal's `record_offset_hazard`
        # doc.
        stats["tag_offset_unsound"] += 1

    sub_s = "None" if sub is None else f"Some({sub})"
    alts_body = ", ".join(alt_srcs)
    group_src = f"VariantGroup {{ index: {idx}, sub: {sub_s}, alternatives: &[{alts_body}] }}"
    return group_src, local_var_sound_until


def gen_table(mod_name, tbl_name, tbl, stats):
    meta = tbl.get("meta") or {}
    # PROCESS_PROC is the only honest signal for "this is a flat byte record".
    # Checking it only when FORMAT is absent -- which this generator used to do
    # -- let 46 tables through that ExifTool does not read with
    # ProcessBinaryData: 31 Sony tables deciphered before parsing, 7 read with
    # ProcessSerialData (Canon::AFInfo's keys are sequence numbers, not
    # offsets), and a handful of bespoke parsers. Emitted as flat records they
    # yield plausible integers at offsets that mean nothing, under real ExifTool
    # tag names. codegen_subdirs.py hard-errors on this same construct.
    if not is_binary_table(meta):
        stats["table_not_binary"] += 1
        return None
    fmt_name = meta.get("FORMAT")
    if not isinstance(fmt_name, str):
        # ExifTool's ProcessBinaryData does `$$tagTablePtr{FORMAT} || 'int8u'`.
        fmt_name = "int8u"
    default_fmt = SCALAR_FORMATS.get(fmt_name)
    if not default_fmt:
        stats["table_bad_format"] += 1
        return None

    try:
        first_entry = int(str(meta.get("FIRST_ENTRY", "0")), 0)
    except ValueError:
        first_entry = 0

    rows = []
    variant_rows = []
    # ProcessBinaryData computes every field's byte offset as `int(index) *
    # increment` -- a static formula that assumes every preceding field is a
    # fixed width. A `var_*` Format (`var_string`, `var_int16u`, ...) breaks
    # that assumption: ExifTool reads it by walking the actual bytes and shifts
    # every later field by however many bytes that field consumed, which this
    # generator cannot compute (the width is data-dependent) and does not try
    # to. The first such field we refuse marks the offset above which the
    # static formula is no longer trustworthy for anything still emitted in
    # this table; `var_sound_until` is that field's index, `var_sound_hit`
    # records whether any emitted field actually lands past it (a table can
    # refuse a var_* field and still have nothing after it to protect).
    var_sound_until = None
    var_sound_hit = False
    for key, tag in sorted(tbl["tags"].items(), key=lambda kv: parse_index(kv[0])[0] or 0):
        idx, sub = parse_index(key)
        if idx is None:
            stats["tag_bad_index"] += 1
            continue
        if "_variants" in tag:
            # Step 23: `dump_tables.pl`'s arrayref-of-alternatives shape,
            # ExifTool's own representation of a model-dependent layout
            # (Canon CameraInfo's 33 alternatives, Sony ExtraInfo3's
            # NEX-vs-everything-else CameraOrientation). Compiled via the
            # closed `Cond` grammar (`conds.py`) when every alternative's
            # `Condition` and per-field shape allow it; refused (and counted
            # by exactly one of the `tag_variant_*_unsupported` reasons)
            # atomically otherwise -- see `compile_variant_group`.
            built = compile_variant_group(tag, idx, sub, stats, var_sound_until)
            if built is None:
                stats["tag_variant_skipped"] += 1
                continue
            group_src, var_sound_until = built
            if var_sound_until is not None and idx > var_sound_until:
                var_sound_hit = True
            variant_rows.append(f"    {group_src},")
            continue

        field_src, var_sound_until = gen_field_literal(tag, idx, sub, stats, var_sound_until)
        if field_src is None:
            continue
        if var_sound_until is not None and idx > var_sound_until:
            var_sound_hit = True
        rows.append(f"    {field_src},")
        stats["tag_emitted"] += 1

    if not rows and not variant_rows:
        return None

    stats["table_emitted"] += 1
    ident = re.sub(r"[^A-Za-z0-9]", "_", f"{mod_name}_{tbl_name}").upper()
    groups = meta.get("GROUPS") or {}
    g0 = groups.get("0", "") if isinstance(groups, dict) else ""
    g2 = groups.get("2", "") if isinstance(groups, dict) else ""

    # Only record the flag when it actually protects a currently-emitted
    # field. A table that refuses a var_* field with nothing emitted after it
    # (the field was trailing, or everything past it was refused for other
    # reasons too) has no live hazard for this schema to guard against.
    if var_sound_hit:
        stats["table_offsets_unsound"] += 1
        sound_until_expr = f"Some({var_sound_until})"
    else:
        sound_until_expr = "None"

    body = "\n".join(rows)
    variants_body = "\n".join(variant_rows)
    variants_expr = (
        "&[]" if not variant_rows else f"&[\n{variants_body}\n    ]"
    )
    return f"""
/// `Image::ExifTool::{mod_name}::{tbl_name}` -- {len(rows)} fields,
/// {len(variant_rows)} `_variants` groups (Step 23).
/// Generated from ExifTool's in-memory tag table. Do not edit by hand.
pub static {ident}: BinaryTable = BinaryTable {{
    module: "{mod_name}",
    table: "{tbl_name}",
    group0: "{rust_str(g0)}",
    group2: "{rust_str(g2)}",
    first_entry: {first_entry},
    default_format: Fmt::{default_fmt[0]},
    offsets_sound_until: {sound_until_expr},
    fields: &[
{body}
    ],
    variants: {variants_expr},
}};
"""


PRELUDE = '''//! ExifTool binary tag tables, generated from ExifTool's own Perl hashes.
//!
//! DO NOT EDIT. Regenerate with:
//!
//! ```sh
//! perl tools/exiftool-tables/dump_tables.pl <exiftool>/lib > tables.json
//! python3 tools/exiftool-tables/codegen.py tables.json -o <this file>
//! ```
//!
//! Only ExifTool's ProcessBinaryData tables are emitted here -- the ones with a
//! FORMAT and a field per offset. That is deliberate: those tables carry the
//! byte layout that `exiftool -listx` does not expose, and layout is what a
//! reader actually needs. Tags whose conversions could not be reproduced
//! exactly are emitted without the conversion or omitted, never approximated;
//! the generator prints a full accounting of what it dropped.

#![allow(clippy::unreadable_literal, clippy::too_many_lines, unused_parens)]
// unused_parens: Step 15's expression compiler (tools/exiftool-tables/exprs.py
// `compile()`) wraps every subexpression in explicit parentheses so operator
// precedence never depends on Rust's grouping matching Perl's -- that is a
// correctness guarantee, not sloppiness, and some of those parens are
// syntactically redundant once the surrounding expression is fixed.

__VERSION_BLOCK__

// `EffectSource` (`Cond::SetMember`) is imported unconditionally like its
// sibling `Cond`/`CmpOp`/`VariantGroup` types even though this particular
// generation run may not have compiled any `Condition` using the
// assignment-as-condition idiom (see `src/exiftool_tables/cond.rs`) -- a
// conditional `use` would depend on which conditions the pinned tree happens
// to carry this release, which is exactly the kind of generator-output
// nondeterminism `codegen.py`'s own module doc warns against elsewhere.
#[allow(unused_imports)]
use super::cond::{CmpOp, Cond, EffectSource, VariantGroup};

/// A binary-table field format.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fmt {
    Int8u,
    Int8s,
    Int16u,
    Int16s,
    /// 16-bit integer read with the opposite endianness to the record.
    Int16uRev,
    Int32u,
    Int32s,
    Float,
    Double,
    Rational64u,
    Rational64s,
    /// `string[N]`: N bytes, truncated at the first NUL.
    Str(u32),
    /// `undef[N]`: N raw bytes.
    Undef(u32),
}

impl Fmt {
    #[must_use]
    pub const fn size(self) -> u32 {
        match self {
            Fmt::Int8u | Fmt::Int8s => 1,
            Fmt::Int16u | Fmt::Int16s | Fmt::Int16uRev => 2,
            Fmt::Int32u | Fmt::Int32s | Fmt::Float => 4,
            Fmt::Double | Fmt::Rational64u | Fmt::Rational64s => 8,
            Fmt::Str(n) | Fmt::Undef(n) => n,
        }
    }
}

/// How a raw value is rendered for display.
///
/// `None` is load-bearing: it means either the tag genuinely has no conversion,
/// or the generator refused to reproduce one it could not verify. Both cases
/// yield the raw value, which is honest, rather than a guess.
#[derive(Clone, Copy, Debug)]
pub enum PrintConv {
    None,
    /// Sorted by key; look up with `binary_search_by_key`.
    IntEnum(&'static [(i64, &'static str)]),
    StrEnum(&'static [(&'static str, &'static str)]),
    Expr(ExprId),
}

/// ExifTool's `Mask`/`BitShift` pair: the field is a slice of the word.
///
/// `ProcessBinaryData` reduces the value to `(val & bits) >> shift` before any
/// conversion runs, so a `PrintConv` on a masked field is keyed on the reduced
/// value, never on the whole word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mask {
    pub bits: u64,
    /// Taken from the table's `BitShift` when it states one, otherwise the
    /// lowest set bit of `bits` -- the same rule ExifTool applies.
    pub shift: u32,
}

impl Mask {
    /// Reduce a raw word to the field's value.
    #[must_use]
    pub const fn apply(self, val: i64) -> i64 {
        ((val as u64 & self.bits) >> self.shift) as i64
    }
}

/// Semantics ExifTool applies to a field that this schema does not reproduce.
///
/// These are Perl, so the mechanical transcription cannot run them. Recording
/// which were dropped is what lets a caller tell "the raw value is the value"
/// from "the raw value is an input to something you still have to do". Any flag
/// set means the decoded value is NOT what ExifTool would report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Omitted {
    /// A `ValueConv` stood between the raw bytes and the value. A `PrintConv`
    /// on such a field is keyed on the converted value, so applying it to the
    /// raw one renders a confident wrong string.
    pub value_conv: bool,
    /// A `RawConv` ran first. ExifTool's common idioms (`$val ? $val : undef`)
    /// suppress the tag entirely for some inputs.
    pub raw_conv: bool,
    /// The field is gated on a `Condition`; ExifTool may not report it at all.
    pub condition: bool,
    /// A `Hook` ran before this field was read. ExifTool's ProcessBinaryData
    /// `Hook` is a Perl closure that can rewrite the format, byte order or
    /// size of fields *after* this one mid-table; the mechanical pass cannot
    /// run it, so the field it decorates -- and everything downstream that a
    /// live Hook could have altered -- is only as trustworthy as an
    /// unconditional read at this offset.
    pub hook: bool,
    /// The bytes at this offset are not this field's value: they are the
    /// entry point to a nested `SubDirectory` table. Decoding them as a
    /// scalar yields a plausible integer that is not what ExifTool reports
    /// for this tag.
    pub subdirectory: bool,
}

impl Omitted {
    pub const NONE: Self = Self {
        value_conv: false,
        raw_conv: false,
        condition: false,
        hook: false,
        subdirectory: false,
    };

    /// True when anything was dropped, i.e. the raw value stands alone.
    #[must_use]
    pub const fn any(self) -> bool {
        self.value_conv || self.raw_conv || self.condition || self.hook || self.subdirectory
    }
}

/// One field within a binary table.
#[derive(Clone, Copy, Debug)]
pub struct Field {
    /// Offset in units of the table's default format.
    pub index: i64,
    /// Sub-index for bit-fields (ExifTool's `12.1` notation).
    pub sub: Option<u32>,
    pub name: &'static str,
    /// Overrides the table default when present.
    pub format: Option<Fmt>,
    /// Number of repetitions of `format` (ExifTool's `format[N]` array
    /// syntax), e.g. 4 for `int16u[4]`. 1 for scalar fields.
    pub count: usize,
    /// Present when the field is a slice of the word at `index`.
    pub mask: Option<Mask>,
    /// What ExifTool does to this field that the transcription does not.
    pub omitted: Omitted,
    pub print_conv: PrintConv,
}

/// A `ProcessBinaryData` table.
#[derive(Clone, Copy, Debug)]
pub struct BinaryTable {
    pub module: &'static str,
    pub table: &'static str,
    pub group0: &'static str,
    pub group2: &'static str,
    pub first_entry: i64,
    pub default_format: Fmt,
    /// `ProcessBinaryData` computes every field's byte offset as `int(index) *
    /// increment`, which assumes every preceding field is a fixed width. A
    /// refused `var_*` field (data-dependent width, e.g. a length-prefixed
    /// string) breaks that assumption: ExifTool shifts every later field by
    /// however many bytes the variable one actually consumed, an amount this
    /// schema cannot compute. `Some(n)` means the static formula is sound only
    /// for fields with `index < n`; a field at or past `n` is at a nominal
    /// offset that may not be its real one. `None` means either no refused
    /// `var_*` field exists in this table, or none of the emitted fields fall
    /// after it.
    pub offsets_sound_until: Option<i64>,
    pub fields: &'static [Field],
    /// Step 23's `_variants` schema: offsets ExifTool's own table declares
    /// as a Perl arrayref of model-dependent alternatives (`dump_tables.pl`'s
    /// `_variants`), compiled to a [`crate::exiftool_tables::cond::Cond`]
    /// per alternative. Disjoint from `fields` by construction -- an offset
    /// with a compiled `_variants` group never also appears in `fields` --
    /// so existing code walking `fields` alone sees exactly what it always
    /// has. Resolve with
    /// [`crate::exiftool_tables::decode_binary_table_variants`].
    pub variants: &'static [VariantGroup],
}

impl BinaryTable {
    /// Byte offset of `field` from the start of the record.
    ///
    /// ExifTool's `ProcessBinaryData` computes `$entry = int($index) *
    /// $increment` (ExifTool.pm) -- the tag index scales from the start of
    /// the data block unconditionally. `FIRST_ENTRY` never enters that
    /// arithmetic; it only bounds the `Unknown > 1` auto-scan range, so
    /// subtracting it here shifted every field of a `FIRST_ENTRY 1` table
    /// one format-width early.
    #[must_use]
    pub fn byte_offset(&self, field: &Field) -> i64 {
        field.index * i64::from(self.default_format.size())
    }

    #[must_use]
    pub fn field_format(&self, field: &Field) -> Fmt {
        match field.format {
            Some(f) => f,
            None => self.default_format,
        }
    }
}

impl PrintConv {
    /// Render `val`, or return `None` to fall back to the raw value.
    #[must_use]
    pub fn apply(&self, val: i64) -> Option<String> {
        match self {
            PrintConv::None => None,
            PrintConv::IntEnum(m) => m
                .binary_search_by_key(&val, |(k, _)| *k)
                .ok()
                .map(|i| m[i].1.to_string()),
            PrintConv::StrEnum(m) => {
                let key = val.to_string();
                m.iter().find(|(k, _)| *k == key).map(|(_, v)| (*v).to_string())
            }
            PrintConv::Expr(e) => e.apply(val as f64),
        }
    }
}
'''


def gen_expr_enum(used):
    """Emit the ExprId enum for every translated expression actually used."""
    if not used:
        return (
            "\n/// No Perl expressions were translated in this build.\n"
            "#[derive(Clone, Copy, Debug)]\npub enum ExprId {}\n\n"
            "impl ExprId {\n"
            "    #[must_use]\n"
            "    pub fn apply(&self, _val: f64) -> Option<String> { None }\n}\n"
        )
    variants = "\n".join(f"    /// `{rust_str(e)}`\n    {i}," for i, e in sorted(used.items()))
    arms = []
    for ident, expr in sorted(used.items()):
        rty, rexpr = exprs.translate_or_compile(expr)
        body = rexpr.replace("{v}", "val")
        # perl_num, not a bare format!("{}", ...) / format!("{v}"): Perl's
        # own numeric-to-string conversion goes through %.15g (scientific
        # notation outside ~[1e-4, 1e15)), which Rust's Display for f64 never
        # produces on its own -- see exprs.rs::perl_num's doc comment for the
        # verify_exprs.py failure that found this.
        if rty == "f64":
            arms.append(
                f"            ExprId::{ident} => "
                f"Some(crate::exiftool_tables::exprs::perl_num({body})),"
            )
        elif rty == "f64_int":
            # Perl's int() returns an integer (IV): exact decimal digits,
            # never %.15g's scientific notation, regardless of magnitude --
            # see exprs.rs::perl_int and exprs.py's _NUMERIC_VTYPES docstring.
            arms.append(
                f"            ExprId::{ident} => "
                f"Some(crate::exiftool_tables::exprs::perl_int({body})),"
            )
        elif rty == "String":
            arms.append(f"            ExprId::{ident} => Some({body}),")
        else:  # Option<f64>
            arms.append(
                f"            ExprId::{ident} => "
                f"({body}).map(crate::exiftool_tables::exprs::perl_num),"
            )
    arm_body = "\n".join(arms)
    return f"""
/// Perl conversions with a hand-verified Rust equivalent.
///
/// Each variant corresponds to one entry in `tools/exiftool-tables/exprs.py`.
/// Adding an entry there fixes every tag sharing that expression at once.
#[derive(Clone, Copy, Debug)]
pub enum ExprId {{
{variants}
}}

impl ExprId {{
    #[must_use]
    pub fn apply(&self, val: f64) -> Option<String> {{
        match self {{
{arm_body}
        }}
    }}
}}
"""


# Every counter the generator keeps, and the heading it prints under. The run
# summary is the only evidence anyone sees that the transcription under-claims
# rather than guesses, so the grouping matters as much as the numbers: an enum
# that lost its BITMASK fallback is not a failure, but it is not a complete
# enum either, and filing it under the same heading as one is what turned a
# partial transcription into a reported success.
REPORT = (
    ("transcribed", (
        ("tables emitted", "table_emitted"),
        ("tags emitted", "tag_emitted"),
        ("variant tags compiled (Step 23)", "tag_variant_emitted"),
        ("int enums", "enum_int"),
        ("string enums", "enum_str"),
        ("exprs translated (exact match)", "expr_translated"),
        ("exprs translated (grammar-compiled, Step 15)", "expr_compiled"),
        ("masked fields", "tag_masked"),
        ("bit fields (frac + Mask)", "tag_fractional_masked"),
    )),
    ("partial -- exact matches kept, fallback dropped", (
        ("int enums", "enum_int_partial"),
        ("string enums", "enum_str_partial"),
    )),
    ("emitted with semantics recorded but not applied", (
        ("ValueConv", "omitted_value_conv"),
        ("RawConv", "omitted_raw_conv"),
        ("Condition", "omitted_condition"),
        ("Hook", "omitted_hook"),
        ("SubDirectory", "omitted_subdirectory"),
        ("bit fields, no Mask", "tag_fractional_bare"),
    )),
    ("refused, not approximated", (
        ("exprs unsupported", "expr_unsupported"),
        ("other PrintConv", "conv_dropped"),
        ("variant tags", "tag_variant_skipped"),
        ("  of which Condition outside the closed grammar", "tag_variant_cond_unsupported"),
        ("  of which a per-field reason (Unknown/format/mask/...)", "tag_variant_field_unsupported"),
        ("Unknown tags", "tag_unknown_skipped"),
        ("unsupported format", "tag_fmt_unsupported"),
        ("  of which var_* (data-dep. width)", "tag_var_format"),
        ("unreadable Mask", "tag_mask_unreadable"),
        ("unreadable index", "tag_bad_index"),
        ("unnamed tags", "tag_no_name"),
        ("tables not ProcessBinaryData", "table_not_binary"),
        ("tables bad FORMAT", "table_bad_format"),
    )),
    ("offsets unsound past a refused var_* field (fields also counted above)", (
        ("tables affected", "table_offsets_unsound"),
        ("fields affected", "tag_offset_unsound"),
    )),
)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("tables_json")
    ap.add_argument("-o", "--out", required=True)
    ap.add_argument("--modules", nargs="*", help="limit to these modules")
    args = ap.parse_args()

    with open(args.tables_json, encoding="utf-8") as fh:
        doc = json.load(fh)

    stats = Counter()
    stats["unsupported_exprs"] = Counter()
    stats["pc_directives_dropped"] = Counter()
    chunks = []
    index_rows = []

    mods = doc["modules"]
    names = args.modules or sorted(mods)
    for mod_name in names:
        mod = mods.get(mod_name)
        if not mod:
            continue
        for tbl_name in sorted(mod["tables"]):
            out = gen_table(mod_name, tbl_name, mod["tables"][tbl_name], stats)
            if out:
                chunks.append(out)
                ident = re.sub(r"[^A-Za-z0-9]", "_", f"{mod_name}_{tbl_name}").upper()
                index_rows.append(f"    &{ident},")

    # Collect the expressions actually referenced so the enum has no dead arms.
    # Iterate in sorted order: set iteration order varies between runs, and a
    # generator whose output depends on it cannot be checked into git.
    # known_num_domain_exprs() is TRANSLATIONS union every numeric-domain
    # expression Step 15's grammar compiler (exprs.compile()) accepted while
    # gen_table ran above -- gen_conv() populates that cache as a side effect
    # of every translate_or_compile() call, so it is complete by this point.
    used = {}
    joined = "".join(chunks)
    for e in sorted(exprs.known_num_domain_exprs()):
        ident = expr_ident(e)
        if ident in used and used[ident] != e:
            raise SystemExit(
                f"identifier collision: {ident!r} maps to both {used[ident]!r} "
                f"and {e!r} -- two conversions would alias to one variant"
            )
        if f"ExprId::{ident}" in joined:
            used[ident] = e

    index = (
        "\n/// Every generated binary table, for iteration and lookup.\n"
        f"pub static ALL_BINARY_TABLES: &[&BinaryTable] = &[\n"
        + "\n".join(index_rows)
        + "\n];\n"
    )

    # Stamp the release these tables came from. ExifTool renames fields and
    # inserts enum values between releases, so verifying against a different
    # one reports hundreds of differences that read as generator bugs rather
    # than as "wrong ExifTool". dump_tables.pl already recorded the version;
    # carrying it into the artifact is what lets verify.py say which it is.
    version = str(doc.get("exiftool_version") or "").strip()
    if not version:
        raise SystemExit(
            "tables JSON has no exiftool_version -- regenerate it with "
            "dump_tables.pl; an unstamped table set cannot be verified"
        )
    version_block = (
        "/// The ExifTool release these tables were transcribed from.\n"
        "///\n"
        "/// `tools/exiftool-tables/verify.py` refuses to compare against any\n"
        "/// other release. Field names and enum values move between versions,\n"
        "/// so a skewed check produces spurious mismatches that look like\n"
        "/// transcription errors. Regenerate with `just regen-tables <version>`.\n"
        f'pub const EXIFTOOL_VERSION: &str = "{version}";'
    )

    with open(args.out, "w", encoding="utf-8") as fh:
        fh.write(PRELUDE.replace("__VERSION_BLOCK__", version_block))
        fh.write(gen_expr_enum(used))
        fh.write(joined)
        fh.write(index)

    ue = stats.pop("unsupported_exprs")
    pcd = stats.pop("pc_directives_dropped")
    print(f"wrote {args.out}")
    printed = set()
    for heading, rows in REPORT:
        if heading:
            print(f"  --- {heading} ---")
        for label, key in rows:
            printed.add(key)
            print(f"  {label:<30}{stats[key]}")

    # The completeness argument for this whole pipeline rests on the refusal
    # count being trustworthy, and four counters were incremented but never
    # printed -- a run reported 2301 refusals against a true 2393 plus 11
    # dropped tables. Asserting that the report covers every key the code
    # touches is what makes the next added counter impossible to forget.
    missed = sorted(set(stats) - printed)
    if missed:
        raise SystemExit(
            "these counters were recorded but not reported: "
            + ", ".join(missed)
            + " -- add them to REPORT; an unreported refusal is a coverage lie"
        )

    if pcd:
        print("\n  PrintConv directives dropped from partial enums:")
        for d, n in pcd.most_common():
            print(f"    {n:>4}  {d}")
    if ue:
        print("\n  top unsupported expressions (translate these next):")
        for e, n in ue.most_common(10):
            flat = e if len(e) <= 58 else e[:55] + "..."
            print(f"    {n:>4}  {flat}")


if __name__ == "__main__":
    main()
