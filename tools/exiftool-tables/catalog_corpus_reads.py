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
"""
from __future__ import annotations

import corpus_read_receipt
import runtime_evidence_inputs as runtime_inputs

OBSERVED_STATE = "observed_matched_read"


def observed_reads(receipt, catalog, root) -> tuple[set, dict]:
    """-> (credited catalog identities, counts) for a validated receipt."""
    if receipt is None:
        return set(), {}
    derived = corpus_read_receipt.validate(receipt, catalog["exiftool_version"])
    # Credit applies to the runtime being joined, not to an older build.
    if receipt["producer"]["runtime_input_manifest_sha256"] != runtime_inputs.runtime_input_manifest(root):
        raise ValueError("corpus read receipt runtime differs from the joined checkout")
    # Every ExifTool source the catalog was built from must be the one observed.
    fingerprint = receipt["native"]["library_fingerprint"]
    for path, fact in catalog["producer"]["sources"].items():
        if fingerprint.get(path) != fact.get("sha256"):
            raise ValueError(f"corpus read receipt ExifTool source differs from the catalog: {path}")
    catalog_ids = {(entry["table"], entry["raw_key"], entry["variant_index"]) for entry in catalog["entries"]}
    credited, outside = set(), 0
    for table, tag_id, variant in derived["credited_coordinates"]:
        identity = (table, tag_id, variant)
        if identity in catalog_ids:
            credited.add(identity)
        else:
            outside += 1
    counts = {"credited_catalog_entries": len(credited),
              "credited_coordinates_outside_catalog": outside,
              "metric_c": derived["metric_c"]}
    return credited, counts
