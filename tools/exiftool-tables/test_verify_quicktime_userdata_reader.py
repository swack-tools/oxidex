"""Portable evidence rejection tests. These do not execute Cargo or ExifTool."""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import quicktime_userdata_specs as compiler
import verify_quicktime_userdata_reader as verifier


def receipt(command, stdout, stderr=b'', returncode=0):
    return {'command': command, 'returncode': returncode,
            'stdout_hex': stdout.hex(), 'stdout_sha256': verifier.sha(stdout),
            'stderr_hex': stderr.hex(), 'stderr_sha256': verifier.sha(stderr)}


def refresh_derived(report, ledger):
    report.update(verifier.derived_observations(report, ledger))
    return report


def synthetic_proof():
    snapshot = {'source_commit':'a'*40, 'source_dirty':False, 'source_fingerprint':'b'*64,
                'runtime_input_manifest_sha256':'c'*64, 'instrument_inputs':{'test-tool':'d'*64}}
    artifact = {'reason':'compiler-artifact', 'target':{'name':'oxidex','kind':['bin']},
                'profile':{'test':False}, 'manifest_path':'/source/Cargo.toml','executable':'/built/oxidex'}
    raw = (json.dumps(artifact)+'\n'+json.dumps({'reason':'build-finished','success':True})+'\n').encode()
    proof = {'schema':verifier.BUILD_SCHEMA,'snapshot':snapshot,'source_root':'/source',
             'command':verifier.BUILD_COMMAND,'returncode':0,'cargo_artifact':artifact,
             'binary':{'path':'/built/oxidex','sha256':'e'*64},
             'cargo_stdout':{'path':'/build/cargo.jsonl','sha256':verifier.sha(raw)},
             'cargo_stderr':{'path':'/build/cargo.stderr','sha256':verifier.sha(b'')},
             'cargo_stdout_hex':raw.hex(),'cargo_stderr_hex':''}
    return proof


def synthetic_report(ledger, public=True):
    grid = verifier.fixture_grid(ledger)
    report = {'schema': verifier.SCHEMA, 'rust_public_route_checked': public,
              'output': '/test-evidence',
              'native': {'perl': {'path': '/pinned/perl'}, 'library': '/pinned/lib',
                         'script': {'path': '/pinned/exiftool'}},
              'build_proof': synthetic_proof() if public else None,
              'producer': synthetic_proof()['snapshot'] if public else None,
              'instrument_inputs': synthetic_proof()['snapshot']['instrument_inputs'],
              'fixture_manifest_sha256': verifier.sha(verifier.canonical({n: verifier.sha(x['bytes']) for n, x in grid.items()})),
              'observations': []}
    for name, entry in grid.items():
        spec = entry['spec']; target = spec['group'] + ':' + spec['name']
        path = Path(report['output']) / (name + '.mov')
        # Synthetic transcripts test the import protocol, not native semantics.
        expected = {target: 2 if spec['source_format']['kind'] == 'unsigned' else 'text'}
        raw = json.dumps([{'SourceFile': str(path), **expected}]).encode()
        for mode in verifier.MODES:
            row = {'fixture': name, 'mode': mode, 'fixture_path': str(path),
                   'fixture_sha256': verifier.sha(entry['bytes']), 'source_identity': spec['source_identity'],
                   'target': target, 'native': receipt(verifier.native_command(report['native'], mode, path), raw),
                   'expected': expected, 'oxidex': None, 'actual': None, 'matched': None}
            if public:
                row.update(oxidex=receipt(verifier.public_command(report['build_proof'], mode, path), raw), actual=expected, matched=True)
            report['observations'].append(row)
    return refresh_derived(report, ledger)


class ReportTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.ledger = compiler.compile_document(json.loads(compiler.SNAPSHOT.read_bytes()))
        cls.base = synthetic_report(cls.ledger)

    def test_complete_grid_counts_distinct_names_not_declarations_or_modes(self):
        self.assertEqual(len(verifier.validate_report(self.base, self.ledger)), 126)
        m = self.base['metric_c']
        self.assertEqual(m['distinct_group1_tag_identities'], 15)
        self.assertEqual(m['distinct_source_identities'], 17)
        self.assertEqual(m['fixture_tag_occurrences'], 63)
        self.assertEqual(m['fully_matched_fixture_tag_occurrences'], 63)
        self.assertEqual(m['matched_modes'], {'print': 63, 'raw': 63})

    def test_missing_duplicate_unknown_fixture_and_mode_refuse(self):
        for kind in ('missing', 'duplicate', 'unknown', 'mode'):
            with self.subTest(kind=kind):
                r = copy.deepcopy(self.base)
                if kind == 'missing': r['observations'].pop()
                elif kind == 'duplicate': r['observations'].append(copy.deepcopy(r['observations'][0]))
                elif kind == 'unknown': r['observations'][0]['fixture'] = 'invented'
                else: r['observations'][0]['mode'] = 'invented'
                with self.assertRaisesRegex(ValueError, 'grid'):
                    verifier.validate_report(r, self.ledger)

    def test_raw_transcript_stdout_and_stderr_hashes_are_required_on_both_sides(self):
        for side in ('native', 'oxidex'):
            for stream in ('stdout', 'stderr'):
                with self.subTest(side=side, stream=stream):
                    r = copy.deepcopy(self.base)
                    r['observations'][0][side][stream + '_hex'] += '20'
                    with self.assertRaisesRegex(ValueError, 'hash'):
                        verifier.validate_report(r, self.ledger)

    def test_changed_command_or_failed_process_cannot_claim_parity(self):
        for side in ('native', 'oxidex'):
            for field, value in [('command', ['wrong']), ('returncode', 1)]:
                r = copy.deepcopy(self.base); r['observations'][0][side][field] = value
                with self.assertRaisesRegex(ValueError, 'command|failed'):
                    verifier.validate_report(r, self.ledger)

    def test_native_absence_wrong_group_and_wrong_projection_refuse(self):
        for value in ({}, {'WrongGroup:PlayAllFrames': 2}):
            r = copy.deepcopy(self.base); row = r['observations'][0]
            row['native'] = receipt(row['native']['command'], json.dumps([value]).encode())
            row['expected'] = value
            with self.assertRaisesRegex(ValueError, 'native generated target'):
                verifier.validate_report(r, self.ledger)
        r = copy.deepcopy(self.base); r['observations'][0]['expected'] = {}
        with self.assertRaisesRegex(ValueError, 'projected'):
            verifier.validate_report(r, self.ledger)

    def test_source_coordinate_fixture_hash_and_manifest_cannot_be_forged(self):
        for field, value in [('target', 'UserData:Other'), ('source_identity', {}), ('fixture_sha256', '0' * 64)]:
            r = copy.deepcopy(self.base); r['observations'][0][field] = value
            with self.assertRaisesRegex(ValueError, 'coordinate'):
                verifier.validate_report(r, self.ledger)
        r = copy.deepcopy(self.base); r['fixture_manifest_sha256'] = '0' * 64
        with self.assertRaisesRegex(ValueError, 'manifest'):
            verifier.validate_report(r, self.ledger)

    def test_typed_json_rejects_string_and_boolean_numeric_impersonation(self):
        for wrong in ('2', True):
            r = copy.deepcopy(self.base); row = r['observations'][0]
            target = row['target']; raw = json.dumps([{target: wrong}]).encode()
            row['oxidex'] = receipt(row['oxidex']['command'], raw); row['actual'] = {target: wrong}
            with self.assertRaisesRegex(ValueError, 'typed transcripts'):
                verifier.validate_report(r, self.ledger)
            row['matched'] = False
            refresh_derived(r, self.ledger)
            verifier.validate_report(r, self.ledger)
            self.assertEqual(r['metric_c']['matched_mode_observations'], 125)
            self.assertEqual(r['metric_c']['fully_matched_fixture_tag_occurrences'], 62)
            self.assertEqual(len(r['failures']), 1)
        self.assertTrue(verifier.json_equal(2, 2.0))
        self.assertFalse(verifier.json_equal(True, 1))

    def test_duplicate_or_multiple_json_objects_are_not_silently_collapsed(self):
        for raw in (b'[{"UserData:PlayAllFrames":1,"UserData:PlayAllFrames":2}]', b'[{},{}]', b'[{"x":NaN}]'):
            with self.assertRaises(ValueError): verifier.json_object(raw)

    def test_counts_and_name_lists_are_recomputed(self):
        for key in ('metric_c', 'observed_identities', 'matched_occurrences', 'failures'):
            r = copy.deepcopy(self.base); r[key] = {} if key == 'metric_c' else ['invented']
            with self.assertRaisesRegex(ValueError, 'claim differs'):
                verifier.validate_report(r, self.ledger)

    def test_native_only_has_no_observed_credit_or_producer(self):
        r = synthetic_report(self.ledger, False)
        self.assertEqual(verifier.validate_report(r, self.ledger), [])
        self.assertEqual(r['observed_identities'], [])
        self.assertEqual(r['metric_c']['public_mode_operations'], 0)
        self.assertEqual(r['metric_c']['distinct_source_identities'], 0)
        r['observations'][0]['matched'] = True
        with self.assertRaisesRegex(ValueError, 'native-only'):
            verifier.validate_report(r, self.ledger)

    def test_changed_source_cohort_cannot_reuse_old_grid(self):
        changed = copy.deepcopy(self.ledger); changed['specs'] = changed['specs'][1:]
        with self.assertRaisesRegex(ValueError, 'manifest'):
            verifier.validate_report(self.base, changed)


class ArtifactTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source, self.ledger, self.rust = [self.root / name for name in ('source.json', 'ledger.json', 'specs.rs')]
        self.source.write_bytes(compiler.SNAPSHOT.read_bytes())
        self.ledger.write_bytes(compiler.LEDGER.read_bytes())
        self.rust.write_bytes(compiler.RUST.read_bytes())

    def test_exact_regenerated_artifacts_replay(self):
        inputs, generated = verifier.authenticated(self.source, self.ledger, self.rust, compiled_rust=compiler.RUST)
        self.assertEqual(len(generated['specs']), 17)
        self.assertEqual(set(inputs), {'source', 'ledger', 'rust'})

    def test_missing_or_modified_ledger_and_rust_refuse(self):
        self.ledger.write_text('{}')
        with self.assertRaisesRegex(ValueError, 'do not replay'):
            verifier.authenticated(self.source, self.ledger, self.rust)
        self.ledger.write_bytes(compiler.LEDGER.read_bytes())
        self.rust.write_text(self.rust.read_text().replace('PlayAllFrames', 'WrongName'))
        with self.assertRaisesRegex(ValueError, 'do not replay'):
            verifier.authenticated(self.source, self.ledger, self.rust)
        self.rust.unlink()
        with self.assertRaises(FileNotFoundError):
            verifier.authenticated(self.source, self.ledger, self.rust)

    def test_consistent_new_source_cannot_impersonate_old_compiled_rust(self):
        doc = json.loads(self.source.read_bytes())
        doc['modules']['QuickTime']['tables']['UserData']['tags']['CNCV']['Name'] = 'FutureName'
        generated = compiler.compile_document(doc)
        self.source.write_text(json.dumps(doc)); self.ledger.write_text(json.dumps(generated)); self.rust.write_text(compiler.render_rust(generated))
        verifier.authenticated(self.source, self.ledger, self.rust)
        with self.assertRaisesRegex(ValueError, 'compiled into'):
            verifier.authenticated(self.source, self.ledger, self.rust, compiled_rust=compiler.RUST)


class BuildProofTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / 'tree'; self.root.mkdir()
        self.binary = Path(self.temp.name) / 'oxidex'; self.binary.write_bytes(b'fresh binary')
        self.log = Path(self.temp.name) / 'cargo.jsonl'
        self.stderr = Path(self.temp.name) / 'cargo.stderr'; self.stderr.write_bytes(b'')
        self.snapshot = {'source_commit': 'a' * 40, 'source_dirty': False, 'source_fingerprint': 'b' * 64,
                         'runtime_input_manifest_sha256': 'c' * 64, 'instrument_inputs': {'tool': 'd' * 64}}
        self.artifact = {'reason': 'compiler-artifact', 'target': {'name': 'oxidex', 'kind': ['bin']},
                         'profile': {'test': False}, 'manifest_path': str(self.root / 'Cargo.toml'),
                         'executable': str(self.binary)}
        self.write_log(self.artifact)
        self.proof = {'schema': verifier.BUILD_SCHEMA, 'snapshot': copy.deepcopy(self.snapshot),
                      'source_root': str(self.root.resolve()), 'command': verifier.BUILD_COMMAND, 'returncode': 0, 'cargo_artifact': self.artifact,
                      'binary': verifier.file_fact(self.binary), 'cargo_stdout': verifier.file_fact(self.log),
                      'cargo_stderr': verifier.file_fact(self.stderr),
                      'cargo_stdout_hex': self.log.read_bytes().hex(), 'cargo_stderr_hex':''}
        self.mock = patch.object(verifier, 'clean_snapshot', return_value=self.snapshot).start()
        self.addCleanup(patch.stopall)

    def write_log(self, artifact, success=True):
        self.log.write_text(json.dumps(artifact) + '\n' + json.dumps({'reason': 'build-finished', 'success': success}) + '\n')

    def test_valid_build_receipt_binds_actual_cargo_executable(self):
        self.assertEqual(verifier.validate_build(self.proof, self.binary, self.root), self.snapshot)

    def test_stale_binary_is_rejected_even_with_fresh_mtime(self):
        self.binary.write_bytes(b'stale changed binary'); os.utime(self.binary, (9999999999, 9999999999))
        with self.assertRaisesRegex(ValueError, 'hash'):
            verifier.validate_build(self.proof, self.binary, self.root)

    def test_stale_commit_runtime_and_tool_inputs_refuse(self):
        for field in ('source_commit', 'runtime_input_manifest_sha256', 'instrument_inputs'):
            proof = copy.deepcopy(self.proof); proof['snapshot'][field] = 'stale'
            with self.assertRaisesRegex(ValueError, 'stale'):
                verifier.validate_build(proof, self.binary, self.root)

    def test_missing_failed_duplicate_or_foreign_cargo_artifact_refuses(self):
        for kind in ('missing', 'failed', 'duplicate', 'foreign', 'test'):
            with self.subTest(kind=kind):
                artifact = copy.deepcopy(self.artifact)
                if kind == 'foreign': artifact['manifest_path'] = '/elsewhere/Cargo.toml'
                if kind == 'test': artifact['profile']['test'] = True
                self.write_log(artifact, success=kind != 'failed')
                if kind == 'missing': self.log.write_text(json.dumps({'reason':'build-finished','success':True}) + '\n')
                if kind == 'duplicate': self.log.write_text(self.log.read_text() + json.dumps(artifact) + '\n')
                proof = copy.deepcopy(self.proof); proof['cargo_artifact'] = artifact; proof['cargo_stdout'] = verifier.file_fact(self.log); proof['cargo_stdout_hex'] = self.log.read_bytes().hex()
                with self.assertRaisesRegex(ValueError, 'Cargo'):
                    verifier.validate_build(proof, self.binary, self.root)

    def test_changed_stdout_stderr_or_claimed_artifact_refuses(self):
        for key in ('cargo_stdout', 'cargo_stderr'):
            proof = copy.deepcopy(self.proof); proof[key]['sha256'] = '0' * 64
            with self.assertRaisesRegex(ValueError, 'hash'):
                verifier.validate_build(proof, self.binary, self.root)
        proof = copy.deepcopy(self.proof); proof['cargo_artifact']['features'] = ['forged']
        with self.assertRaisesRegex(ValueError, 'artifact differs'):
            verifier.validate_build(proof, self.binary, self.root)

    def test_build_wrapper_uses_cargo_receipt_and_refuses_source_changed_during_build(self):
        output = Path(self.temp.name) / 'build-evidence'
        def fake_cargo(command, **kwargs):
            self.assertEqual(command, verifier.BUILD_COMMAND)
            kwargs['stdout'].write(self.log.read_bytes())
            return SimpleNamespace(returncode=0)
        with patch.object(verifier.subprocess, 'run', side_effect=fake_cargo):
            result = verifier.build_proof(output, self.root)
        self.assertEqual(result['binary'], verifier.file_fact(self.binary))
        self.mock.side_effect = [self.snapshot, {**self.snapshot, 'source_commit':'changed'}]
        with patch.object(verifier.subprocess, 'run', side_effect=fake_cargo):
            with self.assertRaisesRegex(ValueError, 'source changed'):
                verifier.build_proof(Path(self.temp.name) / 'changed-build', self.root)


class EndToEndReceiptTests(unittest.TestCase):
    """Exercise persistence/import with mocked processes, never real observations."""
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.folder = Path(self.temp.name).resolve()
        self.ledger = compiler.compile_document(json.loads(compiler.SNAPSHOT.read_bytes()))
        self.grid = verifier.fixture_grid(self.ledger)
        self.binary = self.folder/'oxidex'; self.binary.write_bytes(b'test binary')
        self.proof = synthetic_proof()
        self.proof['source_root'] = str(verifier.ROOT)
        self.proof['snapshot']['instrument_inputs'] = verifier.tool_inputs()
        self.proof['binary'] = verifier.file_fact(self.binary)
        artifact = self.proof['cargo_artifact']
        artifact['manifest_path'] = str(verifier.ROOT/'Cargo.toml')
        artifact['executable'] = str(self.binary)
        log = self.folder/'cargo.jsonl'; log.write_text(json.dumps(artifact)+'\n'+json.dumps({'reason':'build-finished','success':True})+'\n')
        stderr = self.folder/'cargo.stderr'; stderr.write_bytes(b'')
        self.proof.update(cargo_stdout=verifier.file_fact(log), cargo_stderr=verifier.file_fact(stderr), cargo_stdout_hex=log.read_bytes().hex())
        self.proof_path = self.folder/'build-proof.json'; self.proof_path.write_text(json.dumps(self.proof))
        self.native = {'perl':{'path':'/pinned/perl','sha256':'a'*64},
                       'script':{'path':'/pinned/exiftool','sha256':'b'*64},
                       'library':'/pinned/lib','files':{},'exiftool_version':'13.59'}
        self.patches = [patch.object(verifier,'native_identity',return_value=self.native),
                        patch.object(verifier,'validate_native'),
                        patch.object(verifier,'clean_snapshot',return_value=self.proof['snapshot'])]
        for item in self.patches:item.start()
        self.addCleanup(patch.stopall)

    def execute(self, public=False, wrong_type=False):
        output = self.folder/('public' if public else 'native')
        args = SimpleNamespace(source=compiler.SNAPSHOT,ledger=compiler.LEDGER,rust=compiler.RUST,
            output=output,oxidex=self.binary if public else None,build_proof=self.proof_path if public else None,
            perl=Path('/pinned/perl'),lib=Path('/pinned/lib'))
        first = next(iter(self.grid))
        def fake_process(command, **_):
            name = Path(command[-1]).stem
            spec = self.grid[name]['spec']; key = spec['group']+':'+spec['name']
            value = 2 if spec['source_format']['kind']=='unsigned' else 'text'
            if wrong_type and command[0]==str(self.binary) and name==first:value=str(value)
            return receipt(command,json.dumps([{key:value}]).encode())
        import contextlib,io
        with patch.object(verifier,'transcript',side_effect=fake_process),contextlib.redirect_stdout(io.StringIO()):
            result = verifier.compare(args)
        return args,result

    def test_native_only_persists_original_fixture_contract_and_zero_reader_credit(self):
        args,r = self.execute()
        portable=json.loads((args.output/'fixtures.json').read_bytes())
        self.assertEqual(portable['schema'],'quicktime_userdata_native_fixtures_v1')
        self.assertEqual(len(portable['cases']),63)
        self.assertFalse(portable['rust_public_route_checked'])
        self.assertIsNone(portable['runtime_input_manifest_sha256'])
        self.assertEqual(verifier.validate_evidence(r,args.source,args.ledger,args.rust),[])

    def test_public_receipt_replays_and_persisted_fixture_mutation_refuses(self):
        args,r=self.execute(public=True)
        self.assertEqual(len(verifier.validate_evidence(r,args.source,args.ledger,args.rust)),126)
        file=args.output/(r['observations'][0]['fixture']+'.mov');file.write_bytes(file.read_bytes()+b'changed')
        with self.assertRaisesRegex(ValueError,'persisted fixture'):
            verifier.validate_evidence(r,args.source,args.ledger,args.rust)

    def test_import_rejects_input_hash_producer_and_instrument_forgeries(self):
        args,r=self.execute(public=True)
        for key in ('inputs','producer','instrument_inputs'):
            changed=copy.deepcopy(r);changed[key]={}
            with self.assertRaises(ValueError):
                verifier.validate_evidence(changed,args.source,args.ledger,args.rust)

    def test_public_wrong_json_type_is_retained_as_red_evidence(self):
        args,r=self.execute(public=True,wrong_type=True)
        self.assertEqual(len(r['failures']),2)
        self.assertEqual(r['metric_c']['matched_mode_observations'],124)
        self.assertEqual(r['metric_c']['fully_matched_fixture_tag_occurrences'],62)
        saved=json.loads((args.output/'comparison.json').read_bytes())
        self.assertEqual(saved['failures'],r['failures'])


class NativeIdentityTests(unittest.TestCase):
    """Real file/hash replay of a tiny synthetic source tree; no Perl runs."""
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.folder = Path(self.temp.name).resolve()
        self.tree = self.folder/'native'; self.lib = self.tree/'lib'
        self.qt = self.lib/'Image/ExifTool/QuickTime.pm'
        self.qt.parent.mkdir(parents=True); self.qt.write_bytes(b'synthetic native source')
        self.script = self.tree/'exiftool'; self.script.write_bytes(b'synthetic native script')
        self.perl = self.folder/'perl'; self.perl.write_bytes(b'synthetic perl')
        self.manifest = self.folder/'manifest.json'
        files = {str(path.relative_to(self.tree)): verifier.file_fact(path)['sha256']
                 for path in (self.qt,self.script)}
        self.manifest.write_text(json.dumps({'schema':'oxidex_pinned_oracle_sources_v1',
                                            'version':'13.59','files':files}))
        self.pin = patch.object(verifier,'ORACLE_MANIFEST',self.manifest); self.pin.start()
        self.addCleanup(self.pin.stop)
        fact = {'source_file':'Image/ExifTool/QuickTime.pm','source_sha256':files['lib/Image/ExifTool/QuickTime.pm']}
        self.source = {'exiftool_version':'13.59',
            'modules':{'QuickTime':{'tables':{'UserData':{'meta':{'PROCESS_PROC':fact}}}}},
            'quicktime_userdata_reader_protocol':{'charset_map':fact,'dependencies':{'helper':fact}}}
        self.native = {'perl':verifier.file_fact(self.perl),'script':verifier.file_fact(self.script),
            'manifest':verifier.file_fact(self.manifest),'library':str(self.lib),
            'files':{name:verifier.file_fact(self.tree/name) for name in files},
            'exiftool_version':'13.59','perl_version':'v5.38.2'}
        self.version_command = [str(self.perl),'-I'+str(self.lib),str(self.script),'-config','','-ver']
        self.native['version_transcript'] = receipt(self.version_command,b'13.59\n')
        self.native['perl_transcript'] = receipt([str(self.perl),'-e','print $^V'],b'v5.38.2')

    def test_complete_native_files_and_version_receipts_replay(self):
        verifier.validate_native(self.native,self.source)

    def test_changed_missing_or_extra_native_files_refuse(self):
        self.qt.write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError,'pinned release'):
            verifier.validate_native(self.native,self.source)
        self.qt.unlink()
        with self.assertRaisesRegex(ValueError,'universe'):
            verifier.validate_native(self.native,self.source)
        self.qt.write_bytes(b'synthetic native source')
        (self.lib/'extra.pm').write_bytes(b'unknown')
        with self.assertRaisesRegex(ValueError,'universe'):
            verifier.validate_native(self.native,self.source)

    def test_captured_helper_hash_cannot_impersonate_native_source(self):
        source = copy.deepcopy(self.source)
        source['quicktime_userdata_reader_protocol']['dependencies']['helper'] = {
            'source_file':'Image/ExifTool/QuickTime.pm','source_sha256':'a'*64}
        with self.assertRaisesRegex(ValueError,'captured protocol'):
            verifier.validate_native(self.native,source)

    def test_version_or_perl_claim_cannot_override_transcript(self):
        for field,command,output in [('version_transcript',self.version_command,b'13.58'),
                ('perl_transcript',[str(self.perl),'-e','print $^V'],b'v5.40.0')]:
            native=copy.deepcopy(self.native);native[field]=receipt(command,output)
            with self.assertRaisesRegex(ValueError,'version|Perl'):
                verifier.validate_native(native,self.source)

    def test_manifest_perl_and_file_facts_cannot_be_forged(self):
        for field in ('perl','manifest'):
            native=copy.deepcopy(self.native);native[field]['sha256']='a'*64
            with self.assertRaisesRegex(ValueError,'hash'):
                verifier.validate_native(native,self.source)
        native=copy.deepcopy(self.native);native['files']={}
        with self.assertRaisesRegex(ValueError,'file facts'):
            verifier.validate_native(native,self.source)


class BoundaryTests(unittest.TestCase):
    def test_dirty_override_cannot_grant_public_credit(self):
        import quicktime_baseline as baseline
        with patch.object(baseline.instrument, 'git_state', return_value=SimpleNamespace(dirty=True, commit='a'*40)), patch.dict(os.environ, {'OXIDEX_ALLOW_DIRTY_TREE':'1'}):
            with self.assertRaisesRegex(ValueError, 'clean committed'):
                verifier.clean_snapshot()

    def test_cli_requires_immutable_build_proof_before_using_supplied_binary(self):
        run = subprocess.run([sys.executable, str(Path(verifier.__file__)), '--perl', '/missing/perl', '--lib', '/missing/lib', '--lock', '/missing/lock', '--output', '/missing/output', '--oxidex', '/missing/binary'], capture_output=True, text=True)
        self.assertEqual(run.returncode, 2)
        self.assertIn('both --oxidex and --build-proof', run.stderr)


if __name__ == '__main__':
    unittest.main()
