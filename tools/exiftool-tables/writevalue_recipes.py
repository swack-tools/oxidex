"""Compile and evaluate the scalar ``WriteValue`` branch from captured Perl.

This is source-derived helper execution only.  It neither selects a tag nor
writes a file.  Numeric packing and the optional ``$dataPt`` mutation remain
outside this closed step.
"""
from dataclasses import dataclass
import operator
from typing import Any, Mapping

from checkexif_recipes import CodeProvenance, RecipeMalformed, RecipeRefused, _Body, _fact
from checkvalue_recipes import NativeScalar


_COMPARE = {">": operator.gt, ">=": operator.ge, "<": operator.lt,
            "<=": operator.le, "==": operator.eq, "!=": operator.ne}


@dataclass(frozen=True)
class ScalarWriteRecipe:
    provenance: CodeProvenance
    dispatch_lexical: str
    formats: tuple[str, str]
    terminated_format: str
    count_positive_operator: str
    count_positive_bound: int
    diff_negative_operator: str
    terminator: str


@dataclass(frozen=True)
class ScalarWriteResult:
    value: NativeScalar
    count: int


def _comparison(body: _Body) -> str:
    for symbol in (">=", "<=", "==", "!=", ">", "<"):
        if body.tokens[body.at:body.at + len(symbol)] == list(symbol):
            body.at += len(symbol)
            return symbol
    raise RecipeRefused("unsupported WriteValue comparison")


def _literal(body: _Body) -> str:
    token = body.tokens[body.at] if body.at < len(body.tokens) else ""
    if token.startswith('"') and any(char in token[1:-1] for char in "$@"):
        raise RecipeRefused("interpolating WriteValue literal is unsupported")
    return body.quoted()


def _skip_block(body: _Body) -> None:
    """Skip only the proven-unreachable dispatch body, checking brace balance."""
    depth = 1
    while body.at < len(body.tokens) and depth:
        token = body.tokens[body.at]
        body.at += 1
        if token == "{":
            depth += 1
        elif token == "}":
            depth -= 1
    if depth:
        raise RecipeRefused("WriteValue dispatch block is incomplete")


def _dispatch_is_absent(fact: Mapping[str, Any], lexical: str, formats: tuple[str, str]) -> None:
    hashes = fact.get("lexical_hashes")
    if not isinstance(hashes, Mapping) or hashes.get("resolved") is not True:
        raise RecipeRefused("WriteValue lexical hash capture is unresolved")
    bindings = hashes.get("bindings")
    if not isinstance(bindings, Mapping):
        raise RecipeMalformed("WriteValue lexical hash bindings are malformed")
    captured = bindings.get(lexical)
    if not isinstance(captured, Mapping) or captured.get("resolved") is not True:
        raise RecipeRefused("WriteValue dispatch lexical hash is unresolved")
    entries = captured.get("entries")
    if not isinstance(entries, Mapping):
        raise RecipeMalformed("WriteValue dispatch lexical entries are malformed")
    if any(not isinstance(key, str) for key in entries):
        raise RecipeMalformed("WriteValue dispatch lexical key is malformed")
    for format_name in formats:
        if format_name in entries:
            raise RecipeRefused("WriteValue scalar format is intercepted by live dispatch")


def compile_scalar_write(fact: dict[str, Any]) -> ScalarWriteRecipe:
    provenance = _fact(fact, "native_write_helpers.write_value", require_binding=True)
    if provenance.requested_binding != "Image::ExifTool::WriteValue":
        raise RecipeRefused("WriteValue requested binding is stale")
    body = _Body(fact.get("__deparse"))
    body.take("(", "$", "$", ";", "$", "$", "$", "$", ")", "{", "package")
    package = body.tokens[body.at] if body.at < len(body.tokens) else ""
    if not package or "::" not in package:
        raise RecipeRefused("WriteValue package unavailable")
    body.at += 1
    body.take(";", "use", "strict", ";", "(", "my", "(")
    value = body.word("$")
    body.take(",")
    format_name = body.word("$")
    body.take(",")
    count = body.word("$")
    body.take(",")
    data = body.word("$")
    body.take(",")
    offset = body.word("$")
    body.take(")", "=", "@", "_", ")", ";", "(", "my")
    proc = body.word("$")
    body.take("=", "$")
    # ``word`` owns sigils, but this lookup is a hash sigil followed by a
    # variable key and must retain both names for the live-pad join.
    if body.at >= len(body.tokens) or not body.tokens[body.at].isidentifier():
        raise RecipeRefused("WriteValue dispatch is not a lexical hash lookup")
    lexical = "%" + body.tokens[body.at]
    body.at += 1
    body.take("{", "$")
    if body.at >= len(body.tokens) or body.tokens[body.at] != format_name:
        raise RecipeRefused("WriteValue dispatch key is not the format argument")
    body.at += 1
    body.take("}", ")", ";", "my", "$")
    if body.at >= len(body.tokens) or not body.tokens[body.at].isidentifier():
        raise RecipeRefused("WriteValue packed local is unavailable")
    packed = body.tokens[body.at]
    body.at += 1
    body.take(";", "if", "(", "$", proc, ")", "{")
    if len({value, format_name, count, data, offset, proc, packed}) != 7:
        raise RecipeRefused("WriteValue local aliases an argument")
    _skip_block(body)
    body.take("elsif", "(", "(", "(", "$", format_name, "eq")
    first = _literal(body)
    body.take(")", "or", "(", "$", format_name, "eq")
    second = _literal(body)
    body.take(")", ")", ")", "{")
    body.take("(", "(", "$", format_name, "eq")
    terminated = _literal(body)
    body.take(")", "and", "(", "$", value, ".", "=")
    token = body.tokens[body.at] if body.at < len(body.tokens) else ""
    if token != '"\\000"':
        raise RecipeRefused("unsupported WriteValue terminator")
    body.at += 1
    body.take(")", ")", ";", "if", "(", "(", "$", count, "and", "(", "$", count)
    positive = _comparison(body)
    if positive != ">":
        raise RecipeRefused("unsupported WriteValue count guard operator")
    token = body.tokens[body.at] if body.at < len(body.tokens) else ""
    if not token.isdecimal():
        raise RecipeRefused("noninteger WriteValue count guard bound")
    bound = int(token)
    body.at += 1
    body.take(")", ")", ")", "{")
    body.take("(", "my", "$")
    if body.at >= len(body.tokens) or not body.tokens[body.at].isidentifier():
        raise RecipeRefused("WriteValue diff local is unavailable")
    diff = body.tokens[body.at]
    body.at += 1
    if diff in {value, format_name, count, data, offset, proc, packed}:
        raise RecipeRefused("WriteValue diff local aliases an existing local")
    body.take("=", "(", "$", count, "-", "length", "(", "$", value, ")", ")", ")", ";", "if", "(", "$", diff, ")", "{")
    body.take("if", "(", "(", "$", diff)
    negative = _comparison(body)
    if negative != "<":
        raise RecipeRefused("unsupported WriteValue truncation operator")
    body.take("0", ")", ")", "{")
    body.take("if", "(", "(", "$", format_name, "eq")
    truncate_terminated = _literal(body)
    body.take(")", ")", "{")
    body.take("(", "$", count, "or", "(", "return", "(", "undef", ")", ")", ")", ";")
    body.take("(", "$", value, "=", "substr", "(", "$", value, ",", "0", ",", "(", "$", count, "-", "1", ")", ")", ".")
    if body.at >= len(body.tokens) or body.tokens[body.at] != '"\\000"':
        raise RecipeRefused("unsupported WriteValue truncation terminator")
    body.at += 1
    body.take(")", ";", "}", "else", "{")
    body.take("(", "$", value, "=", "substr", "(", "$", value, ",", "0", ",", "$", count, ")", ")", ";", "}", "}", "else", "{")
    body.take("(", "$", value, ".", "=", "(")
    if body.at >= len(body.tokens) or body.tokens[body.at] != '"\\000"':
        raise RecipeRefused("unsupported WriteValue padding literal")
    body.at += 1
    body.take("x", "$", diff, ")", ")", ";", "}", "}", "}", "else", "{")
    body.take("(", "$", count, "=", "length", "(", "$", value, ")", ")", ";", "}")
    # ``$dataPt`` mutation is deliberately not executed, but its exact
    # placement before the scalar return proves this branch has no other call.
    body.take("(", "$", data, "and", "substr", "(", "$", "$", data, ",", "$", offset, ",", "$", count, ")", "=", "$", value, ")", ";", "(", "return", "$", value, ")", ";", "}")
    if body.at >= len(body.tokens):
        raise RecipeRefused("WriteValue fallback is incomplete")
    # The scalar return must be followed by a separate fallback and the numeric
    # tail; we do not execute either.  Balanced completion catches an inserted
    # scalar-side statement while avoiding a numerical grammar claim.
    body.take("else", "{")
    _skip_block(body)
    if body.at >= len(body.tokens) or body.tokens[-1] != "}":
        raise RecipeRefused("WriteValue function tail is incomplete")
    # The remaining numeric-tail return is unreachable after the scalar
    # branch's proven return. It is deliberately not translated here.
    if first == second or terminated not in {first, second} or truncate_terminated != terminated:
        raise RecipeRefused("WriteValue scalar format controls are inconsistent")
    _dispatch_is_absent(fact, lexical, (first, second))
    return ScalarWriteRecipe(provenance, lexical, (first, second), terminated,
                             positive, bound, negative, "\0")


def _append(value: NativeScalar, text: str) -> NativeScalar:
    if value.kind == "utf8":
        return NativeScalar("utf8", value.value + text)
    raw = b"" if value.kind == "undefined" else value.value
    return NativeScalar("bytes", raw + text.encode("latin1"))


def _truncate(value: NativeScalar, count: int) -> NativeScalar:
    if value.kind == "undefined":
        return value
    truncated = value.value[:count]
    if value.kind == "utf8" and not any(ord(char) > 0x7f for char in value.value):
        # Perl's ``substr`` may downgrade an upgraded ASCII scalar.  Keep that
        # state transition explicit instead of pretending all Python ``str``
        # results retain the UTF8 flag; a non-ASCII source retains it even
        # when the selected substring is empty.
        return NativeScalar("bytes", truncated.encode("latin1"))
    return NativeScalar(value.kind, truncated)


def evaluate_scalar_write(recipe: ScalarWriteRecipe, value: NativeScalar,
                          format_name: str, count: int | None) -> ScalarWriteResult:
    """Execute the admitted scalar return branch without ``$dataPt`` mutation."""
    if format_name not in recipe.formats:
        raise RecipeRefused("format is outside the proven WriteValue scalar branch")
    if count is not None and type(count) is not int:
        raise RecipeRefused("noninteger native count is unsupported")
    if format_name == recipe.terminated_format:
        value = _append(value, recipe.terminator)
    length = 0 if value.kind == "undefined" else len(value.value)
    if count and _COMPARE[recipe.count_positive_operator](count, recipe.count_positive_bound):
        diff = count - length
        if diff:
            if _COMPARE[recipe.diff_negative_operator](diff, 0):
                if format_name == recipe.terminated_format:
                    value = _append(_truncate(value, count - 1), recipe.terminator)
                else:
                    value = _truncate(value, count)
            else:
                value = _append(value, recipe.terminator * diff)
        return ScalarWriteResult(value, count)
    return ScalarWriteResult(value, length)
