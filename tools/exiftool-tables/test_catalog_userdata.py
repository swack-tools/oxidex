import collections
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import catalog_userdata
import join_catalog_hydrated as join
import quicktime_userdata_specs as compiler


class CatalogUserDataTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.document = json.loads(compiler.SNAPSHOT.read_bytes())
        # Synthetic join input: bind only this test document to the current
        # producer. This is not a native recapture or an observed-read receipt.
        cls.document["capture_scope"]["dump_tool_sha256"] = hashlib.sha256(
            (compiler.SNAPSHOT.parent.parent / "dump_tables.pl").read_bytes()).hexdigest()
        cls.source = json.dumps(cls.document).encode()
        cls.ledger = compiler.compile_document(cls.document)
        cls.rust = compiler.render_rust(cls.ledger)
        cls.catalog_sources = json.loads((compiler.ROOT / "docs/public/measurements/catalog-source-13.59.json").read_text())["producer"]["sources"]

    def replay(self, source=None, ledger=None, rust=None, sources=None):
        return catalog_userdata.implementation(
            self.ledger if ledger is None else ledger,
            self.source if source is None else source,
            self.rust if rust is None else rust,
            project=join.quicktime_selector_projection, rust_matches=join.quicktime_rust_matches,
            catalog_sources=self.catalog_sources if sources is None else sources)

    def test_complete_source_accounting_preserves_reasons_without_observation_credit(self):
        rows = self.replay()
        self.assertEqual(len(rows), self.ledger["identity_counts"]["source_records"])
        accepted = [row for row in rows.values() if row["generated"]]
        self.assertEqual(len(accepted), 17)
        self.assertEqual(len({(row["group1"], row["name"]) for row in accepted}), 15)
        self.assertTrue(all(row["reasons"] for row in rows.values() if not row["generated"]))
        self.assertTrue(all("observed_read" not in row for row in rows.values()))

    def test_generated_artifact_or_ledger_forgery_refuses(self):
        changed = copy.deepcopy(self.ledger)
        changed["ledger"] = changed["ledger"][:-1]
        with self.assertRaisesRegex(ValueError, "complete source replay"):
            self.replay(ledger=changed)
        changed = copy.deepcopy(self.ledger)
        changed["specs"][0]["name"] = "ForgedTag"
        with self.assertRaisesRegex(ValueError, "complete source replay"):
            self.replay(ledger=changed, rust=compiler.render_rust(changed))
        with self.assertRaisesRegex(ValueError, "complete source replay"):
            self.replay(rust=self.rust + "const FORGED: bool = true;\n")

    def test_protocol_source_must_join_catalog_provenance(self):
        sources = copy.deepcopy(self.catalog_sources)
        sources["Image/ExifTool/QuickTime.pm"]["sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "catalog provenance"):
            self.replay(sources=sources)

    def test_new_supported_row_flows_through_and_callback_remains_accounted(self):
        document = copy.deepcopy(self.document)
        tags = document["modules"]["QuickTime"]["tables"]["UserData"]["tags"]
        tags["TNew"] = {"Name": "FutureScalar", "Format": "int16u"}
        tags["TCbk"] = {"Name": "FutureCallback", "Format": "int16u", "ValueConv": "$val + 1"}
        document["modules"]["QuickTime"]["tables"]["UserData"]["tag_count"] = len(tags)
        ledger = compiler.compile_document(document)
        rows = self.replay(json.dumps(document).encode(), ledger, compiler.render_rust(ledger))
        fresh = next(row for key, row in rows.items() if key[1] == "TNew")
        callback = next(row for key, row in rows.items() if key[1] == "TCbk")
        self.assertTrue(fresh["generated"])
        self.assertEqual(fresh["name"], "FutureScalar")
        self.assertFalse(callback["generated"])
        self.assertIn("unsupported_source_property:ValueConv", callback["reasons"])
        self.assertEqual(len(rows), self.ledger["identity_counts"]["source_records"] + 2)

    def test_partial_inputs_refuse(self):
        for ledger, source, rust in [(self.ledger, None, self.rust), (None, self.source, self.rust),
                                     (self.ledger, self.source, None)]:
            with self.subTest(source_missing=source is None), self.assertRaises(ValueError):
                catalog_userdata.implementation(ledger, source, rust,
                    project=join.quicktime_selector_projection, rust_matches=join.quicktime_rust_matches,
                    catalog_sources=self.catalog_sources)

    def test_catalog_join_counts_userdata_declarations_but_leaves_them_unobserved(self):
        table_name = "Image::ExifTool::QuickTime::UserData"
        catalog = json.loads((compiler.ROOT / "docs/public/measurements/catalog-source-13.59.json").read_text())
        catalog["entries"] = [row for row in catalog["entries"] if row["table"] == table_name]
        names = sorted({row["normalized_name"] for row in catalog["entries"]})
        catalog["unique_names"] = names
        catalog["counts"] = {"catalog_total_tag_entries": len(catalog["entries"]),
                             "distinct_case_insensitive_entry_names": len(names),
                             "catalog_unique_tag_names": len(names)}
        # A table subset carries its own native Writable aggregates.
        facts = [row["native_writable"] for row in catalog["entries"]]
        catalog["counts"].update(
            catalog_native_writable_classes=dict(collections.Counter(fact["class"] for fact in facts)),
            catalog_native_writable_states=dict(collections.Counter(fact["state"] for fact in facts)),
            distinct_case_insensitive_writable_names=len({row["normalized_name"] for row in catalog["entries"]
                                                          if row["native_writable"]["class"] == "writable"}))
        table = copy.deepcopy(self.document["modules"]["QuickTime"]["tables"]["UserData"])
        table["full_name"] = table_name
        hydrated = {"exiftool_version": catalog["exiftool_version"], "hydrated_layouts": {
            "catalog_counts": {"total_tag_entries": len(catalog["entries"])},
            "source_provenance": {"sources": self.catalog_sources}, "tables": {table_name: table}}}
        itemlist = join.quicktime_specs.compile_document(self.document)
        capabilities = join.quicktime_selector.report(self.source)
        rust = join.quicktime_specs.render_rust(itemlist)
        inputs = {"source_sha256": hashlib.sha256(self.source).hexdigest(),
                  "ledger_sha256": join.canonical_hash(itemlist),
                  "capabilities_sha256": join.canonical_hash(capabilities),
                  "rust_sha256": hashlib.sha256(rust.encode()).hexdigest()}
        userdata_inputs = {"source_sha256": inputs["source_sha256"],
                           "ledger_sha256": join.canonical_hash(self.ledger),
                           "rust_sha256": hashlib.sha256(self.rust.encode()).hexdigest()}
        def build(digests):
            return join.build(catalog, hydrated, "catalog", "hydrated", itemlist, capabilities,
                self.source, rust, inputs, userdata_ledger=self.ledger, userdata_rust=self.rust,
                userdata_input_digests=digests)
        result = build(userdata_inputs)
        self.assertEqual(result["counts"]["reader_implementation"]["generated_reader_declaration_unobserved"], 17)
        self.assertTrue(all(row["observed_read"] == "not_observed_yet" for row in result["entries"]))
        self.assertEqual(len(result["entries"]), len(catalog["entries"]))
        self.assertEqual(result["source_tables"][table_name]["catalog_entries"], len(catalog["entries"]))
        with self.assertRaisesRegex(ValueError, "input digests"):
            build({**userdata_inputs, "rust_sha256": "0" * 64})

    def test_observation_import_requires_live_authentication_and_complete_same_fixture_modes(self):
        # The native verifier has separate receipt-forgery tests. This mocks
        # its return to exercise only the catalog consumer's admission rules.
        spec = self.ledger["specs"][0]
        def row(fixture, mode):
            return {"fixture": fixture, "mode": mode, "source_identity": spec["source_identity"],
                    "group1": spec["group"], "tag_name": spec["name"]}
        with tempfile.TemporaryDirectory() as temporary:
            paths = tuple(Path(temporary) / name for name in ("source.json", "ledger.json", "generated.rs"))
            paths[0].write_bytes(self.source)
            paths[1].write_text(json.dumps(self.ledger))
            paths[2].write_text(self.rust)
            def observed():
                return catalog_userdata.observed_reads({}, self.source, self.ledger, self.rust, paths)
            with patch("verify_quicktime_userdata_reader.validate_evidence", side_effect=ValueError("stale native receipt")):
                with self.assertRaisesRegex(ValueError, "stale native receipt"):
                    observed()
            with patch("verify_quicktime_userdata_reader.validate_evidence", return_value=[row("a", "print"), row("b", "raw")]):
                self.assertEqual(observed(), set())
            with patch("verify_quicktime_userdata_reader.validate_evidence", return_value=[row("a", "print"), row("a", "raw")]) as validate:
                expected = {(spec["source_identity"]["raw_key"], tuple(spec["source_identity"]["variant_path"]),
                             spec["group"], spec["name"])}
                self.assertEqual(observed(), expected)
                validate.assert_called_once_with({}, *paths)
            paths[2].write_text("forged artifact")
            with self.assertRaisesRegex(ValueError, "differ from joined inputs"):
                observed()


if __name__ == "__main__":
    unittest.main()
