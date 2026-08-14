#!/usr/bin/env python3
r"""Step 23's `Condition`-string compiler: a closed grammar over the shapes
Step 15's census found cover most of ExifTool's `Condition` population, plus
a maintainer-approved seventh (bitmask).

Same doctrine as `exprs.py` (this file's sibling for `ValueConv`/`PrintConv`):
a `Condition` string compiles only if every one of its constructs is
recognised and provably reproduced; anything else -- a three-way `or` chain,
a `$format`/`$count`-sized-format eval, a `lt`/`ge` string comparison, a bare
function call like `GetByteOrder()` -- fails to parse and `compile_cond`
returns `None`. There is no partial mode: a `_variants` array is only emitted
as a schema `Variant` group when EVERY alternative's `Condition` (or absence
of one) compiles, because dropping just the alternative that would have won
for some models -- while keeping the others -- lets a later, wrong,
alternative win under that model instead. That is a wrong value under a real
tag name, which is exactly what `AGENTS.md` forbids; the atomic all-or-refuse
rule is what keeps a partial transcription from ever looking like a complete
one.

Shapes (`OVERHAUL_STEP15_DECISION.md` s2, condition census, 80.4% of uses
across six shapes; bitmask is the maintainer-approved seventh, the largest
cluster in the residue):
  1. `$$self{Member} == / != N`            -> MemberCmp
  2. `$$self{Member} =~ /regex/`           -> MemberRegex
  3. conjunction of the above (`A and B`)  -> And (nested for 3+ clauses)
  4. bare `$$self{Member}` / `not ...`     -> MemberTruthy
  5. `$$self{Member} eq / ne "str"`        -> MemberStrEq
  6. `$$valPt =~ /regex/`                  -> ValPtRegex
  7. `$$self{Member} & 0xNN`               -> MemberBitAnd  (7th, maintainer add)

Plus `$format`/`$count` comparisons (a named eighth bucket in the census,
1.5% of uses) as MemberCmp's un-membered siblings (FormatEq/CountCmp), and
the ExifTool assignment-as-condition idiom (`($$self{Member} = EXPR) and
...`, `$$self{Member} = 1`) as SetMember -- see `src/exiftool_tables/cond.rs`
module doc for the ExifTool.pm citation on why that idiom needs its own
shape rather than folding into a comparison.

Regex patterns are validated structurally, not by a character allowlist:
`_regex_atoms_ok` walks Python's own `re` parser's AST (the same engine used
to prove the pattern is even syntactically sane) and admits only the opcodes
a vetted, closed subset needs -- literals, alternation, grouping, `^`/`$`/`\b`
anchors, character classes, and the `?` (0-or-1) quantifier. Lookaround,
backreferences, `.`/`\d`/`\w` shorthand classes, and `*`/`+`/`{m,n}` general
quantifiers are all refused: none of them appear in the pinned tree's
binary-table `_variants` population, and admitting general quantifiers would
require verifying Perl's and Rust's `regex` crate engines agree on
catastrophic-backtracking-adjacent behaviour, not just on what matches.
`tools/exiftool-tables/verify_cond.py` differentially checks every pattern
this module accepts against the pinned ExifTool's own Perl regex engine
before trusting it; passing the AST allowlist is necessary, not sufficient.
"""

import re

try:
    import re._parser as sre_parse  # Python >= 3.11
except ImportError:  # pragma: no cover - older Python
    import sre_parse  # type: ignore[no-redef]


class CondCompileError(Exception):
    """Internal: this Condition construct is outside the closed grammar."""


# --- member access: `$$self{Name}` or `$self->{Name}` (ASF.pm's style) ----
_MEMBER = r"(?:\$\$self\{(\w+)\}|\$self->\{(\w+)\})"
_MEMBER_RE = re.compile(_MEMBER)


def _member_name(text):
    """`text` contains exactly one `$$self{X}` or `$self->{X}` reference
    (every atom regex below guarantees that); return `X`. `search`, not
    `match`: callers pass whole atom strings that may have a leading `not `
    or trailing operator/value text around the member reference."""
    m = _MEMBER_RE.search(text)
    if not m:
        return None
    return m.group(1) or m.group(2)


# --- atom regexes -----------------------------------------------------
_RE_REGEX_ATOM = re.compile(
    rf"^{_MEMBER}\s*(=~|!~)\s*/((?:[^/\\]|\\.)*)/([a-z]*)$"
)
_RE_NUM_ATOM = re.compile(
    rf"^{_MEMBER}\s*(==|!=|>=|<=|>|<)\s*(-?(?:0[xX][0-9a-fA-F]+|\d+))$"
)
_RE_STR_ATOM = re.compile(rf'^{_MEMBER}\s*(eq|ne)\s*"([^"]*)"$')
_RE_BARE_ATOM = re.compile(rf"^(not\s+)?{_MEMBER}$")
_RE_VALPT_ATOM = re.compile(r"^\$\$valPt\s*(=~|!~)\s*/((?:[^/\\]|\\.)*)/([a-z]*)$")
_RE_BITAND_BARE = re.compile(rf"^{_MEMBER}\s*&\s*(0[xX][0-9a-fA-F]+|\d+)$")
_RE_BITAND_CMP = re.compile(
    rf"^\(\s*{_MEMBER}\s*&\s*(0[xX][0-9a-fA-F]+|\d+)\s*\)\s*(==|!=)\s*(-?\d+)$"
)
_RE_FORMAT_ATOM = re.compile(r'^\$format\s*eq\s*"([^"]*)"$')
_RE_COUNT_ATOM = re.compile(r"^\$count\s*(==|!=|>=|<=|>|<)\s*(-?\d+)$")
# `($$self{Member} = <source>) [and <rest>]` -- the assignment-as-condition
# idiom (Canon.pm:1312, Pentax.pm:4343, Sony.pm:902).
_RE_SETMEMBER = re.compile(rf"^\(\s*{_MEMBER}\s*=\s*(\$count|-?\d+)\s*\)$")
_RE_SETMEMBER_BARE = re.compile(rf"^{_MEMBER}\s*=\s*(\$count|-?\d+)$")

_CMP_OP = {"==": "Eq", "!=": "Ne", ">=": "Ge", "<=": "Le", ">": "Gt", "<": "Lt"}

# Opcodes the regex-pattern AST walk admits. Anything else (ANY `.`,
# CATEGORY `\d`/`\w`/`\s`, ASSERT/ASSERT_NOT lookaround, GROUPREF
# backreferences, general MIN_REPEAT/MAX_REPEAT with max != 1) is refused.
_ALLOWED_REGEX_OPS = {"literal", "in", "branch", "subpattern", "at", "max_repeat"}


def _regex_ast_ok(node):
    for op, av in node:
        opname = op.name.lower() if hasattr(op, "name") else str(op).lower()
        if opname not in _ALLOWED_REGEX_OPS:
            return False
        if opname == "subpattern":
            # av = (group_number, add_flags, del_flags, subpattern)
            if not _regex_ast_ok(av[3]):
                return False
        elif opname == "branch":
            # av = (None, [branch1, branch2, ...])
            for branch in av[1]:
                if not _regex_ast_ok(branch):
                    return False
        elif opname == "max_repeat":
            lo, hi, sub = av
            if (lo, hi) not in ((0, 1), (1, 1)):
                return False
            if not _regex_ast_ok(sub):
                return False
        elif opname == "in":
            # av: list of (LITERAL, ord) / (RANGE, (lo, hi)) / (NEGATE, None)
            for item_op, item_av in av:
                item_name = (
                    item_op.name.lower() if hasattr(item_op, "name") else str(item_op).lower()
                )
                if item_name not in ("literal", "range", "negate"):
                    return False
        elif opname == "at":
            at_name = av.name if hasattr(av, "name") else str(av)
            if at_name not in ("AT_BEGINNING", "AT_END", "AT_BOUNDARY", "AT_NON_BOUNDARY"):
                return False
    return True


def _validate_regex_pattern(pattern):
    """Structural validation via Python's own regex-parser AST, not a
    character allowlist -- see module docstring. Raises CondCompileError if
    the pattern is unparseable or uses a construct outside the vetted subset.
    Returns the pattern unchanged (it is emitted verbatim, modulo the `\\0`
    -> `\\x00` translation callers do for the bytes domain): Rust's `regex`
    crate syntax agrees with Perl's on every construct this allowlist admits.
    """
    try:
        ast = sre_parse.parse(pattern)
    except re.error as e:
        raise CondCompileError(f"unparseable regex {pattern!r}: {e}") from e
    if not _regex_ast_ok(ast):
        raise CondCompileError(f"regex {pattern!r} uses a construct outside the vetted subset")
    return pattern


def _rust_str(s):
    body = s.replace("\\", "\\\\").replace('"', '\\"')
    return '"' + body + '"'


def _parse_int(text):
    return int(text, 0)


# --- atom compiler: returns Rust `Cond::...{ }` source text, or raises ----


def _compile_atom(text):
    text = text.strip()

    m = _RE_REGEX_ATOM.match(text)
    if m:
        member = _member_name(text)
        op, pattern, flags = m.group(3), m.group(4), m.group(5)
        if any(f not in "i" for f in flags):
            raise CondCompileError(f"unsupported regex flags {flags!r}")
        _validate_regex_pattern(pattern)
        rust_pattern = pattern.replace("\\0", "\\x00")
        negate = "true" if op == "!~" else "false"
        ic = "true" if "i" in flags else "false"
        return (
            "Cond::MemberRegex { member: "
            f'{_rust_str(member)}, pattern: {_rust_str(rust_pattern)}, '
            f"ignore_case: {ic}, negate: {negate} }}"
        )

    m = _RE_NUM_ATOM.match(text)
    if m:
        member = _member_name(text)
        op, num = m.group(3), m.group(4)
        return (
            f"Cond::MemberCmp {{ member: {_rust_str(member)}, "
            f"op: CmpOp::{_CMP_OP[op]}, value: {_parse_int(num)} }}"
        )

    m = _RE_STR_ATOM.match(text)
    if m:
        member = _member_name(text)
        op, s = m.group(3), m.group(4)
        negate = "true" if op == "ne" else "false"
        return (
            f"Cond::MemberStrEq {{ member: {_rust_str(member)}, "
            f"value: {_rust_str(s)}, negate: {negate} }}"
        )

    m = _RE_BITAND_CMP.match(text)
    if m:
        member = _member_name(text)
        mask, op, val = m.group(3), m.group(4), m.group(5)
        return (
            f"Cond::MemberBitAnd {{ member: {_rust_str(member)}, "
            f"mask: {_parse_int(mask)}, op: CmpOp::{_CMP_OP[op]}, value: {int(val)} }}"
        )

    m = _RE_BITAND_BARE.match(text)
    if m:
        member = _member_name(text)
        mask = m.group(3)
        return (
            f"Cond::MemberBitAnd {{ member: {_rust_str(member)}, "
            f"mask: {_parse_int(mask)}, op: CmpOp::Ne, value: 0 }}"
        )

    m = _RE_BARE_ATOM.match(text)
    if m:
        negate = "true" if m.group(1) else "false"
        member = _member_name(text)
        return f"Cond::MemberTruthy {{ member: {_rust_str(member)}, negate: {negate} }}"

    m = _RE_VALPT_ATOM.match(text)
    if m:
        op, pattern, flags = m.group(1), m.group(2), m.group(3)
        if flags:
            raise CondCompileError("$$valPt regex flags are unsupported")
        _validate_regex_pattern(pattern)
        rust_pattern = pattern.replace("\\0", "\\x00")
        negate = "true" if op == "!~" else "false"
        return f"Cond::ValPtRegex {{ pattern: {_rust_str(rust_pattern)}, negate: {negate} }}"

    m = _RE_FORMAT_ATOM.match(text)
    if m:
        return f"Cond::FormatEq {{ value: {_rust_str(m.group(1))} }}"

    m = _RE_COUNT_ATOM.match(text)
    if m:
        op, val = m.group(1), m.group(2)
        return f"Cond::CountCmp {{ op: CmpOp::{_CMP_OP[op]}, value: {int(val)} }}"

    raise CondCompileError(f"unrecognised condition atom {text!r}")


def _compile_setmember(text):
    """`($$self{Member} = <source>) [and <rest>]` and the bare `$$self{Member}
    = <source>` form (no parens, no `and`) -- Pentax.pm:4343's degenerate
    always-true idiom. Returns Rust source text for a `Cond::SetMember{...}`,
    or None if `text` is not this shape at all (not a compile error -- the
    caller tries other shapes next)."""
    # `(...) and <rest>`: split once on the top-level ' and ' that follows a
    # balanced-paren assignment.
    m = re.match(rf"^\(\s*{_MEMBER}\s*=\s*(\$count|-?\d+)\s*\)(?:\s+and\s+(.*))?$", text.strip())
    if m:
        member = _member_name(text)
        source_text = m.group(3)
        rest = m.group(4)
        source = "EffectSource::Count" if source_text == "$count" else f"EffectSource::Const({int(source_text)})"
        if rest:
            then_expr = compile_cond_atoms_conjunction(rest)
            if then_expr is None:
                raise CondCompileError(f"SetMember's trailing clause is outside the grammar: {rest!r}")
            then_rust = f"Some(&{then_expr})"
        else:
            then_rust = "None"
        return (
            f"Cond::SetMember {{ member: {_rust_str(member)}, source: {source}, "
            f"then: {then_rust} }}"
        )
    m = _RE_SETMEMBER_BARE.match(text.strip())
    if m:
        member = _member_name(text)
        source_text = m.group(3)
        source = "EffectSource::Count" if source_text == "$count" else f"EffectSource::Const({int(source_text)})"
        return f"Cond::SetMember {{ member: {_rust_str(member)}, source: {source}, then: None }}"
    return None


def compile_cond_atoms_conjunction(text):
    """Compile `text` as either a single atom or an `and`-conjunction of
    atoms (right-folded into nested `Cond::And`), or a `SetMember` idiom.
    Returns Rust source text for a `Cond` value, or None if nothing in this
    grammar matches (caller decides whether that is a hard refusal)."""
    text = re.sub(r"\s+", " ", text.strip())
    try:
        sm = _compile_setmember(text)
        if sm is not None:
            return sm
        if " and " in text:
            parts = [p.strip() for p in text.split(" and ")]
            compiled = [_compile_atom(p) for p in parts]
            # Right-fold: `A and B and C` -> And(A, And(B, C)) -- same
            # left-to-right short-circuit order as Perl's left-associative
            # `and` chain (see conds.py module docstring).
            expr = compiled[-1]
            for c in reversed(compiled[:-1]):
                expr = f"Cond::And(&{c}, &{expr})"
            return expr
        return _compile_atom(text)
    except CondCompileError:
        return None


def compile_cond(condition):
    """Compile one `_variants` alternative's `Condition` string (or `None`
    for the no-`Condition` catch-all, which becomes `Cond::Always`).

    Returns Rust source text for a `Cond` value, or `None` if this
    `Condition` is outside the closed grammar. `None` here is a normal,
    expected outcome; per this module's docstring, the caller must refuse
    the WHOLE `_variants` array atomically when even one alternative's
    condition does not compile.
    """
    if condition is None:
        return "Cond::Always"
    if not isinstance(condition, str) or not condition.strip():
        return "Cond::Always"
    return compile_cond_atoms_conjunction(condition)
