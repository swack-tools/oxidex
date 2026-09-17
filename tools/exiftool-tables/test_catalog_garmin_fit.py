"""Catalog join of the generated Garmin FIT reader: replay binding and classes."""
from __future__ import annotations

import collections
import copy
import hashlib
import json
import unittest
from pathlib import Path
from unittest.mock import patch

import catalog_garmin_fit
import garmin_fit_specs as specs
import join_catalog_hydrated as join

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
BOUNDED = HERE / "fixtures" / "garmin_fit_source.json"
LEDGER = HERE / "garmin_fit_ledger.json"
RUST = ROOT / "src" / "exiftool_tables" / "fit_tables.rs"
CATALOG = ROOT / "docs" / "public" / "measurements" / "catalog-source-13.59.json"


def verified():
    return specs.unbound_verified_expressions(HERE / "expr_oracle_ledger.json")


class CatalogGarminFitTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.source = BOUNDED.read_bytes()
        document = json.loads(cls.source)
        # The full dump the IFD join authenticates carries the same Garmin module.
        cls.dump = json.dumps({"exiftool_version": document["exiftool_version"],
                               "modules": {"Garmin": document["modules"]["Garmin"]}}).encode()
        cls.protocol = json.dumps(document["garmin_fit_reader_protocol"]).encode()
        cls.ledger = json.loads(LEDGER.read_text())
        cls.rust = RUST.read_text()
        cls.catalog = json.loads(CATALOG.read_text())

    def replay(self, ledger=None, source=None, rust=None, dump=None, sources=None, protocol=None):
        # Unit tests have no full dump, so the oracle ledger's dump binding is
        # replaced by its unbound PASS set; the join CLI uses the bound one.
        with patch.object(catalog_garmin_fit.codegen, "load_oracle_ledger", return_value=verified()):
            return catalog_garmin_fit.implementation(
                self.ledger if ledger is None else ledger, self.source if source is None else source,
                self.rust if rust is None else rust, dump_source=self.dump if dump is None else dump,
                expr_ledger=b"{}", protocol_fact=self.protocol if protocol is None else protocol,
                catalog_sources=self.catalog["producer"]["sources"] if sources is None else sources,
                rust_matches=join.quicktime_rust_matches)

    def test_committed_artifacts_replay_and_classify_every_catalog_row(self):
        rows = self.replay()
        garmin = [entry for entry in self.catalog["entries"] if entry["table"].startswith("Image::ExifTool::Garmin::")]
        states = collections.Counter()
        for entry in garmin:
            row = rows[(entry["table"], entry["raw_key"], entry["variant_index"])]
            self.assertEqual(row["name"], entry["name"])
            states[catalog_garmin_fit.reader_state(row)[0]] += 1
        self.assertEqual(dict(states), {"generated_reader_declaration_unobserved": 599,
                                        "generated_reader_declaration_option_gated": 1123,
                                        "blocked_generated_reader_refusal": 5})

    def test_tampered_or_unbound_inputs_refuse(self):
        ledger = copy.deepcopy(self.ledger); ledger["rows"][0]["name"] = "Renamed"
        dump = json.loads(self.dump); dump["modules"]["Garmin"]["tables"].pop("Session")
        protocol = json.loads(self.protocol)
        protocol["base_types"]["entries"]["0"]["invalid"] = "12345"
        sources = copy.deepcopy(self.catalog["producer"]["sources"])
        sources["Image/ExifTool/Garmin.pm"]["sha256"] = "0" * 64
        for label, kwargs, message in (
            ("ledger", {"ledger": ledger}, "differ from complete source replay"),
            ("rust", {"rust": self.rust.replace("AvgHeartRate", "AvgHeartRateX")}, "differ from complete source replay"),
            ("dump", {"dump": json.dumps(dump).encode()}, "differs from the authenticated full dump"),
            ("provenance", {"sources": sources}, "differs from catalog provenance"),
            ("protocol", {"protocol": json.dumps(protocol).encode()}, "differs from the fresh native capture"),
        ):
            with self.subTest(label), self.assertRaisesRegex(ValueError, message):
                self.replay(**kwargs)
        with self.assertRaisesRegex(ValueError, "together"):
            catalog_garmin_fit.implementation(self.ledger, None, self.rust, dump_source=self.dump, expr_ledger=b"{}",
                                              protocol_fact=self.protocol,
                                              catalog_sources={}, rust_matches=join.quicktime_rust_matches)

    def build(self, entries, tags, ifd=None, fit=True):
        table = "Image::ExifTool::Garmin::Session"
        catalog = copy.deepcopy(self.catalog)
        catalog["entries"] = entries
        unique = sorted({row["normalized_name"] for row in entries})
        catalog["unique_names"] = unique
        catalog["counts"] = {"catalog_total_tag_entries": len(entries), "distinct_case_insensitive_entry_names": len(unique),
                             "catalog_unique_tag_names": len(unique),
                             "catalog_native_writable_classes": dict(collections.Counter(r["native_writable"]["class"] for r in entries)),
                             "catalog_native_writable_states": dict(collections.Counter(r["native_writable"]["state"] for r in entries)),
                             "distinct_case_insensitive_writable_names": 0}
        hydrated = {"exiftool_version": "13.59", "hydrated_layouts": {
            "catalog_counts": {"total_tag_entries": len(entries)},
            "source_provenance": {"sources": catalog["producer"]["sources"]},
            "tables": {table: {"full_name": table, "tags": tags}}}}
        digests = {"source_sha256": hashlib.sha256(self.source).hexdigest(),
                   "ledger_sha256": join.canonical_hash(self.ledger),
                   "rust_sha256": hashlib.sha256(self.rust.encode()).hexdigest()}
        with patch.object(catalog_garmin_fit.codegen, "load_oracle_ledger", return_value=verified()), \
                patch.object(join, "ifd_implementation", return_value=ifd or {}):
            fit_inputs = dict(garmin_fit_source=self.source, garmin_fit_ledger=self.ledger,
                              garmin_fit_rust=self.rust, garmin_fit_input_digests=digests,
                              garmin_fit_protocol_fact=self.protocol) if fit else {}
            ifd_digests = {name: "a" * 64 for name in ("source_sha256", "ledger_sha256", "rust_sha256")} if ifd else None
            return join.build(catalog, hydrated, "c", "h", ifd_source=self.dump, ifd_expr_ledger=b"{}",
                              ifd_input_digests=ifd_digests, **fit_inputs)

    def session_entries(self, *raw_keys):
        return [copy.deepcopy(entry) for entry in self.catalog["entries"]
                if entry["table"] == "Image::ExifTool::Garmin::Session" and entry["raw_key"] in raw_keys]

    def test_join_supersedes_ifd_schema_candidacy_with_fit_reader_state(self):
        entries = self.session_entries("16")
        identity = ("Image::ExifTool::Garmin::Session", "16", 0)
        ifd = {identity: {"name": "AvgHeartRate", "artifact_state": "omitted", "reader_state": "omitted",
                          "reasons": [], "omissions": ["no_format"]}}
        # The IFD schema result is really present and would otherwise stand.
        without_fit = self.build(entries, {"16": {"Name": "AvgHeartRate"}}, ifd=ifd, fit=False)
        self.assertEqual(without_fit["entries"][0]["reader_implementation"], "ifd_schema_declaration_omitted_unobserved")
        result = self.build(entries, {"16": {"Name": entries[0]["name"]}}, ifd=ifd)
        row = result["entries"][0]
        self.assertEqual((row["catalog"]["name"], row["reader_implementation"], row["implementation_refusal_reasons"],
                          row["observed_read"]),
                         ("AvgHeartRate", "generated_reader_declaration_unobserved", None, "not_observed_yet"))
        self.assertIn("garmin_fit", result["inputs"])

    def test_join_refuses_name_conflict_and_unreplayed_rows(self):
        entries = self.session_entries("16")
        entries[0]["name"] = "Different"; entries[0]["normalized_name"] = "different"
        with self.assertRaisesRegex(ValueError, "name identity differs"):
            self.build(entries, {"16": {"Name": "Different"}})
        stray = self.session_entries("16")
        stray[0]["raw_key"] = "9999"
        with self.assertRaisesRegex(ValueError, "absent from the replayed FIT ledger"):
            self.build(stray, {"9999": {"Name": stray[0]["name"]}})


if __name__ == "__main__":
    unittest.main()
