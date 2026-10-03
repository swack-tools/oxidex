"""Controls for selected-source runtime lens splicing."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gen_runtime_lens_types as producer


class RuntimeLensTypesTests(unittest.TestCase):
    def source(self):
        return {'makers': [
            {'make': 'Canon', 'rows': [('-1', 'n/a'), ('1', 'A'), ('1.1', 'other'),
                                       ('65535', 'n/a')], 'base_count': 3,
             'fractional_count': 1},
            {'make': 'Pentax', 'rows': [('3 1', 'B'), ('3 1.1', 'other')],
             'base_count': 1},
        ]}

    def original(self):
        return ('pub static CANON_LENS_TYPES: [(i64, &str); 1] = [\n'
                '        (1, "old"),\n    ];\n'
                'pub static PENTAX_LENS_TYPES: [(u8, u16, &str); 1] = [\n'
                '        (3, 1, "old"),\n    ];\n'
                'pub static OTHER: [u8; 1] = [7];\n')

    def test_splices_only_two_base_arrays_and_matching_fixture(self):
        expected, result, fixture = producer.render(self.source(), self.original())
        self.assertEqual(producer.parsed_arrays(result), expected)
        self.assertIn('pub static OTHER: [u8; 1] = [7];', result)
        self.assertNotIn('other', result)
        self.assertIn('"fractional_key_count_not_covered": 1', fixture)
        self.assertEqual(producer.render(self.source(), result)[1], result)

    def test_unsorted_canon_is_not_current(self):
        source = self.source()
        expected, result, _ = producer.render(source, self.original())
        self.assertEqual(producer.parsed_arrays(result), expected)
        unsorted = result.replace('        (-1, "n/a"),\n        (1, "A"),',
                                  '        (1, "A"),\n        (-1, "n/a"),')
        self.assertNotEqual(producer.parsed_arrays(unsorted), expected)

    def test_unrepresentable_and_duplicate_keys_refuse(self):
        for key in ('256 1', '3 65536', '3 x'):
            with self.subTest(key=key):
                source = self.source()
                source['makers'][1]['rows'][0] = (key, 'B')
                with self.assertRaisesRegex(ValueError, 'unrepresentable Pentax'):
                    producer.render(source, self.original())
        source = self.source()
        source['makers'][0]['rows'][1] = ('65535', 'A')
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            producer.render(source, self.original())

    def test_missing_array_refuses_without_partial_result(self):
        with self.assertRaisesRegex(ValueError, 'exactly one PENTAX'):
            producer.render(self.source(), self.original().split('pub static PENTAX')[0])


if __name__ == '__main__':
    unittest.main()
