"""Closed source admission for SetNewValue's ConvInv error-result caller.

This models only the caller's three-way treatment of ConvInv's returned error.
Tag routing, list handling, deletion, NEW_VALUE construction and every other
SetNewValue branch remain outside the inactive writer composition route.
"""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
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


def _template() -> list[str]:
    try:
        value = json.loads(Path(__file__).with_name("setnewvalue_convinv_full_template.json").read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise RecipeRefused("SetNewValue full-source template is unavailable") from error
    if not isinstance(value, list) or any(not isinstance(token, str) for token in value):
        raise RecipeRefused("SetNewValue full-source template is malformed")
    return value


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
    if tokens != _template():
        raise RecipeRefused("SetNewValue caller control flow is unsupported")
    return SetNewValueConvInvRecipe(provenance)
