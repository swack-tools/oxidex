"""Credit catalog entries from an authenticated corpus read receipt.

The receipt (corpus_read_receipt.py) names, from pinned ExifTool's own tag
information, the exact `(table, tag ID, variant index)` behind every native tag
in every corpus file, and credits a coordinate only when OxiDex's public CLI
matched that identity in both print and raw modes in every file where ExifTool
read it. This module binds the receipt to the joined checkout (runtime inputs,
repository pin, and the catalog's pinned ExifTool sources) and maps credited
coordinates onto catalog entries as `observed_matched_read`. Coordinates outside
the catalog (for example rows ExifTool adds to a table at run time) are counted,
never credited.

It also names the reachability ceiling: every catalog entry whose exact source
row pinned ExifTool read in at least one corpus file. An entry inside that set
but not credited is `native_read_not_matched` -- an OxiDex gap the corpus
already exercises -- while `not_observed_yet` is left meaning only that no
corpus file exercises the entry. The native set is reconstructed from the
receipt's authenticated source transcripts, not from `derive()`, whose return
value is part of every stored receipt's verified claims and must not grow.
"""
from __future__ import annotations

import corpus_read_receipt
import runtime_evidence_inputs as runtime_inputs

OBSERVED_STATE = "observed_matched_read"
NATIVE_STATE = "native_read_not_matched"


def native_coordinates(receipt) -> set:
    """Every `(table, tag ID, variant)` pinned ExifTool read in any corpus file.

    Rows without a table row (unattributable occurrences) never count, nor do
    the filesystem/ExifTool rows the receipt ignores by construction (System
    group, ExifToolVersion), since they are never compared and so can never
    be credited. Each source transcript is re-authenticated by `source_rows`.
    """
    coordinates = set()
    for name in receipt["corpus"]["files"]:
        for rows in corpus_read_receipt.source_rows(receipt, name).values():
            coordinates.update(coordinate for coordinate in rows
                               if coordinate is not None and coordinate[0] is not None)
    return coordinates


def check_exiftool_sources(receipt, catalog) -> None:
    """Every ExifTool source the catalog was built from must be the one observed."""
    fingerprint = receipt["native"]["library_fingerprint"]
    for path, fact in catalog["producer"]["sources"].items():
        if fingerprint.get(path) != fact.get("sha256"):
            raise ValueError(f"corpus read receipt ExifTool source differs from the catalog: {path}")


def catalog_credit(receipt, derived, catalog_ids) -> tuple[set, set, dict]:
    """Map a validated receipt's coordinates onto catalog identities.

    `catalog_ids` holds `(table, raw_key, variant_index)`; the receipt's
    `(table, tag ID, variant)` coordinates use the same representation, so the
    mapping is exact set membership.
    """
    credited, outside = set(), 0
    for table, tag_id, variant in derived["credited_coordinates"]:
        identity = (table, tag_id, variant)
        if identity in catalog_ids:
            credited.add(identity)
        else:
            outside += 1
    native, native_outside = set(), 0
    for coordinate in native_coordinates(receipt):
        if coordinate in catalog_ids:
            native.add(coordinate)
        else:
            native_outside += 1
    # Credit requires a match in a file where ExifTool read the row, so the
    # credited set can never leave the natively read one.
    if not credited <= native:
        raise ValueError("corpus read receipt credits a catalog entry ExifTool never read")
    counts = {"credited_catalog_entries": len(credited),
              "credited_coordinates_outside_catalog": outside,
              "native_catalog_entries": len(native),
              "native_coordinates_outside_catalog": native_outside,
              "metric_c": derived["metric_c"]}
    return credited, native, counts


def observed_reads(receipt, catalog, root) -> tuple[set, set, dict]:
    """-> (credited catalog identities, natively read catalog identities, counts) for a validated receipt."""
    if receipt is None:
        return set(), set(), {}
    derived = corpus_read_receipt.validate(receipt, catalog["exiftool_version"])
    # Credit applies to the runtime being joined, not to an older build.
    if receipt["producer"]["runtime_input_manifest_sha256"] != runtime_inputs.runtime_input_manifest(root):
        raise ValueError("corpus read receipt runtime differs from the joined checkout")
    check_exiftool_sources(receipt, catalog)
    catalog_ids = {(entry["table"], entry["raw_key"], entry["variant_index"]) for entry in catalog["entries"]}
    return catalog_credit(receipt, derived, catalog_ids)
