#!/usr/bin/env python3
r"""Step 23's `Condition`-string compiler: a closed grammar over the shapes
Step 15's census found cover most of ExifTool's `Condition` population, plus
a maintainer-approved seventh (bitmask).

Same doctrine as `exprs.py` (this file's sibling for `ValueConv`/`PrintConv`):
a `Condition` string compiles only if every one of its constructs is
recognised and provably reproduced; anything else -- a three-way `or` chain,
a `$format`/`$count`-sized-format eval, a bare
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
  3. boolean combinations (`A and B`, `A or B`) -> And/Or
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

String `lt`/`le`/`gt`/`ge` comparisons are a separate, deliberately small
shape (`MemberStrCmp`): Perl's string ordering is lexicographic, not numeric,
so `"10.00" lt "2.00"` is true. The only binary-table population is Nikon
firmware versions (Nikon.pm:9188,9198,9217,9227); `cond.rs` compares the UTF-8
byte sequences, whose lexicographic ordering is the same as Perl's code-point
ordering for these ASCII firmware strings.

Regex patterns are validated structurally, not by a character allowlist:
`_regex_atoms_ok` walks Python's own `re` parser's AST (the same engine used
to prove the pattern is even syntactically sane) and admits only the opcodes
a vetted, closed subset needs -- literals, alternation, grouping, `^`/`$`/`\b`
anchors, character classes, and the `?` (0-or-1) quantifier. Lookaround,
backreferences, `.`/`\w` shorthand classes, and `*`/`+`/`{m,n}` general
quantifiers are all refused. A narrowly-scoped exception admits `\d` only in
a Model regex: the one pinned use is Canon.pm:9381's ASCII Canon model family
spelling (`EOS R` followed by an ASCII digit), and Perl and Rust agree for
that actual model-name domain. Other shorthand categories, and `\d` in
byte/value domains, remain refused rather than assuming their Unicode
semantics agree.
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
_RE_STR_CMP_ATOM = re.compile(rf'^{_MEMBER}\s*(lt|le|gt|ge)\s*"([^"]*)"$')
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
_STR_CMP_OP = {"lt": "Lt", "le": "Le", "gt": "Gt", "ge": "Ge"}

# Opcodes the regex-pattern AST walk admits. Anything else (ANY `.`,
# CATEGORY shorthand (except Model-domain `\d`), ASSERT/ASSERT_NOT lookaround,
# GROUPREF backreferences, general MIN_REPEAT/MAX_REPEAT with max != 1) is
# refused.
_ALLOWED_REGEX_OPS = {"literal", "in", "branch", "subpattern", "at", "max_repeat"}


def _regex_ast_ok(node, allow_model_ascii_digit=False):
    for op, av in node:
        opname = op.name.lower() if hasattr(op, "name") else str(op).lower()
        if opname not in _ALLOWED_REGEX_OPS:
            return False
        if opname == "subpattern":
            # av = (group_number, add_flags, del_flags, subpattern)
            if not _regex_ast_ok(av[3], allow_model_ascii_digit):
                return False
        elif opname == "branch":
            # av = (None, [branch1, branch2, ...])
            for branch in av[1]:
                if not _regex_ast_ok(branch, allow_model_ascii_digit):
                    return False
        elif opname == "max_repeat":
            lo, hi, sub = av
            if (lo, hi) not in ((0, 1), (1, 1)):
                return False
            if not _regex_ast_ok(sub, allow_model_ascii_digit):
                return False
        elif opname == "in":
            # av: list of (LITERAL, ord) / (RANGE, (lo, hi)) / (NEGATE, None)
            for item_op, item_av in av:
                item_name = (
                    item_op.name.lower() if hasattr(item_op, "name") else str(item_op).lower()
                )
                if item_name in ("literal", "range", "negate"):
                    continue
                # Python represents `\d` as CATEGORY_DIGIT inside an IN
                # node. This is deliberately *not* a general shorthand
                # allowlist: the Model-only domain proof is in this module's
                # docstring.
                if not (
                    allow_model_ascii_digit
                    and item_name == "category"
                    and getattr(item_av, "name", str(item_av)) == "CATEGORY_DIGIT"
                ):
                    return False
        elif opname == "at":
            at_name = av.name if hasattr(av, "name") else str(av)
            if at_name not in ("AT_BEGINNING", "AT_END", "AT_BOUNDARY", "AT_NON_BOUNDARY"):
                return False
    return True


def _validate_regex_pattern(pattern, allow_model_ascii_digit=False):
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
    if not _regex_ast_ok(ast, allow_model_ascii_digit):
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
        _validate_regex_pattern(pattern, allow_model_ascii_digit=(member == "Model"))
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

    m = _RE_STR_CMP_ATOM.match(text)
    if m:
        member = _member_name(text)
        op, s = m.group(3), m.group(4)
        return (
            f"Cond::MemberStrCmp {{ member: {_rust_str(member)}, "
            f"op: StrCmpOp::{_STR_CMP_OP[op]}, value: {_rust_str(s)} }}"
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


def _strip_outer_parens(text):
    """Drop one pair of parens only when it encloses the entire expression."""
    text = text.strip()
    if not (text.startswith("(") and text.endswith(")")):
        return text
    depth = 0
    quote = None
    in_regex = False
    escaped = False
    for i, ch in enumerate(text):
        if escaped:
            escaped = False
            continue
        if ch == "\\":
            escaped = True
            continue
        if quote:
            if ch == quote:
                quote = None
            continue
        if in_regex:
            if ch == "/":
                in_regex = False
            continue
        if ch in "\"'":
            quote = ch
        elif ch == "/" and text[:i].rstrip().endswith(("=~", "!~")):
            in_regex = True
        elif ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
            if depth == 0 and i != len(text) - 1:
                return text
    return text[1:-1].strip() if depth == 0 and not quote and not in_regex else text


def _split_top_level(text, keyword):
    """Split a closed-grammar Boolean expression on a top-level keyword.

    Quoted strings and slash regexes are opaque, so `and`/`or` text within an
    accepted atom cannot become syntax accidentally.
    """
    parts, start, depth = [], 0, 0
    quote = None
    in_regex = False
    escaped = False
    needle = f" {keyword} "
    i = 0
    while i < len(text):
        ch = text[i]
        if escaped:
            escaped = False
        elif ch == "\\":
            escaped = True
        elif quote:
            if ch == quote:
                quote = None
        elif in_regex:
            if ch == "/":
                in_regex = False
        elif ch in "\"'":
            quote = ch
        elif ch == "/" and text[:i].rstrip().endswith(("=~", "!~")):
            in_regex = True
        elif ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        elif depth == 0 and text.startswith(needle, i):
            parts.append(text[start:i].strip())
            i += len(needle)
            start = i
            continue
        i += 1
    if parts:
        parts.append(text[start:].strip())
        return parts
    return None


def _compile_boolean_expr(text):
    """Parse the closed Boolean grammar using Perl's `and`/`or` precedence.

    `and` binds more tightly than `or`; both short-circuit left-to-right.
    Right-nesting the emitted binary nodes preserves the same evaluation order.
    """
    text = _strip_outer_parens(text)
    for keyword, variant in (("or", "Or"), ("and", "And")):
        parts = _split_top_level(text, keyword)
        if not parts:
            continue
        compiled = [_compile_boolean_expr(p) for p in parts]
        if any(c is None for c in compiled):
            return None
        expr = compiled[-1]
        for c in reversed(compiled[:-1]):
            expr = f"Cond::{variant}(&{c}, &{expr})"
        return expr
    try:
        return _compile_atom(text)
    except CondCompileError:
        return None


def compile_cond_atoms_conjunction(text):
    """Compile a closed-grammar Boolean expression or a `SetMember` idiom.

    Returns Rust source text for a `Cond` value, or None if nothing in this
    grammar matches (caller decides whether that is a hard refusal)."""
    text = re.sub(r"\s+", " ", text.strip())
    try:
        sm = _compile_setmember(text)
        if sm is not None:
            return sm
        return _compile_boolean_expr(text)
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
