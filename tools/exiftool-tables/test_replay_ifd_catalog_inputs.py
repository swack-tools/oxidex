"""Cross-runner IFD replay must preserve artifacts and every source decision."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import codegen
import table_modules
import join_catalog_hydrated as join
from replay_ifd_catalog_inputs import digest, replay


def encoded(value):
    return json.dumps(value, sort_keys=True).encode()


class IfdCatalogReplayTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.document = {"exiftool_version": "13.59", "modules": {"Exif": {"tables": {"Main": {
            "meta": {}, "tags": {"315": {"Name": "Artist", "Format": "int16u"},
                                 "316": {"Name": "Omitted", "Format": "int16u", "RawConv": "$val"}}}}}}}
        cls.source = encoded(cls.document)
        cls.oracle = encoded({"schema": 2, "exiftool_version": "13.59", "perl_version": "v5.38.2",
                              "tables_sha256": digest(cls.source), "probe_counts": {"pass": 1, "fail": 0},
                              "verified_expressions": [], "expression_counts": {"total": 0, "verified": 0},
                              "use_counts": {"total": 0, "verified": 0}})
        chunks, index, _, rows = codegen.gen_ifd_tables(cls.document, ["Exif"], set())
        cls.rust = codegen.render_ifd_files("13.59", chunks, index)
        cls.ledger = {"schema": "oxidex_ifd_identity_ledger_v1", "exiftool_version": "13.59",
                      "source": {"tables_json_sha256": digest(cls.source),
                                 "expr_ledger_sha256": digest(cls.oracle),
                                 "ifd_rust_hash_format": codegen.IFD_RUST_HASH_FORMAT,
                                 "ifd_rust_sha256": codegen._canonical_ifd_rust_sha256(cls.rust)},
                      "rows": sorted(rows, key=lambda row: (row["full_name"], row["raw_key"], tuple(row["variant_path"]))),
                      "counts": {"rows": 2, "emitted": 2, "refused": 0,
                                 "reader_eligible": 1, "reader_omitted": 1},
                      "ownership_rows": [],
                      "ownership_counts": {"binary_rows": 0, "named_raw_key_rows": 0}}

    def fresh(self, document=None):
        document = copy.deepcopy(self.document if document is None else document)
        document["native_capture_context"] = {"perl": "/usr/bin/perl", "runner": "linux"}
        source = json.dumps(document, indent=2).encode()
        oracle = json.loads(self.oracle)
        oracle.update(tables_sha256=digest(source), perl_version="v5.40.0")
        return source, encoded(oracle)

    def test_fresh_envelope_requires_replay_then_passes_unchanged_strict_join(self):
        source, oracle = self.fresh()
        with self.assertRaisesRegex(ValueError, "source digest"):
            join.ifd_implementation(source, self.ledger, self.rust, oracle)
        original = copy.deepcopy(self.ledger)
        rebound = replay(source, self.ledger, self.rust, self.oracle, oracle)
        self.assertEqual(self.ledger, original)
        self.assertEqual(rebound["rows"], original["rows"])
        self.assertEqual(rebound["counts"], original["counts"])
        self.assertEqual(rebound["source"]["tables_json_sha256"], digest(source))
        self.assertEqual(rebound["source"]["expr_ledger_sha256"], digest(oracle))
        actual = join.ifd_implementation(source, rebound, self.rust, oracle)
        self.assertEqual(len(actual), 2)

    def test_fresh_oracle_must_bind_exact_source(self):
        source, _ = self.fresh()
        with self.assertRaisesRegex(ValueError, "not bound"):
            replay(source, self.ledger, self.rust, self.oracle, self.oracle)

    def test_committed_source_and_oracle_bindings_cannot_be_forged(self):
        source, oracle = self.fresh()
        for field in ("tables_json_sha256", "expr_ledger_sha256", "ifd_rust_sha256"):
            changed = copy.deepcopy(self.ledger)
            changed["source"][field] = "0" * 64
            with self.subTest(field=field), self.assertRaises(ValueError):
                replay(source, changed, self.rust, self.oracle, oracle)

    def test_changed_rust_refuses_even_with_updated_committed_hash(self):
        source, oracle = self.fresh()
        rust = dict(self.rust, **{"exif.rs": self.rust["exif.rs"].replace('name: "Artist"', 'name: "Forged"', 1)})
        self.assertNotEqual(rust, self.rust)
        ledger = copy.deepcopy(self.ledger)
        ledger["source"]["ifd_rust_sha256"] = codegen._canonical_ifd_rust_sha256(rust)
        with self.assertRaisesRegex(ValueError, "compiler replay"):
            replay(source, ledger, rust, self.oracle, oracle)

    def test_source_row_changes_refuse_even_when_rust_is_unchanged(self):
        document = copy.deepcopy(self.document)
        document["modules"]["Exif"]["tables"]["Main"]["tags"]["316"]["RawConv"] = "$val + 1"
        source, oracle = self.fresh(document)
        with self.assertRaisesRegex(ValueError, "rows/classifications/counts"):
            replay(source, self.ledger, self.rust, self.oracle, oracle)

    def test_every_identity_classification_and_count_must_match(self):
        source, oracle = self.fresh()
        for mutation in ("missing_row", "reader_state", "omissions", "counts"):
            ledger = copy.deepcopy(self.ledger)
            if mutation == "missing_row":
                ledger["rows"].pop()
            elif mutation == "counts":
                ledger["counts"]["reader_eligible"] += 1
            elif mutation == "reader_state":
                ledger["rows"][1]["reader_state"] = "eligible"
            else:
                ledger["rows"][1]["omissions"] = []
            with self.subTest(mutation=mutation), self.assertRaisesRegex(ValueError, "rows/classifications/counts"):
                replay(source, ledger, self.rust, self.oracle, oracle)

    def test_ownership_rows_and_counts_must_match_fresh_source(self):
        source, oracle = self.fresh()
        forged = {
            "module": "Nikon", "table": "MakerNotes0x56",
            "full_name": "Image::ExifTool::Nikon::MakerNotes0x56",
            "raw_key": "4.1", "variant_path": [],
            "name": "BurstStartSlotNumber", "source_sha256": "0" * 64,
            "source_kind": "binary",
        }
        for mutation in ("row", "count"):
            ledger = copy.deepcopy(self.ledger)
            if mutation == "row":
                ledger["ownership_rows"].append(forged)
            else:
                ledger["ownership_counts"]["binary_rows"] += 1
            with self.subTest(mutation=mutation), self.assertRaisesRegex(
                ValueError, "ownership rows/counts"
            ):
                replay(source, ledger, self.rust, self.oracle, oracle)

    def test_changed_or_failed_oracle_inventory_cannot_reduce_coverage(self):
        source, raw_oracle = self.fresh()
        for field in ("verified_expressions", "expression_counts", "use_counts", "probe_counts"):
            oracle = json.loads(raw_oracle)
            oracle[field] = ["$val + 1"] if field == "verified_expressions" else {}
            with self.subTest(field=field), self.assertRaises(ValueError):
                replay(source, self.ledger, self.rust, self.oracle, encoded(oracle))

    def test_cli_publishes_only_new_bound_ledger_and_preserves_inputs(self):
        source, oracle = self.fresh()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            inputs = {"source": source, "committed-ledger": encoded(self.ledger),
                      "committed-expr-ledger": self.oracle, "fresh-expr-ledger": oracle}
            command = [sys.executable, str(Path(__file__).with_name("replay_ifd_catalog_inputs.py"))]
            for name, value in inputs.items():
                path = root / name
                path.write_bytes(value)
                command.extend(["--" + name, str(path)])
            # The committed Rust is a directory: the hub plus one file per module.
            table_modules.write_files(root / "ifd" / "mod.rs", self.rust)
            committed_rust = {name: (root / "ifd" / name).read_bytes() for name in self.rust}
            command.extend(["--committed-rust", str(root / "ifd" / "mod.rs")])
            output = root / "bound.json"
            result = subprocess.run([*command, "--output", str(output)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("Exact IFD Rust and 2 complete source rows", result.stdout)
            join.ifd_implementation(source, json.loads(output.read_bytes()), self.rust, oracle)
            for refused in (output, root / "source"):
                result = subprocess.run([*command, "--output", str(refused)], capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("new path distinct", result.stderr)
            for name, value in inputs.items():
                self.assertEqual((root / name).read_bytes(), value)
            for name, value in committed_rust.items():
                self.assertEqual((root / "ifd" / name).read_bytes(), value)


if __name__ == "__main__":
    unittest.main()
