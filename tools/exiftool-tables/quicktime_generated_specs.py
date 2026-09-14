#!/usr/bin/env python3
"""Generate Rust ItemList specs consumed by the generic ItemList reader.

This ledger records source acceptance and refusals. Runtime connection and
observed reading remain separate evidence; declarations alone are not coverage.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

import quicktime_atom_tables as selector


ROOT = selector.ROOT
SNAPSHOT = ROOT / "tools/exiftool-tables/fixtures/quicktime_source_13_59.json"
LEDGER = ROOT / "tools/exiftool-tables/quicktime_generated_itemlist_ledger.json"
RUST = ROOT / "src/parsers/quicktime/generated_itemlist_specs.rs"
EXPECTED_PROCESSOR = {
    "__name": "Image::ExifTool::QuickTime::ProcessMOV",
    "__perl": "CODE",
    "__opaque": True,
}
EXPECTED_PROCESSOR_DEPARSE_SHA256 = "5ae906b19e81d6e0a1f7bbbe89522e570edb8fcf0be8bb9eaa81fb47d43c5005"
EXPECTED_TABLE_CONTRACT = {"FORMAT": "string", "GROUPS.0": "QuickTime", "GROUPS.1": "ItemList",
                           "LANG_INFO.__name": "Image::ExifTool::QuickTime::GetLangInfo"}
EXPECTED_READER_PROTOCOL = {
    "kind": "quicktime_itemlist_reader_protocol_v1",
    "string_encoding": {"1": "UTF8", "2": "UTF16", "3": "ShiftJIS", "4": "UTF8", "5": "UTF16"},
    "dependencies": {
        "quicktime_format": ("Image::ExifTool::QuickTime::QuickTimeFormat", "c29829e95ea7df45de2cde0527d51d953583c9c73b26c52931f4bb91b2223883"),
        "read_value": ("Image::ExifTool::ReadValue", "91213f64302774d00eb5fadd4835ca3fbcad3607c70eec98afd89d3423f0ad29"),
        "decode": ("Image::ExifTool::Decode", "8ce89a36fea0f6ce930188e0b11d1846c9fbdb4538ec1f64e51bb24c649bd0e5"),
        "charset_decompose": ("Image::ExifTool::Charset::Decompose", "f07e59a85a207c3895a3e39ec7aa06a1f682199f303e7dbe62217998d66b77e2"),
        "charset_recompose": ("Image::ExifTool::Charset::Recompose", "ea4fbd153500dbe5735c2de7350e79f6779b85918fa9d8b27251ba2043551cef"),
    },
}


def source_format(value):
    if value is None:
        return {"kind": "implicit"}
    if value == "string":
        return {"kind": "string"}
    width = int(value.removeprefix("int").removesuffix("u"))
    return {"kind": "unsigned", "width": width}


def processor_reason(document):
    try:
        processor = document["modules"]["QuickTime"]["tables"]["ItemList"]["meta"]["PROCESS_PROC"]
    except (KeyError, TypeError):
        return "missing_or_changed_processor_contract:PROCESS_PROC"
    if not isinstance(processor, dict):
        return "missing_or_changed_processor_contract:PROCESS_PROC"
    if any(processor.get(key) != value for key, value in EXPECTED_PROCESSOR.items()):
        return "missing_or_changed_processor_contract:PROCESS_PROC"
    body = processor.get("__deparse")
    if not isinstance(body, str) or hashlib.sha256(body.encode()).hexdigest() != EXPECTED_PROCESSOR_DEPARSE_SHA256:
        return "missing_or_changed_processor_contract:PROCESS_PROC"
    meta = document["modules"]["QuickTime"]["tables"]["ItemList"].get("meta", {})
    if (meta.get("FORMAT") != EXPECTED_TABLE_CONTRACT["FORMAT"]
            or meta.get("GROUPS", {}).get("0", "QuickTime") != EXPECTED_TABLE_CONTRACT["GROUPS.0"]
            or meta.get("GROUPS", {}).get("1") != EXPECTED_TABLE_CONTRACT["GROUPS.1"]
            or meta.get("LANG_INFO", {}).get("__name") != EXPECTED_TABLE_CONTRACT["LANG_INFO.__name"]):
        return "missing_or_changed_processor_contract:ItemList_metadata"
    return None


def reader_protocol_reason(document):
    protocol = document.get("quicktime_itemlist_reader_protocol")
    if not isinstance(protocol, dict):
        return "missing_or_changed_reader_protocol:protocol"
    if protocol.get("kind") != EXPECTED_READER_PROTOCOL["kind"]:
        return "missing_or_changed_reader_protocol:kind"
    if protocol.get("string_encoding") != EXPECTED_READER_PROTOCOL["string_encoding"]:
        return "missing_or_changed_reader_protocol:string_encoding"
    if protocol.get("charset_loaded") is not True:
        return "missing_or_changed_reader_protocol:charset_load"
    dependencies = protocol.get("dependencies")
    if not isinstance(dependencies, dict):
        return "missing_or_changed_reader_protocol:dependencies"
    for key, (name, body_sha256) in EXPECTED_READER_PROTOCOL["dependencies"].items():
        fact = dependencies.get(key)
        if not isinstance(fact, dict):
            return f"missing_or_changed_reader_protocol:{key}"
        if (fact.get("__perl") != "CODE" or fact.get("__opaque") is not True
                or fact.get("__name") != name or fact.get("resolved") is not True):
            return f"missing_or_changed_reader_protocol:{key}"
        body = fact.get("__deparse")
        if not isinstance(body, str) or hashlib.sha256(body.encode()).hexdigest() != body_sha256:
            return f"missing_or_changed_reader_protocol:{key}"
    return None


def compile_document(document):
    pin = (ROOT / ".exiftool-version").read_text().strip()
    if document.get("exiftool_version") != pin:
        raise ValueError("fresh source dump differs from repository pin")
    # Do not call selector.report(): its capture provenance assertions belong to
    # the bounded baseline snapshot, whereas regeneration consumes a full fresh
    # hydrated dump. inventory() is the shared pure source-row selection.
    base = selector.inventory(document)
    blocked_protocol = processor_reason(document)
    blocked_reader_protocol = reader_protocol_reason(document)
    specs = []
    ledger = []
    for family in base["families"]:
        for record in family["records"]:
            reasons = list(record["reasons"])
            if family["table"] == "ItemList" and blocked_protocol:
                reasons.append(blocked_protocol)
            if family["table"] == "ItemList" and blocked_reader_protocol:
                reasons.append(blocked_reader_protocol)
            reasons = sorted(set(reasons))
            generated = family["table"] == "ItemList" and not reasons
            entry = {"identity": record["identity"], "generated": generated, "reasons": reasons}
            if generated:
                spec = record["spec"]
                operand = {
                    "raw_fourcc": spec["key_hex"],
                    "name": spec["name"],
                    "group": spec["group"],
                    "source_format": source_format(spec["format"]),
                    "safe_enum_operands": [{"raw": raw, "rendered": rendered}
                                           for raw, rendered in (spec["print_enum"] or {}).items()],
                    "source_identity": record["identity"],
                }
                specs.append(operand)
                entry["generated_spec_sha256"] = hashlib.sha256(
                    json.dumps(operand, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
                ).hexdigest()
            ledger.append(entry)
    specs.sort(key=lambda row: (row["raw_fourcc"], row["name"]))
    ledger.sort(key=lambda row: (row["identity"]["table"], row["identity"]["raw_key"],
                                 row["identity"]["variant_path"]))
    return {
        "schema": "quicktime_generated_itemlist_specs_v1",
        "scope": "generated declarations only; no runtime connection or observed support claimed",
        "source": {"exiftool_version": base["exiftool_version"],
                   "table_sha256": {family["table"]: family["source_table_sha256"]
                                    for family in base["families"]}},
        "protocol": {"table": "ItemList", "processor_contract": {
            **EXPECTED_PROCESSOR, "__deparse_sha256": EXPECTED_PROCESSOR_DEPARSE_SHA256,
            **EXPECTED_TABLE_CONTRACT},
                     "processor_provenance": {
                         "source_file": document["modules"]["QuickTime"]["tables"]["ItemList"]["meta"]["PROCESS_PROC"].get("source_file"),
                         "source_sha256": document["modules"]["QuickTime"]["tables"]["ItemList"]["meta"]["PROCESS_PROC"].get("source_sha256")},
                     "reader_protocol": document.get("quicktime_itemlist_reader_protocol"),
                     "eligible": blocked_protocol is None and blocked_reader_protocol is None,
                     "reason": blocked_protocol or blocked_reader_protocol},
        "specs": specs,
        "ledger": ledger,
        "identity_counts": {"source_records": len(ledger), "generated": len(specs),
                            "omitted": len(ledger) - len(specs)},
    }


def rust_string(value):
    escaped = []
    for char in value:
        if char == "\\":
            escaped.append("\\\\")
        elif char == '"':
            escaped.append('\\"')
        elif char == "\n":
            escaped.append("\\n")
        elif char == "\r":
            escaped.append("\\r")
        elif char == "\t":
            escaped.append("\\t")
        elif ord(char) < 0x20 or ord(char) == 0x7f:
            escaped.append(f"\\u{{{ord(char):x}}}")
        else:
            escaped.append(char)
    return '"' + "".join(escaped) + '"'


def render_format(value):
    if value["kind"] == "implicit":
        return "SourceFormat::Implicit"
    if value["kind"] == "string":
        return "SourceFormat::String"
    return f"SourceFormat::Unsigned({value['width']})"


def render_rust(result):
    lines = [
        "// @generated by tools/exiftool-tables/quicktime_generated_specs.py; DO NOT EDIT.",
        "// Source specs consumed by itemlist_reader.rs; observations are recorded separately.",
        "",
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]",
        "pub(crate) enum SourceFormat {",
        "    Implicit,",
        "    String,",
        "    Unsigned(u8),",
        "}",
        "",
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]",
        "pub(crate) struct EnumOperand {",
        "    pub raw: &'static str,",
        "    pub rendered: &'static str,",
        "}",
        "",
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]",
        "pub(crate) struct ItemListSpec {",
        "    pub raw_fourcc: [u8; 4],",
        "    pub name: &'static str,",
        "    pub group: &'static str,",
        "    pub source_format: SourceFormat,",
        "    pub safe_enum_operands: &'static [EnumOperand],",
        "}",
        "",
        "#[rustfmt::skip]",
        "pub(crate) static ITEMLIST_SPECS: &[ItemListSpec] = &[",
    ]
    for row in result["specs"]:
        raw = ", ".join(f"0x{row['raw_fourcc'][index:index + 2]}" for index in range(0, 8, 2))
        enums = ", ".join("EnumOperand { raw: %s, rendered: %s }" %
                          (rust_string(item["raw"]), rust_string(item["rendered"]))
                          for item in row["safe_enum_operands"])
        lines.append("    ItemListSpec { raw_fourcc: [%s], name: %s, group: %s, source_format: %s, safe_enum_operands: &[%s] }," %
                     (raw, rust_string(row["name"]), rust_string(row["group"]),
                      render_format(row["source_format"]), enums))
    lines.extend([
        "];",
        "",
    ])
    return "\n".join(lines)


def serialized(result):
    return json.dumps(result, sort_keys=True, indent=2, ensure_ascii=False) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dump", type=Path, default=SNAPSHOT,
                        help="full fresh dump for regeneration, or bounded fixture for checks")
    parser.add_argument("--ledger", type=Path, default=LEDGER)
    parser.add_argument("--rust", type=Path, default=RUST)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--replace", action="store_true")
    args = parser.parse_args()
    if args.check and args.replace:
        parser.error("--check and --replace are mutually exclusive")
    try:
        selector.validate_outputs(args.dump, [args.ledger, args.rust],
                                  replace=args.replace, check=args.check)
    except ValueError as exc:
        parser.error(str(exc))
    state = selector.instrument.git_state(ROOT)
    overridden = selector.instrument.refuse_if_dirty(state, "quicktime_generated_specs.py")
    selector.instrument.print_header(
        tool="quicktime_generated_specs.py", git=state, dirty_overridden=overridden,
        extra=["scope: generated declarations only; no runtime support claim", f"source: {args.dump}"])
    result = compile_document(json.loads(args.dump.read_text()))
    outputs = [(args.ledger, serialized(result)), (args.rust, render_rust(result))]
    if args.check:
        for path, contents in outputs:
            if not path.is_file() or path.read_text() != contents:
                parser.error(f"stale generated ItemList artifact: {path}")
    else:
        for path, contents in outputs:
            if path.exists() and not args.replace:
                parser.error(f"output exists: {path}; use --replace")
            with path.open("w" if args.replace else "x") as output:
                output.write(contents)


if __name__ == "__main__":
    main()
