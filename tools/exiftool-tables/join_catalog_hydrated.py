#!/usr/bin/env python3
"""Join native catalog entries to pinned hydrated source rows.

The output is an inventory ledger. It does not claim generated or observed
reader/writer support.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
from collections import Counter, defaultdict
from pathlib import Path
import runtime_evidence_inputs as runtime_inputs
import tempfile
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
import quicktime_atom_tables as quicktime_selector
import quicktime_generated_specs as quicktime_specs
import final_scalar_stage
import setnewvalue_public_migration_ledger as public_migration
import quicktime_baseline as baseline

SCHEMA = "oxidex_catalog_hydrated_join_v2"
CATALOG_SCHEMA = "oxidex_hydrated_catalog_universe_v1"
QUICKTIME_READ_EVIDENCE_SCHEMA = "oxidex_quicktime_generated_read_evidence_v1"


def writer_implementation(source: bytes | None, final_ledger: dict | None, final_rust: str | None,
                          public_ledger: dict | None, public_rust: str | None) -> dict[tuple[str, str, int], dict]:
    """Replay the authenticated writer compilers before indexing public rows.

    This is declaration accounting only: no native write execution is claimed.
    """
    supplied = (source, final_ledger, final_rust, public_ledger, public_rust)
    if all(value is None for value in supplied):
        return {}
    if any(value is None for value in supplied) or not isinstance(source, bytes) or not isinstance(final_rust, str) or not isinstance(public_rust, str):
        raise ValueError("writer replay requires source, final ledger/Rust, and public ledger/Rust together")
    try:
        document = json.loads(source)
    except json.JSONDecodeError as exc:
        raise ValueError("writer source is malformed") from exc
    if not isinstance(document, dict):
        raise ValueError("writer source is malformed")
    expected_final_rust, expected_final = final_scalar_stage.generate(document)
    # Dataclass tuple operands become arrays in the committed JSON artifact.
    expected_final = json.loads(json.dumps(expected_final))
    if final_ledger != expected_final or not quicktime_rust_matches(expected_final_rust, final_rust):
        raise ValueError("writer final artifacts differ from authenticated source replay")
    try:
        compiled_source, current = public_migration.compile_current(document)
        public_migration.validate_ledger(public_ledger)
    except public_migration.RecipeRefused as exc:
        raise ValueError("writer public ledger is malformed") from exc
    if public_ledger.get("source") != compiled_source:
        raise ValueError("writer public ledger source closure differs from authenticated replay")
    if not quicktime_rust_matches(public_migration.render_rust(public_ledger), public_rust):
        raise ValueError("writer public Rust artifact differs from authenticated ledger")
    recipes = {(row["full_name"], row["raw_tag_id"], row["name"], row["physical_write_group"])
               for row in expected_final.get("recipes", [])}
    rows = {}
    for entry in public_ledger["entries"]:
        if entry.get("state") != "current":
            continue
        try:
            expected = current.get(public_migration._entry_key(entry))
        except public_migration.RecipeMalformed as exc:
            raise ValueError("writer public ledger identity is malformed") from exc
        if (expected is None or entry["full_name"] != expected.full_name or entry["name"] != expected.name
                or entry.get("source_control_sha256") != expected.source_control_sha256
                or entry.get("semantics_sha256") != expected.semantics_sha256):
            raise ValueError("writer public ledger identity differs from authenticated replay")
        recipe_key = (entry["full_name"], entry["raw_tag_id"], entry["name"], entry["write_group"])
        if recipe_key not in recipes:
            raise ValueError("writer public ledger is absent from final scalar recipes")
        identity = (entry["full_name"], str(entry["raw_tag_id"]), 0)
        if identity in rows:
            raise ValueError("writer public ledger has duplicate catalog identity")
        rows[identity] = {"name": entry["name"], "write_group": entry["write_group"],
                          "semantics_sha256": entry["semantics_sha256"]}
    if len(rows) != len(recipes):
        raise ValueError("writer public/final recipe conservation failed")
    return rows


def canonical_hash(value: object) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, ensure_ascii=True,
                                     separators=(",", ":")).encode("ascii")).hexdigest()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def read_json(path: Path) -> dict:
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def require_mapping(value: object, label: str) -> dict:
    if not isinstance(value, dict):
        raise ValueError(f"{label} is missing or malformed")
    return value


def validate_catalog(catalog: dict) -> dict[tuple[str, str, int], dict]:
    if catalog.get("schema") != CATALOG_SCHEMA:
        raise ValueError("unsupported catalog schema")
    entries, counts = catalog.get("entries"), require_mapping(catalog.get("counts"), "catalog counts")
    if not isinstance(entries, list):
        raise ValueError("catalog entries are missing or malformed")
    if len(entries) != counts.get("catalog_total_tag_entries"):
        raise ValueError("catalog_total_tag_entries conservation failed")
    names = catalog.get("unique_names")
    if not isinstance(names, list) or not all(isinstance(name, str) for name in names) or len(set(names)) != len(names):
        raise ValueError("catalog unique_names are missing, malformed, or duplicated")
    if set(names) != {entry.get("normalized_name") for entry in entries}:
        raise ValueError("catalog unique_names conservation failed")
    if len(names) != counts.get("distinct_case_insensitive_entry_names"):
        raise ValueError("catalog distinct-case-insensitive-name conservation failed")
    if not isinstance(counts.get("catalog_unique_tag_names"), int):
        raise ValueError("catalog native unique-name counter is missing or malformed")
    identities: dict[tuple[str, str, int], dict] = {}
    for entry in entries:
        if not isinstance(entry, dict):
            raise ValueError("catalog entry is malformed")
        table, raw_key, variant = entry.get("table"), entry.get("raw_key"), entry.get("variant_index")
        name, normalized = entry.get("name"), entry.get("normalized_name")
        if (not isinstance(table, str) or not table or not isinstance(raw_key, str)
                or type(variant) is not int or variant < 0 or not isinstance(name, str)
                or not name or not name.isascii() or normalized != name.lower()):
            raise ValueError("catalog entry identity is malformed")
        if set(require_mapping(entry.get("groups"), "catalog entry groups")) != {"0", "1", "2"}:
            raise ValueError("catalog entry is missing group identity")
        identity = (table, raw_key, variant)
        if identity in identities:
            raise ValueError(f"duplicate catalog identity: {identity!r}")
        identities[identity] = entry
    return identities


def source_rows(hydrated: dict) -> tuple[dict[tuple[str, str, int], dict], dict[str, str]]:
    layouts = require_mapping(hydrated.get("hydrated_layouts"), "hydrated layouts")
    tables = require_mapping(layouts.get("tables"), "hydrated tables")
    rows, table_hashes = {}, {}
    for table_name, table in tables.items():
        if not isinstance(table_name, str) or not isinstance(table, dict) or table.get("full_name") != table_name:
            raise ValueError(f"hydrated table identity mismatch: {table_name}")
        tags = require_mapping(table.get("tags"), f"hydrated tags for {table_name}")
        table_hashes[table_name] = canonical_hash(table)
        for raw_key, row in tags.items():
            if not isinstance(raw_key, str) or not isinstance(row, dict):
                raise ValueError(f"hydrated row identity is malformed: {table_name!r}")
            variants = row.get("_variants", [row])
            if not isinstance(variants, list) or not variants:
                raise ValueError(f"hydrated variants are malformed: {(table_name, raw_key)!r}")
            for variant_index, variant in enumerate(variants):
                if not isinstance(variant, dict):
                    raise ValueError(f"hydrated variant is malformed: {(table_name, raw_key, variant_index)!r}")
                identity = (table_name, raw_key, variant_index)
                if identity in rows:
                    raise ValueError(f"duplicate hydrated source identity: {identity!r}")
                rows[identity] = variant
    return rows, table_hashes


def quicktime_groups(value: object, shared_references: dict | None, label: str) -> dict[str, str]:
    if isinstance(value, dict) and set(value) == {"__ref", "object_id"}:
        if value.get("__ref") != "HASH" or not isinstance(value.get("object_id"), str) or not shared_references:
            raise ValueError(f"QuickTime {label} reference is malformed")
        target = shared_references.get(value["object_id"])
        if (not isinstance(target, dict) or target.get("kind") != "HASH"
                or not isinstance(target.get("properties"), dict)):
            raise ValueError(f"QuickTime {label} reference is missing or malformed")
        value = target["properties"]
    if not isinstance(value, dict) or any(not isinstance(key, str) or not isinstance(item, str)
                                          for key, item in value.items()):
        raise ValueError(f"QuickTime {label} is malformed")
    return dict(value)


def normalize_quicktime_groups(projected: dict, table_meta: dict | None,
                               shared_references: dict | None) -> None:
    if "Groups" not in projected:
        return
    if not isinstance(table_meta, dict) or "GROUPS" not in table_meta:
        raise ValueError("QuickTime table Groups defaults are missing or malformed")
    defaults = quicktime_groups(table_meta["GROUPS"], shared_references, "table Groups")
    groups = quicktime_groups(projected["Groups"], shared_references, "row Groups")
    groups = {key: value for key, value in groups.items() if defaults.get(key) != value}
    if groups:
        projected["Groups"] = groups
    else:
        projected.pop("Groups")


def quicktime_selector_projection(table: str, raw_key: str, row: dict, *, table_meta: dict | None = None,
                                  shared_references: dict | None = None,
                                  variant_path: tuple[int, ...] = ()) -> dict:
    """Invert verified wrappers and inherited defaults before selector hashing."""
    if not isinstance(row, dict):
        raise ValueError("QuickTime hydrated row is malformed")
    projected = dict(row)
    if {"TagID", "Table", "_extra_properties"} & set(projected):
        tag_id = projected.pop("TagID", raw_key)
        table_fact = projected.pop("Table", None)
        if tag_id != raw_key or not isinstance(table_fact, dict) or table not in table_fact.get("table_full_names", []):
            raise ValueError("QuickTime hydrated wrapper identity is malformed")
    extras = projected.pop("_extra_properties", {})
    if not isinstance(extras, dict):
        raise ValueError("QuickTime hydrated wrapper has unprojected source properties")
    extras = dict(extras)
    index = extras.pop("Index", None)
    if index is not None:
        if not variant_path or index != str(variant_path[-1]):
            raise ValueError("QuickTime wrapper Index differs from its variant path")
    semantic = set(extras) - {"GotGroups", "Preferred"}
    if any(key in projected for key in semantic):
        raise ValueError("QuickTime wrapper conflicts with explicit source property")
    if semantic:
        existing = projected.get("_extra_keys", [])
        if not isinstance(existing, list) or not all(isinstance(key, str) for key in existing):
            raise ValueError("QuickTime source extra-key projection is malformed")
        projected["_extra_keys"] = sorted(set(existing) | semantic)
    normalize_quicktime_groups(projected, table_meta, shared_references)
    return projected


def validate_provenance(catalog: dict, hydrated: dict) -> None:
    catalog_sources = require_mapping(require_mapping(catalog.get("producer"), "catalog producer").get("sources"),
                                      "catalog producer sources")
    layouts = require_mapping(hydrated.get("hydrated_layouts"), "hydrated layouts")
    hydrated_sources = require_mapping(require_mapping(layouts.get("source_provenance"), "hydrated source provenance").get("sources"),
                                       "hydrated source manifest")
    if not catalog_sources:
        raise ValueError("catalog producer sources are empty")
    for key, fact in catalog_sources.items():
        if key not in hydrated_sources:
            raise ValueError(f"catalog source is absent from hydrated manifest: {key}")
        if hydrated_sources[key] != fact:
            raise ValueError(f"source provenance mismatch: {key}")


def quicktime_rust_matches(expected: str, supplied: str) -> bool:
    """Accept the replayed artifact, allowing only rustfmt-equivalent layout."""
    if supplied == expected:
        return True
    rustfmt = shutil.which("rustfmt")
    if rustfmt is None:
        return False
    formatted = []
    for source in (expected, supplied):
        result = subprocess.run([rustfmt, "--edition", "2024", "--emit", "stdout"], input=source,
                                text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if result.returncode:
            return False
        formatted.append(result.stdout)
    return formatted[0] == formatted[1]


def quicktime_implementation(itemlist_ledger: dict | None, capabilities: dict | None,
                             bounded_source: bytes | None, emitted_rust: str | None) -> dict[tuple[str, str, str, tuple[int, ...]], dict]:
    """Replay complete bounded artifacts before indexing their QuickTime identities."""
    if all(value is None for value in (itemlist_ledger, capabilities, bounded_source, emitted_rust)):
        return {}
    if any(value is None for value in (itemlist_ledger, capabilities, bounded_source, emitted_rust)):
        raise ValueError("QuickTime replay requires bounded source, ledger, capabilities, and Rust artifact together")
    if not isinstance(bounded_source, bytes) or not isinstance(emitted_rust, str):
        raise ValueError("QuickTime bounded source or Rust artifact is malformed")
    bounded = json.loads(bounded_source)
    if not isinstance(bounded, dict):
        raise ValueError("QuickTime bounded source is malformed")
    expected_ledger = quicktime_specs.compile_document(bounded)
    expected_capabilities = quicktime_selector.report(bounded_source)
    expected_rust = quicktime_specs.render_rust(expected_ledger)
    if itemlist_ledger != expected_ledger:
        raise ValueError("QuickTime generated ledger differs from complete bounded-source replay")
    if capabilities != expected_capabilities:
        raise ValueError("QuickTime capabilities differ from complete bounded-source replay")
    if not quicktime_rust_matches(expected_rust, emitted_rust):
        raise ValueError("QuickTime Rust artifact differs from bounded-source replay")
    tables = bounded["modules"]["QuickTime"]["tables"]
    spec_names = {(spec["source_identity"]["table"], spec["source_identity"]["raw_key"],
                   spec["source_identity"]["source_sha256"], tuple(spec["source_identity"]["variant_path"])): spec["name"]
                  for spec in expected_ledger["specs"]}
    rows = {}
    for record in expected_ledger["ledger"]:
        identity = record.get("identity", {})
        if not isinstance(identity, dict) or identity.get("module") != "QuickTime":
            raise ValueError("QuickTime generated ledger identity is malformed")
        path = identity.get("variant_path")
        key = (identity.get("table"), identity.get("raw_key"), identity.get("source_sha256"), tuple(path) if isinstance(path, list) and all(type(v) is int and v >= 0 for v in path) else None)
        if not all(isinstance(value, str) and value for value in key[:3]) or key[3] is None or key in rows:
            raise ValueError("QuickTime generated ledger identity is duplicated or malformed")
        try:
            source = tables[key[0]]["tags"][key[1]]
            for candidate_path, candidate, _ in quicktime_selector.variants(source):
                if candidate_path == key[3]:
                    if quicktime_selector.digest(candidate) != key[2]:
                        raise ValueError("QuickTime ledger does not match bounded source identity")
                    projected = quicktime_selector_projection("Image::ExifTool::QuickTime::" + key[0], key[1], candidate,
                                                              table_meta=tables[key[0]].get("meta"), variant_path=key[3])
                    key = (key[0], key[1], quicktime_selector.digest(quicktime_selector.semantic_normal_form(projected)), key[3])
                    break
            else:
                raise ValueError("QuickTime ledger variant is absent from bounded source")
        except (KeyError, TypeError):
            raise ValueError("QuickTime bounded source identity is malformed") from None
        if key in rows:
            raise ValueError("QuickTime semantic identity collision")
        rows[key] = {"generated": record.get("generated") is True, "reasons": record.get("reasons"),
                     "source_identity": identity,
                     "name": spec_names.get((identity["table"], identity["raw_key"], identity["source_sha256"], tuple(identity["variant_path"]))) }
    if sum(value["generated"] for value in rows.values()) != expected_ledger["identity_counts"]["generated"]:
        raise ValueError("QuickTime generated acceptance denominator is inconsistent")
    for family in expected_capabilities["families"]:
        for record in family.get("records", []):
            identity = record.get("identity", {})
            path = identity.get("variant_path")
            key = (identity.get("table"), identity.get("raw_key"), identity.get("source_sha256"), tuple(path) if isinstance(path, list) else None)
            if key in rows:
                rows[key]["selector_reasons"] = record.get("reasons")
    return rows


def quicktime_observed_reads(evidence: dict | None, input_digests: dict[str, str] | None, specs: list | None = None) -> dict[tuple[str, str, str, tuple[int, ...]], str]:
    """Accept only immutable, artifact-bound matched read identities."""
    if evidence is None:
        return {}
    if input_digests is None:
        raise ValueError("QuickTime read evidence requires complete generated artifact inputs")
    if evidence.get("schema") != QUICKTIME_READ_EVIDENCE_SCHEMA:
        raise ValueError("QuickTime read evidence schema is unsupported or historical")
    producer = require_mapping(evidence.get("producer"), "QuickTime read evidence producer")
    if producer.get("source_dirty") is not False:
        raise ValueError("QuickTime read evidence is not from an immutable clean source")
    for key in ("source_commit", "source_fingerprint", "runtime_input_manifest_sha256", "runtime_artifact_sha256", "fixture_manifest_sha256"):
        if not isinstance(producer.get(key), str) or not re.fullmatch(r"[a-f0-9]{40,64}", producer[key]):
            raise ValueError("QuickTime read evidence producer binding is malformed")
    if producer.get("pin") != (quicktime_selector.ROOT / ".exiftool-version").read_text().strip():
        raise ValueError("QuickTime read evidence pin differs from repository pin")
    if producer["runtime_input_manifest_sha256"] != runtime_inputs.runtime_input_manifest(quicktime_selector.ROOT):
        raise ValueError("QuickTime read evidence runtime input manifest differs from current inputs")
    if evidence.get("inputs") != dict(sorted(input_digests.items())):
        raise ValueError("QuickTime read evidence generated artifact binding differs")
    from verify_quicktime_reader import observation_evidence
    if specs is None or not isinstance(evidence.get("observations"), list):
        raise ValueError("QuickTime read evidence requires generated specs and native observations")
    occurrences, derived, fixture_digest = observation_evidence(evidence["observations"], specs)
    if (evidence.get("matched_occurrences") != occurrences or evidence.get("observed_identities") != derived
            or producer["fixture_manifest_sha256"] != fixture_digest):
        raise ValueError("QuickTime read evidence claims differ from native observations")
    observations = evidence.get("observed_identities")
    if not isinstance(observations, list):
        raise ValueError("QuickTime read evidence identities are missing or malformed")
    identities = {}
    for observation in observations:
        if not isinstance(observation, dict) or observation.get("matched") is not True:
            raise ValueError("QuickTime read evidence contains an unmatched identity")
        identity = require_mapping(observation.get("source_identity"), "QuickTime observed source identity")
        path = identity.get("variant_path")
        key = (identity.get("table"), identity.get("raw_key"), identity.get("source_sha256"),
               tuple(path) if isinstance(path, list) and all(type(item) is int and item >= 0 for item in path) else None)
        if (not all(isinstance(item, str) and item for item in key[:3]) or key[3] is None
                or observation.get("group1") != "ItemList" or not isinstance(observation.get("tag_name"), str)
                or not observation["tag_name"]):
            raise ValueError("QuickTime observed read identity is malformed")
        if key in identities:
            raise ValueError("QuickTime observed read identity is duplicated")
        identities[key] = observation["tag_name"]
    return identities


def build(catalog: dict, hydrated: dict, catalog_sha: str, hydrated_sha: str,
          itemlist_ledger: dict | None = None, quicktime_capabilities: dict | None = None,
          quicktime_bounded_source: bytes | None = None, quicktime_rust: str | None = None,
          quicktime_input_digests: dict[str, str] | None = None,
          quicktime_read_evidence: dict | None = None,
          writer_source: bytes | None = None, writer_final_ledger: dict | None = None,
          writer_final_rust: str | None = None, writer_public_ledger: dict | None = None,
          writer_public_rust: str | None = None, writer_input_digests: dict[str, str] | None = None) -> dict:
    if catalog.get("exiftool_version") != hydrated.get("exiftool_version"):
        raise ValueError("catalog and hydrated ExifTool versions differ")
    supplied_quicktime = (itemlist_ledger, quicktime_capabilities, quicktime_bounded_source, quicktime_rust)
    if any(value is not None for value in supplied_quicktime):
        if any(value is None for value in supplied_quicktime):
            raise ValueError("QuickTime join inputs must be supplied together")
        bounded = json.loads(quicktime_bounded_source)
        if not isinstance(bounded, dict):
            raise ValueError("QuickTime bounded source is malformed")
        pin = (quicktime_selector.ROOT / ".exiftool-version").read_text().strip()
        if catalog.get("exiftool_version") != pin or bounded.get("exiftool_version") != pin:
            raise ValueError("QuickTime bounded source, catalog, or hydrated version differs from repository pin")
        if quicktime_input_digests is None or set(quicktime_input_digests) != {"source_sha256", "ledger_sha256", "capabilities_sha256", "rust_sha256"}:
            raise ValueError("QuickTime input digests are incomplete")
        if any(not isinstance(value, str) or not re.fullmatch(r"[a-f0-9]{64}", value)
               for value in quicktime_input_digests.values()):
            raise ValueError("QuickTime input digest is malformed")
    catalog_by_id = validate_catalog(catalog)
    validate_provenance(catalog, hydrated)
    hydrated_by_id, table_hashes = source_rows(hydrated)
    quicktime = quicktime_implementation(*supplied_quicktime)
    writer = writer_implementation(writer_source, writer_final_ledger, writer_final_rust, writer_public_ledger, writer_public_rust)
    if writer and (writer_input_digests is None or set(writer_input_digests) != {"source_sha256", "final_ledger_sha256", "final_rust_sha256", "public_ledger_sha256", "public_rust_sha256"}):
        raise ValueError("writer input digests are incomplete")
    if writer:
        capture = writer_public_ledger.get("source", {}).get("capture", {})
        loaded = json.loads(writer_source).get("native_write_capture_context", {}).get("loaded_modules", {})
        sources = catalog["producer"]["sources"]
        expected_paths = {"main_source_sha256": "Image/ExifTool.pm", "exif_source_sha256": "Image/ExifTool/Exif.pm",
                          "writer_source_sha256": "Image/ExifTool/Writer.pl", "write_exif_source_sha256": "Image/ExifTool/WriteExif.pl"}
        if (not isinstance(capture, dict) or capture.get("exiftool_version") != catalog["exiftool_version"] or not isinstance(loaded, dict)):
            raise ValueError("writer source version or native closure is malformed")
        for field, path in expected_paths.items():
            digest, source_fact = capture.get(field), sources.get(path)
            if not isinstance(digest, str) or loaded.get(path) != digest or not isinstance(source_fact, dict) or source_fact.get("sha256") != digest:
                raise ValueError("writer source closure differs from catalog/hydrated provenance")
    observed_quicktime = quicktime_observed_reads(quicktime_read_evidence, quicktime_input_digests,
                                                 itemlist_ledger["specs"] if itemlist_ledger else None)
    hydrated_count = require_mapping(hydrated["hydrated_layouts"].get("catalog_counts"), "hydrated catalog counts").get("total_tag_entries")
    if hydrated_count != len(catalog_by_id):
        raise ValueError("hydrated total_tag_entries differs from catalog denominator")
    records, status_counts, implementation_counts, reader_counts, writer_counts, observed_counts, family_counts = [], Counter(), Counter(), Counter(), Counter(), Counter(), defaultdict(Counter)
    for identity in sorted(catalog_by_id):
        entry, source = catalog_by_id[identity], hydrated_by_id.get(identity)
        if source is None:
            state, status, source_name, row_hash, table_hash = "absent", "source_row_absent", None, None, None
        else:
            source_name = source.get("Name")
            if isinstance(source_name, str) and source_name == entry["name"]:
                state, status = "joined", "source_row_joined"
            else:
                state, status = "conflict", "source_row_name_conflict"
            row_hash, table_hash = canonical_hash(source), table_hashes[identity[0]]
        status_counts[status] += 1
        family_counts[entry["groups"]["1"]][status] += 1
        implementation = reader_implementation = "source_row_not_yet_consumed"
        writer_state = "writer_not_declared"
        refusal = None
        selector_hash = None
        observed_read = "not_observed_yet"
        if source is not None and identity[0].startswith("Image::ExifTool::QuickTime::"):
            variant_path = (identity[2],) if "_variants" in hydrated["hydrated_layouts"]["tables"][identity[0]]["tags"][identity[1]] else ()
            table_document = hydrated["hydrated_layouts"]["tables"][identity[0]]
            shared_references = hydrated["hydrated_layouts"].get("shared_reference_objects", {})
            if not isinstance(shared_references, dict):
                raise ValueError("hydrated shared reference objects are missing or malformed")
            projected = quicktime_selector_projection(identity[0], identity[1], source,
                                                       table_meta=table_document.get("meta"),
                                                       shared_references=shared_references,
                                                       variant_path=variant_path)
            selector_hash = quicktime_selector.digest(quicktime_selector.semantic_normal_form(projected))
            candidate = quicktime.get((identity[0].rsplit("::", 1)[-1], identity[1], selector_hash, variant_path))
            if candidate is not None and state == "joined":
                if candidate["generated"]:
                    implementation = reader_implementation = "generated_reader_declaration_unobserved"
                    source_identity = candidate["source_identity"]
                    evidence_identity = (source_identity["table"], source_identity["raw_key"],
                                         source_identity["source_sha256"], tuple(source_identity["variant_path"]))
                    if observed_quicktime.get(evidence_identity) == candidate["name"]:
                        observed_read = "observed_matched_read"
                else:
                    implementation = reader_implementation = "blocked_generated_reader_refusal"
                    refusal = candidate.get("reasons")
        writer_candidate = writer.get(identity)
        if writer_candidate is not None and state == "joined" and writer_candidate["name"] == entry["name"]:
            writer_state = "generated_writer_declaration_unobserved"
            if implementation == "source_row_not_yet_consumed":
                implementation = writer_state
        implementation_counts[implementation] += 1
        reader_counts[reader_implementation] += 1
        writer_counts[writer_state] += 1
        observed_counts[observed_read] += 1
        records.append({"identity": {"table": identity[0], "raw_key": identity[1], "variant_index": identity[2]},
                        "catalog": {"name": entry["name"], "normalized_name": entry["normalized_name"], "groups": entry["groups"]},
                        "source": {"state": state, "name": source_name, "row_sha256": row_hash, "selector_row_sha256": selector_hash, "table_sha256": table_hash},
                        "source_layout_status": status, "source_derived_implementation": implementation,
                        "reader_implementation": reader_implementation, "writer_implementation": writer_state,
                        "implementation_refusal_reasons": refusal,
                        "observed_read": observed_read, "observed_write": "not_observed_yet"})
    if len(records) != len(catalog_by_id) or sum(status_counts.values()) != len(records):
        raise ValueError("join conservation failed")
    if sum(sum(counts.values()) for counts in family_counts.values()) != len(records):
        raise ValueError("family entry conservation failed")
    inputs = {"catalog_sha256": catalog_sha, "hydrated_sha256": hydrated_sha,
              "exiftool_version": catalog["exiftool_version"]}
    if quicktime_input_digests is not None:
        inputs["quicktime"] = dict(sorted(quicktime_input_digests.items()))
    if writer_input_digests is not None:
        inputs["writer"] = dict(sorted(writer_input_digests.items()))
    if quicktime_read_evidence is not None:
        inputs["quicktime_read_evidence"] = {"sha256": canonical_hash(quicktime_read_evidence),
                                             "producer": quicktime_read_evidence["producer"]}
    return {"schema": SCHEMA, "inputs": inputs, "counts": {"catalog_ordinary_entries": len(catalog_by_id),
            "hydrated_source_rows": len(hydrated_by_id), "joined_records": len(records), "status": dict(sorted(status_counts.items())),
            "implementation": dict(sorted(implementation_counts.items())), "reader_implementation": dict(sorted(reader_counts.items())),
            "writer_implementation": dict(sorted(writer_counts.items())), "observed_read": dict(sorted(observed_counts.items()))},
            "families": {key: dict(sorted(value.items())) for key, value in sorted(family_counts.items())}, "entries": records}


def report(join: dict) -> str:
    counts = join["counts"]
    lines = ["# Catalog-to-hydrated source join", "", "This report records exact source and generated-declaration identities. Generated declarations remain unobserved until immutable fixture evidence joins them.", "",
             f"- ExifTool: `{join['inputs']['exiftool_version']}`", f"- Catalog SHA-256: `{join['inputs']['catalog_sha256']}`",
             f"- Hydrated SHA-256: `{join['inputs']['hydrated_sha256']}`", "", "| Measurement | Count |", "| --- | ---: |",
             f"| Ordinary catalog entries | {counts['catalog_ordinary_entries']} |", f"| Hydrated source coordinates | {counts['hydrated_source_rows']} |",
             f"| Preserved joined records | {counts['joined_records']} |"]
    lines.extend(f"| `{key}` | {value} |" for key, value in counts["status"].items())
    lines += ["", "## Source-derived implementation", "", "| Classification | Count |", "| --- | ---: |"]
    lines.extend(f"| `{key}` | {value} |" for key, value in counts["implementation"].items())
    lines += ["", "## Observed reads", "", "| Classification | Count |", "| --- | ---: |"]
    lines.extend(f"| `{key}` | {value} |" for key, value in counts["observed_read"].items())
    lines += ["", "A join requires exact `(table full name, raw key, variant index)` and exact public-name spelling. Observed reads additionally require a clean, artifact-bound verifier report and exact source identity; writes remain unobserved.", "",
              "## Families", "", "| Family | Status counts |", "| --- | --- |"]
    lines.extend(f"| {key} | " + ", ".join(f"{name}: {count}" for name, count in value.items()) + " |" for key, value in join["families"].items())
    return "\n".join(lines) + "\n"


def aliases(left: Path, right: Path) -> bool:
    return left.resolve() == right.resolve() or (left.exists() and right.exists() and os.path.samefile(left, right))


def validate_destinations(catalog: Path, hydrated: Path, output: Path, report_path: Path, *inputs: Path) -> None:
    if aliases(output, report_path):
        raise ValueError("output and report destinations alias each other")
    for destination in (output, report_path):
        if any(aliases(destination, source) for source in (catalog, hydrated, *inputs)):
            raise ValueError("output or report aliases an input")


def write_staged(documents: list[tuple[Path, str]]) -> None:
    staged = []
    try:
        for destination, body in documents:
            destination.parent.mkdir(parents=True, exist_ok=True)
            with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=destination.parent, prefix=".catalog-hydrated-", delete=False) as handle:
                temporary = Path(handle.name)
                staged.append((temporary, destination))
                handle.write(body)
            temporary.chmod(destination.stat().st_mode & 0o777 if destination.exists() else 0o644)
        for temporary, destination in staged:
            os.replace(temporary, destination)
    finally:
        for temporary, _ in staged:
            temporary.unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalog", required=True, type=Path)
    parser.add_argument("--hydrated", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--quicktime-bounded-source", required=True, type=Path)
    parser.add_argument("--quicktime-itemlist-ledger", required=True, type=Path)
    parser.add_argument("--quicktime-source-capabilities", required=True, type=Path)
    parser.add_argument("--quicktime-itemlist-rust", required=True, type=Path)
    parser.add_argument("--quicktime-read-evidence", type=Path)
    parser.add_argument("--writer-source", type=Path,
                        help="authenticated full native dump used by both writer compilers")
    parser.add_argument("--writer-final-ledger", type=Path)
    parser.add_argument("--writer-final-rust", type=Path)
    parser.add_argument("--writer-public-ledger", type=Path)
    parser.add_argument("--writer-public-rust", type=Path)
    action = parser.add_mutually_exclusive_group()
    action.add_argument("--replace", action="store_true")
    action.add_argument("--check", action="store_true")
    args = parser.parse_args()
    writer_paths = (args.writer_source, args.writer_final_ledger, args.writer_final_rust, args.writer_public_ledger, args.writer_public_rust)
    if any(path is not None for path in writer_paths) and any(path is None for path in writer_paths):
        raise ValueError("writer join inputs must be supplied together")
    validate_destinations(args.catalog, args.hydrated, args.output, args.report,
                          args.quicktime_bounded_source, args.quicktime_itemlist_ledger,
                          args.quicktime_source_capabilities, args.quicktime_itemlist_rust,
                          *(writer_paths if all(path is not None for path in writer_paths) else ()),
                          *([args.quicktime_read_evidence] if args.quicktime_read_evidence else []))
    quicktime_source = args.quicktime_bounded_source.read_bytes()
    quicktime_ledger = args.quicktime_itemlist_ledger.read_bytes()
    quicktime_capabilities = args.quicktime_source_capabilities.read_bytes()
    quicktime_rust = args.quicktime_itemlist_rust.read_text(encoding="utf-8")
    quicktime_digests = {"source_sha256": hashlib.sha256(quicktime_source).hexdigest(),
                         "ledger_sha256": hashlib.sha256(quicktime_ledger).hexdigest(),
                         "capabilities_sha256": hashlib.sha256(quicktime_capabilities).hexdigest(),
                         "rust_sha256": hashlib.sha256(quicktime_rust.encode()).hexdigest()}
    writer_source = args.writer_source.read_bytes() if args.writer_source else None
    writer_final = args.writer_final_ledger.read_bytes() if args.writer_final_ledger else None
    writer_final_rust = args.writer_final_rust.read_text(encoding="utf-8") if args.writer_final_rust else None
    writer_public = args.writer_public_ledger.read_bytes() if args.writer_public_ledger else None
    writer_public_rust = args.writer_public_rust.read_text(encoding="utf-8") if args.writer_public_rust else None
    writer_digests = {"source_sha256": hashlib.sha256(writer_source).hexdigest(),
                      "final_ledger_sha256": hashlib.sha256(writer_final).hexdigest(),
                      "final_rust_sha256": hashlib.sha256(writer_final_rust.encode()).hexdigest(),
                      "public_ledger_sha256": hashlib.sha256(writer_public).hexdigest(),
                      "public_rust_sha256": hashlib.sha256(writer_public_rust.encode()).hexdigest()} if writer_source else None
    join = build(read_json(args.catalog), read_json(args.hydrated), sha256(args.catalog), sha256(args.hydrated),
                 json.loads(quicktime_ledger), json.loads(quicktime_capabilities), quicktime_source, quicktime_rust,
                 quicktime_digests, read_json(args.quicktime_read_evidence) if args.quicktime_read_evidence else None,
                 writer_source, json.loads(writer_final) if writer_final else None, writer_final_rust, json.loads(writer_public) if writer_public else None, writer_public_rust,
                 writer_digests)
    rendered_join, rendered_report = json.dumps(join, indent=2, sort_keys=True) + "\n", report(join)
    if args.check:
        if not args.output.exists() or not args.report.exists():
            raise ValueError("--check requires existing output and report")
        if args.output.read_text(encoding="utf-8") != rendered_join or args.report.read_text(encoding="utf-8") != rendered_report:
            raise ValueError("joined output or report is stale; regenerate with --replace")
        return 0
    if not args.replace and (args.output.exists() or args.report.exists()):
        raise ValueError("output/report exists; pass --replace or --check")
    write_staged([(args.output, rendered_join), (args.report, rendered_report)])
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (KeyError, OSError, ValueError, json.JSONDecodeError) as exc:
        raise SystemExit(f"catalog hydrated join refused: {exc}")
