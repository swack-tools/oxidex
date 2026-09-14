"""Validate deterministic observations of the interpreter UTF-8 primitive.

This is a standard-library boundary, not a translation of Encode's XS code.
The producer captures a pristine child and its own final loaded state. Source
consumers must also join the helper's actual referenced bindings to this state.
Raw provider/interpreter file hashes belong in run diagnostics, not artifacts.
"""
from decimal import Decimal, InvalidOperation
import hashlib
import json
import re
from typing import Any

from checkexif_recipes import RecipeRefused

SUPPORTED = {
    "forced_utf8_ascii": ("61", 1, True, "61"), "forced_utf8_latin1": ("c3a9", 1, True, "c3a9"),
    "u007f": ("7f", 1, True, "7f"), "u0080": ("c280", 1, True, "c280"),
    "u07ff": ("dfbf", 1, True, "dfbf"), "u0800": ("e0a080", 1, True, "e0a080"),
    "uffff": ("efbfbf", 1, True, "efbfbf"), "u10000": ("f0908080", 1, True, "f0908080"),
    "u10ffff": ("f48fbfbf", 1, True, "f48fbfbf"), "unicode_nul": ("c3a900", 2, True, "c3a900"),
    "bytes_utf8": ("c3a9", 2, False, "c383c2a9"), "bytes_nul": ("610062", 3, False, "610062"),
}
UNSUPPORTED = {"undefined", "scalar_reference", "surrogate", "out_of_range"}


def scalar_result(hex_value: str) -> dict:
    return {"domain": "scalar", "supported": True, "utf8_flag": False,
            "bytes_hex": hex_value, "char_length": len(bytes.fromhex(hex_value))}


def _canonical(value: Any) -> str:
    return json.dumps(value, sort_keys=True, allow_nan=False)


def _failure(message: str):
    raise RecipeRefused("UTF8 primitive: " + message)


def perl_version(value: Any) -> int:
    if isinstance(value, bool) or not isinstance(value, (str, int, float)):
        _failure("invalid interpreter version")
    try:
        scaled = Decimal(str(value)) * 1_000_000
        if not scaled.is_finite() or scaled != scaled.to_integral_value() or not 0 < scaled < 2**64:
            _failure("invalid interpreter version")
        return int(scaled)
    except InvalidOperation:
        _failure("invalid interpreter version")


def _binding(fact: Any, requested: str) -> None:
    if not isinstance(fact, dict) or fact.get("resolved") is not True:
        _failure("unresolved binding " + requested)
    if fact.get("requested") != requested:
        _failure("requested binding mismatch " + requested)
    name = fact.get("actual_name")
    if not isinstance(name, str) or re.fullmatch(r"(?:[A-Za-z_]\w*::)+[A-Za-z_]\w*", name) is None:
        _failure("invalid actual binding " + requested)
    if fact.get("implementation") not in {"perl", "xsub"}:
        _failure("missing implementation identity " + requested)
    body = fact.get("body_sha256")
    if not isinstance(body, str) or re.fullmatch(r"[0-9a-f]{64}", body) is None:
        _failure("missing callable fingerprint " + requested)
    if fact.get("prototype") is not None and not isinstance(fact["prototype"], str):
        _failure("invalid prototype " + requested)
    if set(fact) != {"requested", "resolved", "actual_name", "implementation", "prototype", "body_sha256"}:
        _failure("nonportable or unknown binding fields " + requested)


def validate_snapshot(doc: Any) -> None:
    if not isinstance(doc, dict) or doc.get("kind") != "utf8_primitive_snapshot_v1" or doc.get("ok") is not True:
        _failure("snapshot unavailable")
    if set(doc) != {"kind", "ok", "perl_version", "bindings", "utf8_registry", "vectors", "unsupported_domains"}:
        _failure("nonportable or unknown snapshot fields")
    perl_version(doc["perl_version"])
    bindings = doc.get("bindings")
    if not isinstance(bindings, dict) or set(bindings) != {"encode", "is_utf8"}:
        _failure("missing primitive bindings")
    for key in ("encode", "is_utf8"):
        _binding(bindings[key], "Encode::" + key)
    registry = doc.get("utf8_registry")
    if (not isinstance(registry, dict) or set(registry) != {"class", "name", "encode_method"}
            or registry.get("class") != "Encode::utf8" or registry.get("name") != "utf8"):
        _failure("unsupported encoding registry")
    _binding(registry.get("encode_method"), "utf8 registry encode")
    if doc.get("unsupported_domains") != ["out_of_range", "scalar_reference", "surrogate"]:
        _failure("missing unsupported domains")
    vectors = doc.get("vectors")
    if not isinstance(vectors, list) or not all(isinstance(row, dict) for row in vectors):
        _failure("malformed vectors")
    names = [row.get("name") for row in vectors]
    if any(not isinstance(name, str) for name in names) or len(set(names)) != len(names):
        _failure("duplicate or malformed vector names")
    if set(names) != set(SUPPORTED) | {"undefined"}:
        _failure("incomplete native vector population")
    for row in vectors:
        name = row["name"]
        if name == "undefined":
            input_value = {"domain": "undefined", "supported": False}
            flag, encoded = scalar_result(""), input_value
        else:
            input_hex, length, utf8, encoded_hex = SUPPORTED[name]
            input_value = {"domain": "scalar", "supported": True, "utf8_flag": utf8,
                           "bytes_hex": input_hex, "char_length": length}
            flag, encoded = scalar_result("31" if utf8 else ""), scalar_result(encoded_hex)
        expected = {"name": name, "creation_error": None, "input": input_value,
                    "is_utf8": {"result": flag, "error": None, "warning": None},
                    "encode_utf8": {"result": encoded, "error": None, "warning": None}}
        if _canonical(row) != _canonical(expected):
            _failure("native semantics disagree: " + name)


def validate_join(contract: Any) -> dict:
    if not isinstance(contract, dict) or contract.get("kind") != "utf8_primitive_join_v1":
        _failure("missing pristine/final join")
    pristine, final = contract.get("pristine"), contract.get("final")
    validate_snapshot(pristine)
    validate_snapshot(final)
    if _canonical(pristine) != _canonical(final):
        _failure("actual producer differs from pristine")
    return final


def join_sanitize_bindings(dependencies: dict, final: dict) -> None:
    for key in ("encode", "is_utf8"):
        name = "Encode::" + key
        captured, observed = dependencies.get(name), final["bindings"][key]
        if not isinstance(captured, dict) or captured.get("__name") != observed["actual_name"]:
            _failure("Sanitize binding differs: " + name)
        body = captured.get("__deparse")
        if not isinstance(body, str) or hashlib.sha256(body.encode()).hexdigest() != observed["body_sha256"]:
            _failure("Sanitize callable fingerprint unavailable or different: " + name)
