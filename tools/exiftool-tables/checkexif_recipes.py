"""Inactive source recipes for ExifTool's shared ``CheckExif`` helper.

This module records the complete, closed CheckExif control flow as source data.
It does not implement ``CheckValue``, call a native writer, or add a Rust
writer route.  A later mechanism must separately recognize the captured
CheckValue body and prove its effects before executing any recipe.

The parser accepts only the pinned helper's small executable grammar.  It does
not identify tables, rows, or tags by name or ID: source tables are linked by
their effective CHECK_PROC facts, and the CheckValue glob is authenticated
through the top-level native_write_helpers sidecar.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
from typing import Any, Mapping

import native_reader_facts

RECIPE_VERSION = 1
RUNTIME_STATUS = "inactive_source_checkexif_recipe_no_writer_route"
_FQ = re.compile(r"^(?:[A-Za-z_]\w*::)+[A-Za-z_]\w*$")
_SHA256 = re.compile(r"^[0-9a-f]{64}$")


class RecipeRefused(ValueError):
    """The native CHECK_PROC cannot be represented by this closed recipe."""


class RecipeMalformed(ValueError):
    """Captured source facts are internally malformed or untrustworthy."""


@dataclass(frozen=True)
class CodeProvenance:
    requested_binding: str | None
    actual_name: str
    source_file: str
    source_sha256: str
    body_sha256: str
    dependencies: tuple["CodeDependency", ...]


@dataclass(frozen=True)
class CodeDependency:
    binding: str
    fact: CodeProvenance


@dataclass(frozen=True)
class Selector:
    source: str
    property: str


@dataclass(frozen=True)
class MissingFormat:
    falsey: bool
    equals_literal: str
    group_source: Selector
    maker_notes_literal: str
    maker_return: str | None
    other_return: str


@dataclass(frozen=True)
class CheckExifRecipe:
    recipe_sha256: str
    check_proc: CodeProvenance
    source_tables: tuple[tuple[str, str, str], ...]
    format_selectors: tuple[Selector, ...]
    missing_format: MissingFormat
    count_operand: Selector
    check_value: CodeProvenance


@dataclass(frozen=True)
class RecipeReport:
    tables_seen: int
    recipes_emitted: int
    omitted_tables: int
    omissions_by_reason: dict[str, int]


def _mapping(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise RecipeMalformed(f"{context} is not an object")
    return value


def _relative_source_file(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value or "\\" in value:
        raise RecipeMalformed(f"{context} is not a normalized relative path")
    path = PurePosixPath(value)
    if (path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts)
            or path.as_posix() != value or not value.startswith("Image/")):
        raise RecipeMalformed(f"{context} is not a normalized relative path")
    return value


def _fact(fact: Any, context: str, *, require_binding: bool) -> CodeProvenance:
    fact = _mapping(fact, context)
    if fact.get("__perl") != "CODE" or fact.get("resolved") is not True:
        raise RecipeRefused(f"{context} is unresolved")
    actual_name = fact.get("__name")
    if not isinstance(actual_name, str) or _FQ.fullmatch(actual_name) is None:
        raise RecipeMalformed(f"{context} has no actual callable name")
    requested = fact.get("requested_binding")
    if require_binding:
        if not isinstance(requested, str) or _FQ.fullmatch(requested) is None:
            raise RecipeRefused(f"{context} lacks requested binding")
    elif requested is not None and (not isinstance(requested, str) or _FQ.fullmatch(requested) is None):
        raise RecipeMalformed(f"{context} has malformed requested binding")
    source_file = _relative_source_file(fact.get("source_file"), f"{context}.source_file")
    source_sha = fact.get("source_sha256")
    if not isinstance(source_sha, str) or _SHA256.fullmatch(source_sha) is None:
        raise RecipeMalformed(f"{context}.source_sha256 is not SHA-256")
    body = fact.get("__deparse")
    if not isinstance(body, str):
        raise RecipeRefused(f"{context} has no deparsed body")
    dependencies = _mapping(fact.get("dependencies", {}), f"{context}.dependencies")
    dependency_facts = []
    for name, value in sorted(dependencies.items()):
        if not isinstance(name, str) or _FQ.fullmatch(name) is None:
            raise RecipeMalformed(f"{context}.dependencies has malformed binding")
        dependency_facts.append(CodeDependency(
            name, _fact(value, f"{context}.dependencies[{name!r}]", require_binding=False)))
    return CodeProvenance(
        requested,
        actual_name,
        source_file,
        source_sha,
        hashlib.sha256(body.encode("utf-8")).hexdigest(),
        tuple(dependency_facts),
    )


class _Body:
    """Closed parser over the shared lexer that preserves quoted literals."""

    def __init__(self, source: Any):
        try:
            self.tokens = native_reader_facts.body_tokens(source)
        except native_reader_facts.ReaderRefused as error:
            raise RecipeRefused("missing CHECK_PROC body") from error
        self.at = 0

    def take(self, *wanted: str) -> None:
        if self.tokens[self.at:self.at + len(wanted)] != list(wanted):
            raise RecipeRefused("CHECK_PROC executable body is outside the closed grammar")
        self.at += len(wanted)

    def word(self, sigil: str) -> str:
        if self.at + 1 >= len(self.tokens) or self.tokens[self.at] != sigil:
            raise RecipeRefused("CHECK_PROC local binding is outside the closed grammar")
        value = self.tokens[self.at + 1]
        if re.fullmatch(r"[A-Za-z_]\w*", value) is None:
            raise RecipeRefused("CHECK_PROC local binding is outside the closed grammar")
        self.at += 2
        return value

    def quoted(self) -> str:
        if self.at >= len(self.tokens):
            raise RecipeRefused("CHECK_PROC expected a source literal")
        token = self.tokens[self.at]
        if len(token) < 2 or token[0] not in "'\"" or token[-1] != token[0]:
            raise RecipeRefused("CHECK_PROC expected a source literal")
        # Supported canonical literals only use ordinary text.  Preserve source
        # spelling by refusing a new escape grammar instead of unescaping it.
        if "\\" in token[1:-1]:
            raise RecipeRefused("CHECK_PROC literal escaping is unsupported")
        self.at += 1
        return token[1:-1]

    def tag_property(self, tag: str) -> Selector:
        self.take("$", tag, "-", ">", "{")
        property_name = self.quoted()
        self.take("}")
        return Selector("tag", property_name)

    def table_property(self, tag: str) -> Selector:
        self.take("$", tag, "-", ">", "{")
        table_name = self.quoted()
        self.take("}", "{")
        property_name = self.quoted()
        self.take("}")
        if table_name != "Table":
            raise RecipeRefused("CHECK_PROC table selector is unsupported")
        return Selector("table", property_name)

    def selector(self, tag: str) -> Selector:
        start = self.at
        candidate = self.tag_property(tag)
        if self.at < len(self.tokens) and self.tokens[self.at:self.at + 1] == ["{"]:
            # Reparse the same source as a table property.  The first parse
            # consumed the Table key, so restoring is exact and bounded.
            self.at = start
            return self.table_property(tag)
        return candidate


def _signature(body: _Body) -> tuple[str, str]:
    body.take("(", "$", "$", "$", ")", "{", "package")
    if body.at >= len(body.tokens) or _FQ.fullmatch(body.tokens[body.at]) is None:
        raise RecipeRefused("CHECK_PROC has no helper package")
    body.at += 1
    body.take(";", "use", "strict", ";", "(", "my", "(")
    _et = body.word("$")
    body.take(",")
    tag = body.word("$")
    body.take(",")
    value = body.word("$")
    body.take(")", "=", "@", "_", ")", ";")
    if len({_et, tag, value}) != 3:
        raise RecipeRefused("CHECK_PROC aliases local arguments")
    return tag, value


def _format_selectors(body: _Body, tag: str) -> tuple[str, tuple[Selector, ...]]:
    body.take("(", "my")
    format_name = body.word("$")
    body.take("=", "(", "(")
    first = body.selector(tag)
    body.take("|", "|")
    second = body.selector(tag)
    body.take(")", "|", "|")
    third = body.selector(tag)
    body.take(")", ")", ";")
    selectors = (first, second, third)
    expected = {("tag", "Format"), ("tag", "Writable"), ("table", "WRITABLE")}
    if {(selector.source, selector.property) for selector in selectors} != expected:
        raise RecipeRefused("CHECK_PROC format selector set is unsupported")
    return format_name, selectors


def _parse_missing_format(body: _Body, tag: str, format_name: str) -> MissingFormat:
    """Parse the branch separately because Groups->{'0'} has two subscripts."""
    body.take("if", "(", "(", "not", "(", "$", format_name, ")", "or", "(", "$", format_name, "eq")
    equals = body.quoted()
    body.take(")", ")", ")", "{")
    body.take("if", "(", "(")
    body.take("$", tag, "-", ">", "{")
    group_property = body.quoted()
    body.take("}", "{")
    group_index = body.quoted()
    body.take("}", "eq")
    maker = body.quoted()
    body.take(")", ")", "{")
    body.take("(", "return", "(", "undef", ")", ")", ";", "}", "else", "{")
    body.take("(", "return")
    other = body.quoted()
    body.take(")", ";", "}", "}")
    if (group_property, group_index) != ("Groups", "0"):
        raise RecipeRefused("CHECK_PROC missing-format group selector is unsupported")
    return MissingFormat(True, equals, Selector("tag.Groups", group_index), maker, None, other)


def _terminal_call(body: _Body, tag: str, value: str, format_name: str) -> tuple[str, Selector]:
    body.take("(", "return")
    if body.at >= len(body.tokens) or _FQ.fullmatch(body.tokens[body.at]) is None:
        raise RecipeRefused("CHECK_PROC CheckValue callee is not fully qualified")
    callee = body.tokens[body.at]
    body.at += 1
    body.take("(", "$", value, ",", "$", format_name, ",")
    count = body.tag_property(tag)
    body.take(")", ")", ";", "}")
    if body.at != len(body.tokens):
        raise RecipeRefused("CHECK_PROC has an unsupported extra statement")
    return callee, count


def _compile_body(source: Any) -> tuple[tuple[Selector, ...], MissingFormat, str, Selector]:
    body = _Body(source)
    tag, value = _signature(body)
    format_name, selectors = _format_selectors(body, tag)
    missing = _parse_missing_format(body, tag, format_name)
    callee, count = _terminal_call(body, tag, value, format_name)
    return selectors, missing, callee, count


def _table_identity(module: str, name: str, table: Mapping[str, Any]) -> tuple[str, str, str]:
    full_name = f"Image::ExifTool::{module}::{name}"
    if table.get("module") != module or table.get("table") != name or table.get("full_name") != full_name:
        raise RecipeMalformed("native write table identity does not match its map key")
    return module, name, full_name


def _check_proc(table: Mapping[str, Any]) -> Mapping[str, Any]:
    outer = _mapping(table.get("effective_check_proc"), "effective_check_proc")
    if outer.get("present") is not True:
        raise RecipeRefused("CHECK_PROC is absent")
    return _mapping(outer.get("effective"), "effective_check_proc.effective")


def _recipe_identity(check_proc: CodeProvenance, selectors: tuple[Selector, ...], missing: MissingFormat,
                     count: Selector, check_value: CodeProvenance) -> str:
    payload = {
        "check_proc": asdict(check_proc),
        "format_selectors": [asdict(value) for value in selectors],
        "missing_format": asdict(missing),
        "count_operand": asdict(count),
        "check_value": asdict(check_value),
    }
    return hashlib.sha256(json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def compile_recipes(document: Mapping[str, Any]) -> tuple[list[CheckExifRecipe], RecipeReport, list[dict[str, str]]]:
    """Compile every structurally supported captured CHECK_PROC.

    Unsupported bodies are retained as source-table omissions. Malformed source
    facts raise, because treating a corrupted sidecar as a normal refusal would
    hide evidence loss.
    """
    document = _mapping(document, "document")
    helpers = _mapping(document.get("native_write_helpers"), "native_write_helpers")
    tables_by_module = _mapping(document.get("native_write_tables"), "native_write_tables")
    grouped: dict[str, CheckExifRecipe] = {}
    omissions: list[dict[str, str]] = []
    tables_seen = 0
    for module, tables in sorted(tables_by_module.items()):
        if not isinstance(module, str):
            raise RecipeMalformed("native write module key is not a string")
        for name, table in sorted(_mapping(tables, f"native_write_tables[{module!r}]").items()):
            if not isinstance(name, str):
                raise RecipeMalformed("native write table key is not a string")
            table = _mapping(table, f"native_write_tables[{module!r}][{name!r}]")
            tables_seen += 1
            identity = _table_identity(module, name, table)
            try:
                check_proc_fact = _check_proc(table)
                selectors, missing, callee, count = _compile_body(check_proc_fact.get("__deparse"))
                check_proc = _fact(check_proc_fact, "effective_check_proc.effective", require_binding=False)
                if callee != "Image::ExifTool::CheckValue":
                    raise RecipeRefused("CHECK_PROC CheckValue callee is unsupported")
                helper_fact = helpers.get("check_value")
                check_value = _fact(helper_fact, "native_write_helpers.check_value", require_binding=True)
                if check_value.requested_binding != callee:
                    raise RecipeRefused("CHECK_PROC CheckValue binding is stale or unavailable")
                recipe_id = _recipe_identity(check_proc, selectors, missing, count, check_value)
            except RecipeRefused as error:
                omissions.append({"module": module, "table": name, "full_name": identity[2], "reason": str(error)})
                continue
            existing = grouped.get(recipe_id)
            if existing is None:
                grouped[recipe_id] = CheckExifRecipe(recipe_id, check_proc, (identity,), selectors, missing, count, check_value)
            else:
                grouped[recipe_id] = CheckExifRecipe(
                    existing.recipe_sha256, existing.check_proc,
                    tuple(sorted((*existing.source_tables, identity))), existing.format_selectors,
                    existing.missing_format, existing.count_operand, existing.check_value,
                )
    report = RecipeReport(
        tables_seen=tables_seen,
        recipes_emitted=len(grouped),
        omitted_tables=len(omissions),
        omissions_by_reason={reason: sum(item["reason"] == reason for item in omissions)
                             for reason in sorted({item["reason"] for item in omissions})},
    )
    return [grouped[key] for key in sorted(grouped)], report, omissions


def render(document: Mapping[str, Any]) -> str:
    recipes, report, omissions = compile_recipes(document)
    payload = {
        "version": RECIPE_VERSION,
        "runtime_status": RUNTIME_STATUS,
        "recipes": [asdict(recipe) for recipe in recipes],
        "omitted_tables": omissions,
        "report": asdict(report),
    }
    return json.dumps(payload, indent=2, sort_keys=True) + "\n"


def main() -> None:
    import argparse
    parser = argparse.ArgumentParser(description="compile inactive source CheckExif recipes")
    parser.add_argument("tables", type=Path)
    parser.add_argument("-o", "--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.write_text(render(json.loads(args.tables.read_text(encoding="utf-8"))), encoding="utf-8")


if __name__ == "__main__":
    main()
