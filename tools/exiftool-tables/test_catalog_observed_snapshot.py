import copy
import importlib.util
from pathlib import Path
import sys
import unittest

PATH = Path(__file__).with_name("catalog_observed_snapshot.py")
spec = importlib.util.spec_from_file_location("catalog_observed_snapshot", PATH)
publication = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(publication)


def entry(raw_key, observed_read="not_observed_yet", observed_write="not_observed_yet"):
    return {"identity": {"table": "Image::ExifTool::Exif::Main", "raw_key": raw_key, "variant_index": 0},
            "catalog": {"name": "Artist", "groups": {"1": "IFD0"}},
            "source": {"state": "joined", "row_sha256": "a" * 64},
            "source_layout_status": "source_row_joined",
            "source_derived_implementation": "ifd_schema_declaration_eligible_unobserved",
            "reader_implementation": "ifd_schema_declaration_eligible_unobserved",
            "writer_implementation": "generated_writer_declaration_unobserved",
            "implementation_refusal_reasons": None,
            "observed_read": observed_read, "observed_write": observed_write}


def producer():
    return {"source_commit": "1" * 40, "runtime_input_manifest_sha256": "2" * 64}


def receipt():
    return {"schema": "native-receipt-v1", "producer": producer(), "observations": []}


def join(rows, evidence=None):
    counts = {"joined_records": len(rows), "observed_read": {"not_observed_yet": len(rows)}}
    if evidence:
        counts["observed_write"] = {"not_observed_yet": len(rows)}
    inputs = {"exiftool_version": "13.59"}
    if evidence:
        inputs["quicktime_read_evidence"] = {"sha256": publication.canonical_hash(evidence), "producer": evidence["producer"]}
    return {"schema": publication.JOIN_SCHEMA, "inputs": inputs, "counts": counts, "entries": rows}


class CatalogObservedSnapshotTests(unittest.TestCase):
    def test_historical_join_schema_is_retained_but_not_mixed(self):
        evidence = receipt()
        for schema in sorted(publication.JOIN_SCHEMAS):
            source, observed = join([entry("315")]), join([entry("315")], evidence)
            source["schema"] = observed["schema"] = schema
            publication.validate_snapshot(publication.make_authenticated_snapshot(
                source, observed, {"quicktime_read_evidence": evidence}))
        source, observed = join([entry("315")]), join([entry("315")], evidence)
        source["schema"] = "oxidex_catalog_hydrated_join_v2"
        with self.assertRaisesRegex(ValueError, "schema differs"):
            publication.validate_pair(source, observed)

    def test_non_ascii_receipt_uses_catalog_join_hash_contract(self):
        sys.path.insert(0, str(PATH.parent))
        try:
            from join_catalog_hydrated import canonical_hash as joined_hash
        finally:
            sys.path.pop(0)
        evidence = receipt()
        evidence["observations"] = [{"key": "\u00a9nam", "value": "caf\u00e9 \U0001f4f7"}]
        source = join([entry("315")])
        observed = join([entry("315")], evidence)
        observed["inputs"]["quicktime_read_evidence"]["sha256"] = joined_hash(evidence)
        snapshot = publication.make_authenticated_snapshot(
            source, observed, {"quicktime_read_evidence": evidence})
        publication.validate_snapshot(snapshot)
        mutated = copy.deepcopy(evidence)
        mutated["observations"][0]["value"] += "changed"
        with self.assertRaisesRegex(ValueError, "not bound"):
            publication.make_authenticated_snapshot(
                source, observed, {"quicktime_read_evidence": mutated})

    def authenticated_pair(self):
        evidence = receipt()
        source = join([entry("315")])
        observed = join([entry("315")], evidence)
        return source, observed, {"quicktime_read_evidence": evidence}

    def test_snapshot_requires_join_bound_authenticated_receipt(self):
        source, observed, evidence = self.authenticated_pair()
        snapshot = publication.make_authenticated_snapshot(source, observed, evidence)
        publication.validate_snapshot(snapshot)
        self.assertEqual(snapshot["native_evidence"]["source_commit"], "1" * 40)
        self.assertEqual(snapshot["source_table_observations"]["Image::ExifTool::Exif::Main"]["source_entries"], 1)

    def test_refuses_fabricated_or_unbound_evidence(self):
        source, observed, evidence = self.authenticated_pair()
        forged = copy.deepcopy(evidence)
        forged["quicktime_read_evidence"]["producer"]["source_commit"] = "3" * 40
        with self.assertRaisesRegex(ValueError, "not bound"):
            publication.make_authenticated_snapshot(source, observed, forged)
        with self.assertRaisesRegex(ValueError, "requires authenticated"):
            publication.make_authenticated_snapshot(source, join([entry("315")]), {})

    def test_refuses_source_axis_or_summary_mutation(self):
        source, observed, evidence = self.authenticated_pair()
        observed["entries"][0]["catalog"]["name"] = "Forged"
        with self.assertRaisesRegex(ValueError, "source/declaration"):
            publication.make_authenticated_snapshot(source, observed, evidence)
        source, observed, evidence = self.authenticated_pair()
        observed["counts"]["observed_read"] = {"observed_matched_read": 1}
        with self.assertRaisesRegex(ValueError, "observed_read counts"):
            publication.make_authenticated_snapshot(source, observed, evidence)

    def test_current_source_drift_keeps_historical_receipt_valid(self):
        source, observed, evidence = self.authenticated_pair()
        snapshot = publication.make_authenticated_snapshot(source, observed, evidence)
        changed_current = copy.deepcopy(source)
        changed_current["entries"][0]["source"]["row_sha256"] = "b" * 64
        publication.validate_snapshot(snapshot)
        report = publication.render_report(snapshot, changed_current)
        self.assertIn("differs from historical source join", report)


if __name__ == "__main__":
    unittest.main()
