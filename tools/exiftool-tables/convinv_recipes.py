"""Inactive closed ConvInv scalar fallthrough recipe."""
from dataclasses import dataclass
from pathlib import Path
import json
import re
from typing import Any
from checkexif_recipes import RecipeRefused, _fact
import native_reader_facts

@dataclass(frozen=True)
class ConvInvRecipe:
    provenance: Any
    default_type: str = "PrintConv"
    error_separator: str = " for "

def compile_convinv(fact: Any) -> ConvInvRecipe:
    # Active conversion calls have a large native dependency closure. They are
    # deliberately outside this route, so validate the ConvInv CV itself while
    # retaining (but never trusting) the opaque closure.  `_fact` recursively
    # validates executable dependencies for a composed call; doing that here
    # would make an unexecuted ConvertDateTime dependency an implicit input to
    # this scalar fallthrough.
    if not isinstance(fact, dict):
        raise RecipeRefused("ConvInv fact is not an object")
    shallow = dict(fact)
    shallow["dependencies"] = {}
    p = _fact(shallow, "native_write_helpers.conv_inv", require_binding=True)
    if p.requested_binding != "Image::ExifTool::ConvInv":
        raise RecipeRefused("ConvInv binding is stale")
    body = fact.get("__deparse")
    if not isinstance(body, str):
        raise RecipeRefused("ConvInv body is outside scalar closed grammar")
    try:
        tokens = native_reader_facts.body_tokens(body)
    except native_reader_facts.ReaderRefused as error:
        raise RecipeRefused("ConvInv body is outside scalar closed grammar") from error
    template=json.loads((Path(__file__).with_name("convinv_full_template.json")).read_text())
    if len(tokens)!=len(template): raise RecipeRefused("ConvInv body has unsupported statements")
    try: index=template.index("<ERROR_LITERAL>"); default_index=template.index("<DEFAULT_TYPE>")
    except ValueError as error: raise RecipeRefused("ConvInv template is malformed") from error
    if any(actual!=wanted for n,(actual,wanted) in enumerate(zip(tokens,template)) if n not in {index,default_index}):
        raise RecipeRefused("ConvInv source operand/order is unsupported")
    default=tokens[default_index]
    if default not in {"'PrintConv'"}: raise RecipeRefused("ConvInv default conversion type is unsupported")
    literal=tokens[index]
    match=re.fullmatch(r'"\$err2([^$@\\]*)\$\{wgrp1\}:\$tag"',literal)
    if match is None: raise RecipeRefused("ConvInv error literal is unsupported")
    return ConvInvRecipe(p, default_type=default[1:-1], error_separator=match.group(1))

def scalar_fallthrough(recipe: ConvInvRecipe, value: Any, row: dict[str, Any], conv_type: str | None = None):
    if not isinstance(row, dict) or (conv_type is not None and conv_type != recipe.default_type):
        raise RecipeRefused("ConvInv input/type is unsupported")
    # Defined false conversions still alter native control flow: presence refuses.
    for key in ("PrintConv", "PrintConvInv", "ValueConv", "ValueConvInv"):
        if key in row: raise RecipeRefused("ConvInv active conversion: " + key)
    for key in ("List", "RawJoin", "WriteCheck", "RawConvInv"):
        if row.get(key): raise RecipeRefused("ConvInv scalar gate: " + key)
    table=row.get("Table")
    if table is not None and not isinstance(table, dict): raise RecipeRefused("ConvInv table malformed")
    if isinstance(table,dict) and table.get("CHECK_PROC"): raise RecipeRefused("ConvInv CHECK_PROC")
    return value, None
