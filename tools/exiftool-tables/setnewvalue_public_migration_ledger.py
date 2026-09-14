"""Persist only public writer migrations composed from authenticated operands.

This ledger is intentionally distinct from ``setnewvalue_ownership_ledger``.
The latter is the whole selected Exif source inventory; using it as a public
routing fence would make every unimplemented source tag terminal.  This module
owns only identities for which *both* the authenticated SetNewValue addressing
compiler and the authenticated final-scalar compiler produced compatible
operands.  A previously public identity remains terminal if a later selected
release removes it or can no longer compile it.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass
import hashlib
import json
import re
from typing import Any, Mapping

from checkexif_recipes import RecipeMalformed, RecipeRefused
from final_scalar_stage import FinalScalarRecipe, FinalStageRefused, compile_final_scalar_stage
from scalar_helper_codegen import rust_string
from setnewvalue_address_rust_codegen import _source_capture_identity
from setnewvalue_addressing import AddressRow, Addressing, compile_addressing

SCHEMA = "setnewvalue_public_migration_ledger_v1"
_HEX = re.compile(r"^[0-9a-f]{64}$")
_RAW_ID = re.compile(r"^(?:0x[0-9a-fA-F]+|[0-9]+)$")


def _canonical(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def _digest(value: Any) -> str:
    return hashlib.sha256(_canonical(value)).hexdigest()


def _mapping(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise RecipeMalformed(f"{context} is not an object")
    return value


def _sha(value: Any, context: str) -> str:
    if not isinstance(value, str) or _HEX.fullmatch(value) is None:
        raise RecipeMalformed(f"{context} is not a SHA-256 digest")
    return value


def _number(raw_id: Any, context: str) -> int:
    if not isinstance(raw_id, str) or _RAW_ID.fullmatch(raw_id) is None:
        raise RecipeMalformed(f"{context} is not a native numeric tag id")
    value = int(raw_id, 0)
    if not 0 <= value <= 0xffff:
        raise RecipeMalformed(f"{context} is outside u16")
    return value


@dataclass(frozen=True)
class PublicMigrationEntry:
    module: str
    table: str
    full_name: str
    raw_tag_id: int
    name: str
    group0: str
    group1: str
    write_group: str
    source_control_sha256: str
    semantics_sha256: str

    @property
    def key(self) -> tuple[str, str, str, int, str, str, str, str]:
        return (self.module, self.table, self.full_name, self.raw_tag_id,
                self.name.lower(), self.group0, self.group1, self.write_group)


def _entry_from_pair(row: AddressRow, recipe: FinalScalarRecipe) -> PublicMigrationEntry:
    raw_tag_id = _number(row.raw_id, "address row raw_id")
    if (row.module, row.table, row.full_name, raw_tag_id, row.name) != (
            recipe.module, recipe.table, recipe.full_name, recipe.raw_tag_id, recipe.name):
        raise RecipeRefused("address/final row identity does not match")
    if row.group0 != recipe.table_group0 or row.write_group != recipe.physical_write_group:
        raise RecipeRefused("address/final group identity does not match")
    # The final recipe's conversion and source checks form part of public
    # admission.  A tag ID/name match alone must not carry a changed semantic
    # action through an upgrade.
    semantics = {
        "address": asdict(row),
        "final": asdict(recipe),
    }
    return PublicMigrationEntry(
        row.module, row.table, row.full_name, raw_tag_id, row.name,
        row.group0, row.group1, row.write_group, recipe.source_control_sha256,
        _digest(semantics),
    )


def _entry_key(entry: Mapping[str, Any]) -> tuple[str, str, str, int, str, str, str, str]:
    fields = ("module", "table", "full_name", "name", "group0", "group1", "write_group")
    values = {field: entry.get(field) for field in fields}
    if any(not isinstance(value, str) or not value for value in values.values()):
        raise RecipeMalformed("public migration entry identity is malformed")
    raw_tag_id = entry.get("raw_tag_id")
    if type(raw_tag_id) is not int or not 0 <= raw_tag_id <= 0xffff:
        raise RecipeMalformed("public migration entry raw_tag_id is malformed")
    return (values["module"], values["table"], values["full_name"], raw_tag_id,
            values["name"].lower(), values["group0"], values["group1"], values["write_group"])


def _current_from_authenticated(addressing: Addressing, recipes: list[FinalScalarRecipe],
                                capture: Mapping[str, str]) -> tuple[dict[str, Any], dict[tuple[str, str, str, int, str, str, str, str], PublicMigrationEntry]]:
    required = ("exiftool_version", "main_source_sha256", "write_exif_source_sha256",
                "writer_source_sha256", "exif_source_sha256")
    capture = {field: capture.get(field) for field in required}
    if not isinstance(capture["exiftool_version"], str) or not capture["exiftool_version"]:
        raise RecipeRefused("public migration capture release is unavailable")
    for field in required[1:]:
        _sha(capture[field], f"public migration capture {field}")
    # An address row has no right to become public unless every final recipe is
    # bound to the same WriteExif and Exif registry sources as the capture.
    by_identity: dict[tuple[str, str, str, int, str], list[AddressRow]] = {}
    for row in addressing.rows:
        key = (row.module, row.table, row.full_name, _number(row.raw_id, "address row raw_id"), row.name)
        by_identity.setdefault(key, []).append(row)
    current: dict[tuple[str, str, str, int, str, str, str, str], PublicMigrationEntry] = {}
    physical: dict[tuple[int, str], PublicMigrationEntry] = {}
    public_name: dict[tuple[str, str], PublicMigrationEntry] = {}
    for recipe in recipes:
        if recipe.write_proc_source_sha256 != capture["write_exif_source_sha256"]:
            raise RecipeRefused("final recipe WriteExif source does not join address capture")
        if recipe.writer_source_sha256 != capture["writer_source_sha256"]:
            raise RecipeRefused("final recipe Writer source does not join address capture")
        if recipe.registry_source_sha256 != capture["exif_source_sha256"]:
            raise RecipeRefused("final recipe TIFF registry source does not join address capture")
        candidates = by_identity.get((recipe.module, recipe.table, recipe.full_name,
                                      recipe.raw_tag_id, recipe.name), [])
        if len(candidates) != 1:
            raise RecipeRefused("final recipe lacks one exact source-address row")
        entry = _entry_from_pair(candidates[0], recipe)
        if entry.key in current:
            raise RecipeRefused("public migration intersection has duplicate source identity")
        physical_key = (entry.raw_tag_id, entry.write_group)
        if physical_key in physical:
            raise RecipeRefused("public migration intersection has duplicate physical identity")
        name_key = (entry.group0, entry.name.lower())
        if name_key in public_name:
            raise RecipeRefused("public migration intersection has duplicate public name identity")
        current[entry.key] = entry
        physical[physical_key] = entry
        public_name[name_key] = entry
    source = {
        "capture": capture,
        "address_rows_sha256": addressing.source_rows_sha256,
        "address_queries_sha256": addressing.query_names_sha256,
        "final_recipes_sha256": _digest([asdict(recipe) for recipe in sorted(
            recipes, key=lambda recipe: (recipe.module, recipe.table, recipe.full_name,
                                          recipe.raw_tag_id, recipe.name))]),
        "intersection_sha256": _digest([asdict(entry) for _, entry in sorted(current.items())]),
    }
    return source, current


def compile_current(document: Mapping[str, Any]) -> tuple[dict[str, Any], dict[tuple[str, str, str, int, str, str, str, str], PublicMigrationEntry]]:
    """Compile the current public set from one fully authenticated native dump."""
    try:
        addressing, _report = compile_addressing(document)
        capture = _source_capture_identity(document, addressing)
        recipes, _omissions, _registry = compile_final_scalar_stage(document)
    except (RecipeRefused, FinalStageRefused) as error:
        raise RecipeRefused(f"public migration source compilation refused: {error}") from error
    return _current_from_authenticated(addressing, recipes, capture)


def _source_identity(source: Mapping[str, Any]) -> str:
    return _digest(source)


def _source_valid(value: Any) -> dict[str, Any]:
    source = dict(_mapping(value, "public migration source"))
    capture = _mapping(source.get("capture"), "public migration capture")
    expected = ("exiftool_version", "main_source_sha256", "write_exif_source_sha256",
                "writer_source_sha256", "exif_source_sha256")
    if not isinstance(capture.get("exiftool_version"), str) or not capture["exiftool_version"]:
        raise RecipeMalformed("public migration capture release is malformed")
    for field in expected[1:]:
        _sha(capture.get(field), f"public migration capture {field}")
    for field in ("address_rows_sha256", "address_queries_sha256", "final_recipes_sha256", "intersection_sha256"):
        _sha(source.get(field), f"public migration source {field}")
    return source


def validate_ledger(ledger: Mapping[str, Any]) -> dict[str, Any]:
    ledger = dict(_mapping(ledger, "public migration ledger"))
    if ledger.get("schema") != SCHEMA:
        raise RecipeRefused("public migration ledger schema is unsupported")
    digest = ledger.pop("ledger_sha256", None)
    if not isinstance(digest, str) or _HEX.fullmatch(digest) is None or _digest(ledger) != digest:
        raise RecipeRefused("public migration ledger digest is tampered or stale")
    source = _source_valid(ledger.get("source"))
    source_identity = ledger.get("source_identity")
    if source_identity != _source_identity(source):
        raise RecipeRefused("public migration ledger source identity is tampered or stale")
    sources = _mapping(ledger.get("sources"), "public migration source history")
    for identity, historical in sources.items():
        if not isinstance(identity, str) or _HEX.fullmatch(identity) is None:
            raise RecipeMalformed("public migration historical source identity is malformed")
        if _source_identity(_source_valid(historical)) != identity:
            raise RecipeRefused("public migration historical source is tampered or stale")
    if source_identity not in sources:
        raise RecipeRefused("public migration current source is absent from source history")
    entries = ledger.get("entries")
    if not isinstance(entries, list):
        raise RecipeMalformed("public migration entries are malformed")
    prior_key = None
    seen = set()
    for entry in entries:
        entry = _mapping(entry, "public migration entry")
        key = _entry_key(entry)
        if key in seen or (prior_key is not None and key < prior_key):
            raise RecipeRefused("public migration entries are not deterministic")
        seen.add(key); prior_key = key
        if entry.get("state") not in {"current", "removed_or_unsupported"}:
            raise RecipeMalformed("public migration entry state is malformed")
        _sha(entry.get("source_control_sha256"), "public migration entry source control")
        _sha(entry.get("semantics_sha256"), "public migration entry semantics")
        history = entry.get("history")
        if not isinstance(history, list) or not history:
            raise RecipeMalformed("public migration entry history is malformed")
        for event in history:
            event = _mapping(event, "public migration history event")
            if event.get("state") not in {"current", "removed_or_unsupported"}:
                raise RecipeMalformed("public migration history state is malformed")
            if event.get("source_identity") not in sources:
                raise RecipeRefused("public migration history references unauthenticated source")
            _sha(event.get("semantics_sha256"), "public migration history semantics")
        if entry.get("first_seen") != history[0]["source_identity"] or entry.get("last_seen") != history[-1]["source_identity"]:
            raise RecipeRefused("public migration history boundaries are inconsistent")
    predecessor = ledger.get("predecessor")
    if predecessor is not None:
        _sha(predecessor, "public migration predecessor")
    ledger["ledger_sha256"] = digest
    return ledger


def _event(state: str, source_identity: str, semantics: str) -> dict[str, str]:
    return {"state": state, "source_identity": source_identity, "semantics_sha256": semantics}


def build_from_current(source: Mapping[str, Any], current: Mapping[tuple[str, str, str, int, str, str, str, str], PublicMigrationEntry],
                       prior: Mapping[str, Any] | None, *, bootstrap: bool) -> dict[str, Any]:
    """Merge a validated public intersection with prior terminal history."""
    source = _source_valid(source)
    source_identity = _source_identity(source)
    if prior is None:
        if not bootstrap:
            raise RecipeRefused("public migration ledger bootstrap must be explicit when no prior ledger exists")
        old, sources, predecessor = {}, {}, None
    else:
        prior = validate_ledger(prior)
        if prior["source_identity"] == source_identity:
            if prior["source"] != source:
                raise RecipeRefused("public migration source identity collides with different facts")
            return prior
        old = {_entry_key(entry): entry for entry in prior["entries"]}
        sources = dict(prior["sources"])
        predecessor = prior["ledger_sha256"]
    if source_identity in sources and sources[source_identity] != source:
        raise RecipeRefused("public migration source identity collides with different facts")
    sources[source_identity] = source
    entries = []
    for key in sorted(set(old) | set(current)):
        previous = old.get(key)
        now = current.get(key)
        state = "current" if now is not None else "removed_or_unsupported"
        if previous is None:
            history = [_event(state, source_identity, now.semantics_sha256)]
            first_seen = source_identity
        else:
            history = [dict(event) for event in previous["history"]]
            first_seen = previous["first_seen"]
            semantic = now.semantics_sha256 if now is not None else previous["semantics_sha256"]
            event = _event(state, source_identity, semantic)
            if history[-1] != event:
                history.append(event)
        entry = asdict(now) if now is not None else {field: previous[field] for field in (
            "module", "table", "full_name", "raw_tag_id", "name", "group0", "group1", "write_group",
            "source_control_sha256", "semantics_sha256")}
        entry.update({"state": state, "first_seen": first_seen, "last_seen": source_identity, "history": history})
        entries.append(entry)
    payload = {"schema": SCHEMA, "source": source, "source_identity": source_identity,
               "sources": {key: sources[key] for key in sorted(sources)}, "predecessor": predecessor,
               "entries": entries}
    return {**payload, "ledger_sha256": _digest(payload)}


def build_ledger(document: Mapping[str, Any], prior: Mapping[str, Any] | None, *, bootstrap: bool) -> dict[str, Any]:
    source, current = compile_current(document)
    return build_from_current(source, current, prior, bootstrap=bootstrap)


def render_rust(ledger: Mapping[str, Any]) -> str:
    """Render public-routing operands; callers still decide when to activate."""
    ledger = validate_ledger(ledger)
    capture = ledger["source"]["capture"]
    lines = ["// @generated by setnewvalue_public_migration_ledger.py; routing operands only.\n",
             "pub(crate) struct StaticPublicSetNewValueMigration { pub module: &'static str, pub table: &'static str, pub full_name: &'static str, pub raw_tag_id: u16, pub name: &'static str, pub group0: &'static str, pub group1: &'static str, pub write_group: &'static str, pub source_control_sha256: &'static str, pub semantics_sha256: &'static str, pub removed_or_unsupported: bool }\n",
             "pub(crate) struct StaticPublicSetNewValueMigrationCapture { pub exiftool_version: &'static str, pub main_source_sha256: &'static str, pub write_exif_source_sha256: &'static str, pub writer_source_sha256: &'static str, pub exif_source_sha256: &'static str, pub address_rows_sha256: &'static str, pub address_queries_sha256: &'static str, pub final_recipes_sha256: &'static str, pub intersection_sha256: &'static str }\n",
             "pub(crate) const PUBLIC_SET_NEW_VALUE_MIGRATIONS: &[StaticPublicSetNewValueMigration] = &[\n"]
    for entry in ledger["entries"]:
        lines.append("    StaticPublicSetNewValueMigration { module: %s, table: %s, full_name: %s, raw_tag_id: 0x%04x, name: %s, group0: %s, group1: %s, write_group: %s, source_control_sha256: %s, semantics_sha256: %s, removed_or_unsupported: %s },\n" % (
            rust_string(entry["module"]), rust_string(entry["table"]), rust_string(entry["full_name"]), entry["raw_tag_id"],
            rust_string(entry["name"]), rust_string(entry["group0"]), rust_string(entry["group1"]), rust_string(entry["write_group"]),
            rust_string(entry["source_control_sha256"]), rust_string(entry["semantics_sha256"]),
            "true" if entry["state"] == "removed_or_unsupported" else "false"))
    lines.extend(["];\n",
        "pub(crate) const PUBLIC_SET_NEW_VALUE_MIGRATION_CAPTURE: StaticPublicSetNewValueMigrationCapture = StaticPublicSetNewValueMigrationCapture { exiftool_version: %s, main_source_sha256: %s, write_exif_source_sha256: %s, writer_source_sha256: %s, exif_source_sha256: %s, address_rows_sha256: %s, address_queries_sha256: %s, final_recipes_sha256: %s, intersection_sha256: %s };\n" % tuple(rust_string(capture[key]) if key in capture else rust_string(ledger["source"][key]) for key in (
            "exiftool_version", "main_source_sha256", "write_exif_source_sha256", "writer_source_sha256", "exif_source_sha256", "address_rows_sha256", "address_queries_sha256", "final_recipes_sha256", "intersection_sha256")),
        "pub(crate) const PUBLIC_SET_NEW_VALUE_MIGRATION_LEDGER_SHA256: &str = %s;\n" % rust_string(ledger["ledger_sha256"]),
    ])
    return "".join(lines)


def main() -> None:
    import argparse
    from pathlib import Path
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tables", type=Path)
    parser.add_argument("-o", "--output", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--prior-ledger", type=Path)
    parser.add_argument("--write-ledger", type=Path, required=True)
    parser.add_argument("--bootstrap", action="store_true")
    args = parser.parse_args()
    document = json.loads(args.tables.read_text(encoding="utf-8"))
    prior = json.loads(args.prior_ledger.read_text(encoding="utf-8")) if args.prior_ledger else None
    ledger = build_ledger(document, prior, bootstrap=args.bootstrap)
    rust = render_rust(ledger)
    report = {"schema": SCHEMA, "ledger_sha256": ledger["ledger_sha256"],
              "current": sum(entry["state"] == "current" for entry in ledger["entries"]),
              "removed_or_unsupported": sum(entry["state"] == "removed_or_unsupported" for entry in ledger["entries"]),
              "source": ledger["source"]}
    # Compute every output first: malformed prior/current input cannot leave a
    # partially refreshed public boundary on disk.
    args.output.write_text(rust, encoding="utf-8")
    args.report.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    args.write_ledger.write_text(json.dumps(ledger, sort_keys=True, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
