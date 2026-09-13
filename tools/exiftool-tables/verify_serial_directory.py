#!/usr/bin/env python3
"""Independently audit staged ``ProcessSerialData`` Rust facts.

This reads the native dump and emitted Rust directly.  It deliberately does
not import ``serial_directory`` or its Perl recognizer: source selection comes
from the captured PROCESS_PROC identity, while source/table/row properties are
interpreted here before they are compared with the artifact.  Native execution
of the processor remains a separate replay gate.
"""

from __future__ import annotations

import argparse
from collections import Counter
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import sys
from typing import Any

import serial_directory_facts as facts


PROCESSOR_SUFFIX = "::ProcessSerialData"
RUNTIME_FORMATS = frozenset(("int8u", "int16u", "int32u", "string", "undef", "binary"))
ROW_MODELED = frozenset(("Name", "Format", "Condition", "PrintConv", "RawConv", "ValueConv", "SubDirectory", "Groups", "Unknown", "Binary", "List"))
ROW_DOCUMENTARY = frozenset(("Notes", "_shorthand"))
TABLE_MODELED = frozenset(("PROCESS_PROC", "FORMAT", "GROUPS", "VARS"))
TABLE_DOCUMENTARY = frozenset(("NOTES",))


class SerialVerificationError(ValueError):
    """The native dump or Rust artifact cannot be independently audited."""


@dataclass(frozen=True, order=True)
class RowKey:
    module: str
    table: str
    index: int
    variant: bool
    alternative: int


@dataclass(frozen=True)
class NativeTable:
    module: str
    table: str
    processor: tuple[str, str, str, str]
    groups: tuple[str, str, str]
    default_format: str
    rows: dict[RowKey, dict[str, Any]]
    table_reasons: tuple[str, ...]


@dataclass(frozen=True)
class ArtifactTable:
    module: str
    table: str
    groups: tuple[str, str, str]
    default_format: str
    processor: tuple[str, str, str, str]
    gate: tuple[tuple[str, int], ...]
    rows: dict[RowKey, dict[str, Any]]


@dataclass(frozen=True)
class Artifact:
    tables: dict[tuple[str, str], ArtifactTable]
    omissions: dict[RowKey, tuple[str | None, tuple[str, ...]]]
    omitted_tables: dict[tuple[str, str], tuple[str, ...]]


@dataclass(frozen=True)
class Audit:
    expected_tables: int
    expected_alternatives: int
    emitted_tables: int
    emitted_alternatives: int
    omitted_alternatives: int
    mismatches: tuple[str, ...]

    @property
    def ok(self) -> bool:
        return not self.mismatches


_OPEN = {"(": ")", "[": "]", "{": "}"}
_CLOSE = {value: key for key, value in _OPEN.items()}
_STRING = re.compile(r'"((?:[^"\\]|\\.)*)"\Z', re.S)
_INTEGER = re.compile(r"(?:0|[1-9][0-9]*)\Z")
_SIZED = re.compile(r"^([A-Za-z][A-Za-z0-9_]*)\[(.*)\]$", re.S)
_SCALAR = re.compile(r"^[A-Za-z][A-Za-z0-9_]*$")
_PRIOR = re.compile(r"^\$val\{([0-9]+)\}$")
_FLOOR = re.compile(r"^int\(\(\$val\{([0-9]+)\}\+([0-9]+)\)/([1-9][0-9]*)\)$")
_MEMBER_REGEX = re.compile(r"^\$\$self\{([^}]+)\}\s*(=~|!~)\s*/((?:\\.|[^/\\])*)/([a-z]*)$", re.S)
_MEMBER_STRING = re.compile(r"^\$\$self\{([^}]+)\}\s*(eq|ne)\s*'((?:\\.|[^'\\])*)'$", re.S)
_MEMBER_ARROW_STRING = re.compile(r'^\$self->\{([^}]+)\}\s*(eq|ne)\s*"((?:\\.|[^"\\])*)"$', re.S)


def _fail(message: str) -> None:
    raise SerialVerificationError(message)


def _unescape(value: str) -> str:
    """Decode the Rust string subset emitted by codegen's literal writer."""
    out, at = [], 0
    simple = {"\\": "\\", '"': '"', "n": "\n", "r": "\r", "t": "\t", "0": "\0", "'": "'"}
    while at < len(value):
        char = value[at]
        if char != "\\":
            out.append(char)
            at += 1
            continue
        if at + 1 >= len(value):
            _fail("unterminated Rust string escape")
        escaped = value[at + 1]
        if escaped in simple:
            out.append(simple[escaped])
            at += 2
        elif escaped == "x" and at + 3 < len(value) and re.fullmatch(r"[0-9a-fA-F]{2}", value[at + 2:at + 4]):
            out.append(chr(int(value[at + 2:at + 4], 16)))
            at += 4
        elif escaped == "u" and at + 2 < len(value) and value[at + 2] == "{":
            end = value.find("}", at + 3)
            if end < 0 or not re.fullmatch(r"[0-9a-fA-F]{1,6}", value[at + 3:end]):
                _fail("invalid Rust unicode escape")
            out.append(chr(int(value[at + 3:end], 16)))
            at = end + 1
        else:
            _fail(f"unknown Rust string escape \\{escaped}")
    return "".join(out)


def _string(value: str) -> str:
    match = _STRING.fullmatch(value.strip())
    if match is None:
        _fail("expected Rust string literal")
    return _unescape(match.group(1))


def _span(text: str, start: int, terminators: set[str]) -> int:
    """Find an expression end, honoring nested delimiters and strings."""
    stack: list[str] = []
    quoted = False
    escaped = False
    for at in range(start, len(text)):
        char = text[at]
        if quoted:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                quoted = False
            continue
        if char == '"':
            quoted = True
        elif char in _OPEN:
            stack.append(char)
        elif char in _CLOSE:
            if not stack:
                if char in terminators:
                    return at
                _fail("unbalanced Rust expression")
            if stack.pop() != _CLOSE[char]:
                _fail("mismatched Rust expression delimiter")
        elif not stack and char in terminators:
            return at
    if not stack and "," in terminators:
        return len(text)
    _fail("unterminated Rust expression")


def _split_members(body: str, what: str) -> dict[str, str]:
    parts: list[str] = []
    at = 0
    while at < len(body):
        end = _span(body, at, {","})
        part = body[at:end].strip()
        if part:
            parts.append(part)
        at = end + 1
    result: dict[str, str] = {}
    for part in parts:
        match = re.fullmatch(r"([a-z][a-z0-9_]*)\s*:\s*(.+)", part, re.S)
        if match is None or match.group(1) in result:
            _fail(f"malformed or duplicate {what} member")
        result[match.group(1)] = match.group(2).strip()
    return result


def _struct(value: str, constructor: str, fields: set[str]) -> dict[str, str]:
    text = value.strip().removesuffix(",").strip()
    match = re.match(re.escape(constructor) + r"\s*\{", text)
    if match is None:
        _fail(f"expected {constructor} literal")
    end = _span(text, match.end(), {"}"})
    if text[end] != "}" or text[end + 1:].strip():
        _fail(f"malformed {constructor} literal")
    members = _split_members(text[match.end():end], constructor)
    if set(members) != fields:
        _fail(f"{constructor} schema drift: {sorted(set(members) ^ fields)}")
    return members


def _array(value: str, what: str) -> list[str]:
    text = value.strip().removesuffix(",").strip()
    if not text.startswith("&["):
        _fail(f"expected {what} array")
    end = _span(text, 2, {"]"})
    if text[end] != "]" or text[end + 1:].strip():
        _fail(f"malformed {what} array")
    body = text[2:end]
    items: list[str] = []
    at = 0
    while at < len(body):
        end = _span(body, at, {","})
        item = body[at:end].strip()
        if item:
            items.append(item)
        at = end + 1
    return items


def _fmt(value: str) -> str:
    text = re.sub(r"\s+", "", value)
    mapping = {
        "Fmt::Int8u": "int8u", "Fmt::Int16u": "int16u", "Fmt::Int32u": "int32u",
        "Fmt::Str(1)": "string", "Fmt::Undef(1)": "undef", "Fmt::RemainderString": "string",
    }
    if text not in mapping:
        _fail(f"unknown serial Rust format {value!r}")
    return mapping[text]


def _count(value: str) -> tuple[Any, ...]:
    text = re.sub(r"\s+", "", value)
    if match := re.fullmatch(r"SerialCount::Fixed\{value:(\d+)\}", text):
        return ("fixed", int(match.group(1)))
    if match := re.fullmatch(r"SerialCount::PriorRaw\{serial_index:(\d+)\}", text):
        return ("prior", int(match.group(1)))
    if match := re.fullmatch(r"SerialCount::FloorDivPriorRaw\{serial_index:(\d+),add:(\d+),divisor:(\d+)\}", text):
        return ("floor", int(match.group(1)), int(match.group(2)), int(match.group(3)))
    if text == "SerialCount::RemainingBytes":
        return ("remaining",)
    _fail("unknown serial count literal")


def _condition(value: str) -> Any:
    text = value.strip()
    if text == "None":
        return None
    match = re.fullmatch(r"Some\(\s*Cond::MemberRegex\s*\{(.*)\}\s*\)", text, re.S)
    if match:
        fields = _split_members(match.group(1), "Cond::MemberRegex")
        if set(fields) != {"member", "pattern", "ignore_case", "negate"}:
            _fail("serial MemberRegex schema drift")
        if fields["ignore_case"] not in {"true", "false"} or fields["negate"] not in {"true", "false"}:
            _fail("serial MemberRegex boolean is invalid")
        return ("regex", _string(fields["member"]), _string(fields["pattern"]),
                fields["ignore_case"] == "true", fields["negate"] == "true")
    match = re.fullmatch(r"Some\(\s*Cond::MemberStrEq\s*\{(.*)\}\s*\)", text, re.S)
    if match:
        fields = _split_members(match.group(1), "Cond::MemberStrEq")
        if set(fields) != {"member", "value", "negate"} or fields["negate"] not in {"true", "false"}:
            _fail("serial MemberStrEq schema drift")
        return ("string", _string(fields["member"]), _string(fields["value"]), fields["negate"] == "true")
    _fail("unknown serial condition literal")


def _flags(value: str) -> tuple[bool, bool, bool]:
    if value.strip() == "IfdFlags::NONE":
        return (False, False, False)
    fields = _struct(value, "IfdFlags", {"unknown", "binary", "list", "protected", "avoid", "priority"})
    if fields["protected"] != "false" or fields["avoid"] != "false" or fields["priority"] != "None":
        _fail("serial artifact uses unsupported IFD flags")
    if any(fields[name] not in {"true", "false"} for name in ("unknown", "binary", "list")):
        _fail("serial flag boolean is invalid")
    return tuple(fields[name] == "true" for name in ("unknown", "binary", "list"))


def _groups(value: str) -> tuple[str | None, str | None, str | None]:
    if value.strip() == "TagGroups::NONE":
        return (None, None, None)
    fields = _struct(value, "TagGroups", {"g0", "g1", "g2"})
    out = []
    for name in ("g0", "g1", "g2"):
        if fields[name].strip() == "None":
            out.append(None)
        else:
            match = re.fullmatch(r"Some\((.*)\)", fields[name].strip(), re.S)
            if match is None:
                _fail("serial TagGroups member is invalid")
            out.append(_string(match.group(1)))
    return tuple(out)


def _processor(value: str) -> tuple[str, str, str, str]:
    fields = _struct(value, "SerialProcessorFacts", {"name", "source_file", "source_sha256", "source_body_sha256"})
    out = tuple(_string(fields[name]) for name in ("name", "source_file", "source_sha256", "source_body_sha256"))
    if not re.fullmatch(r"[0-9a-f]{64}", out[2]) or not re.fullmatch(r"[0-9a-f]{64}", out[3]):
        _fail("serial processor digest is invalid")
    if (not out[1] or PurePosixPath(out[1]).is_absolute() or "\\" in out[1]
            or any(part in {"", ".", ".."} for part in out[1].split("/"))):
        _fail("serial processor source file is not library-relative")
    return out


def _gate(value: str) -> tuple[tuple[str, int], ...]:
    fields = _struct(value, "GateA", {"blocked_by"})
    pairs = []
    for item in _array(fields["blocked_by"], "GateA"):
        match = re.fullmatch(r"\(\s*(\"(?:[^\"\\]|\\.)*\")\s*,\s*(\d+)\s*\)", item, re.S)
        if match is None or int(match.group(2)) == 0:
            _fail("serial GateA member is invalid")
        pairs.append((_string(match.group(1)), int(match.group(2))))
    if tuple(sorted(pairs)) != tuple(pairs) or len({name for name, _ in pairs}) != len(pairs):
        _fail("serial GateA members are not sorted/unique")
    return tuple(pairs)


def _tag(value: str) -> dict[str, Any]:
    fields = _struct(value, "SerialTag", {"name", "format", "condition", "flags", "raw_conv", "omitted", "value_conv", "print_conv", "groups"})
    if fields["raw_conv"].strip() != "None" or fields["omitted"].strip() != "Omitted::NONE" or fields["value_conv"].strip() != "None" or fields["print_conv"].strip() != "PrintConv::None":
        _fail("serial artifact uses an unverified conversion/omission projection")
    format_fields = _struct(fields["format"], "SerialFormat", {"format", "count"})
    return {
        "name": _string(fields["name"]), "format": _fmt(format_fields["format"]),
        "count": _count(format_fields["count"]), "condition": _condition(fields["condition"]),
        "flags": _flags(fields["flags"]), "groups": _groups(fields["groups"]),
    }


def _entries(value: str, module: str, table: str) -> dict[RowKey, dict[str, Any]]:
    result: dict[RowKey, dict[str, Any]] = {}
    for item in _array(value, "SerialEntry"):
        fields = _struct(item, "SerialEntry", {"serial_index", "alternatives"})
        if _INTEGER.fullmatch(fields["serial_index"].strip()) is None:
            _fail("serial index is invalid")
        index = int(fields["serial_index"])
        alternatives = _array(fields["alternatives"], "SerialTag")
        if not alternatives:
            _fail("serial entry has no alternatives")
        for alternative, tag in enumerate(alternatives):
            # The schema has no `variant` field for emitted rows. The native
            # inventory supplies that identity during audit; use false here and
            # resolve it against the unique native alternative at this index.
            key = RowKey(module, table, index, False, alternative)
            if key in result:
                _fail("duplicate serial artifact row")
            result[key] = _tag(tag)
    return result


def _static_value(source: str, name: str) -> str:
    marker = re.search(r"pub\s+static\s+" + re.escape(name) + r"\s*:\s*[^=]+?=\s*", source)
    if marker is None:
        _fail(f"serial artifact lacks {name}")
    end = _span(source, marker.end(), {";"})
    if source[end] != ";":
        _fail(f"serial artifact {name} is unterminated")
    return source[marker.end():end].strip()


def parse_artifact(path: Path) -> Artifact:
    source = path.read_text(encoding="utf-8")
    table_markers = list(re.finditer(r"pub\s+static\s+([A-Za-z_][A-Za-z0-9_]*)\s*:\s*SerialTable\s*=\s*", source))
    tables: dict[tuple[str, str], ArtifactTable] = {}
    symbols: dict[str, tuple[str, str]] = {}
    for marker in table_markers:
        end = _span(source, marker.end(), {";"})
        if source[end] != ";":
            _fail("unterminated SerialTable static")
        fields = _struct(source[marker.end():end], "SerialTable", {"module", "table", "group0", "group1", "group2", "default_format", "processor", "gate_a", "entries"})
        module, table = _string(fields["module"]), _string(fields["table"])
        key = (module, table)
        if key in tables or marker.group(1) in symbols:
            _fail("duplicate generated serial table")
        tables[key] = ArtifactTable(module, table,
                                    (_string(fields["group0"]), _string(fields["group1"]), _string(fields["group2"])),
                                    _fmt(fields["default_format"]), _processor(fields["processor"]), _gate(fields["gate_a"]),
                                    _entries(fields["entries"], module, table))
        symbols[marker.group(1)] = key
    index = _array(_static_value(source, "ALL_SERIAL_TABLES"), "ALL_SERIAL_TABLES")
    index_symbols = []
    for item in index:
        match = re.fullmatch(r"&([A-Za-z_][A-Za-z0-9_]*)", item)
        if match is None:
            _fail("ALL_SERIAL_TABLES member is invalid")
        index_symbols.append(match.group(1))
    if len(index_symbols) != len(set(index_symbols)) or set(index_symbols) != set(symbols):
        _fail("ALL_SERIAL_TABLES does not exactly index generated serial tables")

    omissions: dict[RowKey, tuple[str | None, tuple[str, ...]]] = {}
    for item in _array(_static_value(source, "OMITTED_SERIAL_NATIVE_ROWS"), "serial omissions"):
        fields = _struct(item, "OmittedSerialNativeRow", {"module", "table", "serial_index", "variant", "alternative", "name", "reasons"})
        if _INTEGER.fullmatch(fields["serial_index"].strip()) is None or _INTEGER.fullmatch(fields["alternative"].strip()) is None:
            _fail("serial omission identity is invalid")
        if fields["variant"].strip() not in {"true", "false"}:
            _fail("serial omission variant is invalid")
        raw_name = fields["name"].strip()
        name = None
        if raw_name != "None":
            match = re.fullmatch(r"Some\((.*)\)", raw_name, re.S)
            if match is None:
                _fail("serial omission name is invalid")
            name = _string(match.group(1))
        reasons = tuple(_string(reason) for reason in _array(fields["reasons"], "serial omission reasons"))
        if not reasons or tuple(sorted(set(reasons))) != reasons:
            _fail("serial omission reasons are empty, duplicate, or unsorted")
        key = RowKey(_string(fields["module"]), _string(fields["table"]), int(fields["serial_index"]), fields["variant"].strip() == "true", int(fields["alternative"]))
        if key in omissions:
            _fail("duplicate serial omission")
        omissions[key] = (name, reasons)

    omitted_tables: dict[tuple[str, str], tuple[str, ...]] = {}
    for item in _array(_static_value(source, "OMITTED_SERIAL_NATIVE_TABLES"), "serial table omissions"):
        fields = _struct(item, "OmittedSerialNativeTable", {"module", "table", "reasons"})
        key = (_string(fields["module"]), _string(fields["table"]))
        reasons = tuple(_string(reason) for reason in _array(fields["reasons"], "serial table omission reasons"))
        if key in omitted_tables or not reasons or tuple(sorted(set(reasons))) != reasons:
            _fail("serial table omission is invalid")
        omitted_tables[key] = reasons
    return Artifact(tables, omissions, omitted_tables)


def _native_truth(value: Any) -> bool:
    if value is None:
        return False
    if isinstance(value, str):
        return value not in {"", "0"}
    if isinstance(value, (bool, int, float)):
        return value != 0
    _fail("serial native flag is not a literal scalar")


def _native_groups(raw: Any, module: str) -> tuple[str, str, str]:
    if raw is None:
        raw = {}
    if not isinstance(raw, dict) or any(not isinstance(key, str) or not isinstance(value, str) for key, value in raw.items()):
        _fail("serial native GROUPS is malformed")
    if set(raw) - {"0", "1", "2"}:
        _fail("serial native GROUPS has unsupported index")
    first_module = module.split("::", 1)[0] if isinstance(module, str) else ""
    if re.fullmatch(r"[A-Za-z_]\w*", first_module) is None:
        _fail("serial native module is malformed for GetTagTable group defaults")
    # ExifTool's `Image::.*?::([^:]*)` takes the first component after
    # Image::ExifTool, rather than the full nested module spelling.
    return tuple(raw.get(str(index)) if raw.get(str(index)) not in {None, "", "0"} else (first_module if index < 2 else "Other") for index in range(3))


def _native_format(tag: dict[str, Any], default: str) -> tuple[str, tuple[Any, ...]]:
    value = tag.get("Format")
    if value is None:
        return default, ("fixed", 1)
    if not isinstance(value, str):
        _fail("serial native Format is not a literal string")
    match = _SIZED.fullmatch(value)
    if match:
        fmt, count = match.groups()
        if _SCALAR.fullmatch(fmt) is None:
            _fail("serial native sized Format is malformed")
        if _INTEGER.fullmatch(count):
            return fmt, ("fixed", int(count))
        if prior := _PRIOR.fullmatch(count):
            return fmt, ("prior", int(prior.group(1)))
        if floor := _FLOOR.fullmatch(count):
            return fmt, ("floor", int(floor.group(1)), int(floor.group(2)), int(floor.group(3)))
        _fail("serial native Format count is outside verifier grammar")
    if value == "string":
        return value, ("remaining",)
    if _SCALAR.fullmatch(value) is None:
        _fail("serial native Format spelling is malformed")
    return value, ("fixed", 1)


def _native_condition(value: Any) -> tuple[Any, bool]:
    if value is None:
        return None, False
    if not isinstance(value, str) or not value.strip():
        _fail("serial native Condition is malformed")
    text = value.strip()
    if match := _MEMBER_REGEX.fullmatch(text):
        member, op, pattern, flags = match.groups()
        if set(flags) - {"i"}:
            _fail("serial native Condition regex flags are unsupported")
        return ("regex", member, pattern, "i" in flags, op == "!~"), True
    if match := _MEMBER_STRING.fullmatch(text):
        member, op, string = match.groups()
        # Perl single-quoted strings in the accepted subset only escape quote
        # and backslash; preserve other backslashes literally.
        string = string.replace("\\'", "'").replace("\\\\", "\\")
        return ("string", member, string, op == "ne"), False
    if match := _MEMBER_ARROW_STRING.fullmatch(text):
        member, op, string = match.groups()
        return ("string", member, string, op == "ne"), False
    # The staged artifact withholds every member-regex condition because the
    # shared Cond evaluator treats absent members differently from Perl's
    # empty-string regex subject.  For such rows we only need to authenticate
    # that native fact and its named refusal, including compound expressions.
    if re.search(r"\$\$self\{[^}]+\}\s*(?:=~|!~)\s*/", text):
        return None, True
    _fail("serial native Condition is outside verifier grammar")


def _native_reasons(tag: Any, default: str) -> tuple[dict[str, Any] | None, tuple[str, ...]]:
    if not isinstance(tag, dict) or not isinstance(tag.get("Name"), str) or not tag["Name"]:
        return None, ("serial_row_shape",)
    reasons: list[str] = []
    try:
        fmt, count = _native_format(tag, default)
        condition, missing_empty = _native_condition(tag.get("Condition"))
    except SerialVerificationError:
        # The emitter sees no descriptor format after a source-row shape
        # refusal, and therefore independently withholds the artifact shape
        # as well. This preserves the exact source/sidecar distinction rather
        # than guessing a partially parsed operand.
        return None, ("serial_emitter_format", "serial_row_shape")
    if fmt not in RUNTIME_FORMATS:
        reasons.append("serial_emitter_format")
    if missing_empty:
        reasons.append("serial_condition_missing_member")
    raw_print = tag.get("PrintConv")
    if raw_print is not None:
        if isinstance(raw_print, dict) and raw_print.get("kind") == "expr" and isinstance(raw_print.get("expr"), str):
            reasons.append("serial_decode_bits_words" if raw_print["expr"] == "Image::ExifTool::DecodeBits($val, undef, 16)" else "serial_print_conv_expr")
        else:
            reasons.append("serial_print_conv")
    if tag.get("RawConv") is not None:
        reasons.append("serial_raw_conv")
    if tag.get("ValueConv") is not None:
        reasons.append("serial_value_conv")
    if tag.get("SubDirectory") is not None:
        reasons.append("serial_subdirectory")
    for name in sorted(tag):
        if name not in ROW_MODELED and name not in ROW_DOCUMENTARY:
            reasons.append(f"serial_row_property_{name}")
    expected = {"name": tag["Name"], "format": fmt, "count": count, "condition": condition,
                "flags": tuple(_native_truth(tag.get(name)) for name in ("Unknown", "Binary", "List")),
                "groups": _native_row_groups(tag.get("Groups"))}
    return expected, tuple(sorted(set(reasons)))


def _native_row_groups(raw: Any) -> tuple[str | None, str | None, str | None]:
    if raw is None:
        return (None, None, None)
    if not isinstance(raw, dict) or set(raw) - {"0", "1", "2"} or any(not isinstance(key, str) or not isinstance(value, str) for key, value in raw.items()):
        _fail("serial native row Groups is malformed")
    return tuple(raw.get(str(index)) for index in range(3))


def native_population(document: dict[str, Any]) -> dict[tuple[str, str], NativeTable]:
    if not isinstance(document, dict) or not isinstance(document.get("modules"), dict):
        _fail("serial native dump lacks module dictionary")
    output: dict[tuple[str, str], NativeTable] = {}
    for module, module_data in document["modules"].items():
        if not isinstance(module, str) or not module or not isinstance(module_data, dict) or not isinstance(module_data.get("tables"), dict):
            _fail("serial native module shape is malformed")
        for table, table_data in module_data["tables"].items():
            if not isinstance(table, str) or not table or not isinstance(table_data, dict):
                _fail("serial native table shape is malformed")
            meta = table_data.get("meta")
            if not isinstance(meta, dict):
                continue
            processor = meta.get("PROCESS_PROC")
            if not isinstance(processor, dict) or not isinstance(processor.get("__name"), str) or not processor["__name"].endswith(PROCESSOR_SUFFIX):
                continue
            try:
                identity = facts.source_identity(processor)
                body_sha = facts.deparse_sha256(processor.get("__deparse"))
            except facts.SerialFactRefused as error:
                _fail(str(error))
            default = meta.get("FORMAT", "int8u")
            if not isinstance(default, str) or _SCALAR.fullmatch(default) is None:
                _fail("serial native table FORMAT is malformed")
            table_reasons = []
            for name in sorted(meta):
                if name in TABLE_MODELED or name in TABLE_DOCUMENTARY:
                    continue
                table_reasons.append("serial_table_priority" if name == "PRIORITY" else f"serial_table_property_{name}")
            tags = table_data.get("tags")
            if not isinstance(tags, dict):
                _fail("serial native selected table has no tag dictionary")
            rows: dict[RowKey, dict[str, Any]] = {}
            for raw_index, raw in tags.items():
                if not isinstance(raw_index, str) or _INTEGER.fullmatch(raw_index) is None:
                    _fail("serial native selected table has non-numeric tag key")
                index = int(raw_index)
                variants = raw.get("_variants") if isinstance(raw, dict) and "_variants" in raw else None
                values = variants if variants is not None else [raw]
                if not isinstance(values, list) or not values:
                    _fail("serial native alternative shape is malformed")
                for alternative, tag in enumerate(values):
                    key = RowKey(module, table, index, variants is not None, alternative)
                    if key in rows:
                        _fail("duplicate serial native alternative")
                    expected, reasons = _native_reasons(tag, default)
                    rows[key] = {"expected": expected, "reasons": reasons,
                                 "name": tag.get("Name") if isinstance(tag, dict) and isinstance(tag.get("Name"), str) else None}
            key = (module, table)
            if key in output:
                _fail("duplicate selected serial native table")
            output[key] = NativeTable(module, table, (identity["name"], identity["source_file"], identity["source_sha256"], body_sha),
                                      _native_groups(meta.get("GROUPS"), module), default, rows, tuple(sorted(set(table_reasons))))
    return output


def audit(document: dict[str, Any], artifact: Artifact) -> Audit:
    native = native_population(document)
    mismatches: list[str] = []
    expected_rows = sum(len(table.rows) for table in native.values())
    emitted_rows = sum(len(table.rows) for table in artifact.tables.values())
    native_row_keys = {row_key for table in native.values() for row_key in table.rows}
    for row_key in sorted(set(artifact.omissions) - native_row_keys):
        mismatches.append(f"{row_key}: generated omission has no selected native source alternative")
    for key in sorted(set(native) - set(artifact.tables) - set(artifact.omitted_tables)):
        mismatches.append(f"{key}: selected native table absent from artifact and table omissions")
    for key in sorted((set(artifact.tables) | set(artifact.omitted_tables)) - set(native)):
        mismatches.append(f"{key}: artifact table has no selected native processor")
    for key in sorted(set(artifact.tables) & set(artifact.omitted_tables)):
        mismatches.append(f"{key}: table appears as both emitted and omitted")
    # `serial_descriptor_refused` is an emitter-side report, not independent
    # proof about a native table.  Accepting it here would let an artifact put
    # every selected table in this sidecar and evade all row/provenance checks.
    # A later verifier may add a native, separately implemented refusal rule;
    # until then a selected table must be emitted and audited.
    for key in sorted(set(native) & set(artifact.omitted_tables)):
        mismatches.append(f"{key}: selected native table is wholly omitted without independent native proof")
    for key, native_table in sorted(native.items()):
        if key in artifact.omitted_tables:
            continue
        emitted = artifact.tables.get(key)
        if emitted is None:
            continue
        if emitted.groups != native_table.groups:
            mismatches.append(f"{key}: effective groups differ from native GetTagTable defaults")
        if emitted.default_format != native_table.default_format:
            mismatches.append(f"{key}: default format differs from native source")
        if emitted.processor != native_table.processor:
            mismatches.append(f"{key}: processor source identity differs")
        expected_gate = Counter(native_table.table_reasons)
        for row in native_table.rows.values():
            for reason in row["reasons"]:
                expected_gate[reason] += 1
        if emitted.gate != tuple(sorted(expected_gate.items())):
            mismatches.append(f"{key}: Gate A reasons/counts differ from native source and omissions")

        native_by_index: dict[tuple[int, int], tuple[RowKey, dict[str, Any]]] = {(row_key.index, row_key.alternative): (row_key, row) for row_key, row in native_table.rows.items()}
        artifact_by_index = {(row_key.index, row_key.alternative): row for row_key, row in emitted.rows.items()}
        for pair, (native_key, source_row) in sorted(native_by_index.items()):
            expected, reasons = source_row["expected"], source_row["reasons"]
            omission = artifact.omissions.get(native_key)
            emitted_row = artifact_by_index.get(pair)
            if reasons:
                if emitted_row is not None:
                    mismatches.append(f"{native_key}: refused native alternative was emitted")
                if omission is None or omission[0] != source_row["name"]:
                    mismatches.append(f"{native_key}: omission name differs from native source")
                if omission is None or omission[1] != reasons:
                    mismatches.append(f"{native_key}: omission reasons differ from native source")
                continue
            if omission is not None:
                mismatches.append(f"{native_key}: supported native alternative was omitted")
                continue
            if emitted_row is None:
                mismatches.append(f"{native_key}: supported native alternative is absent")
                continue
            if emitted_row != expected:
                mismatches.append(f"{native_key}: emitted name/format/count/condition/flags/groups differ from native source")
            # A one-alternative native `_variants` array is not distinguishable
            # from a direct scalar entry in the current SerialEntry schema.
            # Do not silently certify that source-identity change until the
            # artifact carries an emitted variant discriminator.
            if native_key.variant and sum(1 for candidate in native_table.rows if candidate.index == native_key.index) == 1:
                mismatches.append(f"{native_key}: emitted single variant lacks artifact variant identity")
        for pair in sorted(set(artifact_by_index) - set(native_by_index)):
            mismatches.append(f"{key + pair}: generated serial alternative has no native source row")
        expected_omissions = {row_key for row_key, row in native_table.rows.items() if row["reasons"]}
        actual_omissions = {row_key for row_key in artifact.omissions if row_key.module == key[0] and row_key.table == key[1]}
        for row_key in sorted(actual_omissions - expected_omissions):
            mismatches.append(f"{row_key}: generated omission has no refused native source alternative")
    return Audit(len(native), expected_rows, len(artifact.tables), emitted_rows, len(artifact.omissions), tuple(mismatches))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifact", type=Path, help="generated serial_tables.rs")
    parser.add_argument("source_dump", type=Path, help="fresh native dump_tables.pl JSON")
    parser.add_argument("--json-out", type=Path, help="optional machine-readable audit result")
    args = parser.parse_args(argv)
    try:
        document = json.loads(args.source_dump.read_text(encoding="utf-8"))
        artifact = parse_artifact(args.artifact)
        result = audit(document, artifact)
    except (OSError, json.JSONDecodeError, SerialVerificationError) as error:
        parser.error(str(error))
    instrument = {
        "tool": Path(__file__).name,
        "source_dump": str(args.source_dump),
        "source_sha256": hashlib.sha256(args.source_dump.read_bytes()).hexdigest(),
        "artifact": str(args.artifact),
        "artifact_sha256": hashlib.sha256(args.artifact.read_bytes()).hexdigest(),
        "scope": "recorded native source and emitted Rust facts; native processor execution is separate",
    }
    print("=== instrument: verify_serial_directory.py ===", file=sys.stderr)
    print(json.dumps(instrument, sort_keys=True), file=sys.stderr)
    payload = {"expected_tables": result.expected_tables, "expected_alternatives": result.expected_alternatives,
               "emitted_tables": result.emitted_tables, "emitted_alternatives": result.emitted_alternatives,
               "omitted_alternatives": result.omitted_alternatives, "mismatches": list(result.mismatches)}
    if args.json_out:
        args.json_out.write_text(json.dumps(payload, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(payload, sort_keys=True))
    if result.mismatches:
        for mismatch in result.mismatches:
            print(f"serial audit: {mismatch}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
