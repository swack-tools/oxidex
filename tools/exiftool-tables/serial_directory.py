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

import conds
import serial_directory_facts as facts


DESCRIPTOR_VERSION = 1
PROCESS_SERIAL_DATA = "ProcessSerialData"


class SerialDirectoryRefused(ValueError):
    """The native serial table is outside this staged grammar."""


_SCALAR_FORMAT = re.compile(r"^[A-Za-z][A-Za-z0-9_]*$")
_SIZED_FORMAT = re.compile(r"^([A-Za-z][A-Za-z0-9_]*)\[(.*)\]$", re.S)
_PRIOR_VALUE = re.compile(r"^\$val\{([0-9]+)\}$")
_FLOOR_DIV_PRIOR = re.compile(r"^int\(\(\$val\{([0-9]+)\}\+([0-9]+)\)/([1-9][0-9]*)\)$")
_INTEGER = re.compile(r"^(0|[1-9][0-9]*)$")
_FQ = re.compile(r"(?:[A-Za-z_]\w*::)*[A-Za-z_]\w*$")

# These tokens occur in the native control path in the order in which their
# effects matter.  The inventory is deliberately inactive, but it will not
# describe an arbitrary CODE fact as serial processing merely because its name
# happens to end in ProcessSerialData.
_REQUIRED_PROCESSOR_STEPS = (
    ("temporary unknown option", "Options('Unknown',1)"),
    ("unknown generation suppression", "NO_UNKNOWN"),
    ("serial verbose callback", "VerboseDir('SerialData'"),
    ("table default format", "'FORMAT'"),
    ("serial GetTagInfo", "GetTagInfo"),
    ("dynamic count evaluation", "eval($count)"),
    ("remainder string", "eq'string'"),
    ("field length", "FormatSize($format)"),
    ("bounded read", "ReadValue"),
    ("raw value storage", "$val{$index}"),
    ("subdirectory branch", "SubDirectory"),
    ("tag reporting", "FoundTag"),
    ("serial cursor advance", "$pos+=$len"),
    ("unknown option restore", "Options('Unknown',$unknown)"),
    ("unknown generation cleanup", "delete$et->{'NO_UNKNOWN'}"),
)


def _collapse(source):
    """Normalize only whitespace outside quoted Perl literals for step scans."""
    if not isinstance(source, str):
        raise SerialDirectoryRefused("missing serial processor body")
    pieces, at = [], 0
    quoted = re.compile(r"'(?:\\.|[^'\\])*'|\"(?:\\.|[^\"\\])*\"")
    for match in quoted.finditer(source):
        pieces.append(re.sub(r"\s+", "", source[at:match.start()]))
        pieces.append(match.group())
        at = match.end()
    pieces.append(re.sub(r"\s+", "", source[at:]))
    return "".join(pieces)


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
    compact = _collapse(body)
    last = -1
    steps = []
    for effect, token in _REQUIRED_PROCESSOR_STEPS:
        position = compact.find(token, last + 1)
        if position < 0:
            raise SerialDirectoryRefused(f"serial processor lacks ordered {effect}")
        last = position
        steps.append(effect)
    # This source-only checkpoint accepts no extra callbacks or statements.
    # A future body may be supported after its changed semantics receive a
    # descriptor version and native proof; silently retaining it here would
    # turn a source inventory into an unreviewed interpreter.
    methods = Counter(re.findall(r"->([A-Za-z_]\w*)", compact))
    expected_methods = Counter({
        "Options": 3, "VerboseDir": 1, "GetTagInfo": 1, "VerboseInfo": 1,
        "ProcessDirectory": 1, "FoundTag": 1,
    })
    if (methods - expected_methods
            or any(methods[name] > count for name, count in expected_methods.items())
            or compact.count("warn(") > 1):
        raise SerialDirectoryRefused("serial processor has an unsupported callback or statement shape")
    if "$et->{'NO_UNKNOWN'}=1" not in compact:
        raise SerialDirectoryRefused("serial processor unknown suppression is not the authenticated assignment")
    # The native value is saved before the report/subdirectory branch.  This is
    # specifically what lets later Format count expressions consume raw values.
    if compact.find("$val{$index}") > compact.find("FoundTag"):
        raise SerialDirectoryRefused("serial processor stores converted/reporting value before raw state")
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
    regex_member = bool(re.search(r"\$\$self\{[^}]+\}\s*(?:=~|!~)\s*/", condition))
    return {
        "kind": "shared_cond",
        "source": condition,
        "compiled": compiled,
        "missing_member": "empty_string" if regex_member else "shared_default",
    }


def _conversion_operand(tag):
    """Record conversions; this checkpoint refuses unimplemented forms visibly."""
    raw = tag.get("PrintConv")
    if raw is None:
        return {"kind": "none"}, None
    if isinstance(raw, dict) and raw.get("kind") == "expr" and isinstance(raw.get("expr"), str):
        expression = raw["expr"]
        if expression == "Image::ExifTool::DecodeBits($val, undef, 16)":
            return {"kind": "native_expr", "expression": expression}, "serial_decode_bits_words"
        return {"kind": "native_expr", "expression": expression}, "serial_print_conv_expr"
    # A later descriptor version may reuse codegen's typed PrintConv result.
    # This inventory records the literal source rather than pretending a map is
    # already executable in a serial reader.
    return {"kind": "native_print_conv", "source": copy.deepcopy(raw)}, "serial_print_conv"


def _flags(tag):
    return {
        "unknown": bool(tag.get("Unknown")),
        "binary": bool(tag.get("Binary")),
        "list": bool(tag.get("List")),
    }


def _alternative(tag, default_format):
    if not isinstance(tag, dict):
        raise SerialDirectoryRefused("serial alternative is not a tag dictionary")
    name = tag.get("Name")
    if not isinstance(name, str) or not name:
        raise SerialDirectoryRefused("serial alternative has no literal Name")
    format_operand = _format_operand(tag, default_format)
    condition = _condition_operand(tag.get("Condition"))
    conversion, refusal = _conversion_operand(tag)
    reasons = [] if refusal is None else [refusal]
    if tag.get("RawConv") is not None:
        reasons.append("serial_raw_conv")
    if tag.get("ValueConv") is not None:
        reasons.append("serial_value_conv")
    if tag.get("SubDirectory") is not None:
        reasons.append("serial_subdirectory")
    return {
        "name": name,
        "format": format_operand,
        "condition": condition,
        "conversion": conversion,
        "groups": copy.deepcopy(tag.get("Groups") or {}),
        "flags": _flags(tag),
        "native": {
            "format": tag.get("Format"),
            "condition": tag.get("Condition"),
            "print_conv": copy.deepcopy(tag.get("PrintConv")),
        },
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
    return {"serial_index": index, "alternatives": compiled}


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
    return default_format, groups, variables


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
    default_format, groups, variables = _table_meta(meta)
    processor = _processor_contract(meta.get("PROCESS_PROC"))
    tags = table_data.get("tags")
    if not isinstance(tags, dict):
        raise SerialDirectoryRefused("serial table tags are not a dictionary")

    entries, blocked = [], Counter()
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
    compiled_entries = compiled_alternatives = empty_tables = ready_tables = 0
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
            gate_reasons[reason] += count
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


def main(argv=None):
    """Write the complete selected serial-processor population from one dump."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source_dump", help="captured dump_tables.pl JSON input")
    parser.add_argument("--output", required=True, help="destination JSON report")
    args = parser.parse_args(argv)
    try:
        document = json.loads(open(args.source_dump, encoding="utf-8").read())
        population = compile_serial_population(document)
    except (OSError, json.JSONDecodeError, SerialDirectoryRefused) as error:
        parser.error(str(error))
    with open(args.output, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(population_json(population))


if __name__ == "__main__":
    main()
