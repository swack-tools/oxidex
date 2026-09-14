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
    authenticated_ifd1_mandatory_input,
    generated_targets,
    native_requested_insert_is_noop,
    observed_operation,
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
        compare(original, relocated, relocated, 0xa001)
        wrong_pointer_type = copy.deepcopy(relocated)
        wrong_pointer_type["tags"]["34665"]["type"] = 3
        with self.assertRaisesRegex(AssertionError, "pointer type/count"):
            compare(original, relocated, wrong_pointer_type, 0xa001)
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


    def test_source_numeric_case_family_preserves_all_432_string_cases(self):
        from generated_tiff_write_matrix import case_inputs, predecessor_public_targets
        targets = generated_targets()
        predecessor = predecessor_public_targets(targets)
        strings = [target for target in targets if target.case_family == "native_string_scalar"]
        numeric = [target for target in targets if target.case_family == "native_unsigned_numeric_scalar"]
        self.assertEqual(len(predecessor), 15)
        self.assertEqual(len(targets), 19)
        self.assertLess(len(predecessor), len(targets))
        self.assertTrue(set(predecessor).issubset(targets))
        self.assertGreater(sum(len(target.qualifiers) * len(case_inputs(target)) * 3 for target in strings), 0)
        self.assertGreater(sum(len(target.qualifiers) * len(case_inputs(target)) * 3 for target in numeric), 0)
        self.assertTrue(all(case_inputs(target)['insert'] == b'72' for target in numeric))
        self.assertTrue(all('delete' in case_inputs(target) for target in numeric))

    def test_selected_directory_cases_derive_every_eligible_source_recipe(self):
        from generated_tiff_write_matrix import case_inputs, matrix_inputs, explicit_directory_operands, selected_qualifiers, directory_path, predecessor_public_targets
        targets = generated_targets()
        predecessor = predecessor_public_targets(targets)
        directories = explicit_directory_operands()
        self.assertEqual(directories, ("IFD0", "IFD1"))
        counts = {}
        for target in targets:
            for name in selected_qualifiers(target, directories):
                path = directory_path(target, name, directories)
                family = ("selected_" if path else "baseline_") + target.case_family
                counts[family] = counts.get(family, 0) + 3 * len(matrix_inputs(target))
        self.assertEqual(sum(counts.values()), sum(
            3 * len(matrix_inputs(target)) * len(selected_qualifiers(target, directories))
            for target in targets))
        self.assertEqual(sum(3 * len(matrix_inputs(target)) * len(selected_qualifiers(target, directories))
                             for target in predecessor), 1242)

    def test_decimal_public_cases_are_additive_and_typed(self):
        from generated_tiff_write_matrix import matrix_inputs, extended_numeric_inputs, explicit_directory_operands, selected_qualifiers, public_scalar, predecessor_public_targets
        targets, directories = generated_targets(), explicit_directory_operands()
        predecessor = predecessor_public_targets(targets)
        self.assertEqual(sum(len(selected_qualifiers(target, directories)) * len(matrix_inputs(target)) * 3
                             for target in predecessor), 1242)
        self.assertGreater(sum(len(selected_qualifiers(target, directories)) * len(matrix_inputs(target)) * 3
                               for target in targets), 1242)
        for target in targets:
            for case, value in extended_numeric_inputs(target).items():
                self.assertEqual(public_scalar(target, case, value), "float" if case == "numeric_float" else "utf8")

    def test_tiff_ifd1_fixture_preserves_image_and_existing_entries(self):
        from generated_tiff_write_matrix import make_existing_next_ifd_fixture
        from native_write_matrix import make_tiff
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "carrier.tif"
            for order in ("little", "big"):
                make_tiff(path, order)
                original = parse_tiff(path.read_bytes())
                make_existing_next_ifd_fixture(path, "tiff_" + order)
                changed = parse_tiff(path.read_bytes())
                self.assertEqual(changed['tags'], original['tags'])
                self.assertEqual(changed['image_payload_hex'], original['image_payload_hex'])
                self.assertEqual(changed['children']['NextIFD']['tags'], {})

    def test_selected_ifd_comparison_preserves_same_id_in_ifd0(self):
        root = {"byte_order": "little", "image_payload_hex": None, "tags": {
            "282": {"type": 5, "count": 1, "value_hex": "5b00000001000000"}}}
        seed = copy.deepcopy(root)
        seed["children"] = {"NextIFD": copy.deepcopy(root)}
        expected = copy.deepcopy(seed)
        expected["children"]["NextIFD"]["tags"]["282"]["value_hex"] = "2c01000001000000"
        compare(seed, expected, expected, 282, target_directory=("NextIFD",))
        bad = copy.deepcopy(expected)
        bad["tags"]["282"]["value_hex"] = "2c01000001000000"
        with self.assertRaises(AssertionError):
            compare(seed, bad, bad, 282, target_directory=("NextIFD",))
        with self.assertRaises(AssertionError):
            compare(seed, expected, seed, 282, target_directory=("NextIFD",))

    def test_native_seed_mandatory_ifd1_tag_turns_requested_insert_into_update(self):
        seed = {"exif": {"tags": {}, "children": {"NextIFD": {"tags": {
            "282": {"type": 5, "count": 1, "value_hex": "4800000001000000"}
        }}}}}
        self.assertEqual(
            observed_operation(seed, "jpeg", ("NextIFD",), 282, "insert"),
            (True, "update"),
        )
        self.assertEqual(
            observed_operation(seed, "jpeg", ("NextIFD",), 283, "insert"),
            (False, "insert"),
        )
        self.assertEqual(
            observed_operation(seed, "jpeg", ("NextIFD",), 282, "delete"),
            (True, "delete"),
        )
        x_resolution = next(target for target in generated_targets() if target.raw_tag_id == 282)
        self.assertEqual(authenticated_ifd1_mandatory_input(x_resolution), b"72")
        self.assertTrue(native_requested_insert_is_noop(
            seed, copy.deepcopy(seed), "jpeg", ("NextIFD",), x_resolution, "insert", b"72"
        ))
        changed = copy.deepcopy(seed)
        changed["exif"]["children"]["NextIFD"]["tags"]["282"]["value_hex"] = "2c01000001000000"
        self.assertFalse(native_requested_insert_is_noop(
            seed, changed, "jpeg", ("NextIFD",), x_resolution, "insert", b"72"
        ))
        self.assertFalse(native_requested_insert_is_noop(
            seed, seed, "jpeg", ("NextIFD",), x_resolution, "update", b"72"
        ))
        self.assertFalse(native_requested_insert_is_noop(
            seed, seed, "jpeg", ("NextIFD",), x_resolution, "insert", b"300"
        ))

    def test_unchanged_arbitrary_insert_is_not_a_native_mandatory_default_noop(self):
        arbitrary = next(target for target in generated_targets() if target.raw_tag_id == 269)
        seed = {"exif": {"tags": {}, "children": {"NextIFD": {"tags": {
            "269": {"type": 2, "count": 4, "value_hex": "666f6f00"}
        }}}}}
        self.assertIsNone(authenticated_ifd1_mandatory_input(arbitrary))
        self.assertFalse(native_requested_insert_is_noop(
            seed, copy.deepcopy(seed), "jpeg", ("NextIFD",), arbitrary, "insert", b"foo"
        ))

    def test_generated_targets_follow_ledger_identities_and_refuse_malformed_or_empty_ledgers(self):
        from generated_tiff_write_matrix import predecessor_public_targets
        targets = generated_targets()
        self.assertEqual(len(predecessor_public_targets(targets)), 15)
        self.assertGreater(len(targets), 15)
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
