"""Read back word-layout operands and authenticate their independent source facts.

No compiler import. This module checks the artifact schema and its live native
identity/bindings. Native execution and row completeness are separate checks;
a matching source digest alone is never reported as behavioral equivalence.
"""

from dataclasses import dataclass
import hashlib
import json
from pathlib import PurePosixPath
import re

import native_reader_facts
import verify_native_reader


class WordVerificationError(ValueError):
    """The artifact cannot be independently interpreted or authenticated."""


_NUMBERS = frozenset((
    "pair_start", "pair_stride", "key_shift", "value_mask", "header_adjustment",
    "index_divisor", "index_bias", "value_count", "value_size",
))
_BOOLEANS = frozenset(("exact_length_first", "missing_model_as_empty", "short_u16_as_zero"))
_STRINGS = frozenset((
    "invalid_warning", "verbose_directory", "source_file", "source_sha256",
    "source_body_sha256", "reader_contract_sha256",
))
_FIELDS = _NUMBERS | _BOOLEANS | _STRINGS | {"model_condition", "value_format"}
_STRING = re.compile(r'"((?:[^"\\]|\\.)*)"', re.S)
_HASH = re.compile(r"[0-9a-f]{64}")


def _members(text, constructor):
    """Split a Rust struct without treating commas inside literals as fields."""
    match = re.fullmatch(re.escape(constructor) + r"\s*\{(.*)\}\s*", text, re.S)
    if match is None:
        raise WordVerificationError(f"unrecognized {constructor} literal")
    body = match.group(1)
    parts, stack, quoted, escaped, start = [], [], False, False, 0
    closes = {")": "(", "]": "[", "}": "{"}
    for pos, char in enumerate(body):
        if quoted:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                quoted = False
        elif char == '"':
            quoted = True
        elif char in "([{":
            stack.append(char)
        elif char in closes:
            if not stack or stack.pop() != closes[char]:
                raise WordVerificationError("unbalanced word descriptor")
        elif char == "," and not stack:
            parts.append(body[start:pos].strip())
            start = pos + 1
    if stack or quoted:
        raise WordVerificationError("unterminated word descriptor")
    tail = body[start:].strip()
    if tail:
        parts.append(tail)
    result = {}
    for part in parts:
        field = re.fullmatch(r"([a-z][a-z0-9_]*)\s*:\s*(.+)", part, re.S)
        if field is None or field.group(1) in result:
            raise WordVerificationError("malformed or duplicate word descriptor field")
        result[field.group(1)] = field.group(2).strip()
    return result


def _string(value, unescape):
    match = _STRING.fullmatch(value)
    if match is None:
        raise WordVerificationError("word descriptor requires a Rust string")
    return unescape(match.group(1))


def _boolean(value):
    if value not in {"true", "false"}:
        raise WordVerificationError("word descriptor requires a boolean")
    return value == "true"


@dataclass(frozen=True)
class WordDescriptor:
    """Parsed artifact fields. Their digest binds execution evidence to operands."""

    fields: dict

    def fingerprint(self):
        raw = json.dumps(self.fields, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
        return hashlib.sha256(raw.encode("utf-8")).hexdigest()


def parse_rust(text, unescape):
    """Return None for CIFF, or a complete typed word descriptor; reject drift."""
    text = text.strip()
    if text == "KeyedLayout::Ciff10":
        return None
    match = re.fullmatch(r"KeyedLayout::LengthPrefixedU16Pairs\s*\((.*)\)\s*", text, re.S)
    if match is None:
        raise WordVerificationError("unknown keyed layout")
    values = _members(match.group(1).rstrip().removesuffix(",").rstrip(), "WordDirectory")
    if set(values) != _FIELDS:
        raise WordVerificationError(f"word descriptor field drift: {sorted(set(values) ^ _FIELDS)}")
    fields = {}
    for key, value in values.items():
        if key in _NUMBERS:
            if re.fullmatch(r"0|[1-9][0-9]*", value) is None or int(value) > 0xffffffff:
                raise WordVerificationError(f"invalid word integer {key}")
            fields[key] = int(value)
        elif key in _BOOLEANS:
            fields[key] = _boolean(value)
        elif key in _STRINGS:
            fields[key] = _string(value, unescape)
        elif key == "value_format":
            if value != "Fmt::Int8u":
                raise WordVerificationError("unverified native handler format")
            fields[key] = "int8u"
        elif key == "model_condition":
            cond = _members(value, "Cond::MemberRegex")
            if set(cond) != {"member", "pattern", "ignore_case", "negate"}:
                raise WordVerificationError("word condition field drift")
            fields[key] = {name: _string(raw, unescape) if name in {"member", "pattern"}
                           else _boolean(raw) for name, raw in cond.items()}
    _validate_domain(fields)
    return WordDescriptor(fields)


def _validate_domain(fields):
    for name in ("pair_start", "pair_stride", "header_adjustment", "index_divisor"):
        if fields[name] == 0:
            raise WordVerificationError(f"zero word operand {name}")
    if fields["key_shift"] >= 16 or fields["value_mask"] > 0xffff:
        raise WordVerificationError("word operand exceeds unsigned-16 input")
    if any(fields[name] is not True for name in _BOOLEANS):
        raise WordVerificationError("unverified native short-read or predicate ordering")
    if fields["value_count"] != 1 or fields["value_size"] != 1:
        raise WordVerificationError("unverified native handler scalar shape")
    if (fields["pair_start"] % fields["index_divisor"]
            or fields["pair_stride"] % fields["index_divisor"]
            or fields["pair_start"] // fields["index_divisor"] < fields["index_bias"]):
        raise WordVerificationError("native index cannot use the unsigned runtime trace")
    if fields["model_condition"]["member"] != "Model" or fields["model_condition"]["negate"]:
        raise WordVerificationError("unverified native exception predicate")
    for name in ("source_sha256", "source_body_sha256", "reader_contract_sha256"):
        if _HASH.fullmatch(fields[name]) is None:
            raise WordVerificationError(f"missing native digest {name}")
    path = fields["source_file"]
    if (not path or PurePosixPath(path).is_absolute() or "\\" in path
            or any(part in {"", ".", ".."} for part in path.split("/"))):
        raise WordVerificationError("native source path is not library-relative")


def source_mismatch(descriptor, processor, reader_contracts):
    """Authenticate provenance and final native reader binding, not execution."""
    if not isinstance(processor, dict) or processor.get("resolved") is not True:
        return "native word processor is unresolved"
    name = processor.get("__name")
    if (processor.get("__perl") != "CODE" or not isinstance(name, str)
            or re.fullmatch(r"(?:[A-Za-z_]\w*::)+[A-Za-z_]\w*", name) is None):
        return "native word processor identity is missing"
    body = processor.get("__deparse")
    if not isinstance(body, str):
        return "native word processor body is missing"
    try:
        body_sha = hashlib.sha256(body.encode("utf-8", "strict")).hexdigest()
    except UnicodeError:
        return "native word processor body is not UTF-8"
    fields = descriptor.fields
    if (fields["source_file"], fields["source_sha256"], fields["source_body_sha256"]) != (
            processor.get("source_file"), processor.get("source_sha256"), body_sha):
        return "word descriptor source identity differs from live native processor"
    # A bare call binds in the package written in the function body. Neither
    # the table's module nor the displayed CV name is sufficient evidence.
    package = re.match(r"\s*\([^{}]*\)\s*\{\s*package\s+((?:[A-Za-z_]\w*::)*[A-Za-z_]\w*)\s*;", body)
    if package is None:
        return "native word processor package is unavailable"
    snapshot = reader_contracts.get("unsigned16") if isinstance(reader_contracts, dict) else None
    if problem := verify_native_reader.mismatch(fields["reader_contract_sha256"], snapshot):
        return problem
    bindings = processor.get("dependencies")
    binding = bindings.get(package.group(1) + "::Get16u") if isinstance(bindings, dict) else None
    try:
        actual = native_reader_facts.source_fact(binding, "Image::ExifTool::Get16u")
        expected = native_reader_facts.source_fact(snapshot["loaded_functions"]["get16u"], "Image::ExifTool::Get16u")
    except (native_reader_facts.ReaderRefused, KeyError, TypeError):
        return "native word processor reader binding is unresolved or rebound"
    if actual != expected:
        return "native word processor reader binding differs from loaded unsigned reader"
    return None
