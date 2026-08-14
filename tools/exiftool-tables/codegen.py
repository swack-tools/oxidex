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
import hooks
import others
import subdirs

# ExifTool format names -> (Rust Fmt variant, byte width).  Sized formats
# (string[32]) are handled separately since their width is per-field.
#
# Every width here is `%formatSize` in ExifTool.pm:6210-6242, and every decode
# is the matching entry in `%readValueProc` (ExifTool.pm:6243-6268). The pair
# is the whole contract: a format may only appear here once BOTH its width and
# its decoder have been transcribed, because a width with a guessed decoder is
# exactly the "plausible-but-wrong value under a real ExifTool tag name" that
# AGENTS.md rules out -- the field would be emitted, sized correctly, and
# report a confident wrong number.
#
# Step 26 added int64u/int64s, int32uRev, fixed16s/u, fixed32s/u, extended and
# rational32u/s. `fixed64s` and `complex` are deliberately still absent: both
# have a width in %formatSize but neither occurs in any ProcessBinaryData table
# in the pinned tree, so transcribing a decoder for them would be unverifiable
# against a real file.
SCALAR_FORMATS = {
    "int8u": ("Int8u", 1),
    "int8s": ("Int8s", 1),
    "int16u": ("Int16u", 2),
    "int16s": ("Int16s", 2),
    "int32u": ("Int32u", 4),
    "int32s": ("Int32s", 4),
    "int16uRev": ("Int16uRev", 2),
    # ExifTool.pm:6218 int32uRev => 4; decoder Get32uRev (ExifTool.pm:6088),
    # which is DoUnpackRev -- the record's byte order, reversed.
    "int32uRev": ("Int32uRev", 4),
    # ExifTool.pm:6219-6220 int64s/int64u => 8; Get64s/Get64u (%readValueProc,
    # ExifTool.pm:6252-6253).
    "int64u": ("Int64u", 8),
    "int64s": ("Int64s", 8),
    "float": ("Float", 4),
    "double": ("Double", 8),
    # ExifTool.pm:6221-6222 rational32s/u => 4 (a 16/16 pair, not 32/32);
    # GetRational32s/u at ExifTool.pm:6092-6106.
    "rational32u": ("Rational32u", 4),
    "rational32s": ("Rational32s", 4),
    "rational64u": ("Rational64u", 8),
    "rational64s": ("Rational64s", 8),
    # ExifTool.pm:6225-6228 fixed16s/u => 2, fixed32s/u => 4; the four
    # GetFixed* subs at ExifTool.pm:6121-6144 each divide by a fixed
    # denominator and then round to a set number of digits.
    "fixed16s": ("Fixed16s", 2),
    "fixed16u": ("Fixed16u", 2),
    "fixed32s": ("Fixed32s", 4),
    "fixed32u": ("Fixed32u", 4),
    # ExifTool.pm:6232 extended => 10; GetExtended (Writer.pl:4498-4507), the
    # 80-bit IEEE extended AIFF stores its sample rate in.
    "extended": ("Extended", 10),
}

# `pstring` is a scalar in the sense that matters here -- it never shifts a
# later field's offset -- but it is NOT in %formatSize and so must never reach
# the table-FORMAT path: ExifTool.pm:9894-9898 warns and falls back to int8u
# for a table FORMAT it cannot size. It is handled entirely per-field at
# ExifTool.pm:9972-9975, which reads a leading int8u count, advances `$entry`
# past it and then reads that many bytes as a string, leaving `$varSize`
# untouched. That last part is why it is safe: a var_* format shifts every
# subsequent index, a pstring does not.
PER_FIELD_FORMATS = {"pstring": "PString"}

# Step 26. The closed `var_*` grammar -- ExifTool spelling -> Rust `VarKind`.
#
# Every entry is one arm of ProcessBinaryData's variable-format branch
# (ExifTool.pm:10000-10032), which is a fixed `if/elsif` chain over exactly
# these names plus a NUL-scan fallback. Modeling them as data does NOT make
# them decodable: the width still depends on the bytes, so the field is
# emitted carrying its spelling and the runtime refuses to read it, and the
# table's `offsets_sound_until` still marks everything past it unsound. This
# is the same discipline conds.py and subdirs.py follow -- compile what the
# grammar covers, refuse and count the rest with a reason -- and it is why
# these 15 fields move OUT of the "unsupported format" line into their own
# REPORT counter rather than silently vanishing from the accounting.
#
# `var_ustring` is ExifTool's implicit default: the final `elsif ($$dataPt =~
# /\0/g)` arm (ExifTool.pm:10029) scans to the next NUL for any `var_` name
# not matched above. `var_string` is spelled out here because that is the
# name the tables actually use; a `var_` name outside this map is refused
# (`tag_var_unmodeled`) rather than assumed to take the fallback arm, since
# assuming it would be inventing a width.
VAR_KINDS = {
    "var_string": "String",     # NUL-scan fallback, ExifTool.pm:10029-10031
    "var_ustring": "UString",   # UTF-16 to \0\0,   ExifTool.pm:10005-10007
    "var_pstring": "PString",   # int8u count,      ExifTool.pm:10008-10010
    "var_pstr32": "PStr32",     # int32u count,     ExifTool.pm:10011-10017
    "var_ustr32": "UStr32",     # int32u count x2,  ExifTool.pm:10011-10017
    "var_int16u": "Int16u",     # int16u size + 2,  ExifTool.pm:10018-10023
    "var_ue7": "Ue7",           # BPG unsigned exp-Golomb, ExifTool.pm:10024-10028
}

SIZED_RE = re.compile(r"^(\w+)\[(\d+)\]$")


def load_oracle_ledger(path, tables_json, version):
    """Return oracle-approved normalized expressions, or refuse all of them.

    R2 deliberately makes the proof artifact a generation input.  A grammar
    match is only a shape claim; a ValueConv/PrintConv is enabled only after
    verify_exprs.py compared its generated Rust against the pinned Perl.
    The digest prevents an old ledger from approving a changed ExifTool dump.
    ExifTool queues `ValueConv` before `PrintConv` at
    Image/ExifTool.pm:3524-3525 and evaluates conversion text at :3656-3664
    (pinned 13.59).
    """
    if path is None:
        return None
    try:
        ledger = json.load(open(path, encoding="utf-8"))
        digest = hashlib.sha256(open(tables_json, "rb").read()).hexdigest()
        if (
            ledger.get("schema") != 1
            or ledger.get("exiftool_version") != version
            or ledger.get("tables_sha256") != digest
            or ledger.get("probe_counts", {}).get("fail") != 0
        ):
            return None
        return set(ledger["verified_expressions"])
    except (OSError, ValueError, KeyError, TypeError):
        return None


def value_domain(format_name, count):
    """The one scalar input domain a compiled expression may consume.

    ProcessBinaryData reads and masks raw data at ExifTool.pm:10076-10079,
    then passes it to FoundTag at :10163.  GetValue queues ValueConv at
    :3524-3525 and executes it at :3530-3664 before PrintConv.  An array's
    Perl list semantics are not the scalar compiler's semantics, so R2
    refuses it instead of applying a scalar translation element-wise.
    """
    if count != 1:
        return None
    if format_name.startswith("string"):
        return "str"
    if format_name.startswith("undef"):
        return "bytes"
    return "num"


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


def conv_for(tag, stats, input_domain, verified_exprs):
    """Return `(rust_printconv_src, refused)` for one tag.

    `refused` is true only when ExifTool declares a PrintConv that this
    generator cannot reproduce.  The caller then records `Omitted.print_conv`,
    so the field is withheld instead of being emitted with a raw value.
    """
    pc = tag.get("PrintConv")
    if not isinstance(pc, dict):
        return "PrintConv::None", False

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
                ), False
            stats["bitmask_unreadable"] += 1
            return "PrintConv::None", False

        if not m:
            return "PrintConv::None", False

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
                    return "PrintConv::None", False
                stats["other_translated"] += 1
                print_hex = "true" if tag.get("PrintHex") else "false"
                return (
                    "PrintConv::PartialEnumInt { exact: &["
                    f"{_rust_pairs(exact_pairs)}], other: Some({other_id}), "
                    f"print_hex: {print_hex} }}"
                ), False
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
            return "PrintConv::None", False

        # No BITMASK, no OTHER: whatever else is in `directives` is benign
        # (Notes/PrintHex/SeparateTable -- see BENIGN_PC_DIRECTIVES's doc),
        # so the map is a complete enum, unmatched-value fallback and all
        # (IntEnum/StrEnum stay silent on a miss; that is unchanged by Step
        # 25, which scopes the Unknown($val) fallback to the OTHER/BITMASK
        # population only -- see AGENTS.md's step description).
        exact_pairs = _int_pairs(m)
        if exact_pairs is not None:
            stats["enum_int"] += 1
            return f"PrintConv::IntEnum(&[{_rust_pairs(exact_pairs)}])", False
        stats["enum_str"] += 1
        body = ", ".join(
            f'("{rust_str(k)}", "{rust_str(v)}")' for k, v in sorted(m.items())
        )
        return f"PrintConv::StrEnum(&[{body}])", False

    if kind == "expr":
        raw = pc.get("expr")
        t = exprs.translate_or_compile_any(raw)
        normalized = exprs.normalize(raw or "")
        if not t:
            stats["expr_unsupported"] += 1
            stats["unsupported_exprs"][normalized] += 1
            return "PrintConv::None", False
        domain, _rty, _code = t
        if domain != input_domain:
            # Do not let render() fall through to raw bytes after accepting
            # an expression whose `$val` domain is wrong.  That would look
            # like a successful tag with a silently skipped PrintConv.
            stats["expr_refused_input_domain"] += 1
            return "PrintConv::None", False
        if verified_exprs is None or normalized not in verified_exprs:
            stats["expr_refused_oracle"] += 1
            return "PrintConv::None", False
        # translate_or_compile_any() tries the hand-verified TRANSLATIONS
        # exact-match table first, then Step 15's grammar compiler -- counted
        # separately so a coverage report can tell curated translations from
        # mechanically-derived ones.
        if normalized in exprs.TRANSLATIONS:
            stats["expr_translated"] += 1
        else:
            stats["expr_compiled"] += 1
        # Translated expressions are emitted by name so the generated code
        # stays readable and the mapping stays auditable.
        return f"PrintConv::Expr(ExprId::{expr_ident(raw)})", False
    if kind == "code":
        # A `PrintConv => \\&SomeSub` is accepted only if its deparsed body
        # maps exactly to a named, oracle-verified expression translation.
        named = exprs.code_ref_expr(pc.get("deparse"))
        if named:
            stats["expr_translated_code_ref"] += 1
            return f"PrintConv::Expr(ExprId::{expr_ident(named)})", False
        stats["conv_dropped"] += 1
        stats["dropped_code_refs"][exprs.normalize(pc.get("deparse") or "")] += 1
        return "PrintConv::None", True
    stats["conv_dropped"] += 1
    return "PrintConv::None", True


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


def value_conv_for(tag, stats, input_domain, verified_exprs):
    """Compile one scalar ValueConv only after oracle approval.

    `ProcessBinaryData` has already read/masked `$val` at ExifTool.pm:10076-
    10079 and passes it to FoundTag at :10163.  GetValue queues ValueConv at
    :3524-3525 and runs it at :3530-3664 before PrintConv. R2 carries that
    executable ExprId on Field rather than recording an opaque
    `value_conv: true`. Every other shape remains an explicit Omitted refusal.
    """
    vc = tag.get("ValueConv")
    if vc is None:
        return "None", False
    if not isinstance(vc, dict) or vc.get("kind") != "expr":
        stats["value_conv_refused_nonexpr"] += 1
        return "None", False
    raw = vc.get("expr")
    normalized = exprs.normalize(raw or "")
    compiled = exprs.translate_or_compile_any(raw)
    if not compiled:
        stats["value_conv_refused_shape"] += 1
        stats["value_conv_refused_expressions"][normalized] += 1
        return "None", False
    domain, _rty, _code = compiled
    if input_domain is None or domain != input_domain:
        stats["value_conv_refused_input_domain"] += 1
        return "None", False
    if verified_exprs is None or normalized not in verified_exprs:
        stats["value_conv_refused_oracle"] += 1
        return "None", False
    stats["value_conv_compiled"] += 1
    return f"Some(ExprId::{expr_ident(raw)})", True


def omitted_for(tag, stats, condition_resolved=False, value_conv_modeled=False,
                print_conv_refused=False):
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
        if member == "value_conv" and value_conv_modeled:
            continue
        if tag.get(key) is not None:
            flags.append(member)
            stats[f"omitted_{member}"] += 1
    if print_conv_refused:
        # `conv_for` already counts this exact event as `conv_dropped`; adding
        # another scalar would let the coverage report give one fact two names.
        flags.append("print_conv")
    if not flags:
        return "Omitted::NONE"
    return "Omitted { " + ", ".join(
        f"{m}: {'true' if m in flags else 'false'}"
        for m in ("value_conv", "raw_conv", "condition", "hook", "subdirectory", "print_conv")
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
    tag, idx, sub, stats, var_sound_until, default_format, verified_exprs,
    record_offset_hazard=True, condition_resolved=False,
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
        elif f in PER_FIELD_FORMATS:
            fmt_expr = f"Some(Fmt::{PER_FIELD_FORMATS[f]})"
        elif f.startswith("var_"):
            # Data-dependent width. Whether or not the spelling is one this
            # grammar models, the soundness boundary is the same: ExifTool
            # shifts every later field by however many bytes this one really
            # consumed, and that shift is not computable from the table alone.
            stats["tag_var_format"] += 1
            if var_sound_until is None:
                # Only the first one anchors the table's soundness boundary; a
                # second var_* field past it is already inside the region the
                # first one made unsound.
                var_sound_until = idx
            kind = VAR_KINDS.get(f)
            if kind is None:
                # Outside the closed grammar -- most often the
                # `var_<fmt>[<count expr>]` form (ExifTool.pm:9989), whose
                # width needs a Perl eval of the count expression.
                stats["tag_var_unmodeled"] += 1
                stats["tag_fmt_unsupported"] += 1
                return None, var_sound_until
            # Modeled as data, NOT decoded: the field is emitted carrying its
            # own spelling so a caller can see exactly which variable-width
            # rule applies, and `super::runtime::decode_field` refuses to read
            # it. Nothing downstream can mistake this for an extractable value.
            stats["tag_var_modeled"] += 1
            fmt_expr = (
                f'Some(Fmt::Var(VarFmt {{ spelling: "{rust_str(f)}", '
                f"kind: VarKind::{kind} }}))"
            )
        else:
            # Expression-sized or otherwise non-mechanical format.
            stats["tag_fmt_unsupported"] += 1
            return None, var_sound_until

    mask = mask_for(tag, stats)
    if mask is None:
        # A Mask this schema cannot express means the field's value is not
        # the word at that offset. Omit it rather than report the word.
        return None, var_sound_until

    format_name = f if isinstance(f, str) else default_format
    input_domain = value_domain(format_name, count)
    vc, value_conv_modeled = value_conv_for(tag, stats, input_domain, verified_exprs)
    pc, pc_refused = conv_for(tag, stats, input_domain, verified_exprs)
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
    hook_src = compile_hook_field(tag.get("Hook"), stats)
    groups_src = compile_groups_field(tag.get("Groups"), stats)
    field_src = (
        f'Field {{ index: {idx}, sub: {sub_s}, name: "{rust_str(name)}", '
        f"format: {fmt_expr}, count: {count}, mask: {mask}, "
        f"omitted: {omitted_for(tag, stats, condition_resolved, value_conv_modeled, pc_refused)}, "
        f"value_conv: {vc}, print_conv: {pc}, "
        f"subdir: {subdir_src}, hook: {hook_src}, groups: {groups_src} }}"
    )
    return field_src, var_sound_until


def compile_groups_field(src, stats):
    """Step 26: this tag's own `Groups` overrides as a Rust `TagGroups`.

    `AddTagToTable` (ExifTool.pm:9236-9244) fills each group family the tag
    does NOT declare from the table's GROUPS, and keeps the ones it does:

        if ($$tagInfo{Groups}) {
            foreach (keys %{$$tagTablePtr{GROUPS}}) {
                next if $$tagInfo{Groups}{$_};
                $$tagInfo{Groups}{$_} = $$tagTablePtr{GROUPS}{$_};
            }
        } else {
            $$tagInfo{Groups} = { %{$$tagTablePtr{GROUPS}} };
        }

    So the per-family rule is exactly "tag's own value if it has one, else the
    table's" -- which is what `BinaryTable::effective_groups` applies. Only the
    override half is stored per field; the table half is stored once.

    Families 0/1/2 only. ExifTool defines higher families (3=document,
    4=instance, 5=path...), but those are assigned during extraction rather
    than declared in a tag table, so a table-derived value for them would be
    invented rather than transcribed.
    """
    if not isinstance(src, dict) or not src:
        return "TagGroups::NONE"
    parts = []
    for family in ("0", "1", "2"):
        value = src.get(family)
        if isinstance(value, str) and value:
            parts.append(f'Some("{rust_str(value)}")')
            stats["tag_group_override"] += 1
        else:
            parts.append("None")
    if parts == ["None", "None", "None"]:
        # Present but carrying only families this schema does not model.
        stats["tag_group_unmodeled_family"] += 1
        return "TagGroups::NONE"
    return f"TagGroups {{ g0: {parts[0]}, g1: {parts[1]}, g2: {parts[2]} }}"


def compile_hook_field(src, stats):
    """Step 26: this field's `Hook` as a Rust `&[HookEffect]` literal.

    Always returns a valid literal -- `&[]` when the field has no Hook, and
    also when it has one `hooks.py` refuses. The two are told apart by
    `Omitted::hook`, which `omitted_for` sets for ANY Hook, compiled or not:
    modeling a Hook is not running it, so a field carrying one is still only
    as trustworthy as an unconditional read at its nominal offset.
    """
    if not isinstance(src, str) or not src.strip():
        return "&[]"
    literal, reason = hooks.compile_hook(src)
    if literal is None:
        stats["hook_refused"] += 1
        stats["hook_refusal_reasons"][reason] += 1
        return "&[]"
    stats["hook_compiled"] += 1
    return literal


def compile_variant_group(tag, idx, sub, stats, var_sound_until, default_format, verified_exprs):
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
    # `conv_for` and `compile_hook_field` write into these nested Counters
    # (diagnostic listings, not part of the coverage arithmetic REPORT checks),
    # so the trial copy needs its own, or the first write inside a
    # refused-and-discarded trial would crash on a bare `int` where a
    # `Counter` was expected.
    trial_stats["unsupported_exprs"] = Counter()
    trial_stats["pc_directives_dropped"] = Counter()
    trial_stats["other_unregistered_bodies"] = Counter()
    trial_stats["value_conv_refused_expressions"] = Counter()
    trial_stats["dropped_code_refs"] = Counter()
    trial_stats["hook_refusal_reasons"] = Counter()
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
            default_format,
            verified_exprs,
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
# * A field carrying an explicit refusal flag is FINE. `Omitted`'s flags
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
# * An unsupported CODE-ref PrintConv is also explicit now: it carries
#   `Omitted.print_conv`, so it is withheld and countable rather than emitted
#   as a raw value under ExifTool's tag name.
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
    # expressions currently emitted without a safe conversion
    "expr_unsupported",
    # Step 25's unsupported BITMASK/OTHER conversions are still emitted with
    # `PrintConv::None`; unlike CODE refs, they have no Omitted.print_conv
    # refusal flag, so they remain Gate A hazards.
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


def gen_table(mod_name, tbl_name, tbl, stats, verified_exprs):
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
    # Every nested Counter the field path writes into has to exist on the
    # per-table copy too, not just on the run-wide one: `_merge_stats` folds
    # them key-wise afterwards, but a bare `int` here crashes at the first
    # write. Step 26's `hook_refusal_reasons` is one of them.
    stats["unsupported_exprs"] = Counter()
    stats["pc_directives_dropped"] = Counter()
    stats["other_unregistered_bodies"] = Counter()
    stats["value_conv_refused_expressions"] = Counter()
    stats["dropped_code_refs"] = Counter()
    stats["hook_refusal_reasons"] = Counter()

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
            built = compile_variant_group(
                tag, idx, sub, stats, var_sound_until, fmt_name, verified_exprs
            )
            if built is None:
                stats["tag_variant_skipped"] += 1
                continue
            group_src, var_sound_until = built
            if var_sound_until is not None and idx > var_sound_until:
                var_sound_hit = True
            variant_rows.append(f"    {group_src},")
            continue

        field_src, var_sound_until = gen_field_literal(
            tag, idx, sub, stats, var_sound_until, fmt_name, verified_exprs
        )
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
    # Step 26: the table's EFFECTIVE groups, i.e. after `GetTagTable`'s
    # defaulting (ExifTool.pm:8980-8991):
    #
    #     unless ($$defaultGroups{0} and $$defaultGroups{1}) {
    #         if ($tableName =~ /Image::.*?::([^:]*)/) {
    #             $$defaultGroups{0} = $1 unless $$defaultGroups{0};
    #             $$defaultGroups{1} = $1 unless $$defaultGroups{1};
    #         ...
    #     $$defaultGroups{2} = 'Other' unless $$defaultGroups{2};
    #
    # `$1` is the module name: TABLE_NAME is `Image::ExifTool::<Module>::<Table>`
    # and the non-greedy `.*?` makes `([^:]*)` land on <Module>. The outer
    # `unless (0 and 1)` guard is a no-op when both are already set, so the
    # whole rule reduces to the three `or`s below.
    #
    # This matters because the raw hash is usually incomplete: of the 1512
    # tables in the pinned tree only 383 declare all three keys, 608 declare
    # {0,2}, 320 declare {2} alone and 95 declare no GROUPS at all. Emitting
    # the raw values recorded an empty group0 for 512 tables -- a tag with no
    # group is not what ExifTool reports, it is what this generator failed to
    # look up.
    groups = meta.get("GROUPS") if isinstance(meta.get("GROUPS"), dict) else {}
    g0 = groups.get("0") or mod_name
    g1 = groups.get("1") or mod_name
    g2 = groups.get("2") or "Other"

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
    group1: "{rust_str(g1)}",
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
///
/// Every variant's width is ExifTool's own `%formatSize` (ExifTool.pm:6210-6242)
/// and every decode is the matching `%readValueProc` entry
/// (ExifTool.pm:6243-6268); `decode_value` in `super::runtime` carries the
/// per-variant citation. A format appears here only once BOTH halves are
/// transcribed -- a correct width with a guessed decoder emits a confidently
/// wrong number under a real ExifTool tag name, which is worse than the
/// omission it replaces.
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
    /// 32-bit integer read with the opposite endianness to the record
    /// (`Get32uRev`, ExifTool.pm:6088).
    Int32uRev,
    Int64u,
    Int64s,
    Float,
    Double,
    /// A 16/16 numerator/denominator pair -- four bytes total, not eight
    /// (`GetRational32u`, ExifTool.pm:6100-6106).
    Rational32u,
    Rational32s,
    Rational64u,
    Rational64s,
    /// 16-bit fixed point, `/ 0x100` (`GetFixed16s`, ExifTool.pm:6121-6131).
    Fixed16s,
    Fixed16u,
    /// 32-bit fixed point, `/ 0x10000` (`GetFixed32s`, ExifTool.pm:6132-6144).
    Fixed32s,
    Fixed32u,
    /// 80-bit IEEE extended precision (`GetExtended`, Writer.pl:4498-4507).
    /// AIFF stores its sample rate in one of these.
    Extended,
    /// `pstring`: a leading `int8u` length followed by that many bytes of
    /// string (ExifTool.pm:9972-9975). Its total width is data-dependent, but
    /// unlike a `var_*` format it does NOT shift any later field's offset --
    /// ExifTool leaves `$varSize` untouched here -- so it is sound to decode
    /// in place. [`Fmt::size`] reports the length byte only; the real span is
    /// computed at decode time.
    PString,
    /// `string[N]`: N bytes, truncated at the first NUL.
    Str(u32),
    /// `undef[N]`: N raw bytes.
    Undef(u32),
    /// Step 26: a `var_*` format -- a field whose width depends on the bytes
    /// themselves, carrying the rule that governs it.
    ///
    /// This is modeled, NOT decoded. [`super::runtime`] refuses every
    /// `Fmt::Var` field, and [`BinaryTable::offsets_sound_until`] still marks
    /// every later field's offset unsound, exactly as when these fields were
    /// dropped entirely. What changed is only that the schema now says which
    /// variable-width rule applies instead of saying nothing at all.
    Var(VarFmt),
}

/// A `var_*` format, with the spelling ExifTool's own table writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VarFmt {
    /// The Format string verbatim, e.g. `"var_string"`.
    pub spelling: &'static str,
    pub kind: VarKind,
}

/// The arms of `ProcessBinaryData`'s variable-format branch
/// (ExifTool.pm:10000-10032). A closed set: a `var_` spelling outside it is
/// refused by the generator and counted, never assumed to take the fallback
/// arm -- assuming that would be inventing a width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VarKind {
    /// Scan to the next NUL (ExifTool.pm:10029-10031).
    String,
    /// UTF-16, scan to the next `\0\0` (ExifTool.pm:10005-10007).
    UString,
    /// Leading `int8u` count (ExifTool.pm:10008-10010).
    PString,
    /// Leading `int32u` count (ExifTool.pm:10011-10017).
    PStr32,
    /// Leading `int32u` count, doubled for UTF-16 (ExifTool.pm:10011-10017).
    UStr32,
    /// Leading `int16u` size, plus the two bytes of the size word itself;
    /// the payload is read as `undef` (ExifTool.pm:10018-10023).
    Int16u,
    /// BPG unsigned exponential-Golomb (ExifTool.pm:10024-10028).
    Ue7,
}

/// A tag's own group overrides, per family (0, 1, 2).
///
/// `None` in a family means "this tag does not override it", NOT "this tag
/// has no group" -- the value then comes from the table
/// ([`BinaryTable::effective_groups`], ExifTool.pm:9236-9244).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TagGroups {
    pub g0: Option<&'static str>,
    pub g1: Option<&'static str>,
    pub g2: Option<&'static str>,
}

impl TagGroups {
    /// A tag that overrides nothing.
    pub const NONE: Self = Self { g0: None, g1: None, g2: None };
}

/// Step 26: one effect of a `Hook`, compiled to data.
///
/// A `Hook` is Perl that `ProcessBinaryData` evals mid-walk with `$format`,
/// `$varSize`, `$size`, `$dataPt` and `$pos` in scope
/// (ExifTool.pm:10048-10063). It runs AFTER the decorated field's own offset
/// is fixed, so what it changes is this field's format and every LATER
/// field's offset -- which is exactly why a partially-translated Hook is
/// worse than an untranslated one: it silently relocates the rest of the
/// table. `tools/exiftool-tables/hooks.py` therefore compiles a Hook
/// atomically or not at all, and the ones it refuses keep `Omitted::hook`
/// set with nothing here.
///
/// A field carrying compiled effects ALSO still has `Omitted::hook` set:
/// modeling is not applying, and `super::runtime` does not run these. They
/// are recorded so a walker that wants to reproduce ExifTool's offsets has
/// the rule in data form rather than in Perl.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEffect {
    /// `$varSize += EXPR [if COND]` -- shift every later field's offset.
    ShiftVarSize {
        delta: HookDelta,
        /// True for `-=`. Kept separate from the delta's own sign so that a
        /// `MemberTernary` (whose arms carry their own signs) negates as a
        /// whole, the way Perl's `-=` does.
        negate: bool,
        when: Option<HookCond>,
    },
    /// `$$self{M} and $format = "int64u", $varSize += 4` -- Perl's comma is a
    /// low-precedence sequence inside the `and`, so BOTH the format change
    /// and the shift are gated on the same condition.
    SwitchFormat {
        when: HookCond,
        format: Fmt,
        delta: i64,
    },
}

/// The value a [`HookEffect::ShiftVarSize`] shifts by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookDelta {
    Const(i64),
    /// `$size` -- the length of the directory being walked.
    DirSize,
    /// `($$self{M} ? A : B)`. The false arm is normally 0x10000, a deliberate
    /// overshoot that pushes the next field past the end of the record and
    /// ends the walk, rather than reading it at an offset known to be wrong.
    MemberTernary {
        member: &'static str,
        truthy: i64,
        falsy: i64,
    },
    /// `$$self{M} + N` (N may be negative, or 0 for a bare `$$self{M}`).
    MemberPlus { member: &'static str, addend: i64 },
    /// `$$self{M} * N`.
    MemberScaled { member: &'static str, factor: i64 },
}

/// A `Hook`'s gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookCond {
    /// `$$self{M}` -- Perl truthiness (0 and "" and undef are false).
    MemberTruthy(&'static str),
    /// `$$self{M} <op> N`, numeric comparison.
    MemberInt {
        member: &'static str,
        op: CmpOp,
        value: i64,
    },
    /// `$$self{M} and $$self{M} ge "S"` -- Perl's guard-then-compare idiom.
    ///
    /// `op` is a STRING comparison, which is not the numeric one: `"10.0" ge
    /// "9.0"` is false in Perl. ExifTool uses this on firmware version
    /// strings, where getting it wrong would shift offsets on exactly the
    /// bodies the Hook exists to handle.
    MemberStr {
        member: &'static str,
        op: CmpOp,
        value: &'static str,
    },
    /// `$size <op> N`.
    Size { op: CmpOp, value: i64 },
}

impl Fmt {
    /// Byte width of one element, per ExifTool's `%formatSize`
    /// (ExifTool.pm:6210-6242).
    ///
    /// [`Fmt::PString`] is the one variant whose real span is not this number:
    /// it reports 1, the width of its leading length byte, because that is the
    /// only part whose size is known before the bytes are read. `decode_field`
    /// in `super::runtime` special-cases it.
    #[must_use]
    pub const fn size(self) -> u32 {
        match self {
            Fmt::Int8u | Fmt::Int8s | Fmt::PString => 1,
            Fmt::Int16u | Fmt::Int16s | Fmt::Int16uRev | Fmt::Fixed16s | Fmt::Fixed16u => 2,
            Fmt::Int32u
            | Fmt::Int32s
            | Fmt::Int32uRev
            | Fmt::Float
            | Fmt::Rational32u
            | Fmt::Rational32s
            | Fmt::Fixed32s
            | Fmt::Fixed32u => 4,
            Fmt::Double | Fmt::Int64u | Fmt::Int64s | Fmt::Rational64u | Fmt::Rational64s => 8,
            Fmt::Extended => 10,
            Fmt::Str(n) | Fmt::Undef(n) => n,
            // A `var_*` field HAS no static width -- that is the entire
            // property that makes it one. 0 is not a width here, it is the
            // absence of one, and every caller that could act on it refuses
            // the field first (see `super::runtime::decode_field`).
            Fmt::Var(_) => 0,
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
    /// ExifTool declares a `PrintConv` that the generator cannot reproduce.
    /// The field is withheld rather than emitted with a raw value.
    pub print_conv: bool,
}

impl Omitted {
    pub const NONE: Self = Self {
        value_conv: false,
        raw_conv: false,
        condition: false,
        hook: false,
        subdirectory: false,
        print_conv: false,
    };

    /// True when anything was dropped, i.e. the raw value stands alone.
    #[must_use]
    pub const fn any(self) -> bool {
        self.value_conv
            || self.raw_conv
            || self.condition
            || self.hook
            || self.subdirectory
            || self.print_conv
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
    /// Oracle-approved `ValueConv`, applied after mask and before PrintConv.
    /// `None` means there was none, or the generator refused it and left the
    /// corresponding `Omitted::value_conv` flag set.
    pub value_conv: Option<ExprId>,
    pub print_conv: PrintConv,
    /// `Some` when `omitted.subdirectory` is set AND this field's
    /// `SubDirectory.Start`/`Base`/`ProcessProc`/`ByteOrder`/`Validate` fall
    /// within the closed grammar `src/exiftool_tables/subdir.rs` compiles
    /// (Step 27). `None` either because the field has no `SubDirectory` at
    /// all, or because it does but was refused -- `codegen.py`'s REPORT
    /// (`subdir_edge_modeled` vs `subdir_refused_*`) says which, per field,
    /// and never approximates one it cannot compile.
    pub subdir: Option<SubdirEdge>,
    /// Step 26: this field's `Hook`, compiled to data when it falls inside
    /// the closed grammar `tools/exiftool-tables/hooks.py` accepts, else
    /// empty. Empty therefore means either "no Hook" or "a Hook that was
    /// refused" -- `Omitted::hook` is the flag that distinguishes them, and
    /// codegen.py's REPORT breaks the refusals down by reason.
    ///
    /// Non-empty does NOT mean the effects have been applied: nothing in
    /// `super::runtime` runs them, and `Omitted::hook` stays set either way.
    pub hook: &'static [HookEffect],
    /// Step 26: this tag's own `Groups` overrides, empty when it declares
    /// none. Resolve against the table with
    /// [`BinaryTable::effective_groups`] -- this alone is not the tag's
    /// group, it is only the half ExifTool stores per tag.
    pub groups: TagGroups,
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
    /// The table's EFFECTIVE group 0, after `GetTagTable`'s defaulting
    /// (ExifTool.pm:8980-8991): the declared value if it has one, else the
    /// module name. Never empty.
    pub group0: &'static str,
    /// Effective group 1, same rule and same default as `group0`.
    pub group1: &'static str,
    /// Effective group 2, defaulting to `"Other"` (ExifTool.pm:8991).
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

    /// The groups ExifTool would report for `field`, families (0, 1, 2).
    ///
    /// `AddTagToTable` (ExifTool.pm:9236-9244) gives a tag its own value for
    /// every family it declares and the table's for every family it does
    /// not, which is exactly this. The table side is already defaulted (see
    /// [`BinaryTable::group0`]), so the result is never empty.
    #[must_use]
    pub fn effective_groups(&self, field: &Field) -> (&'static str, &'static str, &'static str) {
        (
            field.groups.g0.unwrap_or(self.group0),
            field.groups.g1.unwrap_or(self.group1),
            field.groups.g2.unwrap_or(self.group2),
        )
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
    """Emit typed ExprId execution for every oracle-approved use.

    PrintConv wants a display string; ValueConv needs the actual converted
    value so a following PrintConv sees ExifTool's post-conversion `$val`
    (GetValue: ExifTool.pm:3524-3525, 3530-3664). Keeping those paths distinct avoids
    the old lossy `ExprId::apply -> String` shortcut.
    """
    if not used:
        return (
            "\n/// No Perl expressions were translated in this build.\n"
            "#[derive(Clone, Copy, Debug)]\npub enum ExprId {}\n\n"
            "#[derive(Clone, Debug)]\npub enum ExprValue { Number(f64), String(String) }\n\n"
            "impl ExprId {\n"
            "    #[must_use]\n"
            "    pub fn apply(&self, _val: f64) -> Option<String> { None }\n"
            "    #[must_use]\n"
            "    pub fn apply_str(&self, _val: &str) -> Option<String> { None }\n"
            "    #[must_use]\n"
            "    pub fn apply_bytes(&self, _val: &[u8]) -> Option<String> { None }\n"
            "    #[must_use]\n"
            "    pub fn value_num(&self, _val: f64) -> Option<ExprValue> { None }\n"
            "    #[must_use]\n"
            "    pub fn value_str(&self, _val: &str) -> Option<ExprValue> { None }\n"
            "    #[must_use]\n"
            "    pub fn value_bytes(&self, _val: &[u8]) -> Option<ExprValue> { None }\n}\n"
        )
    variants = "\n".join(f"    /// `{rust_str(e)}`\n    {i}," for i, e in sorted(used.items()))
    render_num = []
    render_str = []
    render_bytes = []
    value_num = []
    value_str = []
    value_bytes = []
    for ident, expr in sorted(used.items()):
        domain, rty, rexpr = exprs.translate_or_compile_any(expr)
        body = rexpr.replace("{v}", "val")
        # perl_num, not a bare format!("{}", ...) / format!("{v}"): Perl's
        # own numeric-to-string conversion goes through %.15g (scientific
        # notation outside ~[1e-4, 1e15)), which Rust's Display for f64 never
        # produces on its own -- see exprs.rs::perl_num's doc comment for the
        # verify_exprs.py failure that found this.
        if domain == "num" and rty == "f64":
            render_num.append(f"            ExprId::{ident} => Some(crate::exiftool_tables::exprs::perl_num({body})),")
            value_num.append(f"            ExprId::{ident} => Some(ExprValue::Number({body})),")
        elif domain == "num" and rty == "f64_int":
            # Perl's int() returns an integer (IV): exact decimal digits,
            # never %.15g's scientific notation, regardless of magnitude --
            # see exprs.rs::perl_int and exprs.py's _NUMERIC_VTYPES docstring.
            render_num.append(f"            ExprId::{ident} => Some(crate::exiftool_tables::exprs::perl_int({body})),")
            value_num.append(f"            ExprId::{ident} => Some(ExprValue::Number({body})),")
        elif domain == "num" and rty == "String":
            render_num.append(f"            ExprId::{ident} => Some({body}),")
            value_num.append(f"            ExprId::{ident} => Some(ExprValue::String({body})),")
        elif domain == "num":  # Option<f64>
            render_num.append(f"            ExprId::{ident} => ({body}).map(crate::exiftool_tables::exprs::perl_num),")
            value_num.append(f"            ExprId::{ident} => ({body}).map(ExprValue::Number),")
        elif domain == "str":
            render_str.append(f"            ExprId::{ident} => Some({body}),")
            value_str.append(f"            ExprId::{ident} => Some(ExprValue::String({body})),")
        else:  # bytes
            render_bytes.append(f"            ExprId::{ident} => Some({body}),")
            value_bytes.append(f"            ExprId::{ident} => Some(ExprValue::String({body})),")

    def arms_or_none(arms):
        # A domain-specific arms list (num/str/bytes) is exhaustive over
        # ExprId on its own whenever EVERY translated expression happens to
        # land in that one domain -- not a corner case: a small `used` set
        # can easily be 100% "num" (as any binary-table-only ExprId subset
        # commonly is). When that happens, appending `_ => None` anyway is
        # dead code, and `cargo clippy -D warnings` (the merge/CI gate) fails
        # the whole build on an `unreachable_patterns` error. Only emit the
        # catch-all when the arms list is provably not exhaustive.
        if len(arms) >= len(used):
            return "\n".join(arms)
        return "\n".join([*arms, "            _ => None,"])

    return f"""
/// Perl conversions with a hand-verified Rust equivalent.
///
/// Each variant corresponds to one entry in `tools/exiftool-tables/exprs.py`.
/// Adding an entry there fixes every tag sharing that expression at once.
#[derive(Clone, Copy, Debug)]
pub enum ExprId {{
{variants}
}}

/// Exact output of an oracle-approved ValueConv before PrintConv runs.
#[derive(Clone, Debug)]
pub enum ExprValue {{
    Number(f64),
    String(String),
}}

impl ExprId {{
    #[must_use]
    pub fn apply(&self, val: f64) -> Option<String> {{
        match self {{
{arms_or_none(render_num)}
        }}
    }}

    #[must_use]
    pub fn apply_str(&self, val: &str) -> Option<String> {{
        let _ = val;
        match self {{
{arms_or_none(render_str)}
        }}
    }}

    #[must_use]
    pub fn apply_bytes(&self, val: &[u8]) -> Option<String> {{
        let _ = val;
        match self {{
{arms_or_none(render_bytes)}
        }}
    }}

    #[must_use]
    pub fn value_num(&self, val: f64) -> Option<ExprValue> {{
        let _ = val;
        match self {{
{arms_or_none(value_num)}
        }}
    }}

    #[must_use]
    pub fn value_str(&self, val: &str) -> Option<ExprValue> {{
        let _ = val;
        match self {{
{arms_or_none(value_str)}
        }}
    }}

    #[must_use]
    pub fn value_bytes(&self, val: &[u8]) -> Option<ExprValue> {{
        let _ = val;
        match self {{
{arms_or_none(value_bytes)}
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
        ("named subs reached via a CODE ref", "expr_translated_code_ref"),
        ("ValueConv ExprIds (R2, oracle-approved)", "value_conv_compiled"),
        ("masked fields", "tag_masked"),
        ("bit fields (frac + Mask)", "tag_fractional_masked"),
        ("SubDirectory edges modeled (Step 27)", "subdir_edge_modeled"),
        ("BITMASK fields (DecodeBits, Step 25)", "bitmask_emitted"),
        ("OTHER conversions registered (Step 25 registry)", "other_translated"),
        ("per-tag group overrides (Step 26)", "tag_group_override"),
    )),
    ("Hook compiled to data, NOT applied (Step 26; Omitted::hook stays set)", (
        ("Hook effects compiled", "hook_compiled"),
    )),
    ("var_* modeled as data, NOT decoded (Step 26; offsets past these stay "
     "unsound and the runtime refuses to read them)", (
        ("var_* fields seen (modeled + refused)", "tag_var_format"),
        ("  of which modeled, carrying their rule", "tag_var_modeled"),
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
        ("  of which refused by the closed grammar (Step 26)", "hook_refused"),
        ("SubDirectory", "omitted_subdirectory"),
        ("bit fields, no Mask", "tag_fractional_bare"),
    )),
    ("refused, not approximated", (
        ("exprs unsupported", "expr_unsupported"),
        ("exprs refused: input domain", "expr_refused_input_domain"),
        ("exprs refused: no matching oracle PASS ledger", "expr_refused_oracle"),
        ("ValueConv non-expression shape", "value_conv_refused_nonexpr"),
        ("ValueConv outside closed grammar", "value_conv_refused_shape"),
        ("ValueConv input domain/array", "value_conv_refused_input_domain"),
        ("ValueConv without matching oracle PASS", "value_conv_refused_oracle"),
        ("other PrintConv (field withheld: Omitted.print_conv)", "conv_dropped"),
        ("variant tags", "tag_variant_skipped"),
        ("  of which Condition outside the closed grammar", "tag_variant_cond_unsupported"),
        ("  of which a per-field reason (Unknown/format/mask/...)", "tag_variant_field_unsupported"),
        ("Unknown tags", "tag_unknown_skipped"),
        ("unsupported format", "tag_fmt_unsupported"),
        ("  of which var_* outside the closed grammar", "tag_var_unmodeled"),
        ("unreadable Mask", "tag_mask_unreadable"),
        ("unreadable index", "tag_bad_index"),
        ("tag Groups naming only families 3+ (not table-derived)", "tag_group_unmodeled_family"),
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
    ap.add_argument("--expr-ledger", help="PASS-only ledger from verify_exprs.py")
    ap.add_argument("--value-conv-ledger-out", help="write R2 ValueConv refusal/coverage ledger")
    args = ap.parse_args()

    with open(args.tables_json, encoding="utf-8") as fh:
        doc = json.load(fh)
    version = str(doc.get("exiftool_version") or "").strip()
    if not version:
        raise SystemExit(
            "tables JSON has no exiftool_version -- regenerate it with "
            "dump_tables.pl; an unstamped table set cannot be verified"
        )
    verified_exprs = load_oracle_ledger(args.expr_ledger, args.tables_json, version)

    stats = Counter()
    stats["unsupported_exprs"] = Counter()
    stats["pc_directives_dropped"] = Counter()
    stats["other_unregistered_bodies"] = Counter()
    stats["value_conv_refused_expressions"] = Counter()
    stats["dropped_code_refs"] = Counter()
    stats["hook_refusal_reasons"] = Counter()
    chunks = []
    index_rows = []

    mods = doc["modules"]
    names = args.modules or sorted(mods)
    for mod_name in names:
        mod = mods.get(mod_name)
        if not mod:
            continue
        for tbl_name in sorted(mod["tables"]):
            out = gen_table(
                mod_name, tbl_name, mod["tables"][tbl_name], stats, verified_exprs
            )
            if out:
                chunks.append(out)
                ident = re.sub(r"[^A-Za-z0-9]", "_", f"{mod_name}_{tbl_name}").upper()
                index_rows.append(f"    &{ident},")

    # Collect the expressions actually referenced so the enum has no dead arms.
    # Iterate in sorted order: set iteration order varies between runs, and a
    # generator whose output depends on it cannot be checked into git.
    # known_exprs() includes string/bytes expressions as well: ConvertDateTime
    # is the first R2 string path, and ValueConv needs its real result rather
    # than the old boolean refusal marker.
    used = {}
    joined = "".join(chunks)
    for e in sorted(exprs.known_exprs()):
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
    vc_refused = stats.pop("value_conv_refused_expressions")
    dcr = stats.pop("dropped_code_refs")
    hrr = stats.pop("hook_refusal_reasons")
    if args.value_conv_ledger_out:
        refused_reasons = {
            key.removeprefix("value_conv_refused_"): value
            for key, value in stats.items()
            if key.startswith("value_conv_refused_")
        }
        ledger = {
            "schema": 1,
            "exiftool_version": version,
            "instrument": "tools/exiftool-tables/codegen.py --expr-ledger (verify_exprs.py PASS ledger)",
            "oracle_ledger": args.expr_ledger,
            "value_conv": {
                "compiled": stats["value_conv_compiled"],
                "refused": sum(refused_reasons.values()),
                "refused_by_reason": dict(sorted(refused_reasons.items())),
                "top_refused_expressions": [
                    {"expression": expression, "uses": uses}
                    for expression, uses in vc_refused.most_common(25)
                ],
            },
        }
        with open(args.value_conv_ledger_out, "w", encoding="utf-8") as fh:
            json.dump(ledger, fh, indent=2, sort_keys=True)
            fh.write("\n")
        print(f"wrote ValueConv ledger  {args.value_conv_ledger_out}")
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

    if hrr:
        # Every Hook refusal, by reason. A bare total would not say whether
        # the grammar is missing an idiom worth adding or is correctly
        # declining Perl that reads the data block.
        print("\n  Hooks refused, by reason (Step 26):")
        for r, n in hrr.most_common():
            print(f"    {n:>4}  {r}")
    if pcd:
        print("\n  PrintConv directives dropped from partial enums:")
        for d, n in pcd.most_common():
            print(f"    {n:>4}  {d}")
    if oub:
        print("\n  top unregistered OTHER closures (add to tools/exiftool-tables/others.py):")
        for body, n in oub.most_common(10):
            print(f"    {n:>4}  {body}")
    if dcr:
        print("\n  PrintConv CODE refs refused (field withheld, not emitted raw):")
        for d, n in dcr.most_common():
            flat = re.sub(r"\s+", " ", d)
            print(f"    {n:>4}  {flat if len(flat) <= 96 else flat[:93] + '...'}")
    if ue:
        print("\n  top unsupported expressions (translate these next):")
        for e, n in ue.most_common(10):
            flat = e if len(e) <= 58 else e[:55] + "..."
            print(f"    {n:>4}  {flat}")
    if vc_refused:
        print("\n  top refused ValueConv expressions (R2):")
        for e, n in vc_refused.most_common(10):
            flat = e if len(e) <= 58 else e[:55] + "..."
            print(f"    {n:>4}  {flat}")


if __name__ == "__main__":
    main()
