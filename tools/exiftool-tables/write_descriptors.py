"""Inactive, source-derived descriptors for a deliberately tiny write class.

This module consumes ``native_write_tables`` from :mod:`dump_tables.pl`.  It
creates no writer route and it does not claim that a captured native writer
body is understood.  The initial descriptor class is intentionally closed:
plain scalar ASCII rows in ``Exif::Main`` with an effective physical
``WriteGroup`` represented by a literal native string.  Everything outside
that class remains in a named omission sidecar.

The compiler has no tag-name or raw-id allowlist.  A compatible added or
renamed source row is represented; a changed type, placement, conversion, or
other write control is refused with a source-derived reason.  The emitted
procedure fingerprints are provenance for a later writer-mechanism contract,
not an admission to execute those procedures.
"""

from __future__ import annotations

from collections import Counter
from dataclasses import dataclass
import hashlib
import json
from pathlib import PurePosixPath
import re
from typing import Any, Mapping


DESCRIPTOR_VERSION = 1
RUNTIME_STATUS = "inactive_source_candidates_no_writer_route"
_SOURCE_MODULE = "Exif"
_SOURCE_TABLE = "Main"
_SUPPORTED_WRITABLE = "string"
_SUPPORTED_VALUE_TYPE = "Ascii"

# These are source fact keys, not an assertion that every other property is
# harmless.  Any new property is deliberately named and withheld.
_ALLOWED_TABLE_PROPERTIES = frozenset(
    {"GROUPS", "SET_GROUP1", "WRITE_GROUP", "WRITE_PROC", "CHECK_PROC"}
)
_ALLOWED_ROW_PROPERTIES = frozenset({"Name", "Writable", "WriteGroup"})
_ALLOWED_ROW_CONTROLS = frozenset({"Writable", "WriteGroup"})
_ID_RE = re.compile(r"^(?:0x[0-9A-Fa-f]+|[0-9]+)$")
_SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
_CODE_NAME_RE = re.compile(r"^[A-Za-z_]\w*(?:::[A-Za-z_]\w*)+$")


class WriteDescriptorError(ValueError):
    """The captured write sidecar is malformed rather than merely unsupported."""


@dataclass(frozen=True)
class WriteDescriptorReport:
    tables_seen: int
    candidate_tables: int
    emitted_tables: int
    emitted_rows: int
    omitted_tables: int
    omitted_rows: int
    omissions_by_reason: dict[str, int]


def _mapping(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise WriteDescriptorError(f"{context} is not an object")
    return value


def _bool(value: Any, context: str) -> bool:
    if not isinstance(value, bool):
        raise WriteDescriptorError(f"{context} is not a boolean")
    return value


def _fact_value(fact: Any, context: str) -> tuple[bool, Any]:
    fact = _mapping(fact, context)
    present = _bool(fact.get("present"), f"{context}.present")
    if present:
        if "value" not in fact:
            raise WriteDescriptorError(f"{context} is present without a value")
        return True, fact["value"]
    if "value" in fact:
        raise WriteDescriptorError(f"{context} is absent but has a value")
    return False, None


def _plain_string(value: Any) -> str | None:
    # A SCALAR ref such as ``\\'IFD0'`` deliberately does not collapse to its
    # inner string: native write behavior may distinguish the reference.
    return value if isinstance(value, str) else None


def _raw_u16(raw_id: str) -> int:
    if not isinstance(raw_id, str) or _ID_RE.fullmatch(raw_id) is None:
        raise WriteDescriptorError(f"write row id {raw_id!r} is not a literal integer")
    value = int(raw_id, 0)
    if not 0 <= value <= 0xFFFF:
        raise WriteDescriptorError(f"write row id {raw_id!r} is outside u16")
    return value


def _row_sort_key(raw_id: str) -> tuple[int, int | str]:
    # Native write tables also contain symbolic lookup aliases.  They are
    # legitimate source rows but not physical u16 entries, so retain them in
    # the deterministic omission sidecar rather than aborting the whole dump.
    try:
        return 0, _raw_u16(raw_id)
    except WriteDescriptorError:
        return 1, str(raw_id)


def _body_sha256(body: Any, context: str) -> str:
    if not isinstance(body, str):
        raise WriteDescriptorError(f"{context} has no captured B::Deparse body")
    return hashlib.sha256(body.encode("utf-8")).hexdigest()


def _relative_source_file(value: Any, context: str) -> str:
    """Require the dumper's canonical library-relative source spelling.

    A nonempty string is not provenance.  In particular, accepting ``..`` or
    an absolute path would make a hand-mutated sidecar appear to bind a source
    file outside the selected ExifTool library.
    """
    if not isinstance(value, str) or not value or "\\" in value:
        raise WriteDescriptorError(f"{context} is not a normalized relative path")
    path = PurePosixPath(value)
    if (
        path.is_absolute()
        or value in (".", "..")
        or any(part in ("", ".", "..") for part in path.parts)
        or path.as_posix() != value
        or not value.startswith("Image/")
        or value == "Image/"
    ):
        raise WriteDescriptorError(f"{context} is not a normalized relative path")
    return value


def _sha256(value: Any, context: str) -> str:
    if not isinstance(value, str) or _SHA256_RE.fullmatch(value) is None:
        raise WriteDescriptorError(f"{context} is not a SHA-256 digest")
    return value


def _procedure_provenance(fact: Any, context: str) -> dict[str, Any]:
    """Return facts that a later writer contract must authenticate again.

    We retain the source file, source digest and deparse digest.  We do not
    attempt to recognize the procedure body here: a procedure name/hash is not
    a description of native writer behavior.
    """
    fact = _mapping(fact, context)
    if not _bool(fact.get("resolved"), f"{context}.resolved"):
        raise WriteDescriptorError(f"{context} is unresolved")
    if fact.get("__perl") != "CODE":
        raise WriteDescriptorError(f"{context} is not a CODE fact")
    name = fact.get("__name")
    if not isinstance(name, str) or _CODE_NAME_RE.fullmatch(name) is None:
        raise WriteDescriptorError(f"{context} has no fully-qualified callable name")
    result = {
        "name": name,
        "source_file": _relative_source_file(fact.get("source_file"), f"{context}.source_file"),
        "source_sha256": _sha256(fact.get("source_sha256"), f"{context}.source_sha256"),
    }
    result["body_sha256"] = _body_sha256(fact.get("__deparse"), context)

    dependencies = fact.get("dependencies", {})
    dependencies = _mapping(dependencies, f"{context}.dependencies")
    result["dependencies"] = [
        _procedure_provenance(dep, f"{context}.dependencies[{name!r}]")
        for name, dep in sorted(dependencies.items())
    ]
    return result


def _router_provenance(doc: Mapping[str, Any]) -> dict[str, Any]:
    router = _mapping(doc.get("native_write_autoload"), "native_write_autoload")
    if not _bool(router.get("supported"), "native_write_autoload.supported"):
        raise WriteDescriptorError("native_write_autoload router is unsupported")
    return _procedure_provenance(router.get("router"), "native_write_autoload.router")


def _table_provenance(doc: Mapping[str, Any], table: Mapping[str, Any]) -> dict[str, Any]:
    router = _router_provenance(doc)
    procedures: dict[str, dict[str, Any]] = {}
    for kind in ("write", "check"):
        proc = _mapping(table.get(f"effective_{kind}_proc"), f"effective_{kind}_proc")
        present = _bool(proc.get("present"), f"effective_{kind}_proc.present")
        if not present:
            raise WriteDescriptorError(f"effective_{kind}_proc is absent")
        procedures[kind] = _procedure_provenance(proc.get("effective"), f"effective_{kind}_proc.effective")
    return {"autoload_router": router, "write_proc": procedures["write"], "check_proc": procedures["check"]}


def _table_identity_matches(table: Mapping[str, Any], module: str, name: str) -> bool:
    """Bind the sidecar map key to the table's own captured identity."""
    return (
        table.get("module") == module
        and table.get("table") == name
        and table.get("full_name") == f"Image::ExifTool::{module}::{name}"
    )


def _effective_groups(table: Mapping[str, Any], module: str) -> dict[str, str]:
    """Resolve native ``GetTagTable`` group defaults from the captured table.

    The native loader fills false groups 0 and 1 with the table-owning module
    and false group 2 with ``Other``.  Keeping those effective values in the
    candidate is required for later encoding/CharsetEXIF authentication; it
    does not enable a writer.
    """
    props = _mapping(table.get("table_properties"), "table.table_properties")
    present, raw_groups = _fact_value(props.get("GROUPS", {"present": False}), "table.table_properties.GROUPS")
    if not present:
        raw_groups = {}
    if not isinstance(raw_groups, Mapping):
        raise WriteDescriptorError("table GROUPS are not an object")
    if any(not isinstance(key, str) or key not in {"0", "1", "2"} for key in raw_groups):
        raise WriteDescriptorError("table GROUPS contain an unrepresented family")
    if any(not isinstance(value, str) for value in raw_groups.values()):
        raise WriteDescriptorError("table GROUPS are not literal strings")
    default_module = module.split("::", 1)[0]
    if not default_module:
        raise WriteDescriptorError("table module has no native group default")
    result = {}
    for family in range(3):
        value = raw_groups.get(str(family))
        # Perl's boolean false values for the source representation are the
        # empty string and "0"; native GetTagTable applies these defaults.
        if value not in (None, "", "0"):
            result[f"group{family}"] = value
        else:
            result[f"group{family}"] = default_module if family in (0, 1) else "Other"
    return result


def _entry_alternatives(entry: Mapping[str, Any], context: str) -> list[tuple[bool, int, Mapping[str, Any]]]:
    kind = entry.get("entry_kind")
    if kind == "HASH":
        return [(False, 0, entry)]
    if kind == "ARRAY":
        alternatives = entry.get("alternatives")
        if not isinstance(alternatives, list):
            raise WriteDescriptorError(f"{context}.alternatives is not a list")
        result = []
        for index, alternative in enumerate(alternatives):
            alternative = _mapping(alternative, f"{context}.alternatives[{index}]")
            if alternative.get("entry_kind", "HASH") not in ("HASH", None):
                raise WriteDescriptorError(f"{context}.alternatives[{index}] is not a hash alternative")
            result.append((True, index, alternative))
        return result
    # Scalar and otherwise unrecognized source row kinds are still represented
    # in the omission sidecar instead of disappearing.
    return [(False, 0, entry)]


def _name_hint(entry: Mapping[str, Any]) -> str | None:
    properties = entry.get("properties")
    if not isinstance(properties, Mapping):
        return None
    fact = properties.get("Name")
    if not isinstance(fact, Mapping):
        return None
    try:
        present, value = _fact_value(fact, "row.properties.Name")
    except WriteDescriptorError:
        return None
    return _plain_string(value) if present else None


def _effective_group(entry: Mapping[str, Any], table: Mapping[str, Any]) -> tuple[bool, Any]:
    controls = _mapping(entry.get("write_controls"), "row.write_controls")
    row_present, row_value = _fact_value(controls.get("WriteGroup"), "row.write_controls.WriteGroup")
    if row_present:
        return True, row_value
    controls = _mapping(table.get("write_controls"), "table.write_controls")
    return _fact_value(controls.get("WRITE_GROUP"), "table.write_controls.WRITE_GROUP")


def _row_reasons(entry: Mapping[str, Any], table: Mapping[str, Any]) -> tuple[list[str], dict[str, Any] | None]:
    reasons: list[str] = []
    if entry.get("entry_kind", "HASH") != "HASH":
        return ["write_row_shape"], None
    properties = _mapping(entry.get("properties"), "row.properties")
    controls = _mapping(entry.get("write_controls"), "row.write_controls")
    unknown = _mapping(entry.get("unknown_properties"), "row.unknown_properties")
    if unknown:
        reasons.extend(f"write_row_unknown_property_{key}" for key in sorted(unknown))
    # The sidecar deliberately presents writer controls twice: in the complete
    # native property map and in the convenient write-controls projection. A
    # stale or hand-mutated projection may not choose whichever copy happens
    # to admit the row.
    for key in _ALLOWED_ROW_CONTROLS:
        property_fact = properties.get(key, {"present": False})
        control_fact = controls.get(key)
        if property_fact != control_fact:
            reasons.append("write_row_control_projection_mismatch")
    for key in sorted(set(properties) - _ALLOWED_ROW_PROPERTIES):
        reasons.append(f"write_row_property_{key}")
    for key in sorted(set(controls) - _ALLOWED_ROW_CONTROLS):
        present, _ = _fact_value(controls[key], f"row.write_controls.{key}")
        if present:
            reasons.append(f"write_row_control_{key}")

    name_present, name_value = _fact_value(properties.get("Name"), "row.properties.Name")
    name = _plain_string(name_value) if name_present else None
    if not name:
        reasons.append("write_name")
    writable_present, writable_value = _fact_value(controls.get("Writable"), "row.write_controls.Writable")
    if not writable_present or writable_value != _SUPPORTED_WRITABLE:
        reasons.append("write_value_type")
    group_present, group_value = _effective_group(entry, table)
    group = _plain_string(group_value) if group_present else None
    if group is None:
        reasons.append("write_physical_group_missing_or_nonliteral")
    elif group not in {"IFD0", "ExifIFD", "GPS"}:
        reasons.append("write_physical_group_unmodeled")
    # The inactive descriptor faithfully carries ordinary native physical
    # groups.  A future runtime may stage IFD0/ExifIFD/GPS independently, but
    # source capture must not erase a real placement merely because its writer
    # primitive has not landed yet.

    if reasons:
        return sorted(set(reasons)), None
    return [], {
        "name": name,
        "value_type": _SUPPORTED_VALUE_TYPE,
        "physical_group": group,
    }


def _table_reasons(
    doc: Mapping[str, Any], module: str, name: str, table: Mapping[str, Any]
) -> tuple[list[str], dict[str, Any] | None, dict[str, str] | None]:
    identity_matches = _table_identity_matches(table, module, name)
    if not identity_matches:
        return ["write_table_identity_mismatch"], None, None
    if (module, name) != (_SOURCE_MODULE, _SOURCE_TABLE):
        return ["write_source_class_unimplemented"], None, None
    unknown = _mapping(table.get("unknown_table_properties"), "table.unknown_table_properties")
    reasons = [f"write_table_unknown_property_{key}" for key in sorted(unknown)]
    props = _mapping(table.get("table_properties"), "table.table_properties")
    controls = _mapping(table.get("write_controls"), "table.write_controls")
    reasons.extend(f"write_table_property_{key}" for key in sorted(set(props) - _ALLOWED_TABLE_PROPERTIES))
    for property_key, control_key in (("WRITE_GROUP", "WRITE_GROUP"), ("WRITE_PROC", "WRITE_PROC"), ("CHECK_PROC", "CHECK_PROC")):
        if props.get(property_key) != controls.get(control_key):
            reasons.append("write_table_control_projection_mismatch")
    try:
        provenance = _table_provenance(doc, table)
    except WriteDescriptorError as error:
        reasons.append("write_provenance_unresolved")
        provenance = None
    try:
        groups = _effective_groups(table, module)
    except WriteDescriptorError:
        reasons.append("write_table_groups_unrepresented")
        groups = None
    return sorted(set(reasons)), provenance, groups


def _rust_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def _rust_provenance(fact: Mapping[str, Any]) -> str:
    deps = ", ".join(_rust_provenance(dep) for dep in fact["dependencies"])
    return (
        "NativeWriteProcedureProvenance { "
        f"name: {_rust_string(fact['name'])}, source_file: {_rust_string(fact['source_file'])}, "
        f"source_sha256: {_rust_string(fact['source_sha256'])}, body_sha256: {_rust_string(fact['body_sha256'])}, "
        f"dependencies: &[{deps}] }}"
    )


def _ident(module: str, table: str) -> str:
    return re.sub(r"[^A-Za-z0-9]", "_", f"WRITE_{module}_{table}").upper()


def rust_source(population: Mapping[str, Any]) -> str:
    """Render inert Rust facts.  No existing module re-exports this source."""
    tables = population["tables"]
    row_omissions = population["omitted_rows"]
    table_omissions = population["omitted_tables"]
    chunks = [f'''// @generated by tools/exiftool-tables/write_descriptors.py.
//! Inactive native write candidates.  These are source provenance and a closed
//! scalar-ASCII shape only; they do not activate a writer or authenticate the
//! behavior of WriteExif/CheckExif.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteValueType {{ Ascii }}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WritePhysicalGroup {{ IFD0, ExifIFD, GPS }}
pub struct NativeWriteTableGroups {{
    pub group0: &'static str,
    pub group1: &'static str,
    pub group2: &'static str,
}}
pub struct NativeWriteProcedureProvenance {{
    pub name: &'static str,
    pub source_file: &'static str,
    pub source_sha256: &'static str,
    pub body_sha256: &'static str,
    pub dependencies: &'static [NativeWriteProcedureProvenance],
}}
pub struct InactiveWriteScalarString {{
    pub raw_id: u16,
    pub name: &'static str,
    pub physical_group: WritePhysicalGroup,
    pub value_type: WriteValueType,
}}
pub struct InactiveWriteTable {{
    pub module: &'static str,
    pub table: &'static str,
    pub full_name: &'static str,
    /// Effective native groups after GetTagTable's false-value defaults.
    /// These facts are needed by a later encoding contract; they do not route
    /// a writer or authenticate WriteExif/CheckExif behavior.
    pub groups: NativeWriteTableGroups,
    pub autoload_router: NativeWriteProcedureProvenance,
    pub write_proc: NativeWriteProcedureProvenance,
    pub check_proc: NativeWriteProcedureProvenance,
    pub tags: &'static [InactiveWriteScalarString],
}}
pub struct OmittedWriteNativeRow {{
    pub module: &'static str,
    pub table: &'static str,
    pub raw_id: &'static str,
    pub variant: bool,
    pub alternative: usize,
    pub name: Option<&'static str>,
    pub reasons: &'static [&'static str],
}}
pub struct OmittedWriteNativeTable {{
    pub module: &'static str,
    pub table: &'static str,
    pub reasons: &'static [&'static str],
}}
pub const INACTIVE_WRITE_DESCRIPTOR_VERSION: u32 = {DESCRIPTOR_VERSION};
pub const INACTIVE_WRITE_RUNTIME_STATUS: &str = {_rust_string(RUNTIME_STATUS)};
''']
    refs = []
    for table in tables:
        symbol = _ident(table["module"], table["table"])
        refs.append(f"    &{symbol},")
        tags = ",\n".join(
            "    InactiveWriteScalarString { "
            f"raw_id: 0x{tag['raw_id']:04x}, name: {_rust_string(tag['name'])}, "
            f"physical_group: WritePhysicalGroup::{tag['physical_group']}, value_type: WriteValueType::Ascii }}"
            for tag in table["tags"]
        )
        prov = table["provenance"]
        chunks.append(
            f"\npub static {symbol}: InactiveWriteTable = InactiveWriteTable {{\n"
            f"    module: {_rust_string(table['module'])}, table: {_rust_string(table['table'])}, full_name: {_rust_string(table['full_name'])},\n"
            "    groups: NativeWriteTableGroups { "
            f"group0: {_rust_string(table['groups']['group0'])}, group1: {_rust_string(table['groups']['group1'])}, "
            f"group2: {_rust_string(table['groups']['group2'])} }},\n"
            f"    autoload_router: {_rust_provenance(prov['autoload_router'])},\n"
            f"    write_proc: {_rust_provenance(prov['write_proc'])},\n"
            f"    check_proc: {_rust_provenance(prov['check_proc'])},\n"
            f"    tags: &[\n{tags}\n    ],\n}};\n"
        )
    chunks.append("\npub static ALL_INACTIVE_WRITE_TABLES: &[&InactiveWriteTable] = &[\n" + "\n".join(refs) + "\n];\n")
    rows = []
    for row in row_omissions:
        name = "None" if row["name"] is None else f"Some({_rust_string(row['name'])})"
        reasons = ", ".join(_rust_string(reason) for reason in row["reasons"])
        rows.append(
            "    OmittedWriteNativeRow { "
            f"module: {_rust_string(row['module'])}, table: {_rust_string(row['table'])}, raw_id: {_rust_string(row['raw_id'])}, "
            f"variant: {str(row['variant']).lower()}, alternative: {row['alternative']}, name: {name}, reasons: &[{reasons}] }},"
        )
    chunks.append("\npub static OMITTED_WRITE_NATIVE_ROWS: &[OmittedWriteNativeRow] = &[\n" + "\n".join(rows) + "\n];\n")
    omissions = []
    for table in table_omissions:
        reasons = ", ".join(_rust_string(reason) for reason in table["reasons"])
        omissions.append(
            "    OmittedWriteNativeTable { "
            f"module: {_rust_string(table['module'])}, table: {_rust_string(table['table'])}, reasons: &[{reasons}] }},"
        )
    chunks.append("\npub static OMITTED_WRITE_NATIVE_TABLES: &[OmittedWriteNativeTable] = &[\n" + "\n".join(omissions) + "\n];\n")
    return "".join(chunks)


def generate(doc: Mapping[str, Any], modules: list[str] | None = None) -> tuple[str, WriteDescriptorReport]:
    """Compile the sidecar into inactive candidates and explicit omissions."""
    write_tables = _mapping(doc.get("native_write_tables"), "native_write_tables")
    requested = set(modules) if modules else None
    population = {"tables": [], "omitted_rows": [], "omitted_tables": []}
    counts: Counter[str] = Counter()

    for module in sorted(write_tables):
        if requested is not None and module not in requested:
            continue
        table_map = _mapping(write_tables[module], f"native_write_tables[{module!r}]")
        for table_name in sorted(table_map):
            table = _mapping(table_map[table_name], f"native_write_tables[{module!r}][{table_name!r}]")
            counts["tables_seen"] += 1
            rows = _mapping(table.get("rows"), f"{module}::{table_name}.rows")
            table_reasons, provenance, groups = _table_reasons(doc, module, table_name, table)
            if not table_reasons:
                counts["candidate_tables"] += 1
            admitted = []
            table_rows = []
            for raw_id in sorted(rows, key=_row_sort_key):
                source_entry = _mapping(rows[raw_id], f"{module}::{table_name} row {raw_id}")
                for variant, alternative, entry in _entry_alternatives(source_entry, f"{module}::{table_name} row {raw_id}"):
                    name = _name_hint(entry)
                    reasons = list(table_reasons)
                    compiled = None
                    try:
                        parsed_raw_id = _raw_u16(raw_id)
                    except WriteDescriptorError:
                        parsed_raw_id = None
                        reasons.append("write_raw_id")
                    if not reasons:
                        reasons, compiled = _row_reasons(entry, table)
                    if reasons:
                        record = {
                            "module": module, "table": table_name, "raw_id": raw_id,
                            "variant": variant, "alternative": alternative, "name": name,
                            "reasons": sorted(set(reasons)),
                        }
                        population["omitted_rows"].append(record)
                        counts["omitted_rows"] += 1
                        counts.update(record["reasons"])
                    else:
                        if parsed_raw_id is None:
                            raise AssertionError("non-u16 write row reached descriptor")
                        admitted.append({"raw_id": parsed_raw_id, **compiled})
                        table_rows.append(raw_id)
                        counts["emitted_rows"] += 1
            if admitted:
                if table_reasons or provenance is None or groups is None:
                    raise AssertionError("admitted write row without table provenance")
                population["tables"].append({
                    "module": module, "table": table_name,
                    "full_name": table["full_name"], "groups": groups, "provenance": provenance,
                    "tags": sorted(admitted, key=lambda tag: tag["raw_id"]),
                })
                counts["emitted_tables"] += 1
            else:
                reasons = table_reasons or ["write_table_has_no_admitted_rows"]
                record = {"module": module, "table": table_name, "reasons": sorted(set(reasons))}
                population["omitted_tables"].append(record)
                counts["omitted_tables"] += 1
                counts.update(record["reasons"])

    population["tables"].sort(key=lambda value: (value["module"], value["table"]))
    population["omitted_rows"].sort(key=lambda value: (value["module"], value["table"], _row_sort_key(value["raw_id"]), value["alternative"]))
    population["omitted_tables"].sort(key=lambda value: (value["module"], value["table"]))
    report = WriteDescriptorReport(
        tables_seen=counts["tables_seen"],
        candidate_tables=counts["candidate_tables"],
        emitted_tables=counts["emitted_tables"],
        emitted_rows=counts["emitted_rows"],
        omitted_tables=counts["omitted_tables"],
        omitted_rows=counts["omitted_rows"],
        omissions_by_reason=dict(sorted((key, value) for key, value in counts.items() if key.startswith("write_"))),
    )
    return rust_source(population), report
