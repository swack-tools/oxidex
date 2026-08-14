#!/usr/bin/env python3
"""Compile ExifTool `Hook` expressions to data, over a closed grammar.

A `Hook` is Perl that `ProcessBinaryData` evals mid-walk, with `$format`,
`$varSize`, `$size`, `$dataPt` and `$pos` in scope (ExifTool.pm:10048-10063).
It runs AFTER the decorated field's own offset is fixed, so what it changes is
the field's format and every LATER field's offset.

The census of the pinned tree contains exactly two idioms:

  1. a `$varSize` shift, optionally gated on a condition, e.g.
     `$varSize -= 4 if $$self{CanonFirm} < 3`
  2. a format switch, e.g.
     `$$self{MediaHeaderVersion} and $format = "int64u", $varSize += 4`

This module compiles those two and REFUSES everything else, counting each
refusal with a reason. That is the same discipline `conds.py` (Condition) and
`subdirs.py` (SubDirectory Start/Base) follow, and it exists for the same
reason: a Hook that is *almost* right silently relocates every subsequent
field in the table, so a partial translation is worse than none. Nothing here
guesses -- `compile_hook` returns None and the caller keeps the field's
`Omitted.hook` flag set, exactly as before this module existed.

Compilation is atomic per Hook: a Hook whose statements are all inside the
grammar compiles as a whole, and one with a single statement outside it
compiles not at all. A half-applied chain would shift offsets by the sum of
only the arms that happened to parse, which is a wrong number rather than a
missing one.
"""

import re

# --- the closed grammar -----------------------------------------------------
#
# Perl comparison operators, numeric and string. ExifTool's Hooks use the
# numeric set on version DataMembers (`$$self{CanonFirm} < 3`) and the string
# set on firmware strings (`$$self{FirmwareVersion} ge "02.10"`), which are
# genuinely different comparisons -- `ge` is Perl's *string* ordering, so
# "10.0" ge "9.0" is FALSE. Keeping them apart in the compiled form is what
# lets the Rust side reproduce that rather than silently comparing numbers.
NUM_OPS = {"==": "Eq", "!=": "Ne", "<": "Lt", ">": "Gt", "<=": "Le", ">=": "Ge"}
STR_OPS = {"eq": "Eq", "ne": "Ne", "lt": "Lt", "gt": "Gt", "le": "Le", "ge": "Ge"}

MEMBER = r"\$\$self\{(\w+)\}"
INT = r"(-?(?:0x[0-9a-fA-F]+|\d+))"


def _int(text):
    return int(text, 16) if text.lower().startswith(("0x", "-0x")) else int(text)


def _rust_str(s):
    return s.replace("\\", "\\\\").replace('"', '\\"')


# --- conditions -------------------------------------------------------------

_C_MEMBER_NUM = re.compile(rf"^{MEMBER}\s*(==|!=|<=|>=|<|>)\s*{INT}$")
_C_MEMBER_TRUTHY = re.compile(rf"^{MEMBER}$")
_C_SIZE_NUM = re.compile(rf"^\$size\s*(==|!=|<=|>=|<|>)\s*{INT}$")
# `$$self{M} and $$self{M} ge "..."` -- Perl's guard-then-compare idiom. Both
# members must be the SAME name; a mismatch is not this idiom and is refused.
_C_MEMBER_STR = re.compile(
    rf"^{MEMBER}\s+and\s+{MEMBER}\s+(eq|ne|lt|gt|le|ge)\s+\"([^\"]*)\"$"
)


def compile_cond(text):
    """-> Rust `HookCond` literal, or None if outside the grammar."""
    text = text.strip()
    if text.startswith("(") and text.endswith(")"):
        text = text[1:-1].strip()

    m = _C_MEMBER_STR.match(text)
    if m and m.group(1) == m.group(2):
        return (
            f'HookCond::MemberStr {{ member: "{_rust_str(m.group(1))}", '
            f'op: CmpOp::{STR_OPS[m.group(3)]}, value: "{_rust_str(m.group(4))}" }}'
        )

    m = _C_MEMBER_NUM.match(text)
    if m:
        return (
            f'HookCond::MemberInt {{ member: "{_rust_str(m.group(1))}", '
            f"op: CmpOp::{NUM_OPS[m.group(2)]}, value: {_int(m.group(3))} }}"
        )

    m = _C_SIZE_NUM.match(text)
    if m:
        return f"HookCond::Size {{ op: CmpOp::{NUM_OPS[m.group(1)]}, value: {_int(m.group(2))} }}"

    m = _C_MEMBER_TRUTHY.match(text)
    if m:
        return f'HookCond::MemberTruthy("{_rust_str(m.group(1))}")'

    return None


# --- deltas -----------------------------------------------------------------

_D_INT = re.compile(rf"^{INT}$")
_D_SIZE = re.compile(r"^\$size$")
# `($$self{CanonFirm} ? -8 : 0x10000)` -- the Canon idiom. The false arm is
# normally 0x10000, a deliberate overshoot that pushes the next field past the
# end of the record and ends the walk rather than reading it at a wrong offset.
_D_TERNARY = re.compile(rf"^\(?\s*{MEMBER}\s*\?\s*{INT}\s*:\s*{INT}\s*\)?$")
_D_MEMBER_OFFSET = re.compile(rf"^{MEMBER}\s*([+-])\s*{INT}$")
_D_MEMBER_SCALED = re.compile(rf"^{MEMBER}\s*\*\s*{INT}$")
_D_MEMBER = re.compile(rf"^{MEMBER}$")


def compile_delta(text):
    """-> Rust `HookDelta` literal, or None if outside the grammar."""
    text = text.strip()

    m = _D_INT.match(text)
    if m:
        return f"HookDelta::Const({_int(m.group(1))})"

    if _D_SIZE.match(text):
        return "HookDelta::DirSize"

    m = _D_TERNARY.match(text)
    if m:
        return (
            f'HookDelta::MemberTernary {{ member: "{_rust_str(m.group(1))}", '
            f"truthy: {_int(m.group(2))}, falsy: {_int(m.group(3))} }}"
        )

    m = _D_MEMBER_OFFSET.match(text)
    if m:
        sign = 1 if m.group(2) == "+" else -1
        return (
            f'HookDelta::MemberPlus {{ member: "{_rust_str(m.group(1))}", '
            f"addend: {sign * _int(m.group(3))} }}"
        )

    m = _D_MEMBER_SCALED.match(text)
    if m:
        return (
            f'HookDelta::MemberScaled {{ member: "{_rust_str(m.group(1))}", '
            f"factor: {_int(m.group(2))} }}"
        )

    m = _D_MEMBER.match(text)
    if m:
        return f'HookDelta::MemberPlus {{ member: "{_rust_str(m.group(1))}", addend: 0 }}'

    return None


# --- statements -------------------------------------------------------------

# `$varSize += EXPR` / `$varSize -= EXPR`, with an optional trailing
# `if COND` -- Perl's statement modifier.
_S_SHIFT = re.compile(r"^\$varSize\s*([+-])=\s*(.+?)(?:\s+if\s+(.+))?$", re.S)
# `COND and $varSize += EXPR` -- the same thing written the other way round.
_S_AND_SHIFT = re.compile(r"^(.+?)\s+and\s+\$varSize\s*([+-])=\s*(.+)$", re.S)
# `$$self{M} and $format = "FMT", $varSize += N` -- the format switch. The
# comma is Perl's low-precedence sequence inside the `and`, so BOTH the format
# assignment and the shift are gated on the same condition.
_S_FORMAT = re.compile(
    rf"^{MEMBER}\s+and\s+\$format\s*=\s*[\"'](\w+)[\"']\s*,\s*"
    rf"\$varSize\s*([+-])=\s*{INT}$",
    re.S,
)

# The formats a Hook is allowed to switch TO. Deliberately not "any format
# name": the Rust `Fmt` this compiles into must be one the runtime can
# actually read, and the only switch the pinned tree performs is to int64u
# (the 64-bit QuickTime date/duration fields).
SWITCHABLE = {"int64u": "Int64u", "int32u": "Int32u", "int16u": "Int16u"}


def _split_statements(src):
    """Perl statements separated by `;`, comments and blank runs removed.

    Only a flat statement list is in the grammar -- a Hook containing a brace
    (an `if {...}` block, a `my` declaration, a nested conditional) is refused
    whole by `compile_hook` before this is called.
    """
    src = re.sub(r"#[^\n]*", "", src)
    return [s.strip() for s in src.split(";") if s.strip()]


def compile_statement(stmt):
    """-> Rust `HookEffect` literal, or None if outside the grammar."""
    m = _S_FORMAT.match(stmt)
    if m:
        fmt = SWITCHABLE.get(m.group(2))
        if fmt is None:
            return None
        sign = 1 if m.group(3) == "+" else -1
        return (
            f'HookEffect::SwitchFormat {{ when: HookCond::MemberTruthy("{_rust_str(m.group(1))}"), '
            f"format: Fmt::{fmt}, delta: {sign * _int(m.group(4))} }}"
        )

    m = _S_SHIFT.match(stmt)
    if m:
        delta = compile_delta(m.group(2))
        if delta is None:
            return None
        sign = m.group(1)
        cond_src = m.group(3)
        when = "None"
        if cond_src is not None:
            cond = compile_cond(cond_src)
            if cond is None:
                return None
            when = f"Some({cond})"
        return f"HookEffect::ShiftVarSize {{ delta: {delta}, negate: {str(sign == '-').lower()}, when: {when} }}"

    m = _S_AND_SHIFT.match(stmt)
    if m:
        cond = compile_cond(m.group(1))
        delta = compile_delta(m.group(3))
        if cond is None or delta is None:
            return None
        return (
            f"HookEffect::ShiftVarSize {{ delta: {delta}, "
            f"negate: {str(m.group(2) == '-').lower()}, when: Some({cond}) }}"
        )

    return None


def compile_hook(src):
    """-> (rust_slice_literal, reason) -- exactly one of the two is None.

    `reason` is a short machine-stable string naming why the Hook was refused,
    so `codegen.py`'s REPORT can break the refusals down instead of printing a
    bare total.
    """
    if not isinstance(src, str) or not src.strip():
        return None, "empty"

    # A brace means a block: an `if (...) {...}`, a `my` declaration, a nested
    # conditional. None of those are in the flat-statement grammar, and
    # detecting them here rather than letting a statement regex half-match one
    # is what keeps a partial parse from compiling into a wrong offset shift.
    if "{" in re.sub(MEMBER, "", src):
        return None, "block or nested conditional"
    if "=~" in src or "!~" in src:
        return None, "regex match"
    if "Get16u" in src or "Get32u" in src or "substr" in src:
        return None, "reads the data block"

    effects = []
    for stmt in _split_statements(src):
        compiled = compile_statement(stmt)
        if compiled is None:
            return None, "statement outside the grammar"
        effects.append(compiled)

    if not effects:
        return None, "empty"
    return "&[" + ", ".join(effects) + "]", None
