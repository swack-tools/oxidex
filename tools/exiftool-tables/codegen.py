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
import others
import subdirs

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
# never which string a matched key maps to, AND that ExifTool's own fallback
# chain (ExifTool.pm:3612-3631) never actually reads. Dropping these leaves
# every emitted entry exactly right, so they cost nothing but are still
# counted. `PrintHex` is the one exception worth spelling out: ExifTool reads
# it from the TAG, `$$tagInfo{PrintHex}` (dump_tables.pl's top-level `tag.get
# ("PrintHex")`, a `TAG_KEYS` entry, used directly by `conv_for` below for
# `PartialEnumInt.print_hex`) -- never from a same-named key living INSIDE the
# PrintConv hash itself, which is the only shape dump_tables.pl's `directives`
# dict can carry (see its `BITMASK|OTHER|Notes|PrintHex|SeparateTable` regex).
# That hash-level `PrintHex` is consequently dead for the runtime fallback and
# stays benign here.
#
# BITMASK and OTHER are different in kind: both are ExifTool's *fallback*
# mechanisms for a value the plain map does not contain (DecodeBits for the
# former, a Perl closure for the latter; ExifTool.pm:3616-3631's `if
# ($$conv{BITMASK}) {...} else { if ($$conv{OTHER}) {...} ... "Unknown
# ($val)" }`), so they are handled explicitly below, not folded into this
# benign set.
BENIGN_PC_DIRECTIVES = {"Notes", "PrintHex", "PrintSort", "SeparateTable"}


def _int_pairs(m):
    """`m` (a PrintConv hash's plain string-keyed exact-match dict) as a
    sorted `[(int, str)]` list, or `None` if any key fails to parse as an
    integer -- the caller falls back to the string-keyed representation (or,
    for the two int-only schemas below, refuses instead) rather than reporting
    a partially-converted table."""
    pairs = []
    for k in m:
        try:
            pairs.append((int(k, 0), m[k]))
        except ValueError:
            return None
    pairs.sort()
    return pairs


def _rust_pairs(pairs):
    return ", ".join(f'({k}, "{rust_str(v)}")' for k, v in pairs)


def conv_for(tag, stats):
    """Return Rust PrintConv construction, or None if this tag must lose it."""
    pc = tag.get("PrintConv")
    if not isinstance(pc, dict):
        return "PrintConv::None"

    kind = pc.get("kind")
    if kind in ("enum", "enum_partial"):
        m = pc.get("map") or {}
        directives = pc.get("directives") or {}

        # BITMASK takes priority over OTHER and is checked FIRST -- exactly
        # ExifTool.pm:3616-3618's branch order (`if ($$conv{BITMASK}) {
        # DecodeBits(...) } else { ...OTHER... }`; OTHER is dead code
        # whenever BITMASK is present). Checked before the `not m` bailout
        # below too: an enum whose only content is a BITMASK sub-hash and an
        # EMPTY exact-match map (e.g. BPG::Main's `Flags`) used to fall
        # through that bailout uncounted -- neither emitted nor reported as a
        # refusal, a silent coverage gap `codegen.py`'s own doctrine forbids.
        # `Bitmask::apply` (runtime.rs) never returns `None`: DecodeBits
        # always renders something, `"(none)"` when no bits are set, so this
        # schema carries no separate "fallback dropped" partial state at all.
        bitmask = directives.get("BITMASK")
        if isinstance(bitmask, dict):
            exact_pairs = _int_pairs(m)
            bits_pairs = []
            bits_ok = True
            for k, v in bitmask.items():
                try:
                    n = int(k)
                except ValueError:
                    bits_ok = False
                    break
                # DecodeBits (ExifTool.pm:6385-6407) indexes bit `i` of a
                # 32-bit word (`BitsPerWord` unset in every BITMASK entry the
                # pinned 13.59 binary-table corpus carries -- codegen.py's
                # REPORT's `bitmask_emitted` count is the census); a bit index
                # outside that word is not a shape this generator verifies.
                if n < 0 or n > 31:
                    bits_ok = False
                    break
                bits_pairs.append((n, v))
            if exact_pairs is not None and bits_ok:
                bits_pairs.sort()
                stats["bitmask_emitted"] += 1
                return (
                    "PrintConv::Bitmask { exact: &["
                    f"{_rust_pairs(exact_pairs)}], bits: &[{_rust_pairs(bits_pairs)}] }}"
                )
            stats["bitmask_unreadable"] += 1
            return "PrintConv::None"

        if not m:
            return "PrintConv::None"

        # OTHER: resolved only through Step 25's deparse-keyed registry
        # (tools/exiftool-tables/others.py) -- an exact match on the
        # closure's own deparsed text, never a pattern guess. Unregistered
        # means this generator does not know what the closure returns for a
        # value outside `m`, and guessing "Unknown ($val)" would very likely
        # be WRONG (most OTHER closures exist precisely to transform such
        # values into something else) -- exactly the approximation AGENTS.md
        # forbids. So an unregistered OTHER refuses the whole conversion,
        # same as it silently did before Step 25; the only change is that the
        # refusal is now counted with its own reason instead of folded into
        # an undifferentiated "partial" bucket.
        other = directives.get("OTHER")
        if other is not None:
            other_id = None
            deparse = None
            if isinstance(other, dict) and other.get("__perl") == "CODE":
                deparse = other.get("__deparse")
                if deparse:
                    other_id = others.translate_other(deparse)
            if other_id is not None:
                exact_pairs = _int_pairs(m)
                if exact_pairs is None:
                    # The registry only carries int-domain closures (see
                    # others.py); a string-keyed exact map paired with a
                    # registered OTHER never occurs in the pinned 13.59
                    # corpus, but refuse rather than guess how the two would
                    # interact if it ever does.
                    stats["other_str_domain_unsupported"] += 1
                    return "PrintConv::None"
                stats["other_translated"] += 1
                print_hex = "true" if tag.get("PrintHex") else "false"
                return (
                    "PrintConv::PartialEnumInt { exact: &["
                    f"{_rust_pairs(exact_pairs)}], other: Some({other_id}), "
                    f"print_hex: {print_hex} }}"
                )
            # Two counters, one event: `pc_directives_dropped["OTHER"]` is the
            # REPORT's human listing, `other_unregistered` is the scalar Gate A
            # (Step 28) reads. Gate A indexes plain ints -- a nested Counter
            # would enter `reasons` as a Counter and be emitted into the
            # generated Rust as one -- and before Step 25 this same event
            # raised `enum_int_partial`, which Gate A already disqualified on.
            # Dropping that counter without putting a scalar back would have
            # quietly ENABLED every table whose only defect is an OTHER this
            # generator cannot reproduce.
            stats["pc_directives_dropped"]["OTHER"] += 1
            stats["other_unregistered"] += 1
            if deparse:
                # Truncated, whitespace-flattened: a reporting key, not a
                # second copy of the registry's exact-match discipline.
                flat = " ".join(deparse.split())[:72]
                stats["other_unregistered_bodies"][flat] += 1
            return "PrintConv::None"

        # No BITMASK, no OTHER: whatever else is in `directives` is benign
        # (Notes/PrintHex/SeparateTable -- see BENIGN_PC_DIRECTIVES's doc),
        # so the map is a complete enum, unmatched-value fallback and all
        # (IntEnum/StrEnum stay silent on a miss; that is unchanged by Step
        # 25, which scopes the Unknown($val) fallback to the OTHER/BITMASK
        # population only -- see AGENTS.md's step description).
        exact_pairs = _int_pairs(m)
        if exact_pairs is not None:
            stats["enum_int"] += 1
            return f"PrintConv::IntEnum(&[{_rust_pairs(exact_pairs)}])"
        stats["enum_str"] += 1
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


def compile_subdir(sd, stats):
    """Step 27: compile a tag's `SubDirectory` hash into Rust source for
    `Field.subdir` (`Option<SubdirEdge>`), or refuse-and-count.

    Returns `"None"` for a tag with no `SubDirectory` at all -- silently, no
    counter touched, since that is simply not this tag's concern. For a tag
    that DOES carry one, returns either `Some(SubdirEdge {...})` (and bumps
    `subdir_edge_modeled`) or `"None"` (and bumps exactly one
    `subdir_refused_*` reason counter) -- never a guess. See
    `src/exiftool_tables/subdir.rs`'s module doc for the ExifTool.pm
    citations behind every check here, and `tools/exiftool-tables/subdirs.py`
    for the Start/Base grammar compiler itself.

    Every field this runs on already has `omitted.subdirectory` set by
    `omitted_for` above (same `tag.get("SubDirectory") is not None` test), so
    `subdir_edge_modeled + sum(subdir_refused_*)` is exactly
    `omitted_subdirectory` -- an accounting identity
    `src/exiftool_tables/mod.rs`'s `subdir_edges_cover_every_subdirectory_
    flagged_field` test pins for the pinned release.
    """
    if not isinstance(sd, dict):
        return "None"

    try:
        module, table = subdirs.parse_tag_table(sd.get("TagTable"))
    except subdirs.SubdirCompileError:
        stats["subdir_refused_tagtable"] += 1
        return "None"

    # ExifTool.pm:10148 routes the TARGET table through `$$subdir{ProcessProc}`
    # instead of the ordinary ProcessBinaryData/ProcessDirectory dispatch when
    # it is set -- a SubdirEdge that named the table but dropped that fact
    # would describe how the target is read wrongly, not merely incompletely.
    if sd.get("ProcessProc") is not None:
        stats["subdir_refused_processproc"] += 1
        return "None"
    # Dead keys in this code path (ProcessBinaryData's SubDirectory branch
    # never reads either) -- refused rather than silently ignored so a future
    # release that starts declaring one is a loud signal, not a silent no-op.
    # See subdir.rs's module doc, "Why byte_order/validate are always inert".
    if sd.get("ByteOrder") is not None:
        stats["subdir_refused_byteorder"] += 1
        return "None"
    if sd.get("Validate") is not None:
        stats["subdir_refused_validate"] += 1
        return "None"

    try:
        start_src = subdirs.compile_start(sd.get("Start"))
    except subdirs.SubdirCompileError:
        stats["subdir_refused_start"] += 1
        return "None"

    base_val = sd.get("Base")
    base_src = "None"
    if base_val is not None:
        try:
            base_expr_src = subdirs.compile_base(base_val)
        except subdirs.SubdirCompileError:
            stats["subdir_refused_base"] += 1
            return "None"
        base_src = f"Some(&{base_expr_src})"

    stats["subdir_edge_modeled"] += 1
    return (
        "Some(SubdirEdge { "
        f'module: "{rust_str(module)}", table: "{rust_str(table)}", '
        f"start: {start_src}, base: {base_src}, byte_order: None, validate: false"
        " })"
    )


def _merge_stats(dst, src):
    """`dst.update(src)` done by hand: `Counter.update` adds values via `+=`,
    which breaks for the three keys (`unsupported_exprs`,
    `pc_directives_dropped`, `other_unregistered_bodies`) whose value is itself
    a nested `Counter` rather than an int -- `0 + Counter(...)` has no
    `__radd__`. Nested Counters
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
    subdir_src = compile_subdir(tag.get("SubDirectory"), stats)
    field_src = (
        f'Field {{ index: {idx}, sub: {sub_s}, name: "{rust_str(name)}", '
        f"format: {fmt_expr}, count: {count}, mask: {mask}, "
        f"omitted: {omitted_for(tag, stats, condition_resolved)}, print_conv: {pc}, "
        f"subdir: {subdir_src} }}"
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
    # `conv_for` writes into these three nested Counters (diagnostic
    # listings, not part of the coverage arithmetic REPORT checks), so the
    # trial copy needs its own, or the first write inside a refused-and-
    # discarded trial would crash on a bare `int` where a `Counter` was
    # expected.
    trial_stats["unsupported_exprs"] = Counter()
    trial_stats["pc_directives_dropped"] = Counter()
    trial_stats["other_unregistered_bodies"] = Counter()
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


# Step 28 Gate A -- static soundness, computable at codegen time with no
# corpus at all.
#
# The design (OVERHAUL_STEP28_DESIGN.md section 3) states it as: "Every field
# in the table is either fully transcribed or carries an explicit refusal
# flag, and the table has no `offsets_sound_until` hazard."
#
# The two halves of that sentence are different failure modes, and only the
# second one is visible in the emitted Rust:
#
# * A field carrying an explicit refusal flag is FINE. `Omitted`'s five flags
#   are the refusal; `DecodedField::emit` and the engine both withhold the
#   field, loudly and countably. A table full of them passes Gate A -- it just
#   reports less.
# * A field the generator DROPPED is not fine. It is absent from the emitted
#   table with nothing to mark its place, so nothing downstream can tell
#   "ExifTool has no tag here" from "we could not transcribe the tag here".
#   Enabling such a table means silently under-reporting an offset ExifTool
#   reads -- the `AGENTS.md` "a gap in a transcribed table is not evidence the
#   tag does not exist" trap, promoted from a documentation hazard to a
#   runtime one.
# * A CONVERSION the generator dropped is worse than either, and is why this
#   gate counts `expr_unsupported`/`conv_dropped`/`enum_*_partial` too. Those
#   fields ARE emitted, with `PrintConv::None`, so the engine reports the raw
#   number where ExifTool prints a string. That is not a missing tag, it is a
#   plausible wrong VALUE under a real ExifTool tag name -- precisely what
#   Gate B would catch on the corpus, and what Gate A can refuse for free on
#   the tables the corpus never covers.
#
# Counters that are NOT disqualifying, and why:
#   tag_unknown_skipped   ExifTool itself hides these behind -u; not reporting
#                         them is what ExifTool does by default (ExifTool.pm:9945).
#   tag_fractional_bare   Step 11 settled this: ExifTool.pm:9957 reads the whole
#                         word at floor(index) regardless of Mask, so a maskless
#                         fractional field is fully transcribed, not partial.
#   omitted_*             the explicit refusal flags the design names.
GATE_A_DISQUALIFYING = (
    # fields dropped outright -- nothing marks the offset
    "tag_fmt_unsupported",
    "tag_var_format",
    "tag_mask_unreadable",
    "tag_bad_index",
    "tag_no_name",
    "tag_variant_skipped",
    "tag_variant_cond_unsupported",
    "tag_variant_field_unsupported",
    # conversions dropped -- the field is emitted, but renders the wrong thing
    "expr_unsupported",
    "conv_dropped",
    # Step 25 retired `enum_int_partial`/`enum_str_partial`: an enum carrying a
    # BITMASK or a registered OTHER is now transcribed in full (PrintConv::
    # Bitmask / PrintConv::PartialEnumInt), so it is no longer partial and no
    # longer disqualifying. The three counters below are what is left of that
    # population -- every one of them still returns `PrintConv::None`, i.e. the
    # conversion IS dropped -- and they inherit the gate weight the two retired
    # counters carried. Without them Gate A would read a Step 25 refusal as a
    # clean table.
    "other_unregistered",
    "other_str_domain_unsupported",
    "bitmask_unreadable",
    # SubDirectory edges refused -- the pointer is emitted with no target, so
    # every tag on the far side is silently absent
    "subdir_refused_processproc",
    "subdir_refused_byteorder",
    "subdir_refused_validate",
    "subdir_refused_tagtable",
    "subdir_refused_start",
    "subdir_refused_base",
)


def gate_a_for(table_stats, offset_hazard):
    """`(passes, reasons)` for one table's own counters.

    `reasons` is a sorted `(counter, n)` list, emitted verbatim into the
    generated Rust so a refusal is readable at the point of refusal rather
    than only in a report nobody opens -- and so the reachability report
    (`tools/exiftool-tables/reachability.py`) can be generated FROM the
    tables instead of hand-audited alongside them.
    """
    reasons = sorted(
        (key, table_stats[key]) for key in GATE_A_DISQUALIFYING if table_stats[key]
    )
    if offset_hazard:
        reasons.append(("offsets_sound_until", 1))
    return (not reasons), reasons


def gen_table(mod_name, tbl_name, tbl, stats):
    """Emit one `BinaryTable` literal, or `None` if the table is not one.

    Step 28: every counter this table's fields touch is tallied into a LOCAL
    `Counter` first and merged into the run-wide `stats` afterwards, so the
    generator can compute Gate A -- "is every field of THIS table either fully
    transcribed or explicitly refused?" -- from the same numbers the run
    summary prints, rather than from a second, unverified pass over the JSON.
    See `gate_a_for` and `src/exiftool_tables/enabled.rs`.
    """
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

    # Step 28 Gate A: field-level counters for THIS table only.
    stats, run_stats = Counter(), stats
    stats["unsupported_exprs"] = Counter()
    stats["pc_directives_dropped"] = Counter()
    stats["other_unregistered_bodies"] = Counter()

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
        # Refusals still count against us (Step 28 D3) even when nothing was
        # emitted to hang them off: the run summary is the denominator.
        _merge_stats(run_stats, stats)
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

    # ExifTool.pm:9471 -- `$priority = $$tbl{PRIORITY}`, consulted by FoundTag
    # when a name collides. `PRIORITY => 0` means "never displace a value
    # already reported under this name"; 86 of the pinned tree's tables set
    # it, and until Step 28 the generated schema dropped it, so every folded
    # engine had to hardcode its own copy (canon/camera_info.rs's
    # `merge_priority0`, shared/tag_priority.rs).
    try:
        priority = meta.get("PRIORITY")
        priority_expr = "None" if priority is None else f"Some({int(str(priority), 0)})"
    except (TypeError, ValueError):
        priority_expr = "None"

    passes, reasons = gate_a_for(stats, var_sound_hit)
    if passes:
        run_stats["gate_a_pass"] += 1
    else:
        run_stats["gate_a_fail"] += 1
    reasons_expr = "&[]" if not reasons else "&[" + ", ".join(
        f'("{k}", {n})' for k, n in reasons
    ) + "]"
    _merge_stats(run_stats, stats)

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
    priority: {priority_expr},
    gate_a: GateA {{ blocked_by: {reasons_expr} }},
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
// Step 27: same "imported unconditionally" reasoning as EffectSource above --
// which of these a given generation run actually constructs depends on which
// SubDirectory Start/Base shapes the pinned tree happens to carry (today:
// every FieldRelative literal, one Add(DirStart, Val) shape, one bare
// BaseExpr::Start -- see src/exiftool_tables/subdir.rs's module doc), not on
// this file's own logic, so a conditional `use` would be the same
// generator-output nondeterminism the comment above already rules out.
#[allow(unused_imports)]
use super::subdir::{BaseExpr, ByteOrderRule, Start, StartExpr, SubdirEdge};

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
    /// ExifTool's `BITMASK` fallback (ExifTool.pm:3616-3618; `DecodeBits` at
    /// ExifTool.pm:6385-6407, ported as `exiftool_tables::runtime::
    /// decode_bits`). `exact` (sorted by key) is checked first -- ExifTool
    /// checks the whole PrintConv hash, including its non-directive keys,
    /// before ever falling back to `BITMASK` -- then any value `exact` does
    /// not cover is decoded bit-by-bit against `bits`. `Bitmask::apply`
    /// never returns `None`: `DecodeBits` always renders something,
    /// `"(none)"` when no bits are set, so this variant carries no separate
    /// "fallback unavailable" state the way `PartialEnumInt` below does.
    Bitmask {
        exact: &'static [(i64, &'static str)],
        bits: &'static [(u32, &'static str)],
    },
    /// An enum whose ExifTool `PrintConv` hash also declares `OTHER`
    /// (ExifTool.pm:3619-3631): `exact` (sorted by key) is checked first,
    /// then the registered `other` closure (Step 25's OTHER registry,
    /// `tools/exiftool-tables/others.py` -> `OtherId`), then ExifTool's own
    /// `"Unknown ($val)"` / `sprintf('Unknown (0x%x)', $val)` fallback
    /// (`print_hex` mirrors the tag's `PrintHex`) for whatever `other`
    /// itself leaves undefined. `other` is `None` only for a
    /// hand-constructed value (never emitted by `codegen.py`, which only
    /// builds this variant once `other` has resolved) -- kept `Option`
    /// because the plain exact+Unknown chain is itself a legitimate,
    /// independently useful shape.
    PartialEnumInt {
        exact: &'static [(i64, &'static str)],
        other: Option<OtherId>,
        print_hex: bool,
    },
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
    /// `Some` when `omitted.subdirectory` is set AND this field's
    /// `SubDirectory.Start`/`Base`/`ProcessProc`/`ByteOrder`/`Validate` fall
    /// within the closed grammar `src/exiftool_tables/subdir.rs` compiles
    /// (Step 27). `None` either because the field has no `SubDirectory` at
    /// all, or because it does but was refused -- `codegen.py`'s REPORT
    /// (`subdir_edge_modeled` vs `subdir_refused_*`) says which, per field,
    /// and never approximates one it cannot compile.
    pub subdir: Option<SubdirEdge>,
}

/// Step 28 Gate A -- whether this table is *statically* sound enough to hand
/// to the generic engine, decided at codegen time with no corpus at all.
///
/// A table passes when `blocked_by` is empty: every field ExifTool declares
/// was either fully transcribed or emitted with an explicit [`Omitted`] flag,
/// every `PrintConv` was reproduced exactly, every `SubDirectory` edge was
/// compiled, and no refused `var_*` field left a live
/// [`BinaryTable::offsets_sound_until`] hazard.
///
/// `blocked_by` names the `codegen.py` counters that fired, with their counts,
/// so a refusal is legible where the table is rather than only in a report --
/// and so `tools/exiftool-tables/reachability.py` can GENERATE the
/// enabled/eligible/refused-with-reason census instead of anyone hand-auditing
/// it. See `codegen.py`'s `GATE_A_DISQUALIFYING` for why each counter
/// disqualifies and, just as importantly, why `tag_unknown_skipped`,
/// `tag_fractional_bare` and the `omitted_*` flags do not.
///
/// Gate A alone never enables a table (Step 28 D1, opt-in): passing it makes a
/// table *eligible*, and `src/exiftool_tables/enabled.rs` -- Gate B's measured
/// allowlist -- decides.
#[derive(Clone, Copy, Debug)]
pub struct GateA {
    /// `(counter, n)` pairs, sorted, empty exactly when the gate passes.
    pub blocked_by: &'static [(&'static str, u32)],
}

impl GateA {
    /// True when nothing blocked this table.
    #[must_use]
    pub const fn passes(self) -> bool {
        self.blocked_by.is_empty()
    }
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
    /// ExifTool's table-level `PRIORITY` (ExifTool.pm:9471, consulted by
    /// `FoundTag` when a tag name collides). `Some(0)` -- 86 of the pinned
    /// tree's 1,512 tables -- means a value from this table must never
    /// displace one already reported under the same name. Before Step 28 this
    /// was dropped from the schema and each engine hardcoded its own copy.
    pub priority: Option<i64>,
    /// Step 28 Gate A: static soundness, computed by `codegen.py` from its
    /// own per-table refusal counters. See [`GateA`].
    pub gate_a: GateA,
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

    /// Step 28: whether the generic engine
    /// ([`crate::exiftool_tables::engine::process_binary_data`]) may walk
    /// this table -- Gate A *and* Gate B's measured allowlist. See
    /// `src/exiftool_tables/enabled.rs`.
    #[must_use]
    pub fn enabled(&self) -> bool {
        super::enabled::is_enabled(self)
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
            PrintConv::Bitmask { exact, bits } => Some(
                exact
                    .binary_search_by_key(&val, |(k, _)| *k)
                    .ok()
                    .map(|i| exact[i].1.to_string())
                    .unwrap_or_else(|| super::runtime::decode_bits(val, bits)),
            ),
            PrintConv::PartialEnumInt { exact, other, print_hex } => Some(
                exact
                    .binary_search_by_key(&val, |(k, _)| *k)
                    .ok()
                    .map(|i| exact[i].1.to_string())
                    .or_else(|| other.and_then(|id| id.apply(val)))
                    .unwrap_or_else(|| super::runtime::unknown_fallback(val, *print_hex)),
            ),
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
    ("Step 28 Gate A (static soundness; a table is ELIGIBLE, not enabled)", (
        ("tables passing gate A", "gate_a_pass"),
        ("tables blocked by gate A", "gate_a_fail"),
    )),
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
        ("SubDirectory edges modeled (Step 27)", "subdir_edge_modeled"),
        ("BITMASK fields (DecodeBits, Step 25)", "bitmask_emitted"),
        ("OTHER conversions registered (Step 25 registry)", "other_translated"),
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
        ("BITMASK unreadable (non-integer/out-of-range bit key)", "bitmask_unreadable"),
        ("OTHER not in the Step 25 registry", "other_unregistered"),
        ("OTHER registered but exact map is string-keyed", "other_str_domain_unsupported"),
    )),
    ("offsets unsound past a refused var_* field (fields also counted above)", (
        ("tables affected", "table_offsets_unsound"),
        ("fields affected", "tag_offset_unsound"),
    )),
    ("SubDirectory edges refused, not approximated (Step 27; of the "
     "'SubDirectory' count above)", (
        ("custom ProcessProc (target not walked the ordinary way)", "subdir_refused_processproc"),
        ("ByteOrder declared (ProcessBinaryData never reads it here)", "subdir_refused_byteorder"),
        ("Validate declared (ProcessBinaryData never reads it here)", "subdir_refused_validate"),
        ("TagTable missing or an unrecognised shape", "subdir_refused_tagtable"),
        ("Start expression outside the closed grammar", "subdir_refused_start"),
        ("Base expression outside the closed grammar", "subdir_refused_base"),
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
    stats["other_unregistered_bodies"] = Counter()
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
        fh.write(others.RUST_SUPPORT)
        fh.write(gen_expr_enum(used))
        fh.write(joined)
        fh.write(index)

    ue = stats.pop("unsupported_exprs")
    pcd = stats.pop("pc_directives_dropped")
    oub = stats.pop("other_unregistered_bodies")
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
    if oub:
        print("\n  top unregistered OTHER closures (add to tools/exiftool-tables/others.py):")
        for body, n in oub.most_common(10):
            print(f"    {n:>4}  {body}")
    if ue:
        print("\n  top unsupported expressions (translate these next):")
        for e, n in ue.most_common(10):
            flat = e if len(e) <= 58 else e[:55] + "..."
            print(f"    {n:>4}  {flat}")


if __name__ == "__main__":
    main()
