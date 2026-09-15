"""Portable failure and fixture checks for the public timing acceptance tool."""
import os
from pathlib import Path
import shutil
import tempfile
import unittest
import jfif_timing_public_write_matrix as matrix


class PublicTimingTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.repo = Path(os.environ.get('OXIDEX_PUBLIC_TIMING_REPO', matrix.ROOT))
        cls.fresh, cls.native, cls.generated = matrix.load_tools(cls.repo)

    def test_carriers_preserve_order_presence_partial_and_zero(self):
        cases = matrix.timing_cases(self.fresh)
        self.assertEqual(len(cases), 12)
        self.assertEqual(len({case.label for case in cases}), 12)
        with tempfile.TemporaryDirectory() as temporary:
            for case in cases:
                with self.subTest(case=case.label):
                    path = Path(temporary) / (case.label + '.jpg')
                    matrix.write_carrier(self.fresh, path, case, self.fresh.DEFAULT_JPEG)
                    doc = self.native.parse_jpeg(path)
                    self.assertEqual(doc['exif'] is not None, case.empty_ifd0)
                    if case.empty_ifd0:
                        self.assertEqual(doc['exif']['tags'], {})
                        self.assertEqual(doc['exif']['byte_order'], 'little')
                    expected = tuple(segment[4:].hex() for segment in case.segments
                                     if segment[:2] == b'\xff\xe0' and segment[4:].startswith(b'JFIF\0'))
                    self.assertEqual(self.fresh.jfif_payloads(path), expected)
            partial = next(case for case in cases if case.label == 'fresh-a-partial-b')
            self.assertEqual(len(partial.segments[-1][4:]), 10)
            self.assertEqual(partial.segments[-1][-3:], b'\x02\x01\x2c')
            zero = next(case for case in cases if case.label == 'fresh-a-zero')
            self.assertEqual(zero.segments[-1][11:16], bytes(5))

    def test_native_noop_missing_set_and_rejected_set_fail(self):
        good = {'returncode': 0, 'stderr': '', 'stdout': '',
                'result': {'write_return': 1, 'error': None, 'set_calls': [{'return': 1}]}}
        matrix.assert_actual_native_set(self.fresh, good, 'good')
        for result in (
            {'write_return': 2, 'error': None, 'set_calls': [{'return': 1}]},
            {'write_return': 1, 'error': None, 'set_calls': []},
            {'write_return': 1, 'error': None, 'set_calls': [{'return': 0}]},
            {'write_return': 1, 'error': None, 'set_calls': [{'return': 2}]},
        ):
            with self.subTest(result=result), self.assertRaises(AssertionError):
                matrix.assert_actual_native_set(self.fresh, {**good, 'result': result}, 'bad')

    def test_full_comparator_rejects_success_without_target_presence(self):
        target = self.generated.generated_targets(self.fresh.LEDGER, self.fresh.RULES)[0]
        with tempfile.TemporaryDirectory() as temporary:
            source, noop = Path(temporary) / 'source.jpg', Path(temporary) / 'noop.jpg'
            matrix.write_carrier(self.fresh, source, matrix.timing_cases(self.fresh)[0], self.fresh.DEFAULT_JPEG)
            shutil.copyfile(source, noop)
            with self.assertRaisesRegex(AssertionError, 'absent-to-present'):
                self.fresh.compare_jpeg(source, noop, noop, target, 'fresh-insert')

    def test_source_joined_cohort_preserves_216_string_and_adds_numeric_requests(self):
        targets = self.generated.generated_targets(self.fresh.LEDGER, self.fresh.RULES)
        predecessor = self.generated.predecessor_public_targets(targets)
        self.assertEqual(len(predecessor), 15)
        self.assertTrue(set(predecessor).issubset(targets))
        self.assertGreater(len(targets), len(predecessor))
        self.assertEqual(len(matrix.timing_cases(self.fresh)) * sum(len(t.qualifiers) for t in predecessor), 360)
        strings = [target for target in predecessor if target.case_family == "native_string_scalar"]
        self.assertEqual(len(matrix.timing_cases(self.fresh)) * sum(len(t.qualifiers) for t in strings), 216)
        self.assertGreater(len(matrix.timing_cases(self.fresh)) * sum(len(t.qualifiers) for t in targets), 360)


if __name__ == '__main__':
    unittest.main()
