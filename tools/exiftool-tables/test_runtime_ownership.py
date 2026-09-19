"""Controls for the generated/runtime ownership inventory."""

import importlib.util
import sys
import json
import tempfile
import unittest
from pathlib import Path


def load_ownership():
    path = Path(__file__).with_name("runtime_ownership.py")
    spec = importlib.util.spec_from_file_location("runtime_ownership", path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


class RuntimeOwnershipTests(unittest.TestCase):
    def setUp(self):
        self.ownership = load_ownership()
        self.root = Path(__file__).parents[2]
        self.generated_row = {
            "module": "Exif", "table": "Main",
            "field": {"kind": "numeric", "value": "0x829a"},
            "owner": "generated", "symbol": "generated::decode",
            "source_release": "13.59", "source_sha256": "0" * 64,
            "refusal": None, "fixture": "tools/exiftool-tables/conv_exif_main_ledger.json",
        }

    def test_schema_row_is_generated(self):
        self.assertEqual(self.generated_row["owner"], "generated")

    def test_duplicate_owner_is_refused(self):
        residual_row = dict(self.generated_row, owner="residual", symbol="core::tiff_helpers")
        with self.assertRaisesRegex(self.ownership.Refused, "duplicate owner"):
            self.ownership.verify_rows([self.generated_row, residual_row])

    def test_enabled_field_without_owner_is_refused(self):
        unowned = dict(self.generated_row, owner=None, symbol=None)
        with self.assertRaisesRegex(self.ownership.Refused, "unowned enabled field"):
            self.ownership.verify_rows([unowned])

    def test_repository_inventory_is_complete_sorted_and_counted(self):
        inventory = self.ownership.build_inventory(self.root)
        self.assertEqual(inventory["category_totals"], {"generated": 551, "not-applicable": 29, "refused": 17, "residual": 19, "walker-owned": 0})
        self.assertEqual(inventory["rows"], sorted(inventory["rows"], key=lambda r: (r["module"], r["table"], r["field"]["kind"], r["field"]["value"], r["owner"])))
        self.assertIn('("Exif", "Main")', (self.root / "src/exiftool_tables/enabled_ifd.rs").read_text())

    def test_live_residual_arrays_exactly_match_fragments(self):
        _, fragments = self.ownership._fragment_digest(self.root / "tools/exiftool-tables/runtime_ownership.d")
        self.assertEqual(self.ownership._canonical(fragments), self.ownership._canonical(self.ownership._expected_residual_rows(self.root)))

    def test_missing_fixture_symbol_and_malformed_provenance_refuse(self):
        row = dict(self.generated_row)
        row["fixture"] = "src/DOES_NOT_EXIST.rs"
        with self.assertRaisesRegex(self.ownership.Refused, "missing fixture"):
            self.ownership._validate_paths_and_symbols(self.root, [row])
        row = dict(self.generated_row, symbol="src/exiftool_tables/conv/exif_main.rs::NOPE")
        with self.assertRaisesRegex(self.ownership.Refused, "missing symbol"):
            self.ownership._validate_paths_and_symbols(self.root, [row])
        row = dict(self.generated_row, source_sha256="not-a-sha")
        with self.assertRaisesRegex(self.ownership.Refused, "source sha256"):
            self.ownership.verify_rows([row])

    def test_symbol_must_be_an_exact_declaration_not_a_prefix_or_comment(self):
        row = next(row for row in self.ownership.load_rows(self.root) if row["owner"] == "generated")
        source = self.root / "src/exiftool_tables/conv/exif_main.rs"
        original = source.read_text()
        try:
            source.write_text(original.replace("pub fn decode(", "pub fn decode_removed(") + "\n// decode remains only as text\n")
            with self.assertRaisesRegex(self.ownership.Refused, "missing symbol"):
                self.ownership._validate_paths_and_symbols(self.root, [row])
        finally:
            source.write_text(original)

    def test_declaration_shaped_block_comments_and_literals_do_not_count(self):
        row = next(row for row in self.ownership.load_rows(self.root) if row["owner"] == "generated")
        source = self.root / "src/exiftool_tables/conv/exif_main.rs"
        original = source.read_text()
        decoys = '''
/* outer /* nested */
pub fn decode(input: &[u8]) {}
*/
let decoy = "\n pub fn decode(input: &[u8]) {}";
let raw = r###"\n pub fn decode(input: &[u8]) {}"###;
let ch = 'd';
'''
        try:
            source.write_text(original.replace("pub fn decode(", "pub fn decode_removed(") + decoys)
            with self.assertRaisesRegex(self.ownership.Refused, "missing symbol"):
                self.ownership._validate_paths_and_symbols(self.root, [row])
        finally:
            source.write_text(original)

    def test_structural_reference_requires_its_own_category(self):
        row = next(row for row in self.ownership.build_inventory(self.root)["rows"] if row["owner"] == "not-applicable")
        self.ownership._validate_paths_and_symbols(self.root, [row])
        wrong = dict(row, owner="generated")
        with self.assertRaisesRegex(self.ownership.Refused, "missing symbol"):
            self.ownership._validate_paths_and_symbols(self.root, [wrong])

    def test_registry_and_residual_discovery_ignore_non_code_decoys(self):
        registry = self.root / "src/exiftool_tables/enabled_ifd.rs"
        original = registry.read_text()
        try:
            registry.write_text(original.replace('(\"Exif\", \"Main\"),', '// (\"Exif\", \"Main\"),', 1) + '\n/* ("Exif", "Main"), */\nlet x = r#"("Exif", "Main")"#;\n')
            with self.assertRaisesRegex(self.ownership.Refused, "enabled-table registry"):
                self.ownership.build_inventory(self.root)
        finally:
            registry.write_text(original)
        carrier = self.root / "src/core/exif_dir_engine.rs"
        original = carrier.read_text()
        try:
            carrier.write_text('/* pub(crate) const IFD0_HAND_KEPT: &[u16] = &[0x83bb]; */\n' + original.replace('    0x83bb,', '    /* removed */', 1))
            rows = self.ownership._expected_residual_rows(self.root)
            self.assertNotIn("IFD0/0x83bb", {row["field"]["value"] for row in rows})
        finally:
            carrier.write_text(original)

    def test_stale_inventory_and_empty_refusal_refuse(self):
        inventory_path = self.root / "tools/exiftool-tables/runtime_ownership.json"
        original = inventory_path.read_text()
        try:
            stale = json.loads(original)
            stale["category_totals"]["generated"] = 0
            inventory_path.write_text(json.dumps(stale))
            with self.assertRaisesRegex(self.ownership.Refused, "stale or tampered"):
                self.ownership.load_rows(self.root)
        finally:
            inventory_path.write_text(original)
        refused = dict(self.generated_row, owner="refused", refusal="")
        with self.assertRaisesRegex(self.ownership.Refused, "refused row has no reason"):
            self.ownership.verify_rows([refused])


if __name__ == "__main__":
    unittest.main()
