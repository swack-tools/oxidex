"""Closed, source-selected addressing for the static EXIF writer rows.

This is an operand compiler.  It resolves an ordinary public spelling to one
static row identity, but does not call SetNewValue, create NEW_VALUE data, or
write a file.  A caller must treat ``owned_unsupported`` as terminal: it may
not fall back to an older hand-written writer implementation.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass
import copy
import hashlib
import json
from pathlib import Path
import re
from typing import Any, Mapping

from checkexif_recipes import RecipeMalformed, RecipeRefused
from convinv_rows import Row, compile_rows
import native_reader_facts
from setnewvalue_convinv_recipes import compile_setnewvalue_convinv


RUNTIME_STATUS = "inactive_source_addressing_no_public_setnewvalue_admission"
_NAME = re.compile(r"^[A-Za-z0-9_]+$")


@dataclass(frozen=True)
class AddressRow:
    module: str
    table: str
    full_name: str
    raw_id: str
    name: str
    group0: str
    group1: str
    write_group: str

    @property
    def identity(self) -> tuple[str, str, str, str, str]:
        return self.module, self.table, self.full_name, self.raw_id, self.name


@dataclass(frozen=True)
class Addressing:
    rows: tuple[AddressRow, ...]
    owned_names: frozenset[str]
    lookup_source_file: str
    lookup_source_sha256: str
    lookup_body_sha256: str
    set_new_value_source_file: str
    set_new_value_source_sha256: str
    set_new_value_body_sha256: str
    source_rows_sha256: str
    query_names_sha256: str
    capture_context: Mapping[str, Any]


@dataclass(frozen=True)
class Resolution:
    state: str
    row: AddressRow | None = None
    reason: str | None = None


def _mapping(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise RecipeMalformed(f"{context} is not an object")
    return value


def _fact(value: Any, context: str) -> tuple[bool, Any]:
    value = _mapping(value, context)
    present = value.get("present")
    if type(present) is not bool:
        raise RecipeMalformed(f"{context}.present is not boolean")
    if present and "value" not in value:
        raise RecipeMalformed(f"{context} is present without value")
    if not present and set(value) != {"present"}:
        raise RecipeMalformed(f"{context} is absent with a value")
    return present, value.get("value")


def _template() -> list[str]:
    try:
        value = json.loads(Path(__file__).with_name("findtaginfo_full_template.json").read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise RecipeRefused("FindTagInfo source template is unavailable") from error
    if not isinstance(value, list) or any(not isinstance(item, str) for item in value):
        raise RecipeRefused("FindTagInfo source template is malformed")
    return value


def _helper_identity(fact: Mapping[str, Any], requested: str, *, context: str) -> tuple[str, str, str, str]:
    """Return the exact source identity that a native observation must re-prove."""
    if fact.get("requested_binding") != requested:
        raise RecipeRefused(f"{context} requested binding is stale")
    try:
        actual, source_file, source_sha256, _tokens = native_reader_facts.source_fact(fact, requested)
    except native_reader_facts.ReaderRefused as error:
        raise RecipeRefused(f"{context} is unresolved or rebound") from error
    body = fact.get("__deparse")
    if not isinstance(body, str):
        raise RecipeRefused(f"{context} has no deparsed body")
    return actual, source_file, source_sha256, hashlib.sha256(body.encode()).hexdigest()


def _find_tag_info_source(document: Mapping[str, Any]) -> tuple[str, str, str, str]:
    helpers = _mapping(document.get("native_write_helpers"), "native_write_helpers")
    fact = _mapping(helpers.get("find_tag_info"), "native_write_helpers.find_tag_info")
    actual, source_file, source_sha256, body_sha256 = _helper_identity(
        fact, "Image::ExifTool::TagLookup::FindTagInfo", context="FindTagInfo")
    try:
        tokens = native_reader_facts.source_fact(
            fact, "Image::ExifTool::TagLookup::FindTagInfo")[3]
    except native_reader_facts.ReaderRefused as error:
        raise RecipeRefused("FindTagInfo is unresolved or rebound") from error
    if actual != "Image::ExifTool::TagLookup::FindTagInfo" or tokens != _template():
        raise RecipeRefused("FindTagInfo name lookup control flow is unsupported")
    return actual, source_file, source_sha256, body_sha256


def _capture_context(document: Mapping[str, Any]) -> dict[str, Any]:
    context = _mapping(document.get("native_capture_context"), "native_capture_context")
    if context.get("schema") != "native_exiftool_capture_context_v1":
        raise RecipeRefused("native capture context is absent or stale")
    required = ("selected_library", "perl_path", "perl_version", "exiftool_version")
    values: dict[str, Any] = {"schema": "native_exiftool_capture_context_v1"}
    for key in required:
        value = context.get(key)
        if not isinstance(value, str) or not value:
            raise RecipeRefused(f"native capture context {key} is unavailable")
        values[key] = value
    closure = _mapping(context.get("loaded_closure"), "native capture context loaded closure")
    modules = closure.get("modules")
    if not isinstance(modules, list) or not modules:
        raise RecipeRefused("native capture context closure modules are unavailable")
    if not isinstance(closure.get("sha256"), str) or not re.fullmatch(r"[0-9a-f]{64}", closure["sha256"]):
        raise RecipeRefused("native capture context closure digest is malformed")
    for module in modules:
        module = _mapping(module, "native capture context closure member")
        for key in ("inc", "source_file", "source_sha256"):
            if not isinstance(module.get(key), str) or not module[key]:
                raise RecipeRefused("native capture context closure member is malformed")
    if _digest(modules) != closure["sha256"]:
        raise RecipeRefused("native capture context closure digest does not match its modules")
    values["loaded_closure"] = {"sha256": closure["sha256"],
                                "modules": [dict(member) for member in modules]}
    return values


def _digest(value: Any) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
                                     ensure_ascii=False).encode("utf-8")).hexdigest()


def _address_rows_digest(rows: tuple[AddressRow, ...]) -> str:
    return _digest([asdict(row) for row in rows])


def _query_names_digest(names: frozenset[str] | set[str]) -> str:
    return _digest(sorted(names))


def _table_groups(document: Mapping[str, Any], row: Row) -> tuple[str, str]:
    tables = _mapping(document.get("native_write_tables"), "native_write_tables")
    table = _mapping(_mapping(tables.get(row.module), f"native_write_tables.{row.module}").get(row.table),
                     f"native_write_tables.{row.module}.{row.table}")
    if (table.get("module"), table.get("table"), table.get("full_name")) != (
            row.module, row.table, row.full_name):
        raise RecipeMalformed("static row table identity disagrees with source table")
    properties = _mapping(table.get("table_properties"), "table.table_properties")
    present, groups = _fact(properties.get("GROUPS", {"present": False}), "table.GROUPS")
    if not present or not isinstance(groups, Mapping):
        raise RecipeRefused("static row table lacks native GROUPS identity")
    group0, group1 = groups.get("0"), groups.get("1")
    if not isinstance(group0, str) or not isinstance(group1, str):
        raise RecipeRefused("static row table has unsupported GROUPS identity")
    # This component deliberately owns only ordinary EXIF/IFD0 addressing.
    if group0 != "EXIF" or group1 != "IFD0":
        raise RecipeRefused("static row table is outside ordinary EXIF/IFD0 addressing")
    return group0, group1


def _owned_names(document: Mapping[str, Any]) -> set[str]:
    """Names in source rows, including rows omitted from the final recipe.

    This makes source-owned but unsupported rows terminal to a future public
    router.  It is source data, not a maintained tag-name list.
    """
    tables = _mapping(document.get("native_write_tables"), "native_write_tables")
    module = _mapping(tables.get("Exif"), "native_write_tables.Exif")
    table = _mapping(module.get("Main"), "native_write_tables.Exif.Main")
    rows = _mapping(table.get("rows"), "native_write_tables.Exif.Main.rows")
    result = set()
    for raw in rows.values():
        raw = _mapping(raw, "source row")
        properties = raw.get("effective_properties") or raw.get("properties")
        if not isinstance(properties, Mapping):
            continue
        name = properties.get("Name")
        if not isinstance(name, Mapping):
            continue
        present, value = _fact(name, "source row Name")
        if present and isinstance(value, str) and _NAME.fullmatch(value):
            result.add(value.lower())
    return result


def compile_addressing(document: Mapping[str, Any]) -> tuple[Addressing, dict[str, Any]]:
    """Compile rows only after both native caller and lookup sources match."""
    document = _mapping(document, "document")
    # The complete caller match authenticates its tag/group split before this
    # narrow operand adopts the documented one-qualifier subset.
    setnew = _mapping(_mapping(document.get("native_write_helpers"), "native_write_helpers").get("set_new_value"),
                      "native_write_helpers.set_new_value")
    compile_setnewvalue_convinv(setnew)
    setnew_actual, setnew_file, setnew_sha256, setnew_body_sha256 = _helper_identity(
        setnew, "Image::ExifTool::SetNewValue", context="SetNewValue")
    if setnew_actual != "Image::ExifTool::SetNewValue":
        raise RecipeRefused("SetNewValue is rebound")
    lookup_actual, source_file, source_sha256, body_sha256 = _find_tag_info_source(document)
    if lookup_actual != "Image::ExifTool::TagLookup::FindTagInfo":
        raise RecipeRefused("FindTagInfo is rebound")
    capture_context = _capture_context(document)
    source_rows, source_report = compile_rows(document)
    address_rows = []
    omissions = []
    for row in source_rows:
        try:
            group0, group1 = _table_groups(document, row)
            address_rows.append(AddressRow(
                row.module, row.table, row.full_name, row.raw_id, row.name,
                group0, group1, row.write_group))
        except RecipeRefused as error:
            omissions.append({"identity": (row.module, row.table, row.full_name, row.raw_id, row.name),
                              "reason": str(error)})
    address_rows = tuple(address_rows)
    owned_names = frozenset(_owned_names(document))
    result = Addressing(address_rows, owned_names,
                       source_file, source_sha256, body_sha256,
                       setnew_file, setnew_sha256, setnew_body_sha256,
                       _address_rows_digest(address_rows), _query_names_digest(owned_names),
                       capture_context)
    return result, {
        "runtime_status": RUNTIME_STATUS,
        "rows_emitted": len(result.rows),
        "rows_omitted": len(omissions),
        "omitted_rows": omissions,
        "source_row_report": source_report,
        "find_tag_info": {"source_file": source_file, "source_sha256": source_sha256,
                          "body_sha256": body_sha256},
        "set_new_value": {"source_file": setnew_file, "source_sha256": setnew_sha256,
                          "body_sha256": setnew_body_sha256},
        "source_rows_sha256": result.source_rows_sha256,
        "query_names_sha256": result.query_names_sha256,
        "native_capture_context": result.capture_context,
    }


def _parse_input(text: str) -> tuple[str | None, str] | Resolution:
    if not isinstance(text, str) or not text:
        return Resolution("outside_migrated_scope", reason="empty tag spelling")
    if text.count(":") > 1:
        return Resolution("owned_unsupported", reason="multiple group qualifiers are unsupported")
    group, name = (text.split(":", 1) if ":" in text else (None, text))
    if not _NAME.fullmatch(name):
        return Resolution("owned_unsupported", reason="non-ordinary tag spelling is unsupported")
    if group is not None and not _NAME.fullmatch(group):
        return Resolution("owned_unsupported", reason="non-ordinary group qualifier is unsupported")
    return group, name


def _candidate_identity(value: Mapping[str, Any]) -> tuple[str, str, str, str, str] | None:
    fields = tuple(value.get(key) for key in ("module", "table", "full_name", "raw_id", "name"))
    return fields if all(isinstance(item, str) for item in fields) else None


def observation_input(addressing: Addressing) -> dict[str, Any]:
    """The sealed payload accepted by the canonical native probe."""
    return {
        "schema": "native_setnewvalue_address_probe_input_v2",
        "capture": _observation_capture(addressing),
        "rows": [asdict(row) for row in addressing.rows],
        "query_names": sorted(addressing.owned_names),
    }


def _helper_capture(requested: str, source_file: str, source_sha256: str, body_sha256: str) -> dict[str, str]:
    return {"requested_binding": requested, "actual_name": requested,
            "source_file": source_file, "source_sha256": source_sha256,
            "body_sha256": body_sha256}


def _observation_capture(addressing: Addressing) -> dict[str, Any]:
    return {
        "source_rows_sha256": addressing.source_rows_sha256,
        "query_names_sha256": addressing.query_names_sha256,
        "native_capture_context": copy.deepcopy(addressing.capture_context),
        "find_tag_info": _helper_capture("Image::ExifTool::TagLookup::FindTagInfo",
                                           addressing.lookup_source_file,
                                           addressing.lookup_source_sha256,
                                           addressing.lookup_body_sha256),
        "set_new_value": _helper_capture("Image::ExifTool::SetNewValue",
                                           addressing.set_new_value_source_file,
                                           addressing.set_new_value_source_sha256,
                                           addressing.set_new_value_body_sha256),
    }


def _validate_observations(addressing: Addressing, observations: Mapping[str, Any]) -> Mapping[str, Any]:
    observations = _mapping(observations, "native lookup observations")
    if observations.get("schema") != "native_setnewvalue_addressing_v2":
        raise RecipeMalformed("native lookup observations have unknown schema")
    capture = _mapping(observations.get("capture"), "native lookup observations.capture")
    if capture != _observation_capture(addressing):
        raise RecipeRefused("native lookup observations do not match the captured source rows/helpers")
    runtime = _mapping(observations.get("runtime"), "native lookup observations.runtime")
    expected_context = addressing.capture_context
    for key in ("selected_library", "perl_path", "perl_version", "exiftool_version"):
        if runtime.get(key) != expected_context[key]:
            raise RecipeRefused(f"native lookup observations used a different {key}")
    closure = _mapping(runtime.get("loaded_closure"), "native lookup observations.runtime.loaded_closure")
    modules = closure.get("modules")
    if not isinstance(modules, list) or not modules:
        raise RecipeMalformed("native lookup observations have no loaded ExifTool closure")
    if not isinstance(closure.get("sha256"), str) or not re.fullmatch(r"[0-9a-f]{64}", closure["sha256"]):
        raise RecipeMalformed("native lookup observations closure digest is malformed")
    for module in modules:
        module = _mapping(module, "native lookup observations.loaded closure member")
        for key in ("inc", "source_file", "source_sha256"):
            if not isinstance(module.get(key), str) or not module[key]:
                raise RecipeMalformed("native lookup observations closure member is malformed")
    if _digest(modules) != closure["sha256"]:
        raise RecipeRefused("native lookup observations loaded closure digest does not match its modules")
    captured_modules = _mapping(expected_context.get("loaded_closure"), "native capture context loaded closure").get("modules")
    captured_by_inc = {member["inc"]: member for member in captured_modules}
    for module in modules:
        expected = captured_by_inc.get(module["inc"])
        if expected is None or expected != module:
            raise RecipeRefused("native lookup observations loaded closure does not match the dump capture")
    helpers = _mapping(runtime.get("helpers"), "native lookup observations.runtime.helpers")
    for key in ("find_tag_info", "set_new_value"):
        if _mapping(helpers.get(key), f"native lookup observations.runtime.helpers.{key}") != capture[key]:
            raise RecipeRefused(f"native lookup {key} identity disagrees with the dump capture")
    queries = _mapping(observations.get("queries"), "native lookup observations.queries")
    if _digest(sorted(queries)) != addressing.query_names_sha256:
        raise RecipeRefused("native lookup query set does not match generated rows")
    return queries


def _observed_candidates(addressing: Addressing, observations: Mapping[str, Any], name: str) -> list[Mapping[str, Any]] | None:
    queries = _validate_observations(addressing, observations)
    query = queries.get(name.lower())
    if query is None:
        return None
    query = _mapping(query, f"native lookup observations.queries[{name.lower()!r}]")
    if query.get("query", "").lower() != name.lower() or not isinstance(query.get("candidates"), list):
        raise RecipeMalformed("native lookup observation is malformed")
    candidates = [_mapping(item, "native lookup candidate") for item in query["candidates"]]
    for candidate in candidates:
        if not isinstance(candidate.get("name"), str) or candidate["name"].lower() != name.lower():
            raise RecipeMalformed("native lookup candidate name does not match its query")
    return candidates


def resolve(addressing: Addressing, observations: Mapping[str, Any], text: str) -> Resolution:
    """Resolve one spelling without falling back to a manual writer route."""
    parsed = _parse_input(text)
    if isinstance(parsed, Resolution):
        return parsed
    group, name = parsed
    lower = name.lower()
    candidates = [row for row in addressing.rows if row.name.lower() == lower]
    if not candidates:
        # An explicitly-qualified spelling may identify a native table that
        # lies entirely outside this EXIF/IFD0 migration despite a colliding
        # bare source name.  Probe all source-owned names for that distinction.
        if group is not None:
            observed = _observed_candidates(addressing, observations, name)
            wanted = group.lower()
            native_matches = [candidate for candidate in observed if isinstance(candidate.get("groups"), Mapping)
                              and any(isinstance(value, str) and value.lower() == wanted
                                      for value in candidate["groups"].values())]
            if (wanted not in {"exif", "ifd0"} and native_matches and
                    all(_candidate_identity(candidate) is None for candidate in native_matches)):
                return Resolution("outside_migrated_scope",
                                  reason="qualified native lookup selects only an unmigrated group")
        state = "owned_unsupported" if lower in addressing.owned_names else "outside_migrated_scope"
        return Resolution(state, reason=("source-owned row has no final addressing recipe"
                                         if state == "owned_unsupported"
                                         else "name is outside migrated source rows"))
    observed = _observed_candidates(addressing, observations, name)
    if observed is None:
        return Resolution("owned_unsupported", reason="native lookup observation is unavailable")
    by_identity = {row.identity: row for row in candidates}
    selected = []
    external = 0
    wanted = group.lower() if group is not None else None
    matched_native = 0
    matched_external = 0
    matched_generated = 0
    for candidate in observed:
        identity = _candidate_identity(candidate)
        row = by_identity.get(identity) if identity else None
        groups = candidate.get("groups")
        if not isinstance(groups, Mapping):
            raise RecipeMalformed("native lookup candidate lacks groups")
        if group is not None:
            matches = any(isinstance(value, str) and value.lower() == wanted for value in groups.values())
            if not matches:
                continue
            matched_native += 1
        if row is None:
            external += 1
            matched_external += 1
        else:
            selected.append(row)
            matched_generated += 1
    # A qualified lookup that native resolves solely to another group is not
    # owned by this EXIF/IFD0 migration, even when its bare spelling collides
    # with a source-owned EXIF field.  The unqualified spelling remains
    # terminal because native ambiguity is real.
    if group is not None and wanted not in {"exif", "ifd0"}:
        if matched_native and matched_generated == 0 and matched_external == matched_native:
            return Resolution("outside_migrated_scope", reason="qualified native lookup selects only an unmigrated group")
        return Resolution("owned_unsupported", reason="group qualifier is outside EXIF/IFD0 subset")
    if external:
        return Resolution("owned_unsupported", reason="native lookup has candidates outside generated rows")
    unique = {row.identity: row for row in selected}
    if not unique:
        return Resolution("owned_unsupported", reason="qualifier does not select a generated row")
    if len(unique) != 1:
        return Resolution("owned_unsupported", reason="native lookup remains ambiguous")
    return Resolution("resolved", row=next(iter(unique.values())))


def resolve_batch(addressing: Addressing, observations: Mapping[str, Any],
                  requests: Mapping[str, Any]) -> tuple[tuple[tuple[AddressRow, Any], ...], tuple[Resolution, ...]]:
    """Resolve aliases and reject conflicting writes to one physical identity."""
    accepted: dict[tuple[str, str, str, str, str], tuple[AddressRow, Any]] = {}
    failures = []
    for text, value in requests.items():
        answer = resolve(addressing, observations, text)
        if answer.state != "resolved" or answer.row is None:
            failures.append(answer)
            continue
        prior = accepted.get(answer.row.identity)
        if prior is not None and prior[1] != value:
            failures.append(Resolution("owned_unsupported", row=answer.row,
                                       reason="conflicting duplicate requests resolve to one physical field"))
            continue
        accepted[answer.row.identity] = (answer.row, value)
    # This is an atomic admission API.  A caller may not send the earlier
    # accepted operands after one alias is rejected; otherwise a conflicting
    # duplicate request could partially write a file.
    if failures:
        return (), tuple(failures)
    return tuple(accepted.values()), ()


def render(addressing: Addressing) -> str:
    return json.dumps({"runtime_status": RUNTIME_STATUS, "rows": [asdict(row) for row in addressing.rows],
                       "source_rows_sha256": addressing.source_rows_sha256,
                       "query_names_sha256": addressing.query_names_sha256,
                       "native_capture_context": copy.deepcopy(addressing.capture_context)},
                      sort_keys=True, indent=2) + "\n"


def main() -> None:
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tables", type=Path)
    parser.add_argument("--rows", type=Path, required=True,
                        help="source rows to pass to setnewvalue_address_probe.pl")
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    addressing, report = compile_addressing(json.loads(args.tables.read_text(encoding="utf-8")))
    args.rows.write_text(json.dumps(observation_input(addressing), sort_keys=True) + "\n",
                        encoding="utf-8")
    args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
