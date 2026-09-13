"""Closed, source-derived inventory compiler for native serial records.

The output is a versioned JSON descriptor, not a Rust layout and not a route
selection.  It records the subset of ``ProcessSerialData`` table semantics a
future shared reader must implement and names every unsupported row.  It never
uses a module, table, camera, or parent-tag allowlist.

The descriptor has two independent source fingerprints: the complete captured
processor body and the raw table facts.  A future verifier can recompute both
with :mod:`serial_directory_facts` without importing this recognizer.
"""

from collections import Counter
import argparse
import copy
from dataclasses import dataclass
import json
import re

import codegen
import conds
import serial_directory_facts as facts
from serial_processor_grammar import SERIAL_PROCESSOR_V1_TOKENS


DESCRIPTOR_VERSION = 2
PROCESS_SERIAL_DATA = "ProcessSerialData"
_RUNTIME_FORMATS = frozenset({"int8u", "int16u", "int16s", "int32u", "string", "undef", "binary"})


class SerialDirectoryRefused(ValueError):
    """The native serial table is outside this staged grammar."""


_SCALAR_FORMAT = re.compile(r"^[A-Za-z][A-Za-z0-9_]*$")
_SIZED_FORMAT = re.compile(r"^([A-Za-z][A-Za-z0-9_]*)\[(.*)\]$", re.S)
_PRIOR_VALUE = re.compile(r"^\$val\{([0-9]+)\}$")
_FLOOR_DIV_PRIOR = re.compile(
    r"^int\(\(\$val\{([0-9]+)\}\+([0-9]+)\)/([1-9][0-9]*)\)(?:\+([0-9]+))?$"
)
_INTEGER = re.compile(r"^(0|[1-9][0-9]*)$")
_FQ = re.compile(r"(?:[A-Za-z_]\w*::)*[A-Za-z_]\w*$")

# These tokens occur in the native control path in the order in which their
# effects matter.  The inventory is deliberately inactive, but it will not
# describe an arbitrary CODE fact as serial processing merely because its name
# happens to end in ProcessSerialData.
# Token grammar for the complete B::Deparse control flow.  Quoted strings and
# regexes are atomic tokens, so source words inside a diagnostic cannot satisfy
# a required executable call.  This accepts the one source processor shape and
# deliberately refuses added statements until they receive an explicit grammar
# extension; the source-body digest is provenance, never the acceptance rule.
_PROCESS_TOKEN = re.compile(
    r"\s+|'(?:\\.|[^'\\])*'|\"(?:\\.|[^\"\\])*\"|/(?!\s)(?:\\.|[^/\\])*/[a-z]*"
    r"|\$(?:[0-9]+|@)|\$\$?[A-Za-z_]\w*|%[A-Za-z_]\w*|@[A-Za-z_]\w*"
    r"|(?:[A-Za-z_]\w*::)*[A-Za-z_]\w*|\d+|\|\||&&|==|<=|>=|\+=|=~|->|\+\+"
    r"|[(){}\[\];,=+*/&<>$@%:\\?!.|-]"
)
_REQUIRED_PROCESSOR_STEPS = (
    ("serial processor package", ("package", "Image::ExifTool::Canon", ";", "use", "strict", ";")),
    ("temporary unknown option", ("$et", "->", "Options", "(", "'Unknown'", ",", "1", ")")),
    ("unknown generation suppression", ("$et", "->", "{", "'NO_UNKNOWN'", "}", "=", "1")),
    ("serial verbose callback", ("$et", "->", "VerboseDir", "(", "'SerialData'")),
    ("table default format", ("$tagTablePtr", "->", "{", "'FORMAT'", "}")),
    ("serial GetTagInfo", ("$et", "->", "GetTagInfo", "(")),
    ("dynamic count evaluation", ("$count", "=", "eval", "(", "$count", ")")),
    ("remainder string", ("$format", "eq", "'string'")),
    ("field length", ("Image::ExifTool::FormatSize", "(", "$format", ")")),
    ("bounded read", ("ReadValue", "(")),
    ("raw value storage", ("$val", "{", "$index", "}", "=", "$val")),
    ("subdirectory branch", ("$tagInfo", "->", "{", "'SubDirectory'", "}")),
    ("tag reporting", ("$et", "->", "FoundTag", "(")),
    ("serial cursor advance", ("$pos", "+=", "$len")),
    ("unknown option restore", ("$et", "->", "Options", "(", "'Unknown'", ",", "$unknown", ")")),
    ("unknown generation cleanup", ("delete", "$et", "->", "{", "'NO_UNKNOWN'", "}")),
)
_TABLE_MODELED_PROPERTIES = frozenset({"PROCESS_PROC", "FORMAT", "GROUPS", "VARS"})
_TABLE_DOCUMENTARY_PROPERTIES = frozenset({"NOTES"})
_ROW_MODELED_PROPERTIES = frozenset({"Name", "Format", "Condition", "PrintConv", "RawConv",
                                     "ValueConv", "SubDirectory", "Groups", "Unknown", "Binary", "List"})
_ROW_DOCUMENTARY_PROPERTIES = frozenset({"Notes", "_shorthand"})


def _processor_tokens(source):
    if not isinstance(source, str):
        raise SerialDirectoryRefused("missing serial processor body")
    tokens, at = [], 0
    while at < len(source):
        match = _PROCESS_TOKEN.match(source, at)
        if match is None:
            raise SerialDirectoryRefused("serial processor syntax is outside the closed grammar")
        if not match.group().isspace():
            tokens.append(match.group())
        at = match.end()
    return tokens


def _processor_contract(processor):
    """Capture ordered native effects without treating this as a runtime parser."""
    try:
        identity = facts.source_identity(processor)
        body = processor.get("__deparse")
        body_sha = facts.deparse_sha256(body)
    except facts.SerialFactRefused as error:
        raise SerialDirectoryRefused(str(error)) from None
    if not identity["name"].endswith(f"::{PROCESS_SERIAL_DATA}") or _FQ.fullmatch(identity["name"]) is None:
        raise SerialDirectoryRefused("processor identity is not a serial-data processor")
    tokens = _processor_tokens(body)
    # Require every executable token and its position.  Unlike a source hash,
    # this grammar deliberately ignores formatting while making the accepted
    # algorithm reviewable: addresses, read bounds, boolean operators, raw
    # state assignment and reporting branches are all operands in this stream.
    # A table may add rows freely; a changed shared processor needs a new
    # grammar version and native proof before it becomes a supported input.
    if tuple(tokens) != SERIAL_PROCESSOR_V1_TOKENS:
        raise SerialDirectoryRefused("serial processor is outside the complete executable grammar")
    steps = [effect for effect, _ in _REQUIRED_PROCESSOR_STEPS]
    return {
        **identity,
        "source_body_sha256": body_sha,
        "effects_in_order": steps,
        "prior_value_domain": "raw_read_value_before_conversion_or_reporting",
        "condition_missing_member": "native_perl_empty_string",
    }


def _raw_count_operand(text):
    """Compile a closed count expression evaluated against prior raw values."""
    if _INTEGER.fullmatch(text):
        return {"kind": "fixed", "value": int(text)}
    match = _PRIOR_VALUE.fullmatch(text)
    if match:
        return {"kind": "prior_raw_value", "serial_index": int(match.group(1))}
    match = _FLOOR_DIV_PRIOR.fullmatch(text)
    if match:
        return {
            "kind": "floor_div_prior_raw_value",
            "serial_index": int(match.group(1)),
            "add": int(match.group(2)),
            "divisor": int(match.group(3)),
            "trailing_add": int(match.group(4) or 0),
        }
    raise SerialDirectoryRefused("serial Format count expression is outside the closed prior-value grammar")


def _format_operand(tag, default_format):
    """Return the native field format and its source count rule.

    ``ProcessSerialData`` reads a scalar field with count one, while an explicit
    bracket count is evaluated before the bytes are read.  The descriptor keeps
    this distinction so a future reader cannot use rendered text as the count.
    """
    explicit = tag.get("Format")
    if explicit is None:
        return {"format": default_format, "format_source": "table_default", "count": {"kind": "fixed", "value": 1}}
    if not isinstance(explicit, str):
        raise SerialDirectoryRefused("serial field Format is not a literal string")
    match = _SIZED_FORMAT.fullmatch(explicit)
    if match:
        base, count = match.groups()
        if _SCALAR_FORMAT.fullmatch(base) is None:
            raise SerialDirectoryRefused("serial sized Format base is unsupported")
        return {"format": base, "format_source": "field", "count": _raw_count_operand(count)}
    if explicit == "string":
        return {"format": explicit, "format_source": "field", "count": {"kind": "remaining_bytes"}}
    if _SCALAR_FORMAT.fullmatch(explicit) is None:
        raise SerialDirectoryRefused("serial field Format spelling is unsupported")
    return {"format": explicit, "format_source": "field", "count": {"kind": "fixed", "value": 1}}


def _condition_operand(condition):
    if condition is None:
        return {"kind": "always", "missing_member": "not_applicable"}
    if not isinstance(condition, str) or not condition.strip():
        raise SerialDirectoryRefused("serial Condition is not a non-empty source string")
    compiled = conds.compile_cond(condition)
    if compiled is None:
        raise SerialDirectoryRefused("serial Condition is outside the shared condition grammar")
    # Perl feeds undef to =~ as an empty scalar.  Existing Cond::MemberRegex
    # treats a missing member as false, so retain this execution requirement
    # even when the current negative EOS predicate happens to agree.
    # This policy is deliberately narrower than general Perl coercion.  The
    # shared condition grammar can execute only its string comparisons and
    # byte regexes with an absent member as Perl's empty scalar.  `defined`,
    # numeric predicates, and every other operation continue to see absence.
    string_member = bool(re.search(
        r"(?:\$\$self\{[^}]+\}|\$self->\{[^}]+\})\s*"
        r"(?:(?:=~|!~)\s*/|(?:eq|ne)\s*(?:['\"]))",
        condition,
    ))
    return {
        "kind": "shared_cond",
        "source": condition,
        "compiled": compiled,
        "missing_member": "empty_string_for_string_ops" if string_member else "shared_default",
    }


def _conversion_operand(tag):
    """Compile only native PrintConv shapes the serial reader executes.

    The descriptor retains source data, not a table identity: a plain
    integer-keyed enum reuses the shared typed enum renderer, while the one
    closed no-lookup DecodeBits expression becomes an explicit serial
    multiword operation. All other conversions remain named refusals.
    """
    raw = tag.get("PrintConv")
    if raw is None:
        return {"kind": "none"}, None
    if isinstance(raw, dict) and raw.get("kind") == "expr" and isinstance(raw.get("expr"), str):
        expression = raw["expr"]
        if expression == "Image::ExifTool::DecodeBits($val, undef, 16)":
            return {"kind": "decode_bits_words", "bits_per_word": 16}, None
        return {"kind": "native_expr", "expression": expression}, "serial_print_conv_expr"
    if isinstance(raw, dict) and raw.get("kind") == "enum":
        mapping = raw.get("map")
        directives = raw.get("directives")
        if directives is not None or not isinstance(mapping, dict):
            return {"kind": "native_print_conv", "source": copy.deepcopy(raw)}, "serial_print_conv"
        pairs = []
        try:
            for key, value in mapping.items():
                if not isinstance(key, str) or not isinstance(value, str):
                    raise ValueError
                pairs.append((int(key, 0), value))
        except ValueError:
            return {"kind": "native_print_conv", "source": copy.deepcopy(raw)}, "serial_print_conv"
        pairs.sort()
        if len({key for key, _ in pairs}) != len(pairs):
            return {"kind": "native_print_conv", "source": copy.deepcopy(raw)}, "serial_print_conv"
        return {"kind": "shared_int_enum", "map": [[key, value] for key, value in pairs]}, None
    return {"kind": "native_print_conv", "source": copy.deepcopy(raw)}, "serial_print_conv"


def _raw_conv_operand(tag):
    """Compile the existing closed data-member capture and no other RawConv."""
    raw = tag.get("RawConv")
    if raw is None:
        return {"kind": "none"}, None
    member = codegen.raw_conv_effect(raw)
    if member is not None:
        return {"kind": "set_member", "member": member}, None
    return {"kind": "native_raw_conv", "source": copy.deepcopy(raw)}, "serial_raw_conv"


def _flags(tag):
    def native_truth(name):
        value = tag.get(name)
        if value is None:
            return False
        if isinstance(value, str):
            return value not in ("", "0")
        if isinstance(value, (bool, int, float)):
            return value != 0
        raise SerialDirectoryRefused(f"serial {name} flag is not a literal scalar")

    return {
        "unknown": native_truth("Unknown"),
        "binary": native_truth("Binary"),
        "list": native_truth("List"),
    }


def _unmodeled_properties(source, modeled, documentary, prefix):
    if not isinstance(source, dict):
        raise SerialDirectoryRefused(f"{prefix} source is not a dictionary")
    reasons = []
    for name in sorted(source):
        if name in modeled or name in documentary:
            continue
        if name == "PRIORITY" and prefix == "serial_table_property":
            reasons.append("serial_table_priority")
        else:
            reasons.append(f"{prefix}_{name}")
    return reasons


def _alternative(tag, default_format):
    if not isinstance(tag, dict):
        raise SerialDirectoryRefused("serial alternative is not a tag dictionary")
    name = tag.get("Name")
    if not isinstance(name, str) or not name:
        raise SerialDirectoryRefused("serial alternative has no literal Name")
    format_operand = _format_operand(tag, default_format)
    condition = _condition_operand(tag.get("Condition"))
    conversion, refusal = _conversion_operand(tag)
    reasons = _unmodeled_properties(tag, _ROW_MODELED_PROPERTIES, _ROW_DOCUMENTARY_PROPERTIES,
                                    "serial_row_property")
    if refusal is not None:
        reasons.append(refusal)
    raw_conv, raw_refusal = _raw_conv_operand(tag)
    if raw_refusal is not None:
        reasons.append(raw_refusal)
    if tag.get("ValueConv") is not None:
        reasons.append("serial_value_conv")
    if tag.get("SubDirectory") is not None:
        reasons.append("serial_subdirectory")
    return {
        "name": name,
        "format": format_operand,
        "condition": condition,
        "conversion": conversion,
        "raw_conv": raw_conv,
        "groups": copy.deepcopy(tag.get("Groups") or {}),
        "flags": _flags(tag),
        "native": copy.deepcopy(tag),
        "refusals": reasons,
    }


def _entry(index, raw, default_format):
    alternatives = raw.get("_variants") if isinstance(raw, dict) else None
    source_alternatives = alternatives if alternatives is not None else [raw]
    if not isinstance(source_alternatives, list) or not source_alternatives:
        raise SerialDirectoryRefused("serial entry has no alternatives")
    compiled = []
    for alternative in source_alternatives:
        try:
            compiled.append(_alternative(alternative, default_format))
        except SerialDirectoryRefused as error:
            # Preserve every native alternative even when one sibling has a
            # field shape this descriptor version cannot express.  Dropping
            # the complete entry would conceal both that refusal and any
            # representable sibling from the source population.
            compiled.append({
                "name": alternative.get("Name") if isinstance(alternative, dict) else None,
                "native": copy.deepcopy(alternative),
                "refusals": ["serial_row_shape"],
                "native_refusal": str(error),
            })
    return {"serial_index": index, "variant": alternatives is not None, "alternatives": compiled}


def _table_meta(meta):
    if not isinstance(meta, dict):
        raise SerialDirectoryRefused("serial table metadata is not a dictionary")
    default_format = meta.get("FORMAT", "int8u")
    if not isinstance(default_format, str) or _SCALAR_FORMAT.fullmatch(default_format) is None:
        raise SerialDirectoryRefused("serial table default FORMAT is unsupported")
    variables = meta.get("VARS") or {}
    if not isinstance(variables, dict) or any(not isinstance(k, str) or not isinstance(v, str) for k, v in variables.items()):
        raise SerialDirectoryRefused("serial table VARS are not literal strings")
    groups = meta.get("GROUPS") or {}
    if not isinstance(groups, dict) or any(not isinstance(k, str) or not isinstance(v, str) for k, v in groups.items()):
        raise SerialDirectoryRefused("serial table GROUPS are not literal strings")
    reasons = _unmodeled_properties(meta, _TABLE_MODELED_PROPERTIES,
                                    _TABLE_DOCUMENTARY_PROPERTIES, "serial_table_property")
    return default_format, groups, variables, reasons


def is_serial_processor(table_data):
    """Whether a native table selected the named shared serial processor.

    This is deliberately a source selector only.  It does not decide whether
    the table can be described by descriptor version 1, nor whether a future
    reader can execute it.  Keeping that selector separate makes a malformed
    or zero-row selected table visible in the population denominator.
    """
    if not isinstance(table_data, dict):
        return False
    processor = table_data.get("meta", {}).get("PROCESS_PROC")
    return (
        isinstance(processor, dict)
        and isinstance(processor.get("__name"), str)
        and processor["__name"].endswith(f"::{PROCESS_SERIAL_DATA}")
    )


def compile_serial_inventory(module, table, table_data):
    """Compile one captured serial table to descriptor version 1.

    It emits no Rust.  A malformed source table receives a named table-level
    refusal so inventory callers can keep it visible instead of dropping it.
    """
    if not isinstance(module, str) or not module or not isinstance(table, str) or not table:
        raise SerialDirectoryRefused("serial table identity is unavailable")
    if not isinstance(table_data, dict):
        raise SerialDirectoryRefused("serial table is not a dictionary")
    meta = table_data.get("meta")
    default_format, groups, variables, table_reasons = _table_meta(meta)
    processor = _processor_contract(meta.get("PROCESS_PROC"))
    tags = table_data.get("tags")
    if not isinstance(tags, dict):
        raise SerialDirectoryRefused("serial table tags are not a dictionary")

    entries, blocked = [], Counter(table_reasons)
    reachable, first_gap, expected_index = True, None, 0
    for raw_index, raw in sorted(tags.items(), key=lambda pair: int(pair[0]) if isinstance(pair[0], str) and pair[0].isdigit() else -1):
        if not isinstance(raw_index, str) or _INTEGER.fullmatch(raw_index) is None:
            blocked["serial_index"] += 1
            continue
        index = int(raw_index)
        if index != expected_index:
            if first_gap is None:
                first_gap = expected_index
            reachable = False
        try:
            entry = _entry(index, raw, default_format)
        except SerialDirectoryRefused as error:
            entry = {"serial_index": index, "alternatives": [], "refusals": [str(error)]}
            blocked["serial_row_shape"] += 1
        if not reachable:
            entry.setdefault("refusals", []).append("serial_unreachable_after_gap")
            blocked["serial_gap"] += 1
        for alternative in entry["alternatives"]:
            for reason in alternative["refusals"]:
                blocked[reason] += 1
        entries.append(entry)
        expected_index = index + 1

    if not entries:
        blocked["serial_empty_table"] += 1
    descriptor = {
        "version": DESCRIPTOR_VERSION,
        "kind": "native_serial_layout_inventory",
        "module": module,
        "table": table,
        "processor": processor,
        "table_facts": {
            "default_format": default_format,
            "groups": groups,
            "variables": variables,
            "documentary": {name: copy.deepcopy(meta[name]) for name in sorted(_TABLE_DOCUMENTARY_PROPERTIES) if name in meta},
            "unmodeled": {name: copy.deepcopy(meta[name]) for name in sorted(meta) if name not in _TABLE_MODELED_PROPERTIES and name not in _TABLE_DOCUMENTARY_PROPERTIES},
            "native_table_sha256": facts.canonical_json_sha256({"meta": meta, "tags": tags}),
        },
        "entries": entries,
        "gate_a": {"blocked_by": [[name, count] for name, count in sorted(blocked.items()) if count]},
        "runtime_status": "inventory_only_no_reader_or_route",
    }
    if first_gap is not None:
        descriptor["first_unreachable_serial_index"] = first_gap
    return descriptor


def _native_alternative_count(tags):
    """Count source alternatives without applying the descriptor grammar."""
    if not isinstance(tags, dict):
        raise SerialDirectoryRefused("serial table tags are not a dictionary")
    count = 0
    for raw in tags.values():
        variants = raw.get("_variants") if isinstance(raw, dict) else None
        count += len(variants) if isinstance(variants, list) else 1
    return count


def compile_serial_population(document):
    """Inventory every captured table that selects ``ProcessSerialData``.

    The population is intentionally based on the native table selector, never
    on generated descriptors.  Thus zero-row, malformed, and descriptor-
    refused tables remain in the report and cannot make the denominator look
    healthier by vanishing from it.
    """
    if not isinstance(document, dict) or not isinstance(document.get("modules"), dict):
        raise SerialDirectoryRefused("serial native document has no module dictionary")

    records = []
    for module, module_data in sorted(document["modules"].items()):
        if not isinstance(module, str) or not module or not isinstance(module_data, dict):
            raise SerialDirectoryRefused("serial native module facts are malformed")
        tables = module_data.get("tables")
        if not isinstance(tables, dict):
            raise SerialDirectoryRefused(f"serial native module {module} has no table dictionary")
        for table, table_data in sorted(tables.items()):
            if not isinstance(table, str) or not table:
                raise SerialDirectoryRefused("serial native table identity is malformed")
            if not is_serial_processor(table_data):
                continue
            tags = table_data.get("tags") if isinstance(table_data, dict) else None
            native_entries = len(tags) if isinstance(tags, dict) else None
            try:
                native_alternatives = _native_alternative_count(tags)
                descriptor = compile_serial_inventory(module, table, table_data)
            except SerialDirectoryRefused as error:
                records.append({
                    "module": module,
                    "table": table,
                    "native_entries": native_entries,
                    "outcome": "descriptor_refused",
                    "refusal": str(error),
                })
                continue
            records.append({
                "module": module,
                "table": table,
                "native_entries": native_entries,
                "native_alternatives": native_alternatives,
                "outcome": "descriptor_compiled",
                "descriptor": descriptor,
            })

    table_outcomes = Counter(record["outcome"] for record in records)
    gate_reasons = Counter()
    table_gate_reasons = Counter()
    compiled_entries = compiled_alternatives = empty_tables = ready_tables = table_blocked_tables = 0
    clear_alternatives = blocked_alternatives = 0
    for record in records:
        if record["native_entries"] == 0:
            empty_tables += 1
        if record["outcome"] != "descriptor_compiled":
            continue
        descriptor = record["descriptor"]
        compiled_entries += len(descriptor["entries"])
        compiled_alternatives += sum(len(entry["alternatives"]) for entry in descriptor["entries"])
        for entry in descriptor["entries"]:
            for alternative in entry["alternatives"]:
                if alternative["refusals"]:
                    blocked_alternatives += 1
                else:
                    clear_alternatives += 1
        for reason, count in descriptor["gate_a"]["blocked_by"]:
            if reason.startswith("serial_table_"):
                table_gate_reasons[reason] += count
            else:
                gate_reasons[reason] += count
        if table_gate_reasons and any(reason.startswith("serial_table_") for reason, _ in descriptor["gate_a"]["blocked_by"]):
            table_blocked_tables += 1
        if not descriptor["gate_a"]["blocked_by"]:
            ready_tables += 1

    native_entries = sum(record["native_entries"] or 0 for record in records)
    native_alternatives = sum(record.get("native_alternatives", 0) for record in records)
    return {
        "version": DESCRIPTOR_VERSION,
        "kind": "native_serial_layout_population",
        "selector": {"processor_suffix": f"::{PROCESS_SERIAL_DATA}"},
        "source_modules_sha256": facts.canonical_json_sha256(document["modules"]),
        "tables": records,
        "summary": {
            "selected_tables": len(records),
            "descriptor_compiled_tables": table_outcomes["descriptor_compiled"],
            "descriptor_refused_tables": table_outcomes["descriptor_refused"],
            "empty_selected_tables": empty_tables,
            "native_entries": native_entries,
            "native_alternatives": native_alternatives,
            "compiled_entries": compiled_entries,
            "compiled_alternatives": compiled_alternatives,
            "row_gate_clear_alternatives": clear_alternatives,
            "row_gate_refused_alternatives": blocked_alternatives,
            "row_gate_clear_tables": ready_tables,
            "row_gate_blocked_by": [[name, count] for name, count in sorted(gate_reasons.items())],
            "table_gate_blocked_tables": table_blocked_tables,
            "table_gate_blocked_by": [[name, count] for name, count in sorted(table_gate_reasons.items())],
        },
        "runtime_status": "inventory_only_no_reader_or_route",
    }


def descriptor_json(descriptor):
    """Stable, versioned JSON for a committed inventory artifact or verifier."""
    if not isinstance(descriptor, dict) or descriptor.get("version") != DESCRIPTOR_VERSION:
        raise SerialDirectoryRefused("serial descriptor version is unsupported")
    return json.dumps(descriptor, sort_keys=True, separators=(",", ":"), ensure_ascii=False) + "\n"


def population_json(population):
    """Stable JSON encoding for an all-source serial processor inventory."""
    if not isinstance(population, dict) or population.get("version") != DESCRIPTOR_VERSION:
        raise SerialDirectoryRefused("serial population version is unsupported")
    if population.get("kind") != "native_serial_layout_population":
        raise SerialDirectoryRefused("serial population kind is unsupported")
    return json.dumps(population, sort_keys=True, separators=(",", ":"), ensure_ascii=False) + "\n"


def stale_reason(existing, module, table, table_data):
    """Recompile source facts and identify stale or no-longer-supported state."""
    try:
        fresh = compile_serial_inventory(module, table, table_data)
    except SerialDirectoryRefused as error:
        return f"native serial source is no longer compilable: {error}"
    if descriptor_json(existing) != descriptor_json(fresh):
        return "native serial descriptor or source identity differs"
    return None


def _rust_fmt(format_operand):
    """Render a descriptor format as the shared ``Fmt`` literal.

    ``SerialCount`` carries the number of elements.  In particular,
    ``string[$val{N}]`` is a one-byte-stride string with a prior-raw count,
    while a bare field ``string`` has ProcessSerialData's remaining-bytes
    rule.  These are deliberately distinct from binary-table formatting.
    """
    if not isinstance(format_operand, dict):
        raise SerialDirectoryRefused("serial emitted Format fact is malformed")
    spelling = format_operand.get("format")
    count = format_operand.get("count")
    if not isinstance(spelling, str) or not isinstance(count, dict):
        raise SerialDirectoryRefused("serial emitted Format fact is incomplete")
    if spelling == "string":
        return "Fmt::RemainderString" if count.get("kind") == "remaining_bytes" else "Fmt::Str(1)"
    if spelling in {"undef", "binary"}:
        return "Fmt::Undef(1)"
    if spelling == "pstring":
        return "Fmt::PString"
    modeled = codegen.SCALAR_FORMATS.get(spelling)
    if modeled is None:
        raise SerialDirectoryRefused(f"serial emitted Format is unsupported: {spelling!r}")
    return f"Fmt::{modeled[0]}"


def _runtime_format_reason(format_operand):
    """Name formats which the staged serial reader has not modeled yet."""
    if not isinstance(format_operand, dict) or not isinstance(format_operand.get("format"), str):
        return "serial_emitter_format"
    return None if format_operand["format"] in _RUNTIME_FORMATS else "serial_emitter_format"


def _rust_count(count):
    """Render a closed descriptor count operand as ``SerialCount``."""
    if not isinstance(count, dict):
        raise SerialDirectoryRefused("serial emitted count fact is malformed")
    kind = count.get("kind")
    if kind == "fixed" and isinstance(count.get("value"), int) and count["value"] >= 0:
        return f"SerialCount::Fixed {{ value: {count['value']} }}"
    if kind == "prior_raw_value" and isinstance(count.get("serial_index"), int) and count["serial_index"] >= 0:
        return f"SerialCount::PriorRaw {{ serial_index: {count['serial_index']} }}"
    if kind == "floor_div_prior_raw_value":
        required = ("serial_index", "add", "divisor", "trailing_add")
        if (all(isinstance(count.get(name), int) for name in required)
                and count["serial_index"] >= 0 and count["add"] >= 0
                and count["divisor"] > 0 and count["trailing_add"] >= 0):
            return (
                "SerialCount::FloorDivPriorRaw { "
                f"serial_index: {count['serial_index']}, add: {count['add']}, "
                f"divisor: {count['divisor']}, trailing_add: {count['trailing_add']} }}"
            )
    if kind == "remaining_bytes" and set(count) == {"kind"}:
        return "SerialCount::RemainingBytes"
    raise SerialDirectoryRefused("serial emitted count is outside the closed descriptor grammar")


def _rust_flags(flags):
    if not isinstance(flags, dict) or any(not isinstance(flags.get(name), bool) for name in ("unknown", "binary", "list")):
        raise SerialDirectoryRefused("serial emitted flags are malformed")
    return codegen.ifd_flags_literal(flags["unknown"], flags["binary"], flags["list"], False, False, None)


def _rust_groups(groups):
    if not isinstance(groups, dict):
        raise SerialDirectoryRefused("serial emitted Groups fact is malformed")
    return codegen.compile_groups_field(groups, Counter())


def _rust_condition(condition):
    """Render a source-derived serial condition and its narrow absent policy."""
    if not isinstance(condition, dict):
        raise SerialDirectoryRefused("serial emitted Condition fact is malformed")
    if condition.get("kind") == "always":
        return "None"
    if condition.get("kind") == "shared_cond":
        compiled = condition.get("compiled")
        missing = condition.get("missing_member")
        if isinstance(compiled, str) and compiled and missing in {"shared_default", "empty_string_for_string_ops"}:
            variant = (
                "SerialMissingMember::SharedDefault"
                if missing == "shared_default"
                else "SerialMissingMember::EmptyStringForStringOps"
            )
            return f"Some(SerialCondition {{ cond: {compiled}, missing_member: {variant} }})"
    raise SerialDirectoryRefused("serial emitted Condition is outside the shared execution contract")


def _rust_print_conv(conversion):
    """Render the closed serial conversion data contract."""
    if conversion == {"kind": "none"}:
        return "SerialPrintConv::None"
    if isinstance(conversion, dict) and conversion.get("kind") == "shared_int_enum":
        pairs = conversion.get("map")
        if (isinstance(pairs, list) and all(isinstance(pair, list) and len(pair) == 2
                                           and isinstance(pair[0], int) and isinstance(pair[1], str)
                                           for pair in pairs)):
            ordered = [(pair[0], pair[1]) for pair in pairs]
            if ordered == sorted(ordered) and len({pair[0] for pair in ordered}) == len(ordered):
                return "SerialPrintConv::Shared(PrintConv::IntEnum(&[" + codegen._rust_pairs(ordered) + "]))"
    if (isinstance(conversion, dict) and conversion.get("kind") == "decode_bits_words"
            and conversion.get("bits_per_word") == 16 and set(conversion) == {"kind", "bits_per_word"}):
        return "SerialPrintConv::DecodeBitsWords { bits_per_word: 16 }"
    raise SerialDirectoryRefused("serial emitted PrintConv is outside the closed execution contract")


def _alternative_emission_reasons(alternative):
    """Return every reason an alternative cannot become a ``SerialTag``.

    Source-descriptor refusals are authoritative.  The additional condition
    check protects reports produced by an older descriptor that has not yet
    classified native missing-member-as-empty semantics.
    """
    if not isinstance(alternative, dict):
        return ["serial_row_shape"]
    reasons = alternative.get("refusals")
    if not isinstance(reasons, list) or any(not isinstance(reason, str) or not reason for reason in reasons):
        return ["serial_row_shape"]
    return sorted(set(reasons))


def _gate_src(reasons):
    if not reasons:
        return "&[]"
    return "&[" + ", ".join(f'(\"{codegen.rust_str(reason)}\", {count})' for reason, count in reasons) + "]"


def _table_default_module(module):
    """Return ExifTool's default group module for a captured table owner.

    ``GetTagTable`` matches the first component after ``Image::ExifTool`` in
    ``Image::ExifTool::<module>::<table>``.  A nested captured module is thus
    ``Fixture`` for ``Fixture::Nested``, not the entire owner spelling.
    """
    if not isinstance(module, str) or not module:
        raise SerialDirectoryRefused("serial table module is unavailable for native group defaults")
    first = module.split("::", 1)[0]
    if re.fullmatch(r"[A-Za-z_]\w*", first) is None:
        raise SerialDirectoryRefused("serial table module is malformed for native group defaults")
    return first


def _table_group(groups, family, module):
    """Resolve a table group with native ``GetTagTable`` defaults.

    ExifTool initializes a loaded table's false group 0 and 1 values from the
    table-owning module, and its false group 2 value to ``Other``.  The dump
    preserves the authored ``GROUPS`` hash before that initialization, so the
    staged literal must apply the same rule rather than treating an absent
    group 1 as an empty string.
    """
    value = groups.get(str(family)) if isinstance(groups, dict) else None
    # Perl's ``unless`` regards both the empty string and the string "0" as
    # false.  _table_meta already authenticates GROUPS values as strings.
    if isinstance(value, str) and value not in ("", "0"):
        return value
    return _table_default_module(module) if family in (0, 1) else "Other"


def _serial_tag_literal(alternative):
    """Render one source-cleared alternative; callers record every refusal."""
    if not isinstance(alternative, dict) or not isinstance(alternative.get("name"), str) or not alternative["name"]:
        raise SerialDirectoryRefused("serial emitted alternative has no Name")
    format_operand = alternative.get("format")
    fmt = _rust_fmt(format_operand)
    count = _rust_count(format_operand.get("count") if isinstance(format_operand, dict) else None)
    condition = _rust_condition(alternative.get("condition"))
    print_conv = _rust_print_conv(alternative.get("conversion"))
    raw_conv = alternative.get("raw_conv")
    if raw_conv == {"kind": "none"}:
        raw_conv_src = "None"
    elif (isinstance(raw_conv, dict) and raw_conv.get("kind") == "set_member"
          and isinstance(raw_conv.get("member"), str) and raw_conv["member"]):
        raw_conv_src = f'Some(RawConvEffect::SetMember {{ member: "{codegen.rust_str(raw_conv["member"])}" }})'
    else:
        raise SerialDirectoryRefused("serial emitted RawConv is outside the closed execution contract")
    return (
        "SerialTag { "
        f'name: "{codegen.rust_str(alternative["name"])}", '
        f"format: SerialFormat {{ format: {fmt}, count: {count} }}, condition: {condition}, "
        f"flags: {_rust_flags(alternative.get('flags'))}, raw_conv: {raw_conv_src}, omitted: Omitted::NONE, "
        f"value_conv: None, print_conv: {print_conv}, groups: {_rust_groups(alternative.get('groups'))} }}"
    )


def serial_rust_source(population):
    """Render a source-selected serial inventory as inactive shared literals.

    This consumes descriptors, never table/name allowlists.  Every selected
    table receives a literal when its descriptor compiled; table Gate A keeps
    partially modeled tables non-executable.  Every withheld source
    alternative is retained in ``OMITTED_SERIAL_NATIVE_ROWS``.
    """
    if not isinstance(population, dict) or population.get("version") != DESCRIPTOR_VERSION:
        raise SerialDirectoryRefused("serial emitted population version is unsupported")
    if population.get("kind") != "native_serial_layout_population" or not isinstance(population.get("tables"), list):
        raise SerialDirectoryRefused("serial emitted population shape is unsupported")

    tables, rows, omitted_tables = [], [], []
    for record in sorted(population["tables"], key=lambda item: (item.get("module", ""), item.get("table", ""))):
        module, table = record.get("module"), record.get("table")
        if not isinstance(module, str) or not module or not isinstance(table, str) or not table:
            raise SerialDirectoryRefused("serial emitted table identity is unavailable")
        if record.get("outcome") != "descriptor_compiled":
            omitted_tables.append((module, table, ["serial_descriptor_refused"]))
            continue
        descriptor = record.get("descriptor")
        if not isinstance(descriptor, dict) or descriptor.get("module") != module or descriptor.get("table") != table:
            raise SerialDirectoryRefused("serial emitted descriptor identity disagrees with population")
        facts_ = descriptor.get("table_facts")
        processor = descriptor.get("processor")
        entries = descriptor.get("entries")
        gate = descriptor.get("gate_a", {}).get("blocked_by")
        if not isinstance(facts_, dict) or not isinstance(processor, dict) or not isinstance(entries, list) or not isinstance(gate, list):
            raise SerialDirectoryRefused("serial emitted descriptor is incomplete")
        default_format = facts_.get("default_format")
        default_operand = {"format": default_format, "count": {"kind": "fixed", "value": 1}}
        if _runtime_format_reason(default_operand) is not None:
            omitted_tables.append((module, table, ["serial_emitter_default_format"]))
            continue
        default_fmt = _rust_fmt(default_operand)
        processor_name = processor.get("name")
        source_file = processor.get("source_file")
        source_sha = processor.get("source_sha256")
        body_sha = processor.get("source_body_sha256")
        if not all(isinstance(value, str) and value for value in (processor_name, source_file, source_sha, body_sha)):
            raise SerialDirectoryRefused("serial emitted processor provenance is incomplete")

        gate_counts = Counter()
        native_gate_names = set()
        for reason, count in gate:
            if not isinstance(reason, str) or not reason or not isinstance(count, int) or count < 1:
                raise SerialDirectoryRefused("serial emitted Gate A fact is malformed")
            gate_counts[reason] += count
            native_gate_names.add(reason)
        rendered_entries = []
        for entry in sorted(entries, key=lambda item: item.get("serial_index", -1)):
            index = entry.get("serial_index") if isinstance(entry, dict) else None
            alternatives = entry.get("alternatives") if isinstance(entry, dict) else None
            variant = entry.get("variant") if isinstance(entry, dict) else None
            if not isinstance(index, int) or index < 0 or not isinstance(alternatives, list) or not isinstance(variant, bool):
                raise SerialDirectoryRefused("serial emitted entry is malformed")
            rendered_alternatives = []
            for alternative_index, alternative in enumerate(alternatives):
                reasons = _alternative_emission_reasons(alternative)
                format_reason = _runtime_format_reason(
                    alternative.get("format") if isinstance(alternative, dict) else None
                )
                if format_reason is not None:
                    reasons = sorted(set(reasons + [format_reason]))
                if reasons:
                    for reason in reasons:
                        # Newer descriptors include this in Gate A.  Count it
                        # here only for backwards-compatible inputs.
                        if reason not in native_gate_names:
                            gate_counts[reason] += 1
                    name = alternative.get("name") if isinstance(alternative, dict) else None
                    name_src = "None" if not isinstance(name, str) else f'Some("{codegen.rust_str(name)}")'
                    rows.append((module, table, index, variant, alternative_index, name_src, reasons))
                    continue
                try:
                    rendered_alternatives.append(_serial_tag_literal(alternative))
                except SerialDirectoryRefused:
                    reason = "serial_emitter_shape"
                    gate_counts[reason] += 1
                    name = alternative.get("name") if isinstance(alternative, dict) else None
                    name_src = "None" if not isinstance(name, str) else f'Some("{codegen.rust_str(name)}")'
                    rows.append((module, table, index, variant, alternative_index, name_src, [reason]))
            if rendered_alternatives:
                rendered_entries.append(
                    f"SerialEntry {{ serial_index: {index}, alternatives: &[{', '.join(rendered_alternatives)}] }}"
                )
        groups = facts_.get("groups")
        tables.append(
            "pub static SERIAL_" + re.sub(r"[^A-Za-z0-9]", "_", f"{module}_{table}").upper() + ": SerialTable = SerialTable { "
            f'module: "{codegen.rust_str(module)}", table: "{codegen.rust_str(table)}", '
            f'group0: "{codegen.rust_str(_table_group(groups, 0, module))}", '
            f'group1: "{codegen.rust_str(_table_group(groups, 1, module))}", '
            f'group2: "{codegen.rust_str(_table_group(groups, 2, module))}", '
            f"default_format: {default_fmt}, processor: SerialProcessorFacts {{ name: \"{codegen.rust_str(processor_name)}\", source_file: \"{codegen.rust_str(source_file)}\", "
            f"source_sha256: \"{codegen.rust_str(source_sha)}\", source_body_sha256: \"{codegen.rust_str(body_sha)}\" }}, "
            f"gate_a: GateA {{ blocked_by: {_gate_src(sorted(gate_counts.items()))} }}, entries: &[{', '.join(rendered_entries)}] }};\n"
        )

    index = ["    &" + re.search(r"SERIAL_[A-Z0-9_]+", table).group(0) + "," for table in tables]
    row_src = []
    for module, table, index_value, variant, alternative, name, reasons in sorted(rows):
        reasons_src = ", ".join(f'"{codegen.rust_str(reason)}"' for reason in reasons)
        row_src.append(
            f'    OmittedSerialNativeRow {{ module: "{codegen.rust_str(module)}", table: "{codegen.rust_str(table)}", '
            f"serial_index: {index_value}, variant: {str(variant).lower()}, alternative: {alternative}, name: {name}, reasons: &[{reasons_src}] }},"
        )
    table_src = []
    for module, table, reasons in sorted(omitted_tables):
        reasons_src = ", ".join(f'"{codegen.rust_str(reason)}"' for reason in reasons)
        table_src.append(
            f'    OmittedSerialNativeTable {{ module: "{codegen.rust_str(module)}", table: "{codegen.rust_str(table)}", reasons: &[{reasons_src}] }},'
        )
    prelude = "//! Generated serial-directory facts; no reader or route is activated by this file.\nuse super::*;\n"
    return prelude + "".join(tables) + (
        "pub static ALL_SERIAL_TABLES: &[&SerialTable] = &[\n" + "\n".join(index) + "\n];\n"
        "pub static OMITTED_SERIAL_NATIVE_ROWS: &[OmittedSerialNativeRow] = &[\n" + "\n".join(row_src) + "\n];\n"
        "pub static OMITTED_SERIAL_NATIVE_TABLES: &[OmittedSerialNativeTable] = &[\n" + "\n".join(table_src) + "\n];\n"
    )


def main(argv=None):
    """Write the complete selected serial-processor population from one dump."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source_dump", help="captured dump_tables.pl JSON input")
    parser.add_argument("--output", required=True, help="destination JSON report")
    parser.add_argument("--rust-output", help="optional inactive shared serial Rust facts")
    args = parser.parse_args(argv)
    try:
        document = json.loads(open(args.source_dump, encoding="utf-8").read())
        population = compile_serial_population(document)
    except (OSError, json.JSONDecodeError, SerialDirectoryRefused) as error:
        parser.error(str(error))
    with open(args.output, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(population_json(population))
    if args.rust_output:
        with open(args.rust_output, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(serial_rust_source(population))


if __name__ == "__main__":
    main()
