"""Compile and evaluate the early-return scalar branch of native CheckValue.

This is executable helper semantics, not a public metadata writer. Numeric
formats remain unsupported. Admission proves the complete entry prefix and
scalar branch, including its unconditional return; the numeric tail cannot run
for an admitted format. Source operators and literals remain recipe operands.
"""
from dataclasses import dataclass
import operator
from typing import Any

from checkexif_recipes import CodeProvenance, RecipeRefused, _Body, _fact


@dataclass(frozen=True)
class NativeScalar:
    """Retain Perl's byte-string versus UTF8-flagged character semantics."""
    kind: str
    value: bytes | str | None

    def __post_init__(self):
        expected = {"bytes": bytes, "utf8": str, "undefined": type(None)}
        if self.kind not in expected or type(self.value) is not expected[self.kind]:
            raise RecipeRefused("scalar kind/value mismatch")


@dataclass(frozen=True)
class ScalarCheckRecipe:
    provenance: CodeProvenance
    formats: tuple[str, str]
    positive_count_operator: str
    positive_count_bound: int
    first_format: str
    first_limit_operator: str
    first_error: str
    second_limit_operator: str
    second_error: str
    padding_operator: str
    padding_character: str


_COMPARE = {">": operator.gt, ">=": operator.ge, "<": operator.lt,
            "<=": operator.le, "==": operator.eq, "!=": operator.ne}


def _comparison(body: _Body) -> str:
    for symbol in (">=", "<=", "==", "!=", ">", "<"):
        tokens = list(symbol)
        if body.tokens[body.at:body.at + len(tokens)] == tokens:
            body.at += len(tokens)
            return symbol
    raise RecipeRefused("unsupported scalar comparison")


def _literal(body: _Body) -> str:
    # Preserve plain error/format literals; double-quoted interpolation is not
    # a literal and must not be mistaken for one by the reference evaluator.
    token = body.tokens[body.at] if body.at < len(body.tokens) else ""
    if token.startswith('"') and any(c in token[1:-1] for c in "$@"):
        raise RecipeRefused("interpolating scalar literal is unsupported")
    return body.quoted()


def compile_scalar_check(fact: dict[str, Any]) -> ScalarCheckRecipe:
    provenance = _fact(fact, "native_write_helpers.check_value", require_binding=True)
    if provenance.requested_binding != "Image::ExifTool::CheckValue":
        raise RecipeRefused("CheckValue requested binding is stale")
    body = _Body(fact.get("__deparse"))
    body.take("(", "$", "$", ";", "$", ")", "{", "package")
    package = body.tokens[body.at] if body.at < len(body.tokens) else ""
    if not package or "::" not in package:
        raise RecipeRefused("CheckValue package unavailable")
    body.at += 1
    body.take(";", "use", "strict", ";", "(", "my", "(")
    value = body.word("$")
    body.take(",")
    format_name = body.word("$")
    body.take(",")
    count = body.word("$")
    body.take(")", "=", "@", "_", ")", ";", "my", "(")
    array = body.word("@")
    body.take(",")
    local_value = body.word("$")
    body.take(",")
    index = body.word("$")
    body.take(")", ";")
    if len({value, format_name, count, local_value, index}) != 5:
        raise RecipeRefused("CheckValue local scalar aliases an argument")
    body.take("if", "(", "(", "(", "$", format_name, "eq")
    first = _literal(body)
    body.take(")", "or", "(", "$", format_name, "eq")
    second = _literal(body)
    body.take(")", ")", ")", "{", "(", "(", "$", count, "and", "(", "$", count)
    count_op = _comparison(body)
    token = body.tokens[body.at] if body.at < len(body.tokens) else ""
    if not token.isdecimal():
        raise RecipeRefused("noninteger count guard bound")
    bound = int(token)
    body.at += 1
    body.take(")", ")", "or", "(", "return", "(", "undef", ")", ")", ")", ";", "(", "my")
    length = body.word("$")
    if length in {value, format_name, count, local_value, index}:
        raise RecipeRefused("CheckValue length aliases an existing scalar")
    body.take("=", "length", "(", "$", "$", value, ")", ")", ";", "if", "(", "(", "$", format_name, "eq")
    branch_format = _literal(body)
    body.take(")", ")", "{", "(", "(", "$", length)
    first_op = _comparison(body)
    body.take("$", count, ")", "and", "(", "return")
    first_error = _literal(body)
    body.take(")", ")", ";", "}", "else", "{", "(", "(", "$", length)
    second_op = _comparison(body)
    body.take("$", count, ")", "and", "(", "return")
    second_error = _literal(body)
    body.take(")", ")", ";", "}", "if", "(", "(", "$", length)
    padding_op = _comparison(body)
    body.take("$", count, ")", ")", "{", "(", "$", "$", value, ".", "=", "(")
    token = body.tokens[body.at] if body.at < len(body.tokens) else ""
    # B::Deparse spells the canonical NUL as a three-digit octal escape.
    if token != '"\\000"':
        raise RecipeRefused("unsupported scalar padding literal")
    body.at += 1
    body.take("x", "(", "$", count, "-", "$", length, ")", ")", ")", ";", "}", "(", "return", "(", "undef", ")", ")", ";", "}")
    if body.at >= len(body.tokens) or body.tokens[-1] != "}":
        raise RecipeRefused("CheckValue function tail is incomplete")
    if first == second:
        raise RecipeRefused("CheckValue scalar format alternatives are not distinct")
    return ScalarCheckRecipe(provenance, (first, second), count_op, bound,
                             branch_format, first_op, first_error, second_op,
                             second_error, padding_op, "\0")


def evaluate_scalar_check(recipe: ScalarCheckRecipe, value: NativeScalar,
                          format_name: str, count: int | None) -> tuple[NativeScalar, str | None]:
    if format_name not in recipe.formats:
        raise RecipeRefused("format is outside the proven early-return scalar branch")
    if count is not None and type(count) is not int:
        raise RecipeRefused("noninteger native count is unsupported")
    if not count or not _COMPARE[recipe.positive_count_operator](count, recipe.positive_count_bound):
        return value, None
    length = 0 if value.kind == "undefined" else len(value.value)
    first = format_name == recipe.first_format
    comparison = recipe.first_limit_operator if first else recipe.second_limit_operator
    if _COMPARE[comparison](length, count):
        return value, recipe.first_error if first else recipe.second_error
    if _COMPARE[recipe.padding_operator](length, count):
        # Perl string repetition with a negative count produces an empty string.
        padding = recipe.padding_character * max(0, count - length)
        if value.kind == "utf8":
            value = NativeScalar("utf8", value.value + padding)
        else:
            raw = b"" if value.kind == "undefined" else value.value
            value = NativeScalar("bytes", raw + padding.encode("latin1"))
    return value, None
