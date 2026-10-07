"""Small source controls for owned ignored fixture staging."""
import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import ignored_suite as suite
import route


class IgnoredSuiteControls(unittest.TestCase):
    def test_builder_and_actions_contexts_keep_distinct_owned_targets(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory).resolve()
            actions_env = {'CARGO_TARGET_DIR': str(workspace / 'target')}
            with patch.object(suite, 'ROOT', route.FLEET_CHECKOUT), \
                 patch.object(route.Path, 'cwd', return_value=route.FLEET_CHECKOUT), \
                 patch.object(route, 'local_worker_context', return_value=True), \
                 patch.object(route, 'trusted_marker', return_value=True), \
                 patch.object(route, 'verified_fleet_checkout', return_value=True) as signed, \
                 patch.object(route, 'verify_ci_fleet_checkout') as ci:
                self.assertEqual(suite.admitted_context(), ('signed-builder', Path('/target')))
                signed.assert_called_once()
                ci.assert_not_called()
            with patch.object(suite, 'ROOT', workspace), \
                 patch.object(route.Path, 'cwd', return_value=workspace), \
                 patch.object(route, 'local_worker_context', return_value=True), \
                 patch.object(route, 'verify_ci_fleet_checkout') as ci, \
                 patch.dict(os.environ, actions_env):
                self.assertEqual(suite.admitted_context(), ('actions-scratch', workspace / 'target'))
                ci.assert_called_once()
                for bad in (str(workspace / 'other'), str(workspace), 'relative/target', '/src/target'):
                    with self.subTest(target=bad), patch.dict(os.environ, {'CARGO_TARGET_DIR': bad}):
                        with self.assertRaisesRegex(RuntimeError, 'canonical owned Cargo target'):
                            suite.admitted_context()
                with patch.object(route, 'verify_ci_fleet_checkout', side_effect=RuntimeError('wrong repo')):
                    with self.assertRaisesRegex(RuntimeError, 'wrong repo'):
                        suite.admitted_context()

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
                             'status': 'PASS', 'native': {'compared_count': 2},
                             'verified': suite.inputs.verify_file(source, rows[-1])})
        manifest = {'inputs': rows}
        manifest_file = root / 'manifest.json'
        manifest_file.write_text(json.dumps(manifest))
        receipt = {'status': 'PASS', 'manifest_sha256': suite.digest(manifest_file), 'inputs': evidence}
        return repo, manifest_file, manifest, receipt

    def zip_member_plan(self, root):
        repo, manifest_file, manifest, receipt = self.fixture_plan(root)
        row = manifest['inputs'][1]
        archive = root / 'cache' / 'fixture-1.zip'
        with zipfile.ZipFile(archive, 'w') as bundle:
            bundle.writestr('notepad++.exe', b'MZ-authentic-extracted-member')
        row.pop('sha256')
        row.pop('bytes')
        row.update(target='tests/samples/pe/notepad++.exe',
                   source={'kind': 'zip_member', 'url': 'https://example.invalid/npp.zip',
                           'member': 'notepad++.exe'},
                   archive_sha256=suite.digest(archive), archive_bytes=archive.stat().st_size,
                   max_bytes=1024, max_member_bytes=100)
        extracted, verified = suite.inputs.materialize(row, root / 'cache')
        receipt['inputs'][1].update(target=row['target'], path=str(extracted), verified=verified)
        manifest_file.write_text(json.dumps(manifest))
        receipt['manifest_sha256'] = suite.digest(manifest_file)
        return repo, manifest_file, manifest, receipt

    def test_zip_member_without_extracted_manifest_hash_uses_probe_digest(self):
        with tempfile.TemporaryDirectory() as directory:
            repo, manifest_file, manifest, receipt = self.zip_member_plan(Path(directory))
            with patch.object(suite.inputs, 'MANIFEST', manifest_file):
                staged = suite.stage_inputs(repo, receipt, manifest)
            member = next(row for row in staged if row['id'] == 'fixture-1')
            self.assertEqual(member['sha256'], receipt['inputs'][1]['verified']['sha256'])
            self.assertEqual(member['state'], 'verified')
            suite.remove_owned_inputs(staged)
            self.assertFalse(Path(member['path']).exists())

    def test_zip_member_forged_receipt_and_partial_copy_fail_closed_with_custody(self):
        with tempfile.TemporaryDirectory() as directory:
            repo, manifest_file, manifest, receipt = self.zip_member_plan(Path(directory))
            receipt['inputs'][1]['verified']['sha256'] = '0' * 64
            with patch.object(suite.inputs, 'MANIFEST', manifest_file), \
                 self.assertRaisesRegex(RuntimeError, 'receipt differs from verified bytes'):
                suite.stage_inputs(repo, receipt, manifest)
            self.assertFalse((repo / manifest['inputs'][1]['target']).exists())
            receipt['inputs'][1]['verified'] = suite.inputs.verify_file(
                Path(receipt['inputs'][1]['path']), manifest['inputs'][1])
            staged, snapshots = [], []
            def partial_copy(_src, dst):
                dst.write(b'partial')
                raise OSError('copy interrupted')
            with patch.object(suite.inputs, 'MANIFEST', manifest_file), \
                 patch.object(suite.shutil, 'copyfileobj', side_effect=partial_copy), \
                 self.assertRaisesRegex(OSError, 'copy interrupted'):
                suite.stage_inputs(repo, receipt, manifest, staged,
                                   on_copy=lambda: snapshots.append([dict(row) for row in staged]))
            self.assertEqual(len(staged), 1)
            self.assertEqual(snapshots, [[staged[0]]])
            self.assertEqual(staged[0]['state'], 'copying')
            self.assertEqual(staged[0]['sha256'], receipt['inputs'][1]['verified']['sha256'])
            self.assertTrue(Path(staged[0]['path']).exists())
            self.assertNotEqual(suite.retained_owned_inputs(staged)[0]['actual_sha256'], staged[0]['sha256'])
            with self.assertRaisesRegex(RuntimeError, 'retaining input'):
                suite.remove_owned_inputs(staged)

    def test_receipt_callback_failure_keeps_newly_created_file_in_custody(self):
        with tempfile.TemporaryDirectory() as directory:
            repo, manifest_file, manifest, receipt = self.zip_member_plan(Path(directory))
            staged = []
            with patch.object(suite.inputs, 'MANIFEST', manifest_file), \
                 self.assertRaisesRegex(OSError, 'receipt interrupted'):
                suite.stage_inputs(repo, receipt, manifest, staged,
                                   on_copy=lambda: (_ for _ in ()).throw(OSError('receipt interrupted')))
            self.assertEqual(len(staged), 1)
            self.assertEqual(staged[0]['state'], 'copying')
            self.assertTrue(Path(staged[0]['path']).is_file())
            self.assertEqual(suite.retained_owned_inputs(staged)[0]['actual_sha256'], suite.digest(Path(staged[0]['path'])))

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

    def test_partial_staging_receipt_tracks_every_owned_copy_and_retained_hash(self):
        with tempfile.TemporaryDirectory() as directory:
            repo, manifest_file, manifest, receipt = self.fixture_plan(Path(directory))
            Path(receipt['inputs'][3]['path']).unlink()
            staged, snapshots = [], []
            with patch.object(suite.inputs, 'MANIFEST', manifest_file), \
                 self.assertRaisesRegex(ValueError, 'missing or nonregular'):
                suite.stage_inputs(repo, receipt, manifest, staged,
                                   on_copy=lambda: snapshots.append([dict(row) for row in staged]))
            self.assertEqual([len(rows) for rows in snapshots], [1, 1, 2, 2])
            self.assertEqual(len(staged), 2)
            altered = Path(staged[0]['path'])
            altered.write_bytes(b'changed')
            retained = suite.retained_owned_inputs(staged)
            self.assertEqual(len(retained), 2)
            self.assertEqual(retained[0]['path'], str(altered))
            self.assertNotEqual(retained[0]['actual_sha256'], retained[0]['sha256'])
            self.assertEqual(retained[1]['actual_sha256'], retained[1]['sha256'])
            with self.assertRaisesRegex(RuntimeError, 'retaining input'):
                suite.remove_owned_inputs(staged)
            self.assertTrue(all(Path(row['path']).exists() for row in staged))

    def test_raw_adapter_and_native_preparer_hashes_are_separately_bound(self):
        import sys
        sys.path.insert(0, str(suite.ROOT / 'tools/exiftool-tables'))
        import raw_scoped_native_adapter as raw
        manifest = {'instrument_sha256': raw.digest(Path(raw.__file__)),
                    'preparer_sha256': raw.digest(raw.PREPARER)}
        raw.verify_instrument_provenance(manifest)
        with self.assertRaisesRegex(RuntimeError, 'adapter changed'):
            raw.verify_instrument_provenance({**manifest, 'instrument_sha256': '0' * 64})
        with self.assertRaisesRegex(RuntimeError, 'preparer changed'):
            raw.verify_instrument_provenance({**manifest, 'preparer_sha256': '0' * 64})

    def test_interrupted_receipt_replace_keeps_last_valid_document(self):
        with tempfile.TemporaryDirectory() as directory:
            receipt = Path(directory) / 'receipt.json'
            suite.write_receipt_atomic(receipt, {'staged_inputs': [{'id': 'first'}]})
            before = receipt.read_bytes()
            with patch.object(suite.os, 'replace', side_effect=OSError('interrupted publish')):
                with self.assertRaisesRegex(OSError, 'interrupted publish'):
                    suite.write_receipt_atomic(receipt, {'staged_inputs': [{'id': 'first'}, {'id': 'second'}]})
            self.assertEqual(receipt.read_bytes(), before)
            self.assertEqual(list(Path(directory).glob('*.tmp')), [])

    def test_retained_audit_records_unreadable_file_and_continues(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first, second = root / 'first', root / 'second'
            first.write_bytes(b'first')
            second.write_bytes(b'second')
            staged = [{'id': 'one', 'path': str(first), 'sha256': 'a' * 64},
                      {'id': 'two', 'path': str(second), 'sha256': 'b' * 64}]
            actual_digest = suite.digest
            def probe(path):
                if path == first:
                    raise OSError('unreadable')
                return actual_digest(path)
            with patch.object(suite, 'digest', side_effect=probe):
                retained = suite.retained_owned_inputs(staged)
            self.assertEqual(len(retained), 2)
            self.assertIn('unreadable', retained[0]['read_error'])
            self.assertEqual(retained[1]['actual_sha256'], actual_digest(second))
