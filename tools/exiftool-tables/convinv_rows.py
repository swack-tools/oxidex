"""Project source-authenticated native ConvInv rows into static Rust operands.

This is deliberately a projection, not a tag list. A row is admitted only when
its table's captured ``CHECK_PROC`` has already passed the closed
``checkexif_recipes`` compiler. The emitted slices contain precisely the
source properties that that authenticated recipe selects.
"""
from __future__ import annotations

from collections import Counter
from dataclasses import asdict, dataclass
import re
from typing import Any, Mapping

from checkexif_recipes import RecipeMalformed, RecipeRefused, Selector, compile_recipes


_CONVERSIONS = ("PrintConv", "PrintConvInv", "ValueConv", "ValueConvInv")
_GATES = (("List", "properties"), ("RawJoin", "properties"),
          ("WriteCheck", "write_controls"), ("RawConvInv", "properties"))
_I64 = re.compile(r"^-?(?:0|[1-9][0-9]*)$")


@dataclass(frozen=True)
class Property:
    name: str
    kind: str
    value: str | int | None


@dataclass(frozen=True)
class Row:
    module: str
    table: str
    full_name: str
    raw_id: str
    name: str
    write_group: str
    conversion: tuple[str, str, str, str]
    gates: tuple[bool, bool, bool, bool]
    tag_properties: tuple[Property, ...]
    table_properties: tuple[Property, ...]
    tag_groups: tuple[Property, ...]


def _mapping(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise RecipeMalformed(f"{context} is not an object")
    return value


def _fact(value: Any, context: str) -> tuple[bool, Any]:
    value = _mapping(value, context)
    present = value.get("present")
    if type(present) is not bool:
        raise RecipeMalformed(f"{context}.present is not boolean")
    if present and "value" not in value:
        raise RecipeMalformed(f"{context} is present without a value")
    if not present and set(value) != {"present"}:
        raise RecipeMalformed(f"{context} is absent but carries a value")
    return present, value.get("value")


def _perl_truth(value: Any, context: str) -> bool:
    """Project Perl truth without treating a false defined scalar as absent."""
    if value is None:
        return False
    if isinstance(value, str):
        return value not in ("", "0")
    # JSON objects and arrays here are Perl references. Empty references remain
    # true in Perl; preserving only their boolean gate is sufficient.
    if isinstance(value, (Mapping, list)):
        return True
    raise RecipeRefused(f"{context} has an unsupported boolean operand")


def _conversion(value: Any, context: str) -> str:
    present, raw = _fact(value, context)
    if not present:
        return "Absent"
    # ``defined $tagInfo{...}`` distinguishes a present undef from both false
    # defined spellings ("" and "0") and from every reference kind.
    return "Undefined" if raw is None else "Defined"


def _text(name: str, value: Any, context: str) -> Property:
    present, raw = _fact(value, context)
    if not present or raw is None:
        return Property(name, "Undefined", None)
    if not isinstance(raw, str):
        raise RecipeRefused(f"{context} is not a scalar text operand")
    return Property(name, "Text", raw)


def _count(name: str, value: Any, context: str) -> Property:
    present, raw = _fact(value, context)
    if not present or raw is None:
        return Property(name, "Undefined", None)
    if not isinstance(raw, str) or _I64.fullmatch(raw) is None:
        raise RecipeRefused(f"{context} is not a signed decimal Count operand")
    number = int(raw)
    if not -(1 << 63) <= number < (1 << 63):
        raise RecipeRefused(f"{context} is outside the Rust i64 Count operand")
    return Property(name, "Integer", number)


def _properties(properties: Mapping[str, Any], selectors: tuple[Selector, ...], *, source: str,
                context: str, count: Selector) -> tuple[Property, ...]:
    names = []
    for selector in selectors:
        if selector.source == source and selector.property not in names:
            names.append(selector.property)
    emitted = []
    for name in names:
        raw = properties.get(name, {"present": False})
        prop_context = f"{context}.{name}"
        is_count = source == count.source and name == count.property
        emitted.append(_count(name, raw, prop_context) if is_count else _text(name, raw, prop_context))
    return tuple(emitted)


def _groups(properties: Mapping[str, Any], selectors: tuple[Selector, ...], context: str) -> tuple[Property, ...]:
    names = []
    for selector in selectors:
        if selector.source == "tag.Groups" and selector.property not in names:
            names.append(selector.property)
    if not names:
        return ()
    present, raw = _fact(properties.get("Groups", {"present": False}), f"{context}.Groups")
    if not present or raw is None:
        return tuple(Property(name, "Undefined", None) for name in names)
    groups = _mapping(raw, f"{context}.Groups.value")
    values = []
    for name in names:
        value = groups.get(name)
        if value is None:
            values.append(Property(name, "Undefined", None))
        elif isinstance(value, str):
            values.append(Property(name, "Text", value))
        else:
            raise RecipeRefused(f"{context}.Groups.{name} is not scalar text")
    return tuple(values)


def _identity(module: Any, table: Any, fact: Mapping[str, Any], context: str) -> tuple[str, str, str]:
    if not isinstance(module, str) or not isinstance(table, str):
        raise RecipeMalformed(f"{context} map identity is not text")
    full_name = f"Image::ExifTool::{module}::{table}"
    if fact.get("module") != module or fact.get("table") != table or fact.get("full_name") != full_name:
        raise RecipeMalformed(f"{context} identity does not match its map key")
    return module, table, full_name


def _same_fact(left: Any, right: Any, context: str) -> None:
    """Require duplicated raw/control facts to agree before projection."""
    _fact(left, context + ".left")
    _fact(right, context + ".right")
    if left != right:
        raise RecipeMalformed(context + " source/control facts disagree")


def _resolver(value: Any, context: str) -> None:
    value = _mapping(value, context)
    if value.get("resolved") is not True or value.get("actual_name") != "Image::ExifTool::GetTagInfo":
        raise RecipeRefused("effective row resolver is not resolved Image::ExifTool::GetTagInfo")
    source = value.get("source_file")
    digest = value.get("source_sha256")
    if not isinstance(source, str) or not source.startswith("Image/") or not re.fullmatch(r"[0-9a-f]{64}", str(digest)):
        raise RecipeMalformed(context + " has no selected-library source identity")


def _containing_table_binding(value: Any, context: str) -> None:
    value = _mapping(value, context)
    if value != {"kind": "containing_table", "ref_identical_to_containing": True}:
        raise RecipeRefused("effective Table binding is not the authenticated containing table")


def compile_rows(document: Mapping[str, Any]) -> tuple[tuple[Row, ...], dict[str, Any]]:
    """Compile every representable row and record each source-side omission."""
    document = _mapping(document, "document")
    recipes, _report, recipe_omissions = compile_recipes(document)
    recipe_by_table = {identity: recipe for recipe in recipes for identity in recipe.source_tables}
    tables = _mapping(document.get("native_write_tables"), "native_write_tables")
    rows: list[Row] = []
    omissions: list[dict[str, str]] = [
        {"module": item["module"], "table": item["table"], "raw_id": "*", "reason": "CHECK_PROC: " + item["reason"]}
        for item in recipe_omissions
    ]
    for module, table_map in sorted(tables.items()):
        table_map = _mapping(table_map, f"native_write_tables[{module!r}]")
        for table_name, table_fact in sorted(table_map.items()):
            table_fact = _mapping(table_fact, f"native_write_tables[{module!r}][{table_name!r}]")
            identity = _identity(module, table_name, table_fact, "native_write_tables")
            recipe = recipe_by_table.get(identity)
            source_rows = _mapping(table_fact.get("rows", {}), f"{identity[2]}.rows")
            resolver = None
            if recipe is not None:
                _resolver(table_fact.get("effective_row_resolver"), f"{identity[2]}.effective_row_resolver")
                context = _mapping(table_fact.get("effective_row_context"),
                                   f"{identity[2]}.effective_row_context")
                if context != {"is_writing": True, "selection": "native_get_tag_info_write_context"}:
                    raise RecipeRefused("effective row selection is not authenticated write context")
            for raw_id, raw_row in sorted(source_rows.items()):
                # Empty strings are valid Perl hash keys (for example
                # QuickTime::eeBox's fallback map). Preserve that identity;
                # later source-rule admission decides whether it is a tag.
                if not isinstance(raw_id, str):
                    raise RecipeMalformed(f"{identity[2]}.rows has invalid raw id")
                base = {"module": identity[0], "table": identity[1], "raw_id": raw_id}
                if recipe is None:
                    omissions.append({**base, "reason": "table CHECK_PROC has no authenticated Rust recipe"})
                    continue
                try:
                    row = _mapping(raw_row, f"{identity[2]}.rows[{raw_id!r}]")
                    if row.get("entry_kind") != "HASH":
                        raise RecipeRefused("row is not a scalar tag-property hash")
                    if row.get("effective_resolution") != "native_get_tag_info":
                        raise RecipeRefused("row effective-property resolution is unrepresented")
                    _containing_table_binding(row.get("effective_table_binding"),
                                              "row.effective_table_binding")
                    unknown = _mapping(row.get("unknown_properties"), "row.unknown_properties")
                    if unknown:
                        raise RecipeRefused("row has unsupported source properties: " + ", ".join(sorted(unknown)))
                    properties = _mapping(row.get("properties"), "row.properties")
                    controls = _mapping(row.get("write_controls"), "row.write_controls")
                    effective = _mapping(row.get("effective_properties"), "row.effective_properties")
                    condition_present, _ = _fact(properties.get("Condition", {"present": False}),
                                                 "row.Condition")
                    if condition_present:
                        raise RecipeRefused("row Condition requires an unrepresented native selection context")
                    for key in ("Writable", "WriteGroup", "PrintConvInv", "ValueConvInv", "RawConvInv"):
                        _same_fact(properties.get(key, {"present": False}), controls.get(key, {"present": False}),
                                   "row." + key)
                    table_properties = _mapping(table_fact.get("table_properties"), "table.table_properties")
                    table_controls = _mapping(table_fact.get("write_controls"), "table.write_controls")
                    _same_fact(table_properties.get("WRITABLE", {"present": False}),
                               table_controls.get("WRITABLE", {"present": False}), "table.WRITABLE")
                    name = _text("Name", effective.get("Name", {"present": False}), "row.effective_properties.Name")
                    write_group = _text("WriteGroup", effective.get("WriteGroup", {"present": False}), "row.effective_properties.WriteGroup")
                    if name.kind != "Text" or write_group.kind != "Text":
                        raise RecipeRefused("row has no scalar Name or WriteGroup identity")
                    conversions = tuple(_conversion(effective.get(key, {"present": False}), f"row.effective_properties.{key}")
                                        for key in _CONVERSIONS)
                    gates = []
                    for key, owner in _GATES:
                        present, raw = _fact(effective.get(key, {"present": False}), f"row.effective_properties.{key}")
                        gates.append(present and _perl_truth(raw, f"row.effective_properties.{key}"))
                    selectors = (*recipe.format_selectors, recipe.missing_format.group_source, recipe.count_operand)
                    tag = _properties(effective, selectors, source="tag", context="row.effective_properties", count=recipe.count_operand)
                    table_props = _properties(table_properties,
                                              selectors, source="table", context="table.table_properties",
                                              count=recipe.count_operand)
                    groups = _groups(effective, selectors, "row.effective_properties")
                    rows.append(Row(identity[0], identity[1], identity[2], raw_id, str(name.value),
                                    str(write_group.value), conversions, tuple(gates), tag, table_props, groups))
                except RecipeRefused as error:
                    omissions.append({**base, "reason": str(error)})
    report = {
        "runtime_status": "inactive; static source rows are not connected to public writing",
        "rows_emitted": len(rows),
        "rows_omitted": len(omissions),
        "omissions_by_reason": dict(sorted(Counter(item["reason"] for item in omissions).items())),
        "omitted_rows": omissions,
        "rows": [asdict(row) for row in rows],
    }
    return tuple(rows), report
