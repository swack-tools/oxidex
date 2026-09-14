#!/usr/bin/env python3
"""Contract tests for the mixed public whole-map write matrix."""
import copy
import os
import tempfile
import importlib.util
from pathlib import Path
import sys
import unittest

HERE = Path(__file__).parent
sys.path.insert(0, str(HERE))


def load(name: str):
    spec = importlib.util.spec_from_file_location(name, HERE / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    assert spec.loader
    spec.loader.exec_module(module)
    return module


native = load("native_write_matrix")
generated = load("generated_tiff_write_matrix")
matrix = load("generated_public_write_batch_matrix")


class PublicWriteBatchMatrixTests(unittest.TestCase):
    def test_cases_use_the_provided_ledger_target_not_a_tag_allowlist(self):
        target = generated.GeneratedTarget(0xBEEF, "LedgerOnly", "EXIF", "IFD0")
        for case in matrix.BATCH_CASES:
            with self.subTest(case=case):
                spec = matrix.batch_case(target, case)
                self.assertEqual(spec["seed"][1]["tag"], "IFD0:LedgerOnly")
                generated_items = spec["generated"]
                names = {item["key"] for item in generated_items}
                self.assertTrue(any(name.endswith(":LedgerOnly") for name in names))
                self.assertEqual(spec["changed_tags"], {315, 0xBEEF})
                self.assertEqual(spec["expect_ok"], case not in {"conflicting_aliases", "forged_generated_identity_after_legacy"})
        fault = matrix.batch_case(target, "forged_generated_identity_after_legacy")
        self.assertEqual(fault["fault"], "forged-final-identity-after-legacy")
        alias = matrix.batch_case(target, "alias_replacement")
        self.assertIn({"key": "IFD0:LedgerOnly", "scalar": "omitted"}, alias["generated"])

    def test_native_batch_assertion_rejects_operand_reordering_and_state_drift(self):
        expected = [
            {"tag": "IFD0:Artist", "scalar": "utf8", "value": "artist"},
            {"tag": "IFD0:LedgerOnly", "scalar": "bytes", "value": "610062"},
        ]
        call = {"returncode": 0, "result": {"write_return": 1, "error": None, "set_calls": [
            {"tag": "IFD0:Artist", "return": 1, "input": {"defined": True, "utf8": True, "hex": "617274697374"}},
            {"tag": "IFD0:LedgerOnly", "return": 1, "input": {"defined": True, "utf8": False, "hex": "610062"}},
        ]}}
        matrix.assert_native_batch(call, "fixture", expected)
        reordered = copy.deepcopy(call)
        reordered["result"]["set_calls"].reverse()
        with self.assertRaisesRegex(AssertionError, "order/name"):
            matrix.assert_native_batch(reordered, "fixture", expected)
        changed = copy.deepcopy(call)
        changed["result"]["set_calls"][1]["input"]["hex"] = "610063"
        with self.assertRaisesRegex(AssertionError, "hex"):
            matrix.assert_native_batch(changed, "fixture", expected)

    @unittest.skipUnless(os.environ.get("EXIFTOOL_PERL") and os.environ.get("OXIDEX_PINNED_EXIFTOOL"),
                         "requires explicitly selected native Perl and library")
    def test_native_batch_preserves_declared_scalar_flags_including_ascii(self):
        operands = [
            {"tag": "IFD0:Artist", "scalar": "utf8", "value": "ascii"},
            {"tag": "IFD0:Software", "scalar": "utf8", "value": ""},
            {"tag": "IFD0:DocumentName", "scalar": "utf8", "value": "é"},
            {"tag": "IFD0:HostComputer", "scalar": "bytes", "value": "610062"},
        ]
        with tempfile.TemporaryDirectory() as temporary:
            source, target = Path(temporary) / "in.tif", Path(temporary) / "out.tif"
            native.make_tiff(source, "little")
            call = native.run_native_batch(
                native.resolve_perl(Path(os.environ["EXIFTOOL_PERL"])),
                native.resolve_library(Path(os.environ["OXIDEX_PINNED_EXIFTOOL"])),
                source, target, operands)
            matrix.assert_native_batch(call, "typed native scalars", operands)
            self.assertTrue(target.is_file())

    def test_multi_target_comparison_preserves_every_unlisted_entry(self):
        seed = {"byte_order": "little", "image_payload_hex": "ff", "tags": {
            "315": {"type": 2, "count": 2, "value_hex": "6100"},
            "316": {"type": 2, "count": 2, "value_hex": "6200"},
            "305": {"type": 2, "count": 2, "value_hex": "7300"},
        }}
        expected = copy.deepcopy(seed)
        expected["tags"]["315"]["value_hex"] = "6300"
        expected["tags"]["316"]["value_hex"] = "6400"
        generated.compare(seed, expected, expected, {315, 316})
        corrupted = copy.deepcopy(expected)
        corrupted["tags"]["305"]["value_hex"] = "7800"
        with self.assertRaisesRegex(AssertionError, "unrelated"):
            generated.compare(seed, corrupted, corrupted, {315, 316})
        with self.assertRaisesRegex(ValueError, "malformed"):
            generated.changed_tag_ids({"316"})

    def test_native_batch_input_refuses_unsupported_scalars_before_any_subprocess(self):
        with self.assertRaisesRegex(ValueError, "unsupported scalar/value"):
            native.run_native_batch(Path("perl"), Path("missing"), Path("in"), Path("out"), [
                {"tag": "IFD0:Artist", "scalar": "integer", "value": "1"},
            ])
        with self.assertRaisesRegex(ValueError, "empty"):
            native.run_native_batch(Path("perl"), Path("missing"), Path("in"), Path("out"), [])


if __name__ == "__main__":
    unittest.main()
