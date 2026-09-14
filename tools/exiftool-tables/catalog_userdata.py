"""Replay UserData artifacts before joining declarations to hydrated rows.

This module indexes source identities only. Observed read credit requires a
separate public-reader receipt; neither an emitted spec nor this replay is a
claim that a fixture exercised the row.
"""
from __future__ import annotations

import json

import quicktime_atom_tables as selector
import quicktime_userdata_specs as compiler


def implementation(ledger, bounded_source, emitted_rust, *, project, rust_matches,
                   catalog_sources):
    if ledger is None and emitted_rust is None:
        return {}
    if ledger is None or bounded_source is None or emitted_rust is None:
        raise ValueError("UserData replay requires source, ledger, and Rust together")
    if not isinstance(bounded_source, bytes) or not isinstance(emitted_rust, str):
        raise ValueError("UserData replay input has an invalid type")
    document = json.loads(bounded_source)
    expected = compiler.compile_document(document)
    if ledger != expected or not rust_matches(compiler.render_rust(expected), emitted_rust):
        raise ValueError("UserData artifacts differ from complete source replay")
    # Bind the source defining the table/caller protocol to the catalog's own
    # pinned source, rather than trusting a version string or matching names.
    processor = expected["protocol"]["processor"]
    source_file = processor.get("source_file")
    source_fact = catalog_sources.get(source_file)
    if (not isinstance(source_fact, dict)
            or source_fact.get("sha256") != processor.get("source_sha256")):
        raise ValueError("UserData protocol source differs from catalog provenance")
    if "hydrated_layouts" in document:
        from capture_quicktime_baseline import hydrated_tables
        tables = hydrated_tables(document)
    else:
        tables = document["modules"]["QuickTime"]["tables"]
    table = tables["UserData"]
    specs = {(row["source_identity"]["raw_key"], tuple(row["source_identity"]["variant_path"])): row
             for row in expected["specs"]}
    rows = {}
    for record in expected["ledger"]:
        identity = record["identity"]
        if identity["module"] != "QuickTime" or identity["table"] != "UserData":
            raise ValueError("UserData ledger has a foreign source identity")
        path = tuple(identity["variant_path"])
        candidates = selector.variants(table["tags"][identity["raw_key"]])
        for candidate_path, candidate, _ in candidates:
            if candidate_path != path:
                continue
            if selector.digest(candidate) != identity["source_sha256"]:
                raise ValueError("UserData ledger source digest differs")
            projected = project("Image::ExifTool::QuickTime::UserData", identity["raw_key"], candidate,
                                table_meta=table.get("meta"), variant_path=path)
            key = ("UserData", identity["raw_key"],
                   selector.digest(selector.semantic_normal_form(projected)), path)
            if key in rows:
                raise ValueError("UserData semantic source identity collision")
            spec = specs.get((identity["raw_key"], path))
            rows[key] = {"generated": record["generated"], "reasons": record["reasons"],
                         "source_identity": identity, "name": spec["name"] if spec else None,
                         "group1": spec["group"] if spec else None}
            break
        else:
            raise ValueError("UserData ledger variant is absent from source")
    if (len(rows) != expected["identity_counts"]["source_records"]
            or sum(row["generated"] for row in rows.values()) != expected["identity_counts"]["generated"]):
        raise ValueError("UserData source identity conservation failed")
    return rows
