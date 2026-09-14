"""Persist source-authenticated ownership across writer-source upgrades.

The ledger is deliberately a terminal-routing artifact.  Its content is
derived from the selected Exif/Main source rows, never a hand-maintained tag
list.  A disappeared source row remains in the union with state ``removed``
until a separately designed retirement policy exists.
"""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
import re
from typing import Any, Mapping

from checkexif_recipes import RecipeMalformed, RecipeRefused
from setnewvalue_addressing import (_NAME, _capture_context, _fact, _mapping,
                                    compile_addressing)


SCHEMA = "setnewvalue_ownership_ledger_v1"
_HEX = re.compile(r"^[0-9a-f]{64}$")


def _canonical(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=False).encode("utf-8")


def _digest(value: Any) -> str:
    return hashlib.sha256(_canonical(value)).hexdigest()


def _identity(source: Mapping[str, Any]) -> str:
    return _digest(source)


def _current_source(document: Mapping[str, Any]) -> tuple[dict[str, Any], dict[tuple[str, str, str], str]]:
    """Authenticate the release then derive qualified ownership from its rows."""
    addressing, report = compile_addressing(document)
    capture = _capture_context(document)
    version = document.get("exiftool_version", capture["exiftool_version"])
    if not isinstance(version, str) or not version or version != capture["exiftool_version"]:
        raise RecipeRefused("ownership ledger ExifTool version disagrees with capture context")
    source = {
        "exiftool_version": version,
        "native_capture_context": capture,
        "source_rows_sha256": addressing.source_rows_sha256,
        "query_names_sha256": addressing.query_names_sha256,
        "find_tag_info": report["find_tag_info"],
        "set_new_value": report["set_new_value"],
    }
    tables = _mapping(document.get("native_write_tables"), "native_write_tables")
    table = _mapping(_mapping(tables.get("Exif"), "native_write_tables.Exif").get("Main"),
                     "native_write_tables.Exif.Main")
    rows = _mapping(table.get("rows"), "native_write_tables.Exif.Main.rows")
    fingerprints: dict[tuple[str, str, str], list[str]] = {}
    for raw in rows.values():
        raw = _mapping(raw, "source ownership row")
        properties = raw.get("effective_properties") or raw.get("properties")
        if not isinstance(properties, Mapping):
            continue
        name = properties.get("Name")
        if not isinstance(name, Mapping):
            continue
        present, value = _fact(name, "source ownership Name")
        if not present or not isinstance(value, str) or not _NAME.fullmatch(value):
            continue
        key = ("EXIF", "IFD0", value.lower())
        fingerprints.setdefault(key, []).append(_digest(raw))
    current = {key: _digest(sorted(values)) for key, values in fingerprints.items()}
    # compile_addressing's owned-name projection and this qualified source
    # projection must agree.  A mismatch is a source-shape change, not an
    # excuse to quietly drop a name from upgrade ownership.
    if {key[2] for key in current} != set(addressing.owned_names):
        raise RecipeRefused("qualified ownership projection disagrees with authenticated source names")
    source["ownership_fingerprints_sha256"] = _digest(
        [{"group0": key[0], "group1": key[1], "name": key[2], "fingerprint": current[key]}
         for key in sorted(current)])
    return source, current


def _source_valid(source: Any) -> dict[str, Any]:
    source = dict(_mapping(source, "ownership ledger source"))
    version = source.get("exiftool_version")
    if not isinstance(version, str) or not version:
        raise RecipeMalformed("ownership ledger source version is malformed")
    context = _capture_context({"native_capture_context": source.get("native_capture_context")})
    if context["exiftool_version"] != version:
        raise RecipeRefused("ownership ledger source capture identity disagrees")
    for key in ("source_rows_sha256", "query_names_sha256", "ownership_fingerprints_sha256"):
        if not isinstance(source.get(key), str) or not _HEX.fullmatch(source[key]):
            raise RecipeMalformed(f"ownership ledger source {key} is malformed")
    for key in ("find_tag_info", "set_new_value"):
        value = _mapping(source.get(key), f"ownership ledger source {key}")
        for field in ("source_file", "source_sha256", "body_sha256"):
            if not isinstance(value.get(field), str) or not value[field]:
                raise RecipeMalformed(f"ownership ledger source {key}.{field} is malformed")
        if not _HEX.fullmatch(value["source_sha256"]) or not _HEX.fullmatch(value["body_sha256"]):
            raise RecipeMalformed(f"ownership ledger source {key} hashes are malformed")
    return source


def _entry_key(entry: Mapping[str, Any]) -> tuple[str, str, str]:
    values = tuple(entry.get(key) for key in ("group0", "group1", "name"))
    if values[0:2] != ("EXIF", "IFD0") or not isinstance(values[2], str) or not _NAME.fullmatch(values[2]):
        raise RecipeMalformed("ownership ledger qualified name is malformed")
    return values  # type: ignore[return-value]


def validate_ledger(ledger: Mapping[str, Any]) -> dict[str, Any]:
    """Verify the digest chain and all source/history identities before reuse."""
    ledger = dict(_mapping(ledger, "ownership ledger"))
    if ledger.get("schema") != SCHEMA:
        raise RecipeRefused("ownership ledger schema is unsupported")
    digest = ledger.pop("ledger_sha256", None)
    if not isinstance(digest, str) or not _HEX.fullmatch(digest) or _digest(ledger) != digest:
        raise RecipeRefused("ownership ledger digest is tampered or stale")
    source = _source_valid(ledger.get("source"))
    identity = ledger.get("source_identity")
    if not isinstance(identity, str) or identity != _identity(source):
        raise RecipeRefused("ownership ledger source identity is tampered or stale")
    entries = ledger.get("entries")
    if not isinstance(entries, list):
        raise RecipeMalformed("ownership ledger entries are malformed")
    previous_key = None
    seen = set()
    for entry in entries:
        entry = _mapping(entry, "ownership ledger entry")
        key = _entry_key(entry)
        if key in seen or (previous_key is not None and key < previous_key):
            raise RecipeRefused("ownership ledger entries are not deterministic")
        seen.add(key)
        previous_key = key
        if entry.get("state") not in {"current", "removed"}:
            raise RecipeMalformed("ownership ledger entry state is malformed")
        for field in ("first_seen", "last_seen"):
            if not isinstance(entry.get(field), str) or not _HEX.fullmatch(entry[field]):
                raise RecipeMalformed(f"ownership ledger entry {field} is malformed")
        fingerprint = entry.get("source_fingerprint")
        if fingerprint is not None and (not isinstance(fingerprint, str) or not _HEX.fullmatch(fingerprint)):
            raise RecipeMalformed("ownership ledger entry fingerprint is malformed")
        history = entry.get("history")
        if not isinstance(history, list) or not history:
            raise RecipeMalformed("ownership ledger entry history is malformed")
        for event in history:
            event = _mapping(event, "ownership ledger history event")
            if event.get("state") not in {"current", "removed"} or not isinstance(event.get("source_identity"), str):
                raise RecipeMalformed("ownership ledger history event is malformed")
        if history[0]["source_identity"] != entry["first_seen"] or history[-1]["source_identity"] != entry["last_seen"]:
            raise RecipeRefused("ownership ledger entry history identity is inconsistent")
    sources = _mapping(ledger.get("sources"), "ownership ledger source history")
    for source_id, historical_source in sources.items():
        if not isinstance(source_id, str) or not _HEX.fullmatch(source_id) or _identity(_source_valid(historical_source)) != source_id:
            raise RecipeRefused("ownership ledger historical source identity is tampered or stale")
    if identity not in sources:
        raise RecipeRefused("ownership ledger current source is absent from source history")
    for entry in entries:
        for event in entry["history"]:
            if event["source_identity"] not in sources:
                raise RecipeRefused("ownership ledger history references an unauthenticated source")
    predecessor = ledger.get("predecessor")
    if predecessor is not None and (not isinstance(predecessor, str) or not _HEX.fullmatch(predecessor)):
        raise RecipeMalformed("ownership ledger predecessor is malformed")
    ledger["ledger_sha256"] = digest
    return ledger


def _event(state: str, identity: str) -> dict[str, str]:
    return {"state": state, "source_identity": identity}


def build_ledger(document: Mapping[str, Any], prior: Mapping[str, Any] | None, *, bootstrap: bool) -> dict[str, Any]:
    """Carry all prior ownership forward; bootstrap is an explicit one-time act."""
    source, current = _current_source(document)
    source_identity = _identity(source)
    if prior is None:
        if not bootstrap:
            raise RecipeRefused("ownership ledger bootstrap must be explicit when no prior ledger exists")
        previous_entries: dict[tuple[str, str, str], Mapping[str, Any]] = {}
        predecessor = None
        sources: dict[str, Any] = {}
    else:
        prior = validate_ledger(prior)
        # A regeneration from the identical authenticated source must be a
        # byte-for-byte no-op. Repointing predecessor to the previous ledger on
        # every run would manufacture an endless history chain without a source
        # upgrade, making the ownership artifact non-deterministic.
        if prior["source_identity"] == source_identity:
            if prior["source"] != source:
                raise RecipeRefused("ownership ledger source identity collides with different source facts")
            return prior
        previous_entries = {_entry_key(entry): entry for entry in prior["entries"]}
        predecessor = prior["ledger_sha256"]
        sources = {key: value for key, value in prior["sources"].items()}
    if source_identity in sources and sources[source_identity] != source:
        raise RecipeRefused("ownership ledger source identity collides with different source facts")
    sources[source_identity] = source
    entries = []
    for key in sorted(set(previous_entries) | set(current)):
        old = previous_entries.get(key)
        now_current = key in current
        state = "current" if now_current else "removed"
        if old is None:
            history = [_event("current", source_identity)]
            first_seen = source_identity
        else:
            history = [dict(event) for event in old["history"]]
            first_seen = old["first_seen"]
            if history[-1] != _event(state, source_identity):
                history.append(_event(state, source_identity))
        entries.append({
            "group0": key[0], "group1": key[1], "name": key[2], "state": state,
            "first_seen": first_seen, "last_seen": source_identity,
            "source_fingerprint": current.get(key), "history": history,
        })
    payload = {"schema": SCHEMA, "source": source, "source_identity": source_identity,
               "sources": {key: sources[key] for key in sorted(sources)},
               "predecessor": predecessor, "entries": entries}
    return {**payload, "ledger_sha256": _digest(payload)}


def owned_names(ledger: Mapping[str, Any]) -> frozenset[str]:
    return frozenset(entry["name"] for entry in validate_ledger(ledger)["entries"])


@dataclass(frozen=True)
class QualifiedOwnership:
    group0: str
    group1: str
    name: str
    removed: bool


def qualified_ownership(ledger: Mapping[str, Any]) -> tuple[QualifiedOwnership, ...]:
    return tuple(QualifiedOwnership(entry["group0"], entry["group1"], entry["name"],
                                    entry["state"] == "removed")
                 for entry in validate_ledger(ledger)["entries"])
