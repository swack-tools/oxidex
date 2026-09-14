#!/usr/bin/env python3
import importlib.util
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

MATRIX = Path(__file__).with_name("native_write_matrix.py")
spec = importlib.util.spec_from_file_location("native_write_matrix", MATRIX)
module = importlib.util.module_from_spec(spec)
assert spec.loader
spec.loader.exec_module(module)
ROOT = Path(__file__).resolve().parents[2]


class NativeWriteMatrixTests(unittest.TestCase):
    def test_tiff_parser_rejects_truncated_ifd_and_value(self):
        with self.assertRaises(ValueError):
            module.parse_tiff(b"II\x2a\x00\x08\x00\x00\x00\x01")
        # A complete one-entry IFD whose five-byte string points outside the
        # file. Keep the count/header valid so this reaches value bounds.
        pack = module.pack
        bad = (b"II" + pack(42, 2, "little") + pack(8, 4, "little")
               + pack(1, 2, "little") + pack(316, 2, "little")
               + pack(2, 2, "little") + pack(5, 4, "little")
               + pack(0x7fffffff, 4, "little") + pack(0, 4, "little"))
        with self.assertRaisesRegex(ValueError, "value.*bounds|value.*outside"):
            module.parse_tiff(bad)


    def test_little_and_big_tiff_carriers_are_parseable(self):
        with tempfile.TemporaryDirectory() as temporary:
            for order in ("little", "big"):
                path = Path(temporary) / f"{order}.tif"
                module.make_tiff(path, order)
                inspected = module.parse_tiff(path.read_bytes())
                self.assertEqual(inspected["byte_order"], order)
                self.assertEqual(inspected["image_payload_hex"], "ff")

    def test_exact_native_host_contract_rejects_each_mutated_dimension(self):
        expected = module.expected_host("embedded_nul")
        self.assertEqual(expected, {"type": 2, "count": 4, "value_hex": "61006200"})
        module.assert_host_contract(dict(expected), expected)
        for dimension, wrong in (("type", 1), ("count", 3), ("value_hex", "61006300")):
            actual = dict(expected)
            actual[dimension] = wrong
            with self.subTest(dimension=dimension):
                with self.assertRaisesRegex(AssertionError, dimension):
                    module.assert_host_contract(actual, expected)

    def test_contract_version_rejects_a_different_native_release(self):
        wrong = {"result": {"exiftool_version": "13.60"}}
        with self.assertRaisesRegex(RuntimeError, "13.59 native-write acceptance baseline"):
            module.assert_contract_version(wrong)

    @unittest.skipUnless(os.environ.get("EXIFTOOL_PERL") and os.environ.get("OXIDEX_PINNED_EXIFTOOL"),
                         "requires explicit pinned native Perl and ExifTool library")
    def test_all_declared_native_rows_execute_and_preserve_carriers(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "matrix.json"
            subprocess.run(["python3", str(MATRIX), "--perl", os.environ["EXIFTOOL_PERL"], "--lib", os.environ["OXIDEX_PINNED_EXIFTOOL"],
                            "--jpeg-base", str(ROOT / "tests/fixtures/jpeg/tag_matrix_base.jpg"), "--output", str(output)], check=True, timeout=120)
            document = json.loads(output.read_text())
        self.assertEqual(document["declared_rows"], 48)
        self.assertEqual(document["executed_rows"], 48)
        self.assertEqual(document["contract_exiftool_release"], "13.59")
        self.assertEqual(document["native_identity"]["result"]["exiftool_version"], "13.59")
        self.assertEqual(len(document["rows"]), 48)
        self.assertEqual({row["carrier"] for row in document["rows"]}, {"tiff_little", "tiff_big", "jpeg"})
        self.assertEqual({row["requested_name"] for row in document["rows"]}, set(module.NAMES))
        self.assertEqual({row["operation"] for row in document["rows"]}, set(module.CASES))
        for row in document["rows"]:
            self.assertEqual(row["operation_call"]["returncode"], 0)
            self.assertTrue(row["verification"]["carrier_image_preserved"])
            self.assertTrue(row["verification"]["artist_preserved"])
            self.assertFalse(row["verification"]["no_op_detected"])
            after = row["verification"]["host_after"]
            self.assertEqual(module.host_storage(after), row["verification"]["expected_host"])
            if after is not None and after["count"] <= 4:
                self.assertTrue(after["inline"])


if __name__ == "__main__":
    unittest.main()
