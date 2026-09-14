import copy
import json
from pathlib import Path
import tempfile
import unittest

import verify_quicktime_keys_reader as v

HERE = Path(__file__).parent


class KeysEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.ledger = json.loads((HERE / 'quicktime_generated_keys_ledger.json').read_text())

    def report(self):
        rows = []
        for name, data in v.cases().items():
            identity = v.fixture_identity(data)
            spec = v.resolved_spec(identity, self.ledger)
            output = {'Keys:' + spec['name']: 'value'} if spec else {}
            for mode in v.MODES:
                raw = json.dumps([output])
                rows.append({'fixture': name, 'mode': mode, 'fixture_sha256': v.sha(data),
                             'fixture_identity': identity, 'expected': output, 'actual': output,
                             'native_json': raw, 'oxidex_json': raw,
                             'native_json_sha256': v.sha(raw.encode()),
                             'oxidex_json_sha256': v.sha(raw.encode()), 'matched': True})
        return {'schema': v.SCHEMA, 'observations': rows}

    def test_fixture_identity_parses_actual_index_namespace_and_key(self):
        cases = v.cases()
        self.assertEqual(v.fixture_identity(cases['ordinal-miss'])['ordinal'], 2)
        self.assertIsNone(v.fixture_identity(cases['ordinal-miss'])['selected'])
        self.assertIsNone(v.fixture_identity(cases['ordinal-zero'])['selected'])
        self.assertEqual(v.fixture_identity(cases['non-mdta'])['selected']['namespace'], 'abcd')
        self.assertEqual(v.resolved_spec(v.fixture_identity(cases['full-retry']), self.ledger)['name'], 'AndroidModel')
        self.assertEqual(v.resolved_spec(v.fixture_identity(cases['count-ignored']), self.ledger)['name'], 'Artist')
        self.assertEqual(v.resolved_spec(v.fixture_identity(cases['nul-key']), self.ledger)['name'], 'Artist')
        self.assertIn('\u00e9 \u2615'.encode(), cases['utf8'])

    def test_complete_report_credits_only_emitted_resolved_generated_rows(self):
        credit = v.validate_report(self.report(), self.ledger)
        names = {row['fixture'] for row in credit}
        self.assertTrue({'prefix', 'full-retry', 'utf8', 'numeric-enum'} <= names)
        self.assertTrue(names.isdisjoint({'unknown', 'refused-gps', 'refused-date', 'ordinal-zero', 'ordinal-miss'}))
        self.assertEqual(len(credit), 2 * len(names))

    def test_duplicate_missing_and_bad_fixture_hash_are_rejected(self):
        report = self.report()
        for rows in (report['observations'][:-1], report['observations'] + [report['observations'][0]]):
            with self.assertRaisesRegex(ValueError, 'grid'):
                v.validate_report({**report, 'observations': rows}, self.ledger)
        report['observations'][0]['fixture_sha256'] = '0' * 64
        with self.assertRaisesRegex(ValueError, 'fixture bytes'):
            v.validate_report(report, self.ledger)

    def test_matching_public_name_cannot_credit_wrong_indexed_source(self):
        report = self.report()
        row = next(row for row in report['observations'] if row['fixture'] == 'full-retry')
        forged = {'Keys:Artist': 'forged'}
        raw = json.dumps([forged])
        for side, claim in (('native_json', 'expected'), ('oxidex_json', 'actual')):
            row[side], row[side + '_sha256'], row[claim] = raw, v.sha(raw.encode()), forged
        with self.assertRaisesRegex(ValueError, 'source identity'):
            v.validate_report(report, self.ledger)

    def test_projection_forgery_is_rejected_even_if_matched_flag_is_true(self):
        report = self.report()
        report['observations'][0]['actual'] = {'Keys:Artist': 'forged'}
        with self.assertRaisesRegex(ValueError, 'projection'):
            v.validate_report(report, self.ledger)

    def test_native_empty_generated_fixture_is_degraded_not_a_match(self):
        report = self.report()
        row = report['observations'][0]
        for side, claim in (('native_json', 'expected'), ('oxidex_json', 'actual')):
            row[side], row[side + '_sha256'], row[claim] = '[{}]', v.sha(b'[{}]'), {}
        with self.assertRaisesRegex(ValueError, 'native generated fixture'):
            v.validate_report(report, self.ledger)

    def test_malformed_fixture_atom_size_refuses_instead_of_looping(self):
        for data in (b'\0' * 8, b'\0\0\0\x09moov', b'x'):
            with self.assertRaises(ValueError):
                v.fixture_identity(data)

    def test_source_replay_accepts_formatted_artifact_and_refuses_mutation(self):
        source = HERE / 'fixtures/quicktime_source_13_59.json'
        ledger = HERE / 'quicktime_generated_keys_ledger.json'
        rust = HERE.parents[1] / 'src/parsers/quicktime/generated_keys_specs.rs'
        self.assertEqual(set(v.authenticated(source, ledger, rust)), {'source_sha256', 'ledger_sha256', 'rust_sha256'})
        with tempfile.TemporaryDirectory() as folder:
            bad = Path(folder) / 'ledger.json'
            document = copy.deepcopy(self.ledger)
            document['identity_counts']['generated'] = 0
            bad.write_text(json.dumps(document))
            with self.assertRaisesRegex(ValueError, 'artifacts do not replay'):
                v.authenticated(source, bad, rust)

    def test_rejects_in_tree_output_before_build_or_native_access(self):
        with self.assertRaisesRegex(ValueError, 'outside the checkout'):
            v.compare(Path('missing'), v.baseline.ROOT / 'never-created', Path('missing'), Path('missing'), Path('missing'))


if __name__ == '__main__':
    unittest.main()
