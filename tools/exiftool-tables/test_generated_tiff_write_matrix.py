"""Negative controls for the internal writer's actual-file comparison."""
import copy
import os
import unittest

from pathlib import Path
import json
import tempfile

import generated_tiff_write_matrix as matrix
from generated_tiff_write_matrix import (
    GeneratedTarget,
    RULES,
    compare,
    compare_carrier,
    authenticated_ifd1_mandatory_input,
    generated_targets,
    predecessor_public_targets,
    native_requested_insert_is_noop,
    observed_operation,
    native_survivor_value,
    selected_rehearsal_contract,
)
import native_write_matrix as native
from native_write_matrix import parse_tiff


class GeneratedTiffComparison(unittest.TestCase):
    def test_predecessor_cohort_refuses_a_retired_identity(self):
        target = GeneratedTarget(315, "Artist", "EXIF", "IFD0")
        cohort = [{"raw_tag_id": 315, "name": "Artist", "group0": "EXIF", "write_group": "IFD0"}]
        import hashlib
        digest = hashlib.sha256(json.dumps(cohort, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")).hexdigest()
        ledger = {"source_identity": "a" * 64, "entries": [{**cohort[0], "state": "current", "history": [{"state": "current"}]}],
                  "predecessor_cohort": cohort, "predecessor_cohort_sha256": digest}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "migration.json"
            path.write_text(json.dumps(ledger))
            self.assertEqual(predecessor_public_targets((target,), path), (target,))
            with self.assertRaisesRegex(ValueError, "absent from current final recipes"):
                predecessor_public_targets((), path)

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

    def test_native_selected_directory_pruning_is_checked_without_masking_retention(self):
        from generated_tiff_write_matrix import assert_selected_target_transition, at_directory
        root = {"byte_order": "little", "image_payload_hex": "ff", "tags": {
            "315": {"type": 2, "count": 2, "value_hex": "6100"}}}
        seed = copy.deepcopy(root)
        seed["children"] = {"NextIFD": {"byte_order": "little", "image_payload_hex": None,
            "tags": {"315": {"type": 2, "count": 2, "value_hex": "6200"}}}}
        native_pruned = copy.deepcopy(root)
        for carrier in ("tiff_little", "tiff_big", "jpeg"):
            def wrap(tiff):
                return {"exif": tiff, "sos_to_end_sha256": "scan", "non_exif_sha256": "other"} if carrier == "jpeg" else tiff
            with self.subTest(carrier=carrier):
                before, after = wrap(seed), wrap(native_pruned)
                assert_selected_target_transition(before, after, carrier, ("NextIFD",), 315, "delete")
                compare_carrier(before, after, after, carrier, 315, target_directory=("NextIFD",), allow_directory_removal=True)
                with self.assertRaisesRegex(AssertionError, "directory identities"):
                    compare_carrier(before, after, before, carrier, 315, target_directory=("NextIFD",), allow_directory_removal=True)
                with self.assertRaisesRegex(AssertionError, "unrelated TIFF directory"):
                    compare_carrier(before, after, after, carrier, 315, target_directory=("NextIFD",))
                for operation, tag in (("update", 315), ("insert", 315), ("delete", 316)):
                    with self.assertRaisesRegex(AssertionError, "without deleting"):
                        assert_selected_target_transition(before, after, carrier, ("NextIFD",), tag, operation)
                with self.assertRaisesRegex(ValueError, "directory is absent"):
                    at_directory(after, carrier, ("NextIFD",))
        for corruption in ("extra_tag", "subtree", "nondefault_mandatory", "image_payload"):
            bad_seed = copy.deepcopy(seed)
            child = bad_seed["children"]["NextIFD"]
            if corruption == "extra_tag":
                child["tags"]["316"] = {"type": 2, "count": 2, "value_hex": "7800"}
            elif corruption == "subtree":
                child["children"] = {"NextIFD": copy.deepcopy(root)}
            elif corruption == "nondefault_mandatory":
                child["tags"]["282"] = {"type": 5, "count": 1, "value_hex": "2c01000001000000"}
            else:
                child["image_payload_hex"] = "feed"
            with self.subTest(corruption=corruption), self.assertRaisesRegex(AssertionError, "pruned IFD1"):
                compare(bad_seed, native_pruned, native_pruned, 315, target_directory=("NextIFD",), allow_directory_removal=True)
        mandatory_seed = copy.deepcopy(seed)
        mandatory_seed["children"]["NextIFD"]["tags"]["282"] = {
            "type": 5, "count": 1, "value_hex": "4800000001000000"}
        compare(mandatory_seed, native_pruned, native_pruned, 315, target_directory=("NextIFD",), allow_directory_removal=True)
        # WriteExif compares mandatory values through each survivor's selected
        # physical format. Compression encoded as LONG is still its captured
        # scalar value, while a changed count or value must keep the directory.
        long_survivor = copy.deepcopy(seed)
        long_survivor["children"]["NextIFD"]["tags"]["259"] = {
            "type": 4, "count": 1, "value_hex": "06000000"
        }
        compare(long_survivor, native_pruned, native_pruned, 315,
                target_directory=("NextIFD",), allow_directory_removal=True)
        for changed in (
            {"type": 4, "count": 2, "value_hex": "0600000006000000"},
            {"type": 4, "count": 1, "value_hex": "07000000"},
        ):
            bad = copy.deepcopy(long_survivor)
            bad["children"]["NextIFD"]["tags"]["259"] = changed
            with self.assertRaisesRegex(AssertionError, "pruned IFD1"):
                compare(bad, native_pruned, native_pruned, 315,
                        target_directory=("NextIFD",), allow_directory_removal=True)
        # Both outputs dropping another child or changing IFD0's same-name tag
        # must remain failures even when native removes the selected IFD.
        seed["children"]["OtherIFD"] = copy.deepcopy(seed["children"]["NextIFD"])
        with self.assertRaisesRegex(AssertionError, "unrelated TIFF directory"):
            compare(seed, native_pruned, native_pruned, 315, target_directory=("NextIFD",), allow_directory_removal=True)
        native_pruned["children"] = {"OtherIFD": copy.deepcopy(seed["children"]["OtherIFD"])}
        native_pruned["tags"]["315"]["value_hex"] = "6300"
        with self.assertRaisesRegex(AssertionError, "unrelated tag"):
            compare(seed, native_pruned, native_pruned, 315, target_directory=("NextIFD",), allow_directory_removal=True)

    def test_fixed_width_ifd1_survivor_carriers_require_exact_count_one_bytes(self):
        # Pinned WriteValue probe: each of these forms packs 6 at count 0/1,
        # but count 2 is undef. The raw count-zero record cannot expose the
        # inferred native byte span, so it is deliberately a visible non-match.
        forms = ((1, "int8u", 1), (6, "int8s", 1), (8, "int16s", 2), (9, "int32s", 4))
        for order in ("little", "big"):
            for field_type, name, width in forms:
                capability = [{"format_name": name, "tiff_type": field_type,
                               "width": width, "operation": "write_value_scalar"}]
                expected = (6 & ((1 << (width * 8)) - 1)).to_bytes(width, order).hex()
                entry = {"type": field_type, "count": 1, "value_hex": expected}
                with self.subTest(order=order, field_type=field_type):
                    self.assertEqual(native_survivor_value({"value": 6}, entry, order, capability), expected)
                    self.assertIsNone(native_survivor_value(
                        {"value": 6}, {"type": field_type, "count": 0, "value_hex": ""}, order, capability))
                    self.assertIsNone(native_survivor_value(
                        {"value": 6}, {"type": field_type, "count": 2,
                                        "value_hex": expected * 2}, order, capability))
                    wrong = dict(entry); wrong["value_hex"] = (7).to_bytes(width, order).hex()
                    self.assertEqual(native_survivor_value({"value": 6}, wrong, order, capability), expected)
                    self.assertNotEqual(wrong["value_hex"], expected)

    @unittest.skipUnless(os.environ.get("EXIFTOOL_PERL") and os.environ.get("OXIDEX_PINNED_EXIFTOOL"),
                         "requires canonical EXIFTOOL_PERL and OXIDEX_PINNED_EXIFTOOL")
    def test_pinned_native_ifd1_cleanup_uses_actual_fixed_width_tiff_and_jpeg_carriers(self):
        """Drive IFD1:Artist removal against raw, source-derived TIFF records.

        This is intentionally a native oracle fixture.  The matrix's Rust
        driver consumes the same physical shape later; this test proves that
        the fixture itself uses real file bytes and that native WriteExif,
        rather than the checker, decides pruning.
        """
        perl = native.resolve_perl(Path(os.environ["EXIFTOOL_PERL"]))
        library = native.resolve_library(Path(os.environ["OXIDEX_PINNED_EXIFTOOL"]))
        native.assert_contract_version(native.native_identity(perl, library))
        artist = next(target for target in generated_targets()
                      if target.name == "Artist")
        forms = (1, 6, 8, 9)
        base = matrix.ROOT / "tests/fixtures/jpeg/edge_cases/orientation_2.jpg"
        self.assertTrue(base.is_file(), "committed JPEG carrier is absent")
        count_zero = []
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for carrier in ("tiff", "jpeg"):
                for order in ("little", "big"):
                    for field_type in forms:
                        for count in (0, 1, 2):
                            with self.subTest(carrier=carrier, order=order, field_type=field_type, count=count):
                                raw = root / "raw.tif"
                                metadata = native.make_mandatory_ifd1_survivor_tiff(
                                    raw, order, field_type, count, artist.raw_tag_id
                                )
                                source = raw if carrier == "tiff" else root / "raw.jpg"
                                if carrier == "jpeg":
                                    native.wrap_tiff_exif_in_jpeg(source, base, raw.read_bytes())
                                seeded = native.inspect(source, "tiff_" + order if carrier == "tiff" else "jpeg")
                                child = (seeded if carrier == "tiff" else seeded["exif"])["children"]["NextIFD"]
                                self.assertEqual(child["tags"][str(metadata["survivor_tag_id"])]["type"], field_type)
                                self.assertEqual(child["tags"][str(metadata["survivor_tag_id"])]["count"], count)
                                self.assertIn(str(artist.raw_tag_id), child["tags"])
                                output = root / ("output.tif" if carrier == "tiff" else "output.jpg")
                                output.unlink(missing_ok=True)
                                call = native.run_native_batch(
                                    perl, library, source, output,
                                    [{"tag": "IFD1:Artist", "scalar": "undefined"}],
                                )
                                native.assert_native(call, f"{carrier}/{order}/{field_type}/{count}")
                                observed = native.inspect(output, "tiff_" + order if carrier == "tiff" else "jpeg")
                                root_after = observed if carrier == "tiff" else observed["exif"]
                                after = root_after["children"].get("NextIFD")
                                if count == 1:
                                    self.assertIsNone(after, "matching one-scalar survivor must prune IFD1")
                                    matrix.assert_prunable_ifd1(child, {str(artist.raw_tag_id)})
                                else:
                                    self.assertIsNotNone(after, "native non-scalar survivor must retain IFD1")
                                    self.assertNotIn(str(artist.raw_tag_id), after["tags"])
                                if count == 0:
                                    count_zero.append((carrier, order, field_type, after is None))
        # Count zero is not inferred from WriteValue's scalar probe: this
        # assertion preserves the observed native classification in the file
        # fixture.  A changed native outcome fails visibly here.
        self.assertTrue(count_zero)
        self.assertTrue(all(not pruned for _, _, _, pruned in count_zero), count_zero)

    def test_mandatory_ifd1_cleanup_rejects_mismatched_capture_hashes(self):
        with tempfile.TemporaryDirectory() as temporary:
            temporary = Path(temporary)
            bad = json.loads(matrix.MANDATORY_LEDGER.read_text())
            bad["recipe"]["write_value_source_sha256"] = "0" * 64
            ledger = temporary / "mandatory.json"
            ledger.write_text(json.dumps(bad))
            from unittest.mock import patch
            with patch.object(matrix, "MANDATORY_LEDGER", ledger), self.assertRaisesRegex(
                ValueError, "does not join selected writer artifacts"
            ):
                matrix.mandatory_cleanup_recipe()

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
