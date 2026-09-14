"""Portable structure checks for the fresh/empty public JPEG instrument."""
from __future__ import annotations

from pathlib import Path
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).parent))
import fresh_jpeg_public_write_matrix as matrix
import native_write_matrix as native


class FreshJpegPublicWriteMatrixTests(unittest.TestCase):
    def test_raw_jfif_carriers_and_empty_ifd0_orders_are_parseable(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for case in matrix.CARRIERS:
                with self.subTest(case=case.label):
                    path = root / f"{case.label}.jpg"
                    matrix.write_carrier(path, case)
                    document = native.parse_jpeg(path)
                    raw = matrix.raw_jfif_payload(case.jfif)
                    self.assertEqual(matrix.jfif_payloads(path), () if raw is None else (raw.hex(),))
                    if case.empty_ifd0_order is None:
                        self.assertIsNone(document["exif"])
                    else:
                        self.assertEqual(document["exif"]["byte_order"], case.empty_ifd0_order)
                        self.assertEqual(document["exif"]["tags"], {})

    def test_generated_cohort_drives_request_count_without_tag_allowlist(self) -> None:
        targets = matrix.generated_targets(matrix.LEDGER, matrix.RULES)
        self.assertGreater(len(targets), 0)
        declared = len(matrix.CARRIERS) * sum(len(target.qualifiers) for target in targets) * len(matrix.OPERATIONS)
        self.assertGreater(declared, 0)
        self.assertEqual(len({(target.raw_tag_id, target.name) for target in targets}), len(targets))

    def test_delete_noop_requires_exif_absence_and_preserves_raw_jfif(self) -> None:
        case = next(case for case in matrix.CARRIERS if case.label == "fresh-jfif-nonzero")
        target = matrix.generated_targets(matrix.LEDGER, matrix.RULES)[0]
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source, native_output, generated = root / "source.jpg", root / "native.jpg", root / "generated.jpg"
            matrix.write_carrier(source, case)
            shutil.copyfile(source, native_output)
            shutil.copyfile(source, generated)
            matrix.compare_jpeg(source, native_output, generated, target, "delete-absent-noop")
            self.assertEqual(matrix.jfif_payloads(generated), (matrix.raw_jfif_payload(case.jfif).hex(),))

    def test_insert_and_empty_reject_native_noop_outputs(self) -> None:
        case = next(case for case in matrix.CARRIERS if case.label == "fresh-no-jfif")
        target = matrix.generated_targets(matrix.LEDGER, matrix.RULES)[0]
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source, noop = root / "source.jpg", root / "noop.jpg"
            matrix.write_carrier(source, case)
            shutil.copyfile(source, noop)
            source_doc, noop_doc = native.parse_jpeg(source), native.parse_jpeg(noop)
            for operation in ("fresh-insert", "defined-empty"):
                with self.subTest(operation=operation):
                    with self.assertRaisesRegex(AssertionError, "absent-to-present"):
                        matrix._assert_native_target_transition(source_doc, noop_doc, target, operation)

    def test_delete_noop_rejects_retained_empty_ifd0(self) -> None:
        case = next(case for case in matrix.CARRIERS if case.label == "empty-ifd0-little")
        target = matrix.generated_targets(matrix.LEDGER, matrix.RULES)[0]
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "empty.jpg"
            matrix.write_carrier(path, case)
            document = native.parse_jpeg(path)
            with self.assertRaisesRegex(AssertionError, "retained or created"):
                matrix._assert_native_target_transition(document, document, target, "delete-absent-noop")

    def test_empty_ifd_delete_accepts_both_absent_but_rejects_generated_retention(self) -> None:
        target = matrix.generated_targets(matrix.LEDGER, matrix.RULES)[0]
        for case in matrix.CARRIERS:
            if case.empty_ifd0_order is None:
                continue
            with self.subTest(order=case.empty_ifd0_order), tempfile.TemporaryDirectory() as temp:
                source, cleared = Path(temp) / "source.jpg", Path(temp) / "cleared.jpg"
                matrix.write_carrier(source, case)
                matrix.write_carrier(cleared, matrix.CarrierCase("cleared", case.jfif, None))
                matrix.compare_jpeg(source, cleared, cleared, target, "delete-absent-noop")
                with self.assertRaisesRegex(AssertionError, "retained or created"):
                    matrix.compare_jpeg(source, cleared, source, target, "delete-absent-noop")

    def test_exact_target_count_is_compared_against_native(self) -> None:
        target = matrix.generated_targets(matrix.LEDGER, matrix.RULES)[0]
        expected = {"byte_order": "big", "tags": {str(target.raw_tag_id): {"type": 2, "count": 1, "value_hex": "00"}}, "children": {}}
        changed = {"byte_order": "big", "tags": {str(target.raw_tag_id): {"type": 2, "count": 2, "value_hex": "0000"}}, "children": {}}
        with self.assertRaisesRegex(AssertionError, "type/count/value"):
            matrix._compare_tiff(expected, changed)

    def test_request_preserves_existing_public_driver_schema(self) -> None:
        target = matrix.generated_targets(matrix.LEDGER, matrix.RULES)[0]
        request = matrix._request(Path("input.jpg"), Path("output.jpg"), target, target.qualifiers[0], "utf8", "value")
        self.assertEqual(set(request), {"route", "carrier", "input", "output", "key", "scalar", "value"})
        self.assertEqual(request["route"], "public-api")

    def test_rejects_invalid_raw_jfif_density_input(self) -> None:
        with self.assertRaisesRegex(ValueError, "malformed"):
            matrix.raw_jfif_payload(matrix.RawJfif("bad", 1, -1, 1))


if __name__ == "__main__":
    unittest.main()
