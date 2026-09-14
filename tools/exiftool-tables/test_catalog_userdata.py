import copy
import json
import unittest

import catalog_userdata
import join_catalog_hydrated as join
import quicktime_userdata_specs as compiler


class CatalogUserDataTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.source = compiler.SNAPSHOT.read_bytes()
        cls.document = json.loads(cls.source)
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


if __name__ == "__main__":
    unittest.main()
