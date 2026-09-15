#!/usr/bin/env python3
"""Generate Garmin FIT message and field specs for the generic FIT executor.

`Image::ExifTool::Garmin::ProcessFIT` walks self-describing records: each
definition message names a global message number (a `Garmin::FIT` edge to a
field table) and, per field, a FIT base type that fixes the value's format at
run time. This generator turns the hydrated dump into Rust data for one
handwritten executor (`src/parsers/specialized/fit.rs`):

* the message map (number, name, `Unknown` flag, field table);
* every field row of every dumped Garmin table, in source order of key, with
  its conversions compiled by the shared expression compiler (`exprs.py`) and
  approved by the oracle PASS ledger. Each conversion carries the domain it
  was compiled for; the executor applies it only to a value of that domain;
* the base-type table captured from ProcessFIT's live `%baseType` pad.

Every source row is emitted, accepted or not. A refused row carries its
reason and the executor withholds it, so a refused row can never fall
through to the `Common` table or be reported raw. The companion ledger
(`garmin_fit_ledger.json`) records acceptance, exact refusal reasons and the
run-time connection of each row. Declarations are not observed reading; see
`verify_garmin_fit_reader.py` for native comparisons.

The executor's control flow is written against one reviewed ProcessFIT body
(docs/reference/garmin-fit-source-review.md). A changed body, a changed value
reader or an unresolved base-type capture refuses the whole protocol rather
than letting new Perl run under old semantics.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

import exprs


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = "garmin_fit_generated_specs_v1"
MODULE = "Garmin"
FIT_TABLE = "FIT"
COMMON_TABLE = "Common"
DEV_TABLE = "Dev"
REVIEW = "docs/reference/garmin-fit-source-review.md"

# The executor reproduces exactly this ProcessFIT body; any other body is a
# protocol the executor has not been reviewed against.
REVIEWED_PROCESS_FIT_DEPARSE_SHA256 = frozenset({
    "0bc600c4c1907a98068ec5983437b5edb6ca7d3bb0f2713342a91d84bb3663a8",
})
# Value readers ProcessFIT reaches through ReadValue. The same two reviewed
# ReadValue deparse forms the QuickTime reader accepts (bounded and hydrated
# captures differ only in declaration spelling).
REVIEWED_DEPENDENCIES = {
    "Image::ExifTool::ReadValue": frozenset({
        "91213f64302774d00eb5fadd4835ca3fbcad3607c70eec98afd89d3423f0ad29",
        "226a9d703536d68c9b036bb4f122398d5a6ac95ef6a146fcaaa3420ccf65eedc",
    }),
    "Image::ExifTool::GetFloat": frozenset({
        "79f0291e5979141dfe831b895f4b3f55ef3b60ef41a3705891c0e49fb264c5c4",
    }),
    "Image::ExifTool::GetDouble": frozenset({
        "a9e556f84fa093a53ddd4be7058af8136d3269e64c8e243f26225afa93bc42e6",
    }),
}

# ExifTool format names the executor can decode, with their byte width. This
# is the generic format vocabulary of ReadValue, not a list of FIT types: a
# base type appears in the generated table only because the source pad
# declares it, and one whose format falls outside this set is refused.
DECODABLE_FORMATS = {
    "int8u": 1, "int8s": 1, "int16u": 2, "int16s": 2, "int32u": 4, "int32s": 4,
    "int64u": 8, "int64s": 8, "float": 4, "double": 8, "string": 1, "undef": 1,
}
SIXTY_FOUR_BIT = frozenset({"int64u", "int64s"})

# Row keys that affect FIT extraction and are modeled, and keys that only
# document the tag (TagNames HTML) and have no reading effect.
MODELED_ROW_KEYS = frozenset({"Name", "Groups", "RawConv", "ValueConv", "PrintConv"})
DOCUMENTATION_ROW_KEYS = frozenset({"Notes", "SeparateTable", "PrintConvColumns", "_shorthand"})
# `IsTimeStamp` survives the table projection only as a presence marker. The
# protocol fact captures its values; under the reviewed ProcessFIT body it
# can set the message timestamp slot only when field 253 is absent, and such
# a slot is never read (Garmin.pm 6389-6394, 6460).
INERT_EXTRA_KEYS = frozenset({"IsTimeStamp"})
TABLE_META_KEYS = frozenset({"GROUPS", "VARS", "NOTES", "TAG_PREFIX"})
DOMAINS = {"num": "ConvDomain::Num", "str": "ConvDomain::Str", "bytes": "ConvDomain::Bytes"}


def sha256_json(value) -> str:
    return hashlib.sha256(
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    ).hexdigest()


def body_sha256(fact) -> str | None:
    if not isinstance(fact, dict) or fact.get("resolved") is not True:
        return None
    body = fact.get("__deparse")
    return hashlib.sha256(body.encode()).hexdigest() if isinstance(body, str) else None


def protocol_reasons(protocol) -> list[str]:
    """Reasons the reviewed executor may not run over this source at all."""
    if not isinstance(protocol, dict) or protocol.get("kind") != "garmin_fit_reader_protocol_v1":
        return ["missing_or_changed_reader_protocol:kind"]
    reasons = []
    if body_sha256(protocol.get("process_fit")) not in REVIEWED_PROCESS_FIT_DEPARSE_SHA256:
        reasons.append("missing_or_changed_reader_protocol:ProcessFIT")
    dependencies = protocol.get("dependencies") or {}
    for name, accepted in sorted(REVIEWED_DEPENDENCIES.items()):
        if body_sha256(dependencies.get(name)) not in accepted:
            reasons.append(f"missing_or_changed_reader_protocol:{name}")
    base_types = protocol.get("base_types")
    if not isinstance(base_types, dict) or base_types.get("resolved") is not True:
        reasons.append("missing_or_changed_reader_protocol:base_types")
    return reasons


def base_type_rows(protocol, protocol_admitted: bool) -> list[dict]:
    rows = []
    entries = (protocol.get("base_types") or {}).get("entries") or {}
    sizes = protocol.get("format_sizes") or {}
    integer = protocol.get("perl_integer") or {}
    wide_ints = integer.get("ivsize") == 8 and integer.get("uvsize") == 8
    for key, entry in sorted(entries.items(), key=lambda item: int(item[0])):
        fmt = entry.get("format")
        reasons = []
        if not protocol_admitted:
            reasons.append("protocol_refused")
        if fmt not in DECODABLE_FORMATS:
            reasons.append(f"undecodable_format:{fmt}")
        elif sizes.get(fmt) != DECODABLE_FORMATS[fmt]:
            reasons.append(f"format_size_mismatch:{fmt}")
        if fmt in SIXTY_FOUR_BIT and not wide_ints:
            # Get64u/Get64s return `$hi * 2**32 + $lo`; only a 64-bit IV/UV
            # Perl keeps that sum an exact integer.
            reasons.append("perl_integer_width_not_64")
        rows.append({
            "id": int(key), "format": fmt, "fit_name": entry.get("fit_name"),
            "invalid": entry.get("invalid"), "admitted": not reasons, "reasons": reasons,
        })
    return rows


def typed_conversion(conv, verified_exprs, kind):
    """-> ((expr_text, domain) | None, reason | None) for one expression conversion."""
    if not isinstance(conv, dict) or conv.get("kind") != "expr":
        return None, f"{kind}_not_expression"
    raw = conv.get("expr")
    compiled = exprs.translate_or_compile_any(raw)
    if not compiled:
        return None, f"{kind}_uncompiled"
    domain = compiled[0]
    if domain not in DOMAINS:
        return None, f"{kind}_domain_unsupported:{domain}"
    if verified_exprs is None or exprs.normalize(raw or "") not in verified_exprs:
        return None, f"{kind}_not_oracle_verified"
    return (raw, domain), None


def compile_row(row, verified_exprs):
    """-> (spec, reasons) for one field row of a message table."""
    import codegen  # lazy: codegen imports this module while compiling

    if not isinstance(row, dict):
        return None, ["conditional_variants_unsupported"]
    reasons = []
    for key in sorted(row):
        if key == "_extra_keys":
            for extra in row[key]:
                if extra not in INERT_EXTRA_KEYS:
                    reasons.append(f"uncaptured_source_property:{extra}")
        elif key not in MODELED_ROW_KEYS and key not in DOCUMENTATION_ROW_KEYS:
            reasons.append(f"unsupported_source_property:{key}")
    name = row.get("Name")
    if not isinstance(name, str) or not name:
        reasons.append("missing_name")
    groups = row.get("Groups") or {}
    if not isinstance(groups, dict) or set(groups) - {"2"}:
        reasons.append("group_override_unsupported")
    spec = {"name": name, "group2": groups.get("2") if isinstance(groups, dict) else None,
            "raw_conv": None, "value_conv": None, "print_conv": {"kind": "none"}}
    for key, kind in (("RawConv", "raw_conv"), ("ValueConv", "value_conv")):
        if row.get(key) is not None:
            conv, reason = typed_conversion(row[key], verified_exprs, kind)
            spec[kind] = conv and {"expr": conv[0], "domain": conv[1]}
            if reason:
                reasons.append(reason)
    print_conv = row.get("PrintConv")
    if isinstance(print_conv, dict) and print_conv.get("kind") == "expr":
        conv, reason = typed_conversion(print_conv, verified_exprs, "print_conv")
        if conv:
            spec["print_conv"] = {"kind": "typed", "expr": conv[0], "domain": conv[1]}
        else:
            reasons.append(reason)
    elif print_conv is not None:
        # Hash conversions do not depend on the input domain; reuse the
        # shared table compiler so FIT renders exactly as the binary/IFD
        # tables do (`runtime::render`, including `Unknown ($val)`).
        stats = codegen.new_ifd_stats()
        rust, refused = codegen.conv_for({"PrintConv": print_conv}, stats, None, verified_exprs)
        if refused or rust == "PrintConv::None":
            dropped = sorted(key for key, value in stats.items()
                             if isinstance(value, int) and value and key != "enum_empty")
            reasons.append("print_conv_table_refused" + (":" + ",".join(dropped) if dropped else ""))
        else:
            spec["print_conv"] = {"kind": "table", "rust": rust}
    return spec, sorted(set(reasons))


def table_groups(meta, module_default):
    groups = meta.get("GROUPS") or {}
    return (groups.get("0") or module_default, groups.get("1") or module_default,
            groups.get("2") or "Other")


def compile_document(document, verified_exprs):
    pin = (ROOT / ".exiftool-version").read_text().strip()
    if str(document.get("exiftool_version")) != pin:
        raise ValueError("source dump differs from repository pin")
    module = (document.get("modules") or {}).get(MODULE)
    if not isinstance(module, dict) or FIT_TABLE not in (module.get("tables") or {}):
        raise ValueError("source dump has no Garmin::FIT table")
    tables = module["tables"]
    protocol = document.get("garmin_fit_reader_protocol")
    blocked = protocol_reasons(protocol)
    # Every declared base type enters ProcessFIT's field list and record
    # size, so one the executor cannot size makes every field list wrong.
    if any(reason.startswith(("undecodable_format:", "format_size_mismatch:"))
           for row in base_type_rows(protocol or {}, True) for reason in row["reasons"]):
        blocked.append("missing_or_changed_reader_protocol:base_type_format")
    admitted = not blocked
    base_types = base_type_rows(protocol or {}, admitted)

    fit = tables[FIT_TABLE]
    header_groups = list(table_groups(fit.get("meta") or {}, MODULE))
    rows, messages = [], []
    edges = {}
    for key, edge in fit.get("tags", {}).items():
        identity = {"table": f"Image::ExifTool::{MODULE}::{FIT_TABLE}", "raw_key": key, "variant_index": 0}
        reasons, connection = list(blocked), None
        if key == "vers":
            if not (isinstance(edge, dict) and edge.get("Name") == "ProtocolVersion"
                    and set(edge) <= {"Name", "Notes"}):
                reasons.append("changed_header_row")
            connection = "protocol_header"
        elif key == COMMON_TABLE:
            sub = edge.get("SubDirectory") if isinstance(edge, dict) else None
            if not (isinstance(sub, dict) and sub == {"TagTable": f"Image::ExifTool::{MODULE}::{COMMON_TABLE}"}):
                reasons.append("changed_common_edge")
            connection = "common_edge"
        elif not key.isdigit() or int(key) > 0xFFFF:
            reasons.append("message_key_outside_u16_protocol")
        else:
            sub = edge.get("SubDirectory") if isinstance(edge, dict) else None
            extra = sorted(set(edge) - {"Name", "SubDirectory", "Unknown"}) if isinstance(edge, dict) else ["shape"]
            name = edge.get("Name") if isinstance(edge, dict) else None
            table = None
            if extra:
                reasons.extend(f"unsupported_edge_property:{prop}" for prop in extra)
            if not isinstance(name, str) or not name:
                reasons.append("missing_name")
            if sub is None:
                # No table: ProcessFIT builds `Image::ExifTool::Garmin::<Name>`
                # with GROUPS {Garmin, <Name>, Unknown} and no fields
                # (Garmin.pm 6364-6375). Refuse if that would overwrite a
                # dumped table of the same name.
                if name in tables:
                    reasons.append("synthesized_table_name_collision")
                connection = "message_edge_synthesized_table"
            else:
                target = sub.get("TagTable") if isinstance(sub, dict) else None
                table = target.rsplit("::", 1)[-1] if isinstance(target, str) else None
                if not isinstance(sub, dict) or set(sub) != {"TagTable"}:
                    reasons.append("unsupported_subdirectory_shape")
                if not (isinstance(target, str) and target == f"Image::ExifTool::{MODULE}::{table}"
                        and table in tables):
                    reasons.append("message_table_missing")
                connection = "message_edge"
            unknown = isinstance(edge, dict) and str(edge.get("Unknown", "")) not in ("", "0")
            if not reasons:
                if table is not None:
                    edges[table] = {"num": int(key), "unknown": unknown}
                messages.append({"num": int(key), "name": name, "table": table, "unknown": unknown})
        rows.append({"identity": identity, "name": edge.get("Name") if isinstance(edge, dict) else None,
                     "generated": not reasons, "reasons": sorted(set(reasons)),
                     "runtime_connection": connection})

    common = tables.get(COMMON_TABLE) or {}
    common_keys = {int(key) for key in common.get("tags", {}) if key.isdigit()}
    rendered_tables = {}
    for table_name in sorted(tables):
        if table_name == FIT_TABLE:
            continue
        table = tables[table_name]
        meta = table.get("meta") or {}
        meta_reasons = [f"unsupported_table_property:{key}" for key in sorted(set(meta) - TABLE_META_KEYS)]
        if table_name == COMMON_TABLE:
            connection = "default_mode_common"
        elif table_name == DEV_TABLE:
            connection = "developer_fields_require_unknown_option"
        elif table_name in edges:
            connection = ("unknown_option_not_exposed" if edges[table_name]["unknown"]
                          else "default_mode_field_list")
        else:
            connection = "unreachable_from_fit_edges"
        g0, g1, g2 = table_groups(meta, MODULE)
        fields = []
        for key, row in sorted(table.get("tags", {}).items(),
                               key=lambda item: (not item[0].isdigit(), int(item[0]) if item[0].isdigit() else 0, item[0])):
            identity = {"table": f"Image::ExifTool::{MODULE}::{table_name}", "raw_key": key, "variant_index": 0}
            spec, reasons = compile_row(row, verified_exprs)
            reasons = list(blocked) + meta_reasons + reasons
            if not key.isdigit() or int(key) > 0xFF:
                # ProcessFIT reads field numbers as one byte (`unpack "C3"`).
                reasons.append("field_key_outside_u8_protocol")
            elif table_name != COMMON_TABLE and int(key) in common_keys:
                reasons.append("shadows_common_field")
            reasons = sorted(set(reasons))
            entry = {"identity": identity, "name": spec.get("name") if spec else None,
                     "generated": not reasons, "reasons": reasons,
                     "runtime_connection": connection}
            if spec and key.isdigit() and int(key) <= 0xFF:
                entry["spec_sha256"] = sha256_json(spec)
                fields.append({"num": int(key), "spec": spec, "reasons": reasons})
            rows.append(entry)
        rendered_tables[table_name] = {"groups": [g0, g1, g2], "fields": fields,
                                       "connection": connection}

    messages.sort(key=lambda row: row["num"])
    rows.sort(key=lambda row: (row["identity"]["table"], row["identity"]["raw_key"]))
    generated = [row for row in rows if row["generated"]]
    connection_counts = {}
    for row in rows:
        bucket = connection_counts.setdefault(row["runtime_connection"] or "none", {"generated": 0, "refused": 0})
        bucket["generated" if row["generated"] else "refused"] += 1
    reason_counts = {}
    for row in rows:
        for reason in row["reasons"]:
            reason_counts[reason] = reason_counts.get(reason, 0) + 1
    table_digests = {name: sha256_json(table) for name, table in sorted(tables.items())}
    return {
        "schema": SCHEMA,
        "scope": ("generated FIT declarations and their runtime connection; "
                  "observed reading is recorded separately by verify_garmin_fit_reader.py"),
        "review": REVIEW,
        "source": {"exiftool_version": str(document.get("exiftool_version")),
                   "table_sha256": table_digests},
        "protocol": {
            "kind": (protocol or {}).get("kind"),
            "admitted": admitted,
            "reasons": blocked,
            "process_fit_deparse_sha256": body_sha256((protocol or {}).get("process_fit")),
            "process_fit_source_file": ((protocol or {}).get("process_fit") or {}).get("source_file"),
            "process_fit_source_sha256": ((protocol or {}).get("process_fit") or {}).get("source_sha256"),
            "dependency_deparse_sha256": {
                name: body_sha256(((protocol or {}).get("dependencies") or {}).get(name))
                for name in sorted(REVIEWED_DEPENDENCIES)},
            "perl_integer": (protocol or {}).get("perl_integer"),
            "is_timestamp": (protocol or {}).get("is_timestamp"),
            "options": {"Unknown": "not_exposed", "ExtractEmbedded": "not_exposed",
                        "reachable_mode": "default"},
            "header_groups": header_groups,
        },
        "base_types": base_types,
        "messages": messages,
        "tables": rendered_tables,
        "rows": rows,
        "counts": {
            "source_rows": len(rows),
            "generated": len(generated),
            "refused": len(rows) - len(generated),
            "by_runtime_connection": dict(sorted(connection_counts.items())),
            "refusal_reasons": dict(sorted(reason_counts.items())),
        },
    }


def rust_str(value: str) -> str:
    """A quoted Rust string literal (`codegen.rust_str` escapes the body only)."""
    import codegen
    return f'"{codegen.rust_str(value)}"'


def rust_ident(table: str) -> str:
    out = []
    for index, char in enumerate(table):
        if char.isupper() and index and (table[index - 1].islower() or table[index - 1].isdigit()):
            out.append("_")
        out.append(char.upper() if char.isalnum() else "_")
    return "FIT_TABLE_" + "".join(out)


def render_conv(conv):
    import codegen
    if conv is None:
        return "None"
    return (f"Some(TypedConv {{ expr: ExprId::{codegen.expr_ident(conv['expr'])}, "
            f"domain: {DOMAINS[conv['domain']]} }})")


def render_print_conv(print_conv):
    import codegen
    if print_conv["kind"] == "none":
        return "FitPrintConv::None"
    if print_conv["kind"] == "table":
        return f"FitPrintConv::Table({print_conv['rust']})"
    return (f"FitPrintConv::Typed(TypedConv {{ expr: ExprId::{codegen.expr_ident(print_conv['expr'])}, "
            f"domain: {DOMAINS[print_conv['domain']]} }})")


FORMATS = {
    "int8u": "FitFormat::Int8u", "int8s": "FitFormat::Int8s", "int16u": "FitFormat::Int16u",
    "int16s": "FitFormat::Int16s", "int32u": "FitFormat::Int32u", "int32s": "FitFormat::Int32s",
    "int64u": "FitFormat::Int64u", "int64s": "FitFormat::Int64s", "float": "FitFormat::Float",
    "double": "FitFormat::Double", "string": "FitFormat::String", "undef": "FitFormat::Undef",
}


def render_rust(result) -> str:
    lines = [
        "// @generated by tools/exiftool-tables/garmin_fit_specs.py; DO NOT EDIT.",
        "//! Garmin FIT message and field specs from the pinned hydrated dump.",
        "//!",
        "//! Consumed by `parsers::specialized::fit`. Declarations only: the",
        "//! ledger (`tools/exiftool-tables/garmin_fit_ledger.json`) records",
        "//! acceptance and refusals, and native comparisons record observations.",
        "",
        "use super::fit_schema::{FitBaseType, FitField, FitFormat, FitMessage, FitPrintConv, FitProtocol, FitTable};",
        "use super::runtime::{ConvDomain, TypedConv};",
        "#[allow(unused_imports)]",
        "use super::{ExprId, PrintConv};",
        "",
    ]
    protocol = result["protocol"]
    reason = "; ".join(protocol["reasons"])
    for name, table in sorted(result["tables"].items()):
        g0, g1, g2 = table["groups"]
        lines.append("#[rustfmt::skip]")
        lines.append(f"static {rust_ident(name)}_FIELDS: &[FitField] = &[")
        for field in table["fields"]:
            spec = field["spec"]
            withheld = "None" if not field["reasons"] else f"Some({rust_str('; '.join(field['reasons']))})"
            if field["reasons"]:
                # A refused row keeps its identity (so lookup never falls
                # through to Common) but carries no executable conversion.
                raw_conv = value_conv = "None"
                print_conv = "FitPrintConv::None"
            else:
                raw_conv = render_conv(spec["raw_conv"])
                value_conv = render_conv(spec["value_conv"])
                print_conv = render_print_conv(spec["print_conv"])
            group2 = "None" if spec["group2"] is None else f"Some({rust_str(spec['group2'])})"
            name_src = rust_str(spec["name"] or "")
            lines.append(
                f"    FitField {{ num: {field['num']}, name: {name_src}, group2: {group2}, "
                f"raw_conv: {raw_conv}, value_conv: {value_conv}, print_conv: {print_conv}, "
                f"withheld: {withheld} }},")
        lines.append("];")
        lines.append("#[rustfmt::skip]")
        lines.append(
            f"pub(crate) static {rust_ident(name)}: FitTable = FitTable {{ table: {rust_str(name)}, "
            f"group0: {rust_str(g0)}, group1: {rust_str(g1)}, group2: {rust_str(g2)}, "
            f"fields: {rust_ident(name)}_FIELDS }};")
        lines.append("")
    lines.append("#[rustfmt::skip]")
    lines.append("static FIT_MESSAGES: &[FitMessage] = &[")
    for message in result["messages"]:
        table = "None" if message["table"] is None else f"Some(&{rust_ident(message['table'])})"
        lines.append(f"    FitMessage {{ num: {message['num']}, name: {rust_str(message['name'])}, "
                     f"unknown: {str(message['unknown']).lower()}, table: {table} }},")
    lines.append("];")
    lines.append("")
    lines.append("#[rustfmt::skip]")
    lines.append("static FIT_BASE_TYPES: &[FitBaseType] = &[")
    for base in result["base_types"]:
        fmt = FORMATS.get(base["format"])
        if fmt is None:
            continue
        lines.append(
            f"    FitBaseType {{ id: {base['id']}, format: {fmt}, fit_name: {rust_str(base['fit_name'])}, "
            f"invalid: {rust_str(base['invalid'])}, admitted: {str(base['admitted']).lower()} }},")
    lines.append("];")
    lines.append("")
    refusal = "None" if protocol["admitted"] else f"Some({rust_str(reason)})"
    lines.extend([
        "/// The FIT protocol as generated from the pinned source.",
        "pub(crate) static FIT_PROTOCOL: FitProtocol = FitProtocol {",
        f"    refusal: {refusal},",
        "    base_types: FIT_BASE_TYPES,",
        "    messages: FIT_MESSAGES,",
        f"    common: &{rust_ident(COMMON_TABLE)},",
        f"    header_group0: {rust_str(protocol['header_groups'][0])},",
        f"    header_group1: {rust_str(protocol['header_groups'][1])},",
        f"    header_group2: {rust_str(protocol['header_groups'][2])},",
        "};",
        "",
    ])
    return "\n".join(lines)


def serialized(result) -> str:
    return json.dumps(result, sort_keys=True, indent=2, ensure_ascii=False) + "\n"


def generate(document, verified_exprs):
    """-> (rust_source, ledger_document) for codegen.py's ExprId census."""
    result = compile_document(document, verified_exprs)
    return render_rust(result), result


def load_verified(path: Path) -> set[str]:
    ledger = json.loads(path.read_text())
    return set(ledger.get("verified_expressions") or [])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dump", type=Path, help="dump_tables.pl output containing the Garmin module")
    parser.add_argument("--expr-ledger", type=Path,
                        default=ROOT / "tools/exiftool-tables/expr_oracle_ledger.json")
    parser.add_argument("--ledger-out", type=Path)
    parser.add_argument("--rust-out", type=Path)
    args = parser.parse_args()
    rust, result = generate(json.loads(args.dump.read_text()), load_verified(args.expr_ledger))
    if args.ledger_out:
        args.ledger_out.write_text(serialized(result))
    if args.rust_out:
        args.rust_out.write_text(rust)
    print(json.dumps(result["counts"], indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
