"""Closed, source-selected addressing for the static EXIF writer rows.

This is an operand compiler.  It resolves an ordinary public spelling to one
static row identity, but does not call SetNewValue, create NEW_VALUE data, or
write a file.  A caller must treat ``owned_unsupported`` as terminal: it may
not fall back to an older hand-written writer implementation.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass
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


def _find_tag_info_source(document: Mapping[str, Any]) -> tuple[str, str, str]:
    helpers = _mapping(document.get("native_write_helpers"), "native_write_helpers")
    fact = _mapping(helpers.get("find_tag_info"), "native_write_helpers.find_tag_info")
    if fact.get("requested_binding") != "Image::ExifTool::TagLookup::FindTagInfo":
        raise RecipeRefused("FindTagInfo requested binding is stale")
    try:
        actual, source_file, source_sha256, tokens = native_reader_facts.source_fact(
            fact, "Image::ExifTool::TagLookup::FindTagInfo")
    except native_reader_facts.ReaderRefused as error:
        raise RecipeRefused("FindTagInfo is unresolved or rebound") from error
    if actual != "Image::ExifTool::TagLookup::FindTagInfo" or tokens != _template():
        raise RecipeRefused("FindTagInfo name lookup control flow is unsupported")
    body = fact.get("__deparse")
    if not isinstance(body, str):
        raise RecipeRefused("FindTagInfo has no deparsed body")
    import hashlib
    return source_file, source_sha256, hashlib.sha256(body.encode()).hexdigest()


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
    compile_setnewvalue_convinv(document["native_write_helpers"]["set_new_value"])
    source_file, source_sha256, body_sha256 = _find_tag_info_source(document)
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
    result = Addressing(tuple(address_rows), frozenset(_owned_names(document)),
                       source_file, source_sha256, body_sha256)
    return result, {
        "runtime_status": RUNTIME_STATUS,
        "rows_emitted": len(result.rows),
        "rows_omitted": len(omissions),
        "omitted_rows": omissions,
        "source_row_report": source_report,
        "find_tag_info": {"source_file": source_file, "source_sha256": source_sha256,
                          "body_sha256": body_sha256},
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


def _observed_candidates(observations: Mapping[str, Any], name: str) -> list[Mapping[str, Any]] | None:
    observations = _mapping(observations, "native lookup observations")
    if observations.get("schema") != "native_setnewvalue_addressing_v1":
        raise RecipeMalformed("native lookup observations have unknown schema")
    queries = _mapping(observations.get("queries"), "native lookup observations.queries")
    query = queries.get(name.lower())
    if query is None:
        return None
    query = _mapping(query, f"native lookup observations.queries[{name.lower()!r}]")
    if query.get("query", "").lower() != name.lower() or not isinstance(query.get("candidates"), list):
        raise RecipeMalformed("native lookup observation is malformed")
    return [_mapping(item, "native lookup candidate") for item in query["candidates"]]


def resolve(addressing: Addressing, observations: Mapping[str, Any], text: str) -> Resolution:
    """Resolve one spelling without falling back to a manual writer route."""
    parsed = _parse_input(text)
    if isinstance(parsed, Resolution):
        return parsed
    group, name = parsed
    lower = name.lower()
    candidates = [row for row in addressing.rows if row.name.lower() == lower]
    if not candidates:
        state = "owned_unsupported" if lower in addressing.owned_names else "outside_migrated_scope"
        return Resolution(state, reason=("source-owned row has no final addressing recipe"
                                         if state == "owned_unsupported"
                                         else "name is outside migrated source rows"))
    observed = _observed_candidates(observations, name)
    if observed is None:
        return Resolution("owned_unsupported", reason="native lookup observation is unavailable")
    by_identity = {row.identity: row for row in candidates}
    selected = []
    external = 0
    for candidate in observed:
        identity = _candidate_identity(candidate)
        row = by_identity.get(identity) if identity else None
        groups = candidate.get("groups")
        if not isinstance(groups, Mapping):
            raise RecipeMalformed("native lookup candidate lacks groups")
        if group is not None:
            wanted = group.lower()
            if wanted == "exif":
                matches = groups.get("0") == "EXIF"
            elif wanted == "ifd0":
                matches = groups.get("1") == "IFD0"
            else:
                return Resolution("owned_unsupported", reason="group qualifier is outside EXIF/IFD0 subset")
            if not matches:
                continue
        if row is None:
            external += 1
        else:
            selected.append(row)
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
    return tuple(accepted.values()), tuple(failures)


def render(addressing: Addressing) -> str:
    return json.dumps({"runtime_status": RUNTIME_STATUS, "rows": [asdict(row) for row in addressing.rows]},
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
    args.rows.write_text(json.dumps([asdict(row) for row in addressing.rows], sort_keys=True) + "\n",
                         encoding="utf-8")
    args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
