"""Negative controls for the internal writer's actual-file comparison."""
import copy
import unittest

from pathlib import Path
import json
import tempfile

from generated_tiff_write_matrix import (
    GeneratedTarget,
    RULES,
    compare,
    compare_carrier,
    generated_targets,
    selected_rehearsal_contract,
)
from native_write_matrix import parse_tiff


class GeneratedTiffComparison(unittest.TestCase):
    def test_selected_release_contract_requires_pin_native_and_regenerated_ledger_to_agree(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pin, ledger = root / ".exiftool-version", root / "ledger.json"
            pin.write_text("11.78\n")
            ledger.write_text(json.dumps({"exiftool_version": "11.78", "emitted": True, "reason": None}))
            identity = {"result": {"exiftool_version": "11.78"}}
            contract = selected_rehearsal_contract(identity, "11.78", pin, ledger)
            self.assertEqual(contract["mode"], "selected-release-rehearsal")
            self.assertEqual(contract["ledger_exiftool_version"], "11.78")
            pin.write_text("12.64\n")
            with self.assertRaisesRegex(ValueError, "checkout pin"):
                selected_rehearsal_contract(identity, "11.78", pin, ledger)
            pin.write_text("11.78\n")
            ledger.write_text(json.dumps({"exiftool_version": "12.64", "emitted": True, "reason": None}))
            with self.assertRaisesRegex(ValueError, "ledger"):
                selected_rehearsal_contract(identity, "11.78", pin, ledger)
            ledger.write_text(json.dumps({"exiftool_version": "11.78", "emitted": False, "reason": "unsupported"}))
            with self.assertRaisesRegex(ValueError, "did not emit"):
                selected_rehearsal_contract(identity, "11.78", pin, ledger)

    def test_relocated_ifd_targets_match_but_changed_target_data_and_cycles_refuse(self):
        def file_at(offset, color=1):
            root = (b"II\x2a\0\x08\0\0\0\x01\0\x69\x87\x04\0\x01\0\0\0"
                    + offset.to_bytes(4, "little") + bytes(4))
            child = b"\x01\0\x01\xa0\x03\0\x01\0\0\0" + color.to_bytes(4, "little") + bytes(4)
            return root + bytes(offset - len(root)) + child
        original = parse_tiff(file_at(26), False)
        relocated = parse_tiff(file_at(28), False)
        compare(original, relocated, original, 0xa001)
        changed = parse_tiff(file_at(26, 2), False)
        with self.assertRaisesRegex(AssertionError, "type/count/value"):
            compare(original, relocated, changed, 0xa001)
        cyclic = bytearray(file_at(26))
        cyclic[18:22] = (8).to_bytes(4, "little")
        with self.assertRaisesRegex(ValueError, "directory pointer"):
            parse_tiff(bytes(cyclic), False)
        child_corrupted = parse_tiff(file_at(28, 3), False)
        with self.assertRaisesRegex(AssertionError, "unrelated tag"):
            compare(original, relocated, child_corrupted, 0xa001)

    def test_jpeg_comparison_rejects_non_exif_and_scan_mutations(self):
        tiff = {"byte_order": "little", "image_payload_hex": None, "tags": {}}
        document = {"exif": tiff, "sos_to_end_sha256": "scan", "non_exif_sha256": "other"}
        compare_carrier(document, document, document, "jpeg", 316)
        for key in ("sos_to_end_sha256", "non_exif_sha256", "exif"):
            changed = copy.deepcopy(document)
            changed[key] = None if key == "exif" else "changed"
            with self.subTest(key=key), self.assertRaises(AssertionError):
                compare_carrier(document, document, changed, "jpeg", 316)

    def test_comparison_rejects_wrong_value_type_count_missing_extra_and_payload(self):
        seed = {"byte_order": "little", "image_payload_hex": "ff", "tags": {
            "315": {"type": 2, "count": 2, "value_hex": "6100"},
            "316": {"type": 2, "count": 2, "value_hex": "6200"},
        }}
        expected = copy.deepcopy(seed)
        expected["tags"]["316"]["value_hex"] = "6300"
        compare(seed, expected, expected, 316)
        for dimension, wrong in (("type", 1), ("count", 3), ("value_hex", "6400")):
            actual = copy.deepcopy(expected)
            actual["tags"]["316"][dimension] = wrong
            with self.subTest(dimension=dimension), self.assertRaises(AssertionError):
                compare(seed, expected, actual, 316)
        mutations = []
        actual = copy.deepcopy(expected)
        del actual["tags"]["316"]
        mutations.append(actual)
        actual = copy.deepcopy(expected)
        actual["tags"]["999"] = dict(actual["tags"]["316"])
        mutations.append(actual)
        actual = copy.deepcopy(expected)
        actual["image_payload_hex"] = "fe"
        mutations.append(actual)
        actual = copy.deepcopy(expected)
        actual["byte_order"] = "big"
        mutations.append(actual)
        for actual in mutations:
            with self.assertRaises(AssertionError):
                compare(seed, expected, actual, 316)

    def test_both_writers_corrupting_same_unrelated_tag_is_not_parity(self):
        seed = {"byte_order": "little", "image_payload_hex": "ff", "tags": {
            "315": {"type": 2, "count": 2, "value_hex": "6100"}}}
        corrupted = copy.deepcopy(seed)
        corrupted["tags"]["315"]["value_hex"] = "6200"
        with self.assertRaisesRegex(AssertionError, "unrelated tag"):
            compare(seed, corrupted, corrupted, 316)


    def test_generated_targets_follow_ledger_identities_and_refuse_malformed_or_empty_ledgers(self):
        targets = generated_targets()
        self.assertEqual(len(targets), 9)
        self.assertEqual(targets[0], GeneratedTarget(0x010d, "DocumentName", "EXIF", "IFD0"))
        self.assertEqual(targets[0].qualifiers, ("EXIF:DocumentName", "IFD0:DocumentName"))
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ledger.json"
            path.write_text(json.dumps({"emitted": True, "reason": None, "recipes": [{
                "raw_tag_id": 7, "name": "Tag", "table_group0": "EXIF", "physical_write_group": "IFD0",
                "module": "Exif", "table": "Main", "full_name": "Image::ExifTool::Exif::Main"}]}))
            with self.assertRaisesRegex(ValueError, "ledger and Rust rule identities differ"):
                generated_targets(path)
            path.write_text(json.dumps({"emitted": True, "reason": None, "recipes": []}))
            with self.assertRaisesRegex(ValueError, "no recipes"):
                generated_targets(path)
            path.write_text(json.dumps({"emitted": True, "reason": None, "recipes": [{
                "raw_tag_id": 7, "name": "Tag", "table_group0": "EXIF", "physical_write_group": "IFD0",
                "module": "Exif", "table": "Other", "full_name": "Image::ExifTool::Exif::Main"}]}))
            with self.assertRaisesRegex(ValueError, "outside"):
                generated_targets(path)

    def test_compare_preserves_every_non_target_id_and_transition_refuses_noop(self):
        seed = {"byte_order": "little", "image_payload_hex": "ff", "tags": {
            "269": {"type": 2, "count": 2, "value_hex": "6100"},
            "316": {"type": 2, "count": 2, "value_hex": "6200"}}}
        expected = copy.deepcopy(seed)
        expected["tags"]["269"]["value_hex"] = "6300"
        compare(seed, expected, expected, 269)
        actual = copy.deepcopy(expected)
        actual["tags"]["316"]["value_hex"] = "6400"
        with self.assertRaisesRegex(AssertionError, "unrelated tag"):
            compare(seed, expected, actual, 269)
        with self.assertRaisesRegex(AssertionError, "no-op"):
            from native_write_matrix import assert_target_transition
            assert_target_transition(seed, seed, "tiff_little", 269, "update")
