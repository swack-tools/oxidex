#!/usr/bin/env python3
r"""Step 23's `Condition`-string compiler: a closed grammar over the shapes
Step 15's census found cover most of ExifTool's `Condition` population, plus
a maintainer-approved seventh (bitmask).

Same doctrine as `exprs.py` (this file's sibling for `ValueConv`/`PrintConv`):
a `Condition` string compiles only if every one of its constructs is
recognised and provably reproduced; anything else -- a parenthesised group,
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

Slice I-3 (the IFD tables' first Gate-A blockers, Olympus.pm's FocusInfo)
widens the CONNECTIVES, not the atom set. `_tokenize` splits a Condition
into atoms and the boolean operators between them, and `_Parser` rebuilds
the tree at Perl's own precedence (perlop, lowest first):

    or  <  and  <  not  <  ||  <  &&  <  !

so `A || B and C` is `(A || B) and C`, `A or B and C` is `A or (B and C)`,
and `not A || B` is `not (A || B)`. Disjunction (`or`, `||`) becomes a
right-nested `Cond::Or` exactly as `and`/`&&` become `Cond::And` (the same
left-to-right short-circuit order Perl's left-associative chains have).
Negation (`not`, `!`) is admitted over exactly the two shapes whose
negation the schema carries as a flag -- a bare `$$self{Member}` and a
`defined $$self{Member}` test -- and is refused over anything else, so
`not A || B` (which Perl reads as the negation of the whole disjunction)
is a refusal, never a mis-grouped `(not A) || B`. `defined $$self{Member}`
(also `defined($$self{Member})`) is the new `Cond::MemberDefined` atom, and
`=~ m/.../` is accepted as the spelling of `=~ /.../` it is in Perl.
Parenthesised grouping, `xor`, `m{}`/`m()` delimiters, `not` over a
comparison or regex, and a `($$self{X} = EXPR) and ... or ...` tail
(Perl: `((X = EXPR) and ...) or ...`) stay refused: every one of them is
counted, none is approximated. `tools/exiftool-tables/verify_cond.py`
probes every accepted connective shape against the pinned Perl, precedence
included.

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
# `m?/`: Perl's `=~ m/.../` is the same match operator as `=~ /.../`
# (perlop, "m/PATTERN/"); Olympus.pm:3467 (FocusInfo 0x31b) spells it that
# way. Only the `/` delimiter is admitted -- `m{...}` / `m(...)` change what
# a `/` inside the body means and are refused.
_RE_REGEX_ATOM = re.compile(
    rf"^{_MEMBER}\s*(=~|!~)\s*m?/((?:[^/\\]|\\.)*)/([a-z]*)$"
)
_RE_NUM_ATOM = re.compile(
    rf"^{_MEMBER}\s*(==|!=|>=|<=|>|<)\s*(-?(?:0[xX][0-9a-fA-F]+|\d+))$"
)
_RE_STR_ATOM = re.compile(rf'^{_MEMBER}\s*(eq|ne)\s*"([^"]*)"$')
# A bare member. Its negation (`not $$self{X}`, `!$$self{X}`) is the
# parser's job (`_Parser._negated`), never the atom's.
_RE_BARE_ATOM = re.compile(rf"^{_MEMBER}$")
# `defined $$self{X}` / `defined($$self{X})` / `defined ($$self{X})`: a data
# member's presence (ExifTool.pm:4331-4332 -- `Init` deletes every
# DataMember before each file, so "defined" means "some RawConv or
# Condition stored it while reading this file"). Perl's `defined` is a
# named unary operator (perlop): it binds tighter than `&&`/`||`/`and`/`or`
# and looser than `!`, so `!defined $$self{X} || Y` is `(!defined $$self{X})
# || Y` and `not defined $$self{X} and Y` is `(not defined $$self{X}) and
# Y` -- exactly what `_Parser` produces.
_RE_DEFINED_ATOM = re.compile(rf"^defined\s*(?:\(\s*{_MEMBER}\s*\)|{_MEMBER})$")
_RE_VALPT_ATOM = re.compile(r"^\$\$valPt\s*(=~|!~)\s*/((?:[^/\\]|\\.)*)/([a-z]*)$")
_RE_BITAND_BARE = re.compile(rf"^{_MEMBER}\s*&\s*(0[xX][0-9a-fA-F]+|\d+)$")
_RE_BITAND_CMP = re.compile(
    rf"^\(\s*{_MEMBER}\s*&\s*(0[xX][0-9a-fA-F]+|\d+)\s*\)\s*(==|!=)\s*(-?\d+)$"
)
_RE_FORMAT_ATOM = re.compile(r'^\$format\s*(eq|ne)\s*"([^"]*)"$')
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


_OCTAL_DIGITS = "01234567"


def _perl_regex_to_rust(pattern):
    """Translate the Perl escapes Rust's `regex` crate spells differently,
    scanning escape by escape so an escaped backslash is never re-read.

    Octal escapes: Perl's `\\0`, `\\012`, and -- inside a character class,
    where a numeric escape is always a character -- `\\4` (Sony.pm's
    `[\\0-\\4]`) become `\\xNN`. Outside a class a `\\1`..`\\9` would be a
    backreference, which `_validate_regex_pattern`'s allowlist already
    refuses, so every numeric escape that reaches here is octal. The old
    `replace("\\0", "\\x00")` left `\\4` for the Rust crate to reject, and a
    rejected pattern fails CLOSED (`regex_match_bytes` -> false): `$$valPt =~
    /^610[\\0-\\4]/` answered 0 where Perl answered 1 -- found by
    verify_cond.py once its census reached the IFD tables (slice I-2).
    """
    out = []
    i, n = 0, len(pattern)
    while i < n:
        ch = pattern[i]
        if ch != "\\" or i + 1 >= n:
            out.append(ch)
            i += 1
            continue
        nxt = pattern[i + 1]
        if nxt in _OCTAL_DIGITS:
            j = i + 1
            while j < n and j < i + 4 and pattern[j] in _OCTAL_DIGITS:
                j += 1
            out.append(f"\\x{int(pattern[i + 1:j], 8):02x}")
            i = j
            continue
        out.append(ch + nxt)
        i += 2
    return "".join(out)


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
        rust_pattern = _perl_regex_to_rust(pattern)
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
        member = _member_name(text)
        return f"Cond::MemberTruthy {{ member: {_rust_str(member)}, negate: false }}"

    m = _RE_DEFINED_ATOM.match(text)
    if m:
        member = _member_name(text)
        return f"Cond::MemberDefined {{ member: {_rust_str(member)}, negate: false }}"

    m = _RE_VALPT_ATOM.match(text)
    if m:
        op, pattern, flags = m.group(1), m.group(2), m.group(3)
        if flags:
            raise CondCompileError("$$valPt regex flags are unsupported")
        _validate_regex_pattern(pattern)
        rust_pattern = _perl_regex_to_rust(pattern)
        negate = "true" if op == "!~" else "false"
        return f"Cond::ValPtRegex {{ pattern: {_rust_str(rust_pattern)}, negate: {negate} }}"

    m = _RE_FORMAT_ATOM.match(text)
    if m:
        negate = "true" if m.group(1) == "ne" else "false"
        return f"Cond::FormatEq {{ value: {_rust_str(m.group(2))}, negate: {negate} }}"

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
            # `(X = EXPR) and A or B` is `((X = EXPR) and A) or B` in Perl
            # (`and` binds tighter than `or`); `then` can only carry the
            # `and`-level tail, so a top-level `or` in the tail is refused
            # rather than silently re-grouped under the assignment.
            toks = _tokenize(rest)
            if ("op", "or") in toks:
                raise CondCompileError(
                    f"SetMember's trailing clause carries a top-level `or`: {rest!r}"
                )
            then_expr = _Parser(toks).parse()
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


def _collapse_ws_outside_literals(text):
    """Collapse runs of whitespace to one space EXCEPT inside a regex body
    (`=~`/`!~` followed by `/.../`) or a quoted string, where every byte is
    the pattern. `Sony.pm`'s MakerNotes signature `VHAB     \0` (five
    spaces) compiled to `VHAB \x00` under the old whole-text collapse and
    disagreed with the pinned Perl on the first probe that carried the real
    signature -- found by verify_cond.py once its census reached the IFD
    tables (slice I-2)."""
    out = []
    i, n = 0, len(text)
    while i < n:
        ch = text[i]
        # regex literal after =~ / !~ (the only place a bare `/` opens one)
        if ch in "=!" and text.startswith("~", i + 1):
            out.append(ch + "~")
            i += 2
            j = i
            while j < n and text[j].isspace():
                j += 1
            if j < n and text[j] == "m" and j + 1 < n and text[j + 1] == "/":
                # `=~ m/.../`: the `m` is part of the operator spelling, not
                # of the body; keep it and open the body one char later.
                out.append(" m" if j > i else "m")
                j += 1
            if j < n and text[j] == "/":
                out.append(" " if j > i and text[j - 1] != "m" else "")
                k = j + 1
                while k < n:
                    if text[k] == "\\" and k + 1 < n:
                        k += 2
                        continue
                    if text[k] == "/":
                        break
                    k += 1
                out.append(text[j:k + 1])
                i = k + 1
            continue
        if ch in "\"'":
            k = i + 1
            while k < n:
                if text[k] == "\\" and k + 1 < n:
                    k += 2
                    continue
                if text[k] == ch:
                    break
                k += 1
            out.append(text[i:k + 1])
            i = k + 1
            continue
        if ch.isspace():
            while i < n and text[i].isspace():
                i += 1
            out.append(" ")
            continue
        out.append(ch)
        i += 1
    return "".join(out)


# --- boolean connectives: tokenizer + precedence parser (slice I-3) -------

# `(literal, token)` pairs, tried in this order at a connective position.
_CONNECTIVE_TOKENS = (("and ", "and"), ("or ", "or"), ("||", "||"), ("&&", "&&"))


def _skip_delimited(text, i, close):
    """`text[i]` is an opening delimiter; return the index just past the
    matching `close`, honouring backslash escapes. An unterminated literal
    runs to the end of the text (the atom regexes then refuse it)."""
    k = i + 1
    n = len(text)
    while k < n:
        if text[k] == "\\" and k + 1 < n:
            k += 2
            continue
        if text[k] == close:
            return k + 1
        k += 1
    return n


def _tokenize(text):
    """Split a whitespace-collapsed Condition into `("atom", text)` and
    `("op", name)` tokens, `name` one of `and`, `or`, `||`, `&&`, `not`,
    `!`. Connectives are recognised only at the top level: a regex body
    (after `=~`/`!~`, optionally `m`-prefixed) and a quoted string are
    skipped whole, so `/ or /` and `"a && b"` never split an atom. Prefix
    `not `/`!` are emitted where an operand is expected; everything else at
    an operand position runs to the next top-level connective and is one
    atom for `_compile_atom` to accept or refuse -- which is how a
    parenthesised group `(A or B)` refuses: `(A` is not an atom."""
    toks = []
    i, n = 0, len(text)
    expect_operand = True
    while i < n:
        if text[i] == " ":
            i += 1
            continue
        if expect_operand:
            if text.startswith("not ", i):
                toks.append(("op", "not"))
                i += 4
                continue
            if text[i] == "!" and not text.startswith("!~", i):
                toks.append(("op", "!"))
                i += 1
                continue
            start = i
            while i < n:
                if text[i] in "=!" and text.startswith("~", i + 1):
                    i += 2
                    while i < n and text[i] == " ":
                        i += 1
                    if i + 1 < n and text[i] == "m" and text[i + 1] == "/":
                        i += 1
                    if i < n and text[i] == "/":
                        i = _skip_delimited(text, i, "/")
                    continue
                if text[i] in "\"'":
                    i = _skip_delimited(text, i, text[i])
                    continue
                if (
                    text.startswith(" and ", i)
                    or text.startswith(" or ", i)
                    or text.startswith("||", i)
                    or text.startswith("&&", i)
                ):
                    break
                i += 1
            chunk = text[start:i].strip()
            if not chunk:
                raise CondCompileError(f"missing operand before {text[i:]!r}")
            toks.append(("atom", chunk))
            expect_operand = False
            continue
        for literal, name in _CONNECTIVE_TOKENS:
            if text.startswith(literal, i):
                toks.append(("op", name))
                i += len(literal)
                expect_operand = True
                break
        else:
            raise CondCompileError(f"unexpected text after an operand: {text[i:]!r}")
    if expect_operand:
        raise CondCompileError("dangling connective at the end of the condition")
    return toks


def _rfold(kind, parts):
    """Right-fold `parts` into nested `Cond::<kind>(&a, &b)`: `A and B and
    C` -> `And(A, And(B, C))`, the same left-to-right short-circuit order
    Perl's left-associative chain has (see module docstring). Byte-for-byte
    the text `compile_cond_atoms_conjunction` emitted before slice I-3 for an
    `and` chain, so the committed tables do not churn."""
    expr = parts[-1]
    for c in reversed(parts[:-1]):
        expr = f"Cond::{kind}(&{c}, &{expr})"
    return expr


class _Parser:
    """Perl precedence over `_tokenize`'s tokens (perlop, lowest first):
    `or` < `and` < `not` < `||` < `&&` < `!`. Each level is a method; a
    chain at one level right-folds into `Cond::Or`/`Cond::And`."""

    def __init__(self, toks):
        self._toks = toks
        self._i = 0

    def _peek_op(self):
        if self._i < len(self._toks) and self._toks[self._i][0] == "op":
            return self._toks[self._i][1]
        return None

    def _take(self):
        tok = self._toks[self._i]
        self._i += 1
        return tok

    def parse(self):
        expr = self._parse_or()
        if self._i != len(self._toks):
            raise CondCompileError(f"unparsed tail {self._toks[self._i:]!r}")
        return expr

    def _parse_or(self):
        parts = [self._parse_and()]
        while self._peek_op() == "or":
            self._take()
            parts.append(self._parse_and())
        return _rfold("Or", parts)

    def _parse_and(self):
        parts = [self._parse_not()]
        while self._peek_op() == "and":
            self._take()
            parts.append(self._parse_not())
        return _rfold("And", parts)

    def _parse_not(self):
        if self._peek_op() == "not":
            self._take()
            expr = self._negated()
            # `not` binds looser than `||`/`&&`: `not A || B` is
            # `not (A || B)`, a negation over a disjunction the schema has no
            # node for. Refuse rather than emit `(not A) || B`.
            if self._peek_op() in ("||", "&&"):
                raise CondCompileError(
                    "`not` over a `||`/`&&` chain (Perl groups the whole chain under "
                    "the `not`) is outside the grammar"
                )
            return expr
        return self._parse_oror()

    def _parse_oror(self):
        parts = [self._parse_andand()]
        while self._peek_op() == "||":
            self._take()
            parts.append(self._parse_andand())
        return _rfold("Or", parts)

    def _parse_andand(self):
        parts = [self._parse_bang()]
        while self._peek_op() == "&&":
            self._take()
            parts.append(self._parse_bang())
        return _rfold("And", parts)

    def _parse_bang(self):
        if self._peek_op() == "!":
            self._take()
            # `!` binds tighter than every connective, so whatever follows
            # the negated operand is handled by the levels above.
            return self._negated()
        return self._parse_atom()

    def _parse_atom(self):
        if self._i >= len(self._toks) or self._toks[self._i][0] != "atom":
            got = self._toks[self._i] if self._i < len(self._toks) else None
            raise CondCompileError(f"expected an operand, found {got!r}")
        return _compile_atom(self._take()[1])

    def _negated(self):
        """The operand of a `not`/`!`: a bare member or a `defined` test --
        the two shapes whose negation the schema carries as a flag
        (`MemberTruthy`/`MemberDefined { negate: true }`). Anything else
        (`!$$self{X} == 1`, which Perl reads as `(!$$self{X}) == 1`; `not
        $$self{X} =~ /p/`) is refused rather than negated as a whole."""
        if self._i >= len(self._toks) or self._toks[self._i][0] != "atom":
            got = self._toks[self._i] if self._i < len(self._toks) else None
            raise CondCompileError(f"`not`/`!` needs a member or `defined` operand, found {got!r}")
        text = self._take()[1]
        if _RE_DEFINED_ATOM.match(text):
            member = _member_name(text)
            return f"Cond::MemberDefined {{ member: {_rust_str(member)}, negate: true }}"
        if _RE_BARE_ATOM.match(text):
            member = _member_name(text)
            return f"Cond::MemberTruthy {{ member: {_rust_str(member)}, negate: true }}"
        raise CondCompileError(
            f"`not`/`!` over {text!r} is outside the grammar (only a bare member or a "
            "`defined` test negates)"
        )


def _compile_text(text):
    """`compile_cond_atoms_conjunction` without the refusal-to-None wrap:
    raises `CondCompileError` naming the first construct outside the
    grammar (what `refusal_reason` reports)."""
    text = _collapse_ws_outside_literals(text.strip())
    sm = _compile_setmember(text)
    if sm is not None:
        return sm
    return _Parser(_tokenize(text)).parse()


def compile_cond_atoms_conjunction(text):
    """Compile `text` as a single atom, a boolean combination of atoms at
    Perl precedence (`_Parser`: `or`/`||` -> right-nested `Cond::Or`,
    `and`/`&&` -> right-nested `Cond::And`, `not`/`!` over a bare member or
    a `defined` test), or a `SetMember` idiom. Returns Rust source text for
    a `Cond` value, or None if anything in it is outside this grammar
    (caller decides whether that is a hard refusal)."""
    try:
        return _compile_text(text)
    except CondCompileError:
        return None


def refusal_reason(condition):
    """Why `compile_cond(condition)` returned None: the message of the first
    `CondCompileError` the compiler raised, for reports that must NAME the
    construct (`triage_bump.py`). `None` when the Condition compiles. The
    AUTO/COND decision itself is always `compile_cond`'s -- this is the same
    parser, run once more for its message, never a second grammar."""
    if condition is None or not isinstance(condition, str) or not condition.strip():
        return None if condition is None else "unrecognised condition shape (not a non-empty string)"
    try:
        _compile_text(condition)
    except CondCompileError as e:
        return str(e)
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
