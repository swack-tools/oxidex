"""Portable structure checks for fresh JPEG mixed public batches."""
from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).parent))
import fresh_jpeg_public_batch_matrix as matrix
from generated_tiff_write_matrix import GeneratedTarget


class FreshJpegPublicBatchMatrixTests(unittest.TestCase):
    def test_instrument_identifier_is_single_sourced(self) -> None:
        self.assertEqual(matrix.INSTRUMENT, "fresh_jpeg_public_batch_matrix_v2")

    def test_mandatory_candidates_join_generated_source_artifacts(self) -> None:
        candidates = matrix.mandatory_legacy_candidates()
        self.assertGreater(len(candidates), 0)
        for candidate in candidates:
            with self.subTest(candidate=candidate):
                self.assertTrue(candidate.name)
                self.assertTrue(candidate.directory)
                self.assertNotEqual(candidate.default_value, candidate.override_value)
                self.assertEqual(candidate.qualifier, f"{candidate.directory}:{candidate.name}")

    def test_mandatory_candidates_refuse_ambiguous_or_unjoined_source_rows(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            ledger = root / "mandatory.json"
            rules = root / "address.rs"
            ledger.write_text(
                '{"writer_tables_joined": true, "recipe": {"directories": '
                '[{"directory": "IFD1", "defaults": ['
                '{"tag_id": 282, "kind": "Integer", "value": 72},'
                '{"tag_id": 283, "kind": "Integer", "value": 2}]}]}}',
                encoding="utf-8",
            )
            rules.write_text("", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "no source-addressed"):
                matrix.mandatory_legacy_candidates(ledger, rules)
            rules.write_text(
                '''StaticSetNewValueAddress { index: 0, module: "Exif", table: "Main", full_name: "Image::ExifTool::Exif::Main", raw_id: "282", name: "One", group0: "EXIF", group1: "IFD0", write_group: "IFD0", }
                   StaticSetNewValueAddress { index: 1, module: "Exif", table: "Main", full_name: "Image::ExifTool::Exif::Main", raw_id: "282", name: "Two", group0: "EXIF", group1: "IFD0", write_group: "IFD0", }''',
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "ambiguous"):
                matrix.mandatory_legacy_candidates(ledger, rules)

    def test_cases_use_source_selected_target_and_mandatory_identity(self) -> None:
        target = GeneratedTarget(0xBEEF, "TargetFromLedger", "EXIF", "IFD0")
        mandatory = matrix.MandatoryCandidate(0xBEAD, "MandatoryFromLedger", "EXIF", "IFD0", "IFD1", 72, 2)
        delete_legacy = matrix.batch_case(target, mandatory, "generated-delete-legacy-set")
        override = matrix.batch_case(target, mandatory, "generated-set-mandatory-override")
        removal = matrix.batch_case(target, mandatory, "generated-set-mandatory-delete")

        self.assertEqual(delete_legacy["native"][0], {"tag": "EXIF:TargetFromLedger", "scalar": "undefined"})
        self.assertEqual(delete_legacy["public"][1], {"key": "IFD0:Artist", "scalar": "utf8", "value": "batch-artist"})
        self.assertEqual(override["native"][1], {"tag": "IFD1:MandatoryFromLedger", "scalar": "utf8", "value": "2"})
        self.assertEqual(override["public"][1], {"key": "IFD1:MandatoryFromLedger", "scalar": "integer", "value": "2"})
        self.assertEqual(removal["native"][1], {"tag": "IFD1:Artist", "scalar": "utf8", "value": "batch-artist"})
        self.assertEqual(removal["native"][2], {"tag": "IFD1:MandatoryFromLedger", "scalar": "undefined"})

    def test_jfif_adjusted_candidates_follow_raw_carrier_properties(self) -> None:
        zero = next(case for case in matrix.CARRIERS if case.label == "fresh-jfif-zero")
        nonzero = next(case for case in matrix.CARRIERS if case.label == "fresh-jfif-nonzero")
        zero_candidate = matrix.jfif_adjusted_candidates(zero)[0]
        nonzero_candidate = matrix.jfif_adjusted_candidates(nonzero)[0]
        self.assertEqual((zero_candidate.directory, zero_candidate.name, zero_candidate.default_value, zero_candidate.override_value), ("IFD0", "XResolution", 0, 1))
        self.assertEqual((nonzero_candidate.directory, nonzero_candidate.name, nonzero_candidate.default_value, nonzero_candidate.override_value), ("IFD0", "XResolution", 72, 300))

    def test_ifd1_mandatory_entry_uses_parsed_next_ifd(self) -> None:
        document = {
            "exif": {"tags": {"282": {"value_hex": "wrong-level"}},
                     "children": {"NextIFD": {"tags": {"282": {"value_hex": "right-level"}}}}}
        }
        self.assertEqual(matrix._entry(document, 282, "IFD1"), {"value_hex": "right-level"})
        self.assertEqual(matrix._entry(document, 282, "IFD0"), {"value_hex": "wrong-level"})
        with self.assertRaisesRegex(ValueError, "outside parsed JPEG scope"):
            matrix._entry(document, 282, "ExifIFD")

    def test_native_batch_requires_typed_utf8_input_state(self) -> None:
        expected = [{"tag": "EXIF:Source", "scalar": "utf8", "value": "ascii"}]
        good = {"returncode": 0, "result": {"write_return": 1, "error": None,
                "set_calls": [{"tag": "EXIF:Source", "return": 1,
                               "input": {"defined": True, "utf8": True, "hex": "6173636969"}}]}}
        matrix.assert_native_batch(good, "typed", expected)
        bad = {"returncode": 0, "result": {**good["result"], "set_calls": [{"tag": "EXIF:Source", "return": 1,
                "input": {"defined": True, "utf8": False, "hex": "6173636969"}}]}}
        with self.assertRaisesRegex(AssertionError, "operand utf8 differs"):
            matrix.assert_native_batch(bad, "untyped", expected)

    def test_request_uses_only_committed_public_batch_schema(self) -> None:
        target = GeneratedTarget(0xBEEF, "Target", "EXIF", "IFD0")
        mandatory = matrix.MandatoryCandidate(0xBEAD, "Mandatory", "EXIF", "IFD0", "IFD1", 72, 2)
        spec = matrix.batch_case(target, mandatory, "generated-set-mandatory-override")
        request = {"route": "public-batch", "carrier": "jpeg", "input": "input.jpg", "output": "output.jpg", "batch": spec["public"]}
        self.assertEqual(set(request), {"route", "carrier", "input", "output", "batch"})
        self.assertTrue(all(set(item).issubset({"key", "scalar", "value"}) for item in request["batch"]))
        self.assertNotIn("key", request)


if __name__ == "__main__":
    unittest.main()
