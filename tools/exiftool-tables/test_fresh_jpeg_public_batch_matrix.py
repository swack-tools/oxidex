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
    def test_mandatory_candidate_is_joined_from_generated_source_artifacts(self) -> None:
        candidate = matrix.mandatory_ifd0_candidate()
        self.assertGreaterEqual(candidate.raw_tag_id, 0)
        self.assertTrue(candidate.name)
        self.assertTrue(candidate.group0)
        self.assertEqual(candidate.qualifier, f"{candidate.write_group}:{candidate.name}")

    def test_mandatory_candidate_refuses_unjoined_or_ambiguous_source_rows(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            ledger = root / "mandatory.json"
            rules = root / "address.rs"
            ledger.write_text(
                '{"writer_tables_joined": true, "recipe": {"directories": '
                '[{"directory": "IFD0", "defaults": [{"tag_id": 531}]}]}}',
                encoding="utf-8",
            )
            rules.write_text("", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "no generated address identity"):
                matrix.mandatory_ifd0_candidate(ledger, rules)
            rules.write_text(
                '''StaticSetNewValueAddress { index: 0, module: "Exif", table: "Main", full_name: "Image::ExifTool::Exif::Main", raw_id: "0x213", name: "One", group0: "EXIF", group1: "Image", write_group: "IFD0", }
                   StaticSetNewValueAddress { index: 1, module: "Exif", table: "Main", full_name: "Image::ExifTool::Exif::Main", raw_id: "0x213", name: "Two", group0: "EXIF", group1: "Image", write_group: "IFD0", }''',
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "ambiguous"):
                matrix.mandatory_ifd0_candidate(ledger, rules)

    def test_cases_use_source_selected_target_and_mandatory_identity(self) -> None:
        target = GeneratedTarget(0xBEEF, "TargetFromLedger", "EXIF", "IFD0")
        mandatory = matrix.MandatoryCandidate(0xBEAD, "MandatoryFromLedger", "EXIF", "IFD0")
        delete_legacy = matrix.batch_case(target, mandatory, "generated-delete-legacy-set")
        override = matrix.batch_case(target, mandatory, "generated-set-mandatory-override")
        removal = matrix.batch_case(target, mandatory, "generated-set-mandatory-delete")

        self.assertEqual(delete_legacy["native"][0], {"tag": "EXIF:TargetFromLedger", "scalar": "undefined"})
        self.assertEqual(delete_legacy["public"][1], {"key": "IFD0:Artist", "scalar": "utf8", "value": "batch-artist"})
        self.assertEqual(override["native"][1], {"tag": "IFD0:MandatoryFromLedger", "scalar": "utf8", "value": "2"})
        self.assertEqual(override["public"][1], {"key": "IFD0:MandatoryFromLedger", "scalar": "integer", "value": "2"})
        self.assertEqual(removal["native"][1], {"tag": "IFD0:MandatoryFromLedger", "scalar": "undefined"})
        self.assertEqual(removal["public"][1], {"key": "IFD0:MandatoryFromLedger", "scalar": "undefined"})

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
        mandatory = matrix.MandatoryCandidate(0xBEAD, "Mandatory", "EXIF", "IFD0")
        spec = matrix.batch_case(target, mandatory, "generated-set-mandatory-override")
        request = {"route": "public-batch", "carrier": "jpeg", "input": "input.jpg", "output": "output.jpg", "batch": spec["public"]}
        self.assertEqual(set(request), {"route", "carrier", "input", "output", "batch"})
        self.assertTrue(all(set(item).issubset({"key", "scalar", "value"}) for item in request["batch"]))
        self.assertNotIn("key", request)


if __name__ == "__main__":
    unittest.main()
