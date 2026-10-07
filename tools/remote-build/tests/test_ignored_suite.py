"""Small source controls for owned ignored fixture staging."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import ignored_suite as suite


class IgnoredSuiteControls(unittest.TestCase):
    def fixture_plan(self, root):
        repo = root / 'checkout'
        repo.mkdir()
        cache = root / 'cache'
        cache.mkdir()
        rows, evidence = [], []
        for index in range(25):
            identity = f'fixture-{index}'
            value = f'authentic-{index}'.encode()
            target = f'tests/samples/owned/{identity}.bin'
            source = cache / identity
            source.write_bytes(value)
            kind = 'repo' if index == 0 else 'pinned'
            if kind == 'repo':
                (repo / target).parent.mkdir(parents=True)
                (repo / target).write_bytes(value)
            rows.append({'id': identity, 'target': target, 'source': {'kind': kind},
                         'sha256': hashlib.sha256(value).hexdigest(), 'bytes': len(value)})
            evidence.append({'id': identity, 'target': target, 'path': str(source),
                             'status': 'PASS', 'native': {'compared_count': 2}})
        manifest = {'inputs': rows}
        manifest_file = root / 'manifest.json'
        manifest_file.write_text(json.dumps(manifest))
        receipt = {'status': 'PASS', 'manifest_sha256': suite.digest(manifest_file), 'inputs': evidence}
        return repo, manifest_file, manifest, receipt

    def test_stage_all_25_and_remove_only_24_owned_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            repo, manifest_file, manifest, receipt = self.fixture_plan(Path(directory))
            with patch.object(suite.inputs, 'MANIFEST', manifest_file):
                staged = suite.stage_inputs(repo, receipt, manifest)
            self.assertEqual(len(staged), 24)
            self.assertTrue(all(Path(row['path']).is_file() for row in staged))
            suite.remove_owned_inputs(staged)
            self.assertTrue((repo / manifest['inputs'][0]['target']).is_file())
            self.assertTrue(all(not Path(row['path']).exists() for row in staged))

    def test_zero_native_comparison_refuses_before_staging(self):
        with tempfile.TemporaryDirectory() as directory:
            repo, manifest_file, manifest, receipt = self.fixture_plan(Path(directory))
            receipt['inputs'][0]['native']['compared_count'] = 0
            with patch.object(suite.inputs, 'MANIFEST', manifest_file), \
                 self.assertRaisesRegex(RuntimeError, 'positive native proof'):
                suite.stage_inputs(repo, receipt, manifest)
            self.assertFalse((repo / manifest['inputs'][1]['target']).exists())

    def test_changed_owned_copy_is_retained_and_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            repo, manifest_file, manifest, receipt = self.fixture_plan(Path(directory))
            with patch.object(suite.inputs, 'MANIFEST', manifest_file):
                staged = suite.stage_inputs(repo, receipt, manifest)
            altered = Path(staged[0]['path'])
            altered.write_bytes(b'changed')
            with self.assertRaisesRegex(RuntimeError, 'retaining input'):
                suite.remove_owned_inputs(staged)
            self.assertTrue(altered.exists())

    def test_all_executable_targets_and_normal_docs_are_separate(self):
        self.assertEqual(suite.EXECUTABLE_COMMAND,
                         ['cargo', 'test', '--release', '--workspace', '--all-features', '--locked',
                          '--no-fail-fast', '--lib', '--bins', '--tests', '--message-format=json',
                          '--', '--include-ignored'])
        self.assertEqual(suite.DOC_COMMAND,
                         ['cargo', 'test', '--doc', '--workspace', '--all-features', '--locked'])

    def test_driver_population_requires_fresh_nonempty_requests(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            owner = root / 'owner.json'
            owner.write_text(json.dumps({'declared': 1530}))
            scalar = {'rows': [{'id': f'cell-{n}'} for n in range(1530)],
                      'owner_report': str(owner), 'requests': str(root / 'scalar.json'),
                      'results': str(root / 'scalar-results.json')}
            import sys
            sys.path.insert(0, str(suite.ROOT / 'tools/exiftool-tables'))
            import raw_scoped_native_adapter as raw_owner
            raw_rows = [{'target': target[0], 'op': op} for target in raw_owner.TARGETS
                        for op in raw_owner.OPS]
            raw = {'rows': raw_rows, 'unsupported': [], 'requests': str(root / 'raw.jsonl'),
                   'results': str(root / 'raw-results.jsonl')}
            with self.assertRaisesRegex(RuntimeError, 'absent/empty'):
                suite.validate_driver_manifests(scalar, raw)
            Path(scalar['requests']).write_text(json.dumps([{'output': f'/target/scalar/{n}'}
                                                           for n in range(1530)]))
            Path(raw['requests']).write_text(''.join(json.dumps({'output': f'/target/raw/{n}'}) + '\n'
                                                       for n in range(len(raw_rows))))
            self.assertEqual(suite.validate_driver_manifests(scalar, raw)['raw_source_cells'],
                             len(raw_rows))
            Path(raw['results']).write_text('{}\n')
            with self.assertRaisesRegex(RuntimeError, 'stale'):
                suite.validate_driver_manifests(scalar, raw)
            Path(raw['results']).unlink()
            raw['rows'] = raw_rows[:-1]
            with self.assertRaisesRegex(RuntimeError, 'current owner/source cohort'):
                suite.validate_driver_manifests(scalar, raw)

    def test_cargo_artifact_parser_requires_one_existing_lib_test_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / 'oxidex-test'
            binary.write_bytes(b'candidate')
            log = root / 'cargo.jsonl'
            record = {'reason': 'compiler-artifact', 'target': {'name': 'oxidex', 'kind': ['lib']},
                      'profile': {'test': True}, 'executable': str(binary)}
            log.write_text(json.dumps(record) + '\n')
            self.assertEqual(suite.release_lib_artifact(log), binary)
            log.write_text(log.read_text() + json.dumps(record) + '\n')
            with self.assertRaisesRegex(RuntimeError, 'one exact'):
                suite.release_lib_artifact(log)
            binary.unlink()
            with self.assertRaisesRegex(RuntimeError, 'one exact'):
                suite.release_lib_artifact(log)
