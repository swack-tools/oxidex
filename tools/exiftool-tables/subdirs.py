#!/usr/bin/env python3
r"""Step 27's `SubDirectory` compiler: a closed grammar over ExifTool.pm
`ProcessBinaryData`'s `Start`/`Base` eval-strings (ExifTool.pm:10102-10151;
full citation and semantics in `src/exiftool_tables/subdir.rs`'s module doc,
which this file's output is checked against).

Same doctrine as `conds.py` (this file's sibling for `_variants` `Condition`
strings): a `SubDirectory` compiles to a `SubdirEdge` only if every one of its
constructs is recognised and provably reproduced -- `TagTable` parses into a
two-level `Image::ExifTool::<Module>::<Table>` name, `Start`/`Base` (when
present) are pure arithmetic over the small variable set ExifTool actually
binds in each eval scope, and neither `ProcessProc`, `ByteOrder` nor
`Validate` is declared (see `subdir.rs` for why: the first changes how the
*target* is walked, which this schema cannot express without lying about it;
the other two are keys `ProcessBinaryData`'s SubDirectory branch never even
reads, so a table declaring one needs a human to check what changed before
this compiler trusts it). Anything else -- an unparseable arithmetic
expression, a non-arithmetic AST node (`**`, a function call, a comparison),
a `TagTable` with the wrong nesting depth -- fails to compile, and the caller
must count the refusal by reason rather than emit a guess.

`Start`/`Base` are validated structurally, not by a character allowlist:
`_compile_arith` parses the (variable-substituted) Perl expression with
Python's own `ast.parse` -- the same "use a real parser, walk a vetted opcode
allowlist" discipline `conds.py` applies to regex patterns via Python's `re`
parser -- and `_node_to_rust` recurses through the resulting tree, raising on
any node shape outside `{Constant, Name, BinOp(Add|Sub|Mult), UnaryOp(USub|
UAdd)}`. Because Python and Rust agree on the surface syntax for exactly this
restricted subset (integer literals, `+`/`-`/`*`, parens, identifiers), the
recursion doubles as Rust code generation: there is no separate "render to
Rust" pass to drift out of sync with the validator.
"""

import ast
import re

# The two ExifTool.pm eval scopes a SubDirectory field's Start/Base can
# reference (ExifTool.pm:10129 and :10120 respectively) -- see subdir.rs's
# module doc for why they are kept separate rather than one shared variable
# set. Maps the Perl variable name (sans `$`) to the Rust enum's variant name.
START_VARS = {"val": "Val", "dirStart": "DirStart"}
BASE_VARS = {"start": "Start", "base": "Base"}

_PERL_VAR_RE = re.compile(r"\$(\w+)")

_BINOP_RUST = {ast.Add: "Add", ast.Sub: "Sub", ast.Mult: "Mul"}


class SubdirCompileError(Exception):
    """Internal: this SubDirectory construct is outside the closed grammar."""


def _substitute_vars(text, allowed_vars):
    """Replace every `$name` in `text` with its Rust-identifier form from
    `allowed_vars`, raising if `text` references a variable outside that map
    -- e.g. a `Base` expression that reaches for `$val`, which real
    ExifTool.pm never binds in that eval scope (ExifTool.pm:10120's marker
    lists only `$start`/`$base`)."""

    def repl(m):
        name = m.group(1)
        if name not in allowed_vars:
            raise SubdirCompileError(f"unknown variable ${name} in {text!r}")
        return allowed_vars[name]

    return _PERL_VAR_RE.sub(repl, text)


def _node_to_rust(node, rust_enum):
    """Recursively validate `node` (a Python `ast` node from the
    variable-substituted expression) against the closed arithmetic grammar,
    and return Rust source constructing it as `rust_enum::...`. Raises
    SubdirCompileError for any node shape outside `{Constant(int), Name,
    BinOp(Add|Sub|Mult), UnaryOp(USub|UAdd)}` -- the opcode allowlist, walked
    via the real parser rather than assumed from a character blacklist."""
    if isinstance(node, ast.Expression):
        return _node_to_rust(node.body, rust_enum)
    if isinstance(node, ast.Constant):
        if not isinstance(node.value, int) or isinstance(node.value, bool):
            raise SubdirCompileError(f"non-integer constant {node.value!r}")
        return f"{rust_enum}::Const({node.value})"
    if isinstance(node, ast.Name):
        # `_substitute_vars` already proved every `$var` maps to one of these
        # identifiers; any OTHER bare identifier reaching here means the
        # source text had a bareword Perl never treats as one of our
        # variables (e.g. a stray function name) slip past substitution.
        return f"{rust_enum}::{node.id}"
    if isinstance(node, ast.UnaryOp):
        if isinstance(node.op, ast.UAdd):
            return _node_to_rust(node.operand, rust_enum)
        if isinstance(node.op, ast.USub):
            inner = _node_to_rust(node.operand, rust_enum)
            return f"{rust_enum}::Neg(&{inner})"
        raise SubdirCompileError(f"unsupported unary operator {ast.dump(node.op)}")
    if isinstance(node, ast.BinOp):
        variant = _BINOP_RUST.get(type(node.op))
        if variant is None:
            raise SubdirCompileError(f"unsupported binary operator {ast.dump(node.op)}")
        left = _node_to_rust(node.left, rust_enum)
        right = _node_to_rust(node.right, rust_enum)
        return f"{rust_enum}::{variant}(&{left}, &{right})"
    raise SubdirCompileError(f"disallowed syntax node {type(node).__name__}")


def _compile_arith(perl_text, rust_enum, allowed_vars):
    """Compile one Perl arithmetic expression (already known to reference
    only `allowed_vars`) into Rust source for `rust_enum`, or raise
    SubdirCompileError. `rust_enum` is `"StartExpr"` or `"BaseExpr"`."""
    text = perl_text.strip()
    if not text:
        raise SubdirCompileError("empty expression")
    substituted = _substitute_vars(text, allowed_vars)
    try:
        tree = ast.parse(substituted, mode="eval")
    except SyntaxError as e:
        raise SubdirCompileError(f"unparseable expression {perl_text!r}: {e}") from e
    return _node_to_rust(tree, rust_enum)


_TAGTABLE_RE = re.compile(r"^Image::ExifTool::(\w+)::(\w+)$")


def parse_tag_table(tagtable):
    """`SubDirectory.TagTable` -> `(module, table)`, or raise
    SubdirCompileError. Every binary-table SubDirectory in the pinned 13.59
    census is exactly `Image::ExifTool::<Module>::<Table>` (two levels under
    the package prefix); a different nesting depth is refused rather than
    guessed at which segment is the "module"."""
    if not isinstance(tagtable, str):
        raise SubdirCompileError(f"TagTable is not a string: {tagtable!r}")
    m = _TAGTABLE_RE.match(tagtable)
    if not m:
        raise SubdirCompileError(f"unrecognised TagTable shape {tagtable!r}")
    return m.group(1), m.group(2)


def compile_start(value):
    """`SubDirectory.Start` -> Rust source for a `Start` value.

    Mirrors ExifTool.pm:10124-10137's own branch on whether the raw string
    contains a literal `$`:
      * absent, or a bare integer with no `$` -> `Start::FieldRelative(N)`
        (ExifTool.pm:10134-10136's `$start += $dirStart + $entry`).
      * contains `$` -> compiled via `_compile_arith` against `START_VARS`
        and wrapped `Start::Expr(&EXPR)` (ExifTool.pm:10129-10130).
    Raises SubdirCompileError for anything else (non-string, unparseable).
    """
    if value is None:
        return "Start::FieldRelative(0)"
    if not isinstance(value, str):
        raise SubdirCompileError(f"Start is not a string: {value!r}")
    text = value.strip()
    if "$" not in text:
        try:
            literal = int(text, 0) if text else 0
        except ValueError as e:
            raise SubdirCompileError(f"unparseable literal Start {value!r}") from e
        return f"Start::FieldRelative({literal})"
    expr_src = _compile_arith(text, "StartExpr", START_VARS)
    return f"Start::Expr(&{expr_src})"


def compile_base(value):
    """`SubDirectory.Base` -> Rust source for a `BaseExpr` value (the caller
    wraps it `Some(&...)`), or raise SubdirCompileError. Unlike `Start`,
    ExifTool.pm:10119-10123 evaluates `Base` unconditionally whenever the key
    is defined -- there is no literal-vs-`$` branch -- so a bare integer
    (`Base => 4`) is just an `ast.Constant` `_compile_arith` already handles;
    no separate literal path is needed here."""
    if not isinstance(value, str):
        raise SubdirCompileError(f"Base is not a string: {value!r}")
    return _compile_arith(value, "BaseExpr", BASE_VARS)
