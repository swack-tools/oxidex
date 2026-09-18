"""Closed source admission for SetNewValue's ConvInv error-result caller.

This models only the caller's three-way treatment of ConvInv's returned error.
Tag routing, list handling, deletion, NEW_VALUE construction and every other
SetNewValue branch remain outside the inactive writer composition route.
"""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
import re
from pathlib import Path
from typing import Any

from checkexif_recipes import CodeDependency, CodeProvenance, RecipeRefused
import native_reader_facts

RUNTIME_STATUS = "inactive_source_operand_no_public_setnewvalue_admission"


@dataclass(frozen=True)
class SetNewValueConvInvRecipe:
    provenance: CodeProvenance
    undefined_error: str = "continue"
    defined_false_error: str = "skip_tag"
    truthy_error: str = "refuse"


_FULL_TEMPLATE_PATHS = (
    "setnewvalue_convinv_full_template.json",
    # Complete token profiles captured from the immutable selected 11.78/12.64
    # source pair, as `final_scalar_stage.py` does for WriteExif.  Each
    # describes a whole historical SetNewValue body, including branches that
    # differ from 13.59 in ways unrelated to ConvInv.  A version label never
    # admits a source: only exact token consumption of one of these does, and
    # `_CONVINV_OPERAND` below must still hold.
    "setnewvalue_convinv_full_template_11_78.json",
    "setnewvalue_convinv_full_template_12_64.json",
)

# The one region this recipe models, compared on B::Deparse with every
# whitespace character removed (the same convention `final_scalar_stage.py`
# uses for its operands).  `undefined_error`, `defined_false_error` and
# `truthy_error` are read off exactly these statements: `defined($e)` false
# continues, `$e` false takes `goto WriteAlso`, and a true `$e` becomes `$err`.
#
# The whole-body templates prove no executable token was ignored; this proves
# the modeled control flow is actually present.  Without it, capturing a later
# release's template would silently extend the three-way recipe to a body whose
# ConvInv handling had changed.  It is checked on raw source rather than on
# `body_tokens`, because that tokenizer reads a Perl division `/` as the start
# of a regex literal and can fold thousands of characters -- this region
# included, on 12.64 -- into one token.
_CONVINV_OPERAND = re.sub(r"\s+", "", (
    "($val, $e) = $self->ConvInv($val, $tagInfo, $tag, $wgrp1, $self->{'ConvType'}, $wantGroup)); "
    "if (defined($e)) { ($e or (goto WriteAlso)); ($err = $e); }"))


def _load_template(name: str) -> list[str]:
    try:
        value = json.loads(Path(__file__).with_name(name).read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise RecipeRefused(f"SetNewValue full-source template {name} is unavailable") from error
    if not isinstance(value, list) or any(not isinstance(token, str) for token in value):
        raise RecipeRefused(f"SetNewValue full-source template {name} is malformed")
    return value


def _templates() -> tuple[list[str], ...]:
    return tuple(_load_template(name) for name in _FULL_TEMPLATE_PATHS)


def _template() -> list[str]:
    """The 13.59 profile alone, for callers and tests that need the pin's."""
    return _load_template(_FULL_TEMPLATE_PATHS[0])


def _source_provenance(fact: Any, expected_name: str, context: str) -> CodeProvenance:
    if not isinstance(fact, dict):
        raise RecipeRefused(f"{context} is unavailable")
    try:
        actual_name, source_file, source_sha256, _ = native_reader_facts.source_fact(
            fact, expected_name)
    except native_reader_facts.ReaderRefused as error:
        raise RecipeRefused(f"{context} is unresolved or rebound") from error
    body = fact.get("__deparse")
    if not isinstance(body, str):
        raise RecipeRefused(f"{context} has no deparsed body")
    return CodeProvenance(
        fact.get("requested_binding"), actual_name, source_file, source_sha256,
        hashlib.sha256(body.encode("utf-8")).hexdigest(), (),
    )


def _caller_provenance(fact: Any) -> CodeProvenance:
    """Authenticate this caller and the direct helper whose result it reads.

    The sidecar deliberately preserves SetNewValue's complete dependency graph.
    This operand does not execute that broad graph: it composes only with the
    separately compiled ConvInv recipe. Requiring the direct loaded ConvInv
    binding prevents a caller source match from being disconnected from that
    operand, without rejecting because an unrelated transitive helper remains
    unimplemented.
    """
    caller = _source_provenance(
        fact, "Image::ExifTool::SetNewValue", "native_write_helpers.set_new_value")
    if caller.requested_binding != "Image::ExifTool::SetNewValue":
        raise RecipeRefused("SetNewValue requested binding is stale")
    dependencies = fact.get("dependencies") if isinstance(fact, dict) else None
    if not isinstance(dependencies, dict):
        raise RecipeRefused("SetNewValue lacks direct ConvInv source fact")
    conv_inv = _source_provenance(
        dependencies.get("Image::ExifTool::ConvInv"), "Image::ExifTool::ConvInv",
        "SetNewValue direct ConvInv")
    return CodeProvenance(
        caller.requested_binding, caller.actual_name, caller.source_file,
        caller.source_sha256, caller.body_sha256,
        (CodeDependency("Image::ExifTool::ConvInv", conv_inv),),
    )


def compile_setnewvalue_convinv(fact: Any) -> SetNewValueConvInvRecipe:
    """Accept only the complete canonical callable and its authenticated fact."""
    provenance = _caller_provenance(fact)
    try:
        tokens = native_reader_facts.body_tokens(fact.get("__deparse"))
    except native_reader_facts.ReaderRefused as error:
        raise RecipeRefused("SetNewValue body is unavailable") from error
    # Compare the *entire* deparsed callable.  This is deliberately stricter
    # than recognizing the local ConvInv substring: added statements before,
    # between, or after error handling must cause a named refusal.
    if not any(tokens == template for template in _templates()):
        raise RecipeRefused("SetNewValue caller control flow is unsupported")
    # Exactly once: a second copy would be a second, unmodeled ConvInv caller.
    if re.sub(r"\s+", "", fact["__deparse"]).count(_CONVINV_OPERAND) != 1:
        raise RecipeRefused("SetNewValue ConvInv error handling is not the modeled operand")
    return SetNewValueConvInvRecipe(provenance)
