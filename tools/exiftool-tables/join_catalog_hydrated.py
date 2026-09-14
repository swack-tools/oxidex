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
from collections import Counter, defaultdict
from pathlib import Path
import tempfile
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
import quicktime_atom_tables as quicktime_selector

SCHEMA = "oxidex_catalog_hydrated_join_v2"
CATALOG_SCHEMA = "oxidex_hydrated_catalog_universe_v1"


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


def quicktime_selector_projection(table: str, raw_key: str, row: dict) -> dict:
    """Invert only verified dump wrappers before using the selector's digest."""
    if not {"TagID", "Table", "Groups", "_extra_properties"} & set(row):
        return row
    projected = dict(row)
    tag_id = projected.pop("TagID", raw_key)
    table_fact = projected.pop("Table", None)
    groups = projected.pop("Groups", None)
    extras = projected.pop("_extra_properties", {})
    if tag_id != raw_key or not isinstance(table_fact, dict) or table not in table_fact.get("table_full_names", []):
        raise ValueError("QuickTime hydrated wrapper identity is malformed")
    if not isinstance(groups, dict) or groups.get("__ref") != "HASH":
        raise ValueError("QuickTime hydrated wrapper groups are malformed")
    if not isinstance(extras, dict):
        raise ValueError("QuickTime hydrated wrapper has unprojected source properties")
    semantic = set(extras) - {"GotGroups", "Preferred"}
    if any(key in projected for key in semantic):
        raise ValueError("QuickTime wrapper conflicts with explicit source property")
    if semantic:
        existing = projected.get("_extra_keys", [])
        if not isinstance(existing, list) or not all(isinstance(key, str) for key in existing):
            raise ValueError("QuickTime source extra-key projection is malformed")
        projected["_extra_keys"] = sorted(set(existing) | semantic)
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


def quicktime_implementation(itemlist_ledger: dict | None, capabilities: dict | None) -> dict[tuple[str, str, str, tuple[int, ...]], dict]:
    """Index emitted QuickTime selector facts by table, key, and source hash."""
    if itemlist_ledger is None or capabilities is None:
        return {}
    if itemlist_ledger.get("schema") != "quicktime_generated_itemlist_specs_v1":
        raise ValueError("unsupported QuickTime generated ledger schema")
    bounded = read_json(Path(__file__).with_name("fixtures") / "quicktime_source_13_59.json")
    tables = bounded["modules"]["QuickTime"]["tables"]
    rows = {}
    for record in itemlist_ledger.get("ledger", []):
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
                    key = (key[0], key[1], quicktime_selector.digest(quicktime_selector.semantic_normal_form(candidate)), key[3])
                    break
            else:
                raise ValueError("QuickTime ledger variant is absent from bounded source")
        except (KeyError, TypeError):
            raise ValueError("QuickTime bounded source identity is malformed") from None
        if key in rows:
            raise ValueError("QuickTime semantic identity collision")
        rows[key] = {"generated": record.get("generated") is True, "reasons": record.get("reasons")}
    for family in capabilities.get("families", []):
        for record in family.get("records", []):
            identity = record.get("identity", {})
            path = identity.get("variant_path")
            key = (identity.get("table"), identity.get("raw_key"), identity.get("source_sha256"), tuple(path) if isinstance(path, list) else None)
            if key in rows:
                rows[key]["selector_reasons"] = record.get("reasons")
    return rows


def build(catalog: dict, hydrated: dict, catalog_sha: str, hydrated_sha: str,
          itemlist_ledger: dict | None = None, quicktime_capabilities: dict | None = None) -> dict:
    if catalog.get("exiftool_version") != hydrated.get("exiftool_version"):
        raise ValueError("catalog and hydrated ExifTool versions differ")
    catalog_by_id = validate_catalog(catalog)
    validate_provenance(catalog, hydrated)
    hydrated_by_id, table_hashes = source_rows(hydrated)
    quicktime = quicktime_implementation(itemlist_ledger, quicktime_capabilities)
    hydrated_count = require_mapping(hydrated["hydrated_layouts"].get("catalog_counts"), "hydrated catalog counts").get("total_tag_entries")
    if hydrated_count != len(catalog_by_id):
        raise ValueError("hydrated total_tag_entries differs from catalog denominator")
    records, status_counts, implementation_counts, family_counts = [], Counter(), Counter(), defaultdict(Counter)
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
        implementation = "source_row_not_yet_consumed"
        refusal = None
        selector_hash = None
        if source is not None and identity[0].startswith("Image::ExifTool::QuickTime::"):
            variant_path = (identity[2],) if "_variants" in hydrated["hydrated_layouts"]["tables"][identity[0]]["tags"][identity[1]] else ()
            projected = quicktime_selector_projection(identity[0], identity[1], source)
            selector_hash = quicktime_selector.digest(quicktime_selector.semantic_normal_form(projected))
            candidate = quicktime.get((identity[0].rsplit("::", 1)[-1], identity[1], selector_hash, variant_path))
            if candidate is not None:
                if candidate["generated"]:
                    implementation = "generated_reader_declaration_unobserved"
                else:
                    implementation = "blocked_generated_reader_refusal"
                    refusal = candidate.get("reasons")
        implementation_counts[implementation] += 1
        records.append({"identity": {"table": identity[0], "raw_key": identity[1], "variant_index": identity[2]},
                        "catalog": {"name": entry["name"], "normalized_name": entry["normalized_name"], "groups": entry["groups"]},
                        "source": {"state": state, "name": source_name, "row_sha256": row_hash, "selector_row_sha256": selector_hash, "table_sha256": table_hash},
                        "source_layout_status": status, "source_derived_implementation": implementation,
                        "implementation_refusal_reasons": refusal,
                        "observed_read": "not_observed_yet", "observed_write": "not_observed_yet"})
    if len(records) != len(catalog_by_id) or sum(status_counts.values()) != len(records):
        raise ValueError("join conservation failed")
    if sum(sum(counts.values()) for counts in family_counts.values()) != len(records):
        raise ValueError("family entry conservation failed")
    return {"schema": SCHEMA, "inputs": {"catalog_sha256": catalog_sha, "hydrated_sha256": hydrated_sha,
            "exiftool_version": catalog["exiftool_version"]}, "counts": {"catalog_ordinary_entries": len(catalog_by_id),
            "hydrated_source_rows": len(hydrated_by_id), "joined_records": len(records), "status": dict(sorted(status_counts.items())),
            "implementation": dict(sorted(implementation_counts.items()))},
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
    lines += ["", "A join requires exact `(table full name, raw key, variant index)` and exact public-name spelling. Each matched row records canonical row and table hashes for later implementation evidence.", "",
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
    parser.add_argument("--quicktime-itemlist-ledger", type=Path,
                        default=Path(__file__).with_name("quicktime_generated_itemlist_ledger.json"))
    parser.add_argument("--quicktime-source-capabilities", type=Path,
                        default=Path(__file__).with_name("quicktime_source_capabilities.json"))
    action = parser.add_mutually_exclusive_group()
    action.add_argument("--replace", action="store_true")
    action.add_argument("--check", action="store_true")
    args = parser.parse_args()
    validate_destinations(args.catalog, args.hydrated, args.output, args.report,
                          args.quicktime_itemlist_ledger, args.quicktime_source_capabilities)
    join = build(read_json(args.catalog), read_json(args.hydrated), sha256(args.catalog), sha256(args.hydrated),
                 read_json(args.quicktime_itemlist_ledger), read_json(args.quicktime_source_capabilities))
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
