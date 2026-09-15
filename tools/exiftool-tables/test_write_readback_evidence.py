"""Portable byte/transcript fixtures; no native execution or Cargo build."""
import copy
import json
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import generated_tiff_write_matrix as matrix
import write_readback_evidence as evidence

WRITERS = {('Image::ExifTool::Exif::Main', '316', 0): {'name': 'HostComputer', 'write_group': 'IFD0', 'group0': 'EXIF', 'wire_format': 'string'}}
PROTOCOL = {'explicit_directories': ['IFD0', 'IFD1']}


def tiff(value=b'new\0'):
    rows = [(256, 4, 1, 1), (257, 4, 1, 1), (273, 4, 1, 128), (279, 4, 1, 1)]
    if value is not None:
        rows.append((316, 2, len(value), int.from_bytes(value.ljust(4, b'\0'), 'little')))
    raw = b'II*\0' + struct.pack('<I', 8) + struct.pack('<H', len(rows))
    raw += b''.join(struct.pack('<HHII', *row) for row in rows) + bytes(4)
    return raw.ljust(128, b'\0') + b'\xff'


def readback(path, value='new', *, missing=False):
    command = evidence.read_command(Path('/fixture/perl'), Path('/fixture/lib'), str(path))
    doc = {'SourceFile': str(path)}
    if not missing:
        doc['IFD0:HostComputer'] = value
    return evidence.completed_transcript(command, subprocess.CompletedProcess(command, 0, json.dumps([doc]).encode(), b''))


def row_fixture(root):
    paths = {name: root / (name + '.tif') for name in ('seeded', 'native_output', 'output')}
    for name, path in paths.items():
        path.write_bytes(tiff(b'old\0' if name == 'seeded' else b'new\0'))
    row = {'id': 'fixture-update', 'carrier': 'tiff_little', 'target': {'raw_tag_id': 316, 'name': 'HostComputer',
            'table_group0': 'EXIF', 'physical_write_group': 'IFD0', 'wire_format': 'string'},
           'requested_name': 'EXIF:HostComputer', 'target_directory': [], 'operation': 'update',
           'requested_input_hex': b'new'.hex(), 'public_scalar': 'bytes', 'effective_state': 'mutated', 'state': 'passed',
           **{name: str(path) for name, path in paths.items()}}
    row['driver_result'] = {'output': row['output'], 'ok': True, 'warnings': []}
    native_result = {'write_return': 1, 'error': None, 'set_calls': [{'return': 1}]}
    row['native_call'] = {'returncode': 0, 'result': native_result, 'stdout': json.dumps(native_result), 'stderr': '',
                          'command': ['/fixture/perl', '-I/fixture/lib', '-e', '<native-write-program>', '/fixture/lib', row['seeded'], row['native_output'], 'update', 'EXIF:HostComputer']}
    row['files'] = {name: evidence.file_fact(path) for name, path in paths.items()}
    row['readbacks'] = {name: readback(row[name]) for name in ('native_output', 'output')}
    return row


def observe(row):
    return evidence.observation(row, WRITERS, PROTOCOL, Path('/fixture/perl'), Path('/fixture/lib'))


class ReadbackTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.row = row_fixture(self.root)

    def test_actual_mutation_and_group1_transcripts_credit_one_operation(self):
        observed, reason = observe(self.row)
        self.assertIsNone(reason)
        self.assertEqual(observed['group1_name'], 'IFD0:HostComputer')
        self.assertEqual(observed['source_identity']['raw_key'], '316')
        counts = evidence.summarize([observed, {**observed, 'case_id': 'another-case'}])
        self.assertEqual(counts['successful_write_operations'], 2)
        self.assertEqual(counts['distinct_group1_names'], 1)

    def test_no_write_noop_delete_failed_or_absent_target_earns_no_credit(self):
        for mutate, expected in (
            (lambda r: r.update(operation='delete'), 'deletion'),
            (lambda r: r.update(public_scalar='undefined'), 'deletion'),
            (lambda r: r.update(requested_input_hex=None), 'deletion'),
            (lambda r: r.update(effective_state='native_noop'), 'no_mutation'),
            (lambda r: r.update(state='failed'), 'wire_comparison_failed'),
        ):
            row = copy.deepcopy(self.row); mutate(row)
            self.assertEqual(observe(row), (None, expected))
        # Even a forged 'mutated' summary cannot credit an unchanged wire value.
        Path(self.row['seeded']).write_bytes(tiff())
        self.row['files']['seeded'] = evidence.file_fact(self.row['seeded'])
        self.assertEqual(observe(self.row), (None, 'no_mutation'))
        for key in ('native_output', 'output'):
            Path(self.row[key]).write_bytes(tiff(None))
            self.row['files'][key] = evidence.file_fact(self.row[key])
        self.assertEqual(observe(self.row), (None, 'target_absent'))

    def test_missing_group1_name_does_not_credit_present_physical_tag(self):
        self.row['readbacks']['output'] = readback(self.row['output'], missing=True)
        self.assertEqual(observe(self.row), (None, 'group1_target_absent'))

    def test_equal_value_in_wrong_group_does_not_credit_requested_context(self):
        call = self.row['readbacks']['output']
        wrong_group = [{'SourceFile': self.row['output'], 'IFD1:HostComputer': 'new'}]
        self.row['readbacks']['output'] = evidence.completed_transcript(
            call['command'], subprocess.CompletedProcess(call['command'], 0, json.dumps(wrong_group).encode(), b''))
        self.assertEqual(observe(self.row), (None, 'group1_target_absent'))
        self.row['readbacks']['output'] = call
        self.assertEqual(observe(self.row)[0]['group1_name'], 'IFD0:HostComputer')

    def test_wire_output_mutation_refuses_even_with_matching_transcripts(self):
        Path(self.row['output']).write_bytes(tiff(b'bad\0'))
        with self.assertRaisesRegex(ValueError, 'bytes changed'):
            observe(self.row)
        self.row['files']['output'] = evidence.file_fact(self.row['output'])
        with self.assertRaises(AssertionError):
            observe(self.row)

    def test_readback_value_and_json_type_mutations_refuse(self):
        for value in ('wrong', 1, True, None):
            row = copy.deepcopy(self.row)
            row['readbacks']['output'] = readback(row['output'], value)
            with self.assertRaisesRegex(ValueError, 'values differ'):
                observe(row)

    def test_full_transcript_command_hash_sourcefile_and_duplicates_refuse(self):
        base = self.row['readbacks']['output']
        for mutate in (
            lambda c: c['command'].__setitem__(c['command'].index('-G1'), '-G0'),
            lambda c: c.update(returncode=1),
            lambda c: c.update(stdout_sha256='0' * 64),
        ):
            call = copy.deepcopy(base); mutate(call)
            with self.assertRaises(ValueError):
                evidence.read_json(call, base['command'])
        for stdout, stderr in (
            (b'[{"SourceFile":"wrong"}]', b''),
            (b'[{"SourceFile":"x","IFD0:HostComputer":"a","IFD0:HostComputer":"b"}]', b''),
            (base['stdout_hex'].encode(), b'warning'),
        ):
            call = evidence.completed_transcript(base['command'], subprocess.CompletedProcess(base['command'], 0, stdout, stderr))
            with self.assertRaises(ValueError):
                evidence.read_json(call, base['command'])

    def test_source_target_and_selected_directory_are_independently_derived(self):
        row = copy.deepcopy(self.row)
        row.update(requested_name='IFD1:HostComputer', target_directory=['NextIFD'])
        self.assertEqual(evidence.target_identity(row, WRITERS, ['IFD0', 'IFD1'])[1], 'IFD1:HostComputer')
        row['target_directory'] = []
        with self.assertRaisesRegex(ValueError, 'source/directory'):
            evidence.target_identity(row, WRITERS, ['IFD0', 'IFD1'])
        row = copy.deepcopy(self.row); row['target']['name'] = 'FakeName'
        with self.assertRaises(ValueError):
            observe(row)

    def test_native_and_public_write_failures_cannot_credit_wire_bytes(self):
        for mutate in (
            lambda r: r['driver_result'].update(ok=False),
            lambda r: r['driver_result'].update(warnings=['warning']),
            lambda r: r['native_call']['result'].update(write_return=0),
            lambda r: r['native_call'].update(stdout='{}'),
        ):
            row = copy.deepcopy(self.row); mutate(row)
            with self.assertRaises((ValueError, AssertionError)):
                observe(row)

    def test_clean_override_and_stale_build_proof_are_not_observed_evidence(self):
        from types import SimpleNamespace
        with patch.object(evidence, 'git_state', return_value=SimpleNamespace(dirty=True)):
            with self.assertRaisesRegex(ValueError, 'clean source'):
                evidence.clean_snapshot(self.root)
        binary = self.root / 'binary'; binary.write_bytes(b'fixture')
        artifact = {'reason': 'compiler-artifact', 'target': {'name': 'oxidex', 'kind': ['lib', 'staticlib']},
                    'profile': {'test': True}, 'executable': str(binary)}
        cargo = self.root / 'cargo.jsonl'; cargo.write_text(json.dumps(artifact) + '\n')
        proof = {'schema': evidence.BUILD_SCHEMA, 'source_dirty': False, 'source_commit': 'a' * 40,
                 'runtime_input_manifest_sha256': 'b' * 64, 'binary': evidence.file_fact(binary),
                 'cargo_artifact': artifact, 'cargo_log': evidence.file_fact(cargo)}
        with patch.object(evidence.runtime_inputs, 'runtime_input_manifest', return_value='b' * 64):
            evidence.validate_build(proof, binary, self.root)
            for mutate in (lambda x: x.update(source_dirty=True),
                           lambda x: x.update(runtime_input_manifest_sha256='c' * 64),
                           lambda x: x['cargo_artifact']['target'].update(kind=['bin']),
                           lambda x: x['binary'].update(sha256='d' * 64)):
                changed = copy.deepcopy(proof); mutate(changed)
                with self.assertRaises(ValueError):
                    evidence.validate_build(changed, binary, self.root)

class ImportTests(unittest.TestCase):
    def test_import_recomputes_counts_and_rejects_mutated_producer_files_or_rows(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve(); row = row_fixture(root)
            perl = root / 'perl'; perl.write_bytes(b'fixture perl')
            library = root / 'lib'; library.mkdir()
            cli = root / 'exiftool'; cli.write_bytes(b'fixture cli')
            capture = {'exiftool_version': '13.59'}; sources = {}
            for field, name in evidence.SOURCE_FILES.items():
                path = library / name; path.parent.mkdir(parents=True, exist_ok=True); path.write_text(field)
                sources[field] = evidence.file_fact(path); capture[field] = evidence.sha(path)
            row['native_call']['command'][:5] = [str(perl), '-I' + str(library), '-e', '<native-write-program>', str(library)]
            for name, call in row['readbacks'].items():
                call['command'] = evidence.read_command(perl, library, row[name])
            binary = root / 'binary'; binary.write_bytes(b'fixture binary')
            artifact = {'reason': 'compiler-artifact', 'target': {'name': 'oxidex', 'kind': ['lib']},
                        'profile': {'test': True}, 'executable': str(binary)}
            cargo = root / 'cargo.jsonl'; cargo.write_text(json.dumps(artifact) + '\n')
            producer = {'source_commit': 'a' * 40, 'source_dirty': False, 'pin': '13.59',
                        'runtime_input_manifest_sha256': 'b' * 64, 'instrument_inputs': evidence.tool_inputs()}
            proof = {**producer, 'schema': evidence.BUILD_SCHEMA, 'binary': evidence.file_fact(binary),
                     'cargo_artifact': artifact, 'cargo_log': evidence.file_fact(cargo)}
            identity_result = {'perl': str(perl), 'config_file': '', 'exiftool_version': '13.59'}
            identity = {'returncode': 0, 'result': identity_result, 'stdout': json.dumps(identity_result)}
            inputs = {'source_sha256': 'a' * 64, 'final_ledger_sha256': 'c' * 64,
                      'final_rust_sha256': evidence.sha(evidence.ROOT / 'src/writers/generated_tiff_scalar_final_rules.rs'),
                      'public_ledger_sha256': 'd' * 64, 'public_rust_sha256': evidence.sha(evidence.PUBLIC_RUST)}
            original = {k: v for k, v in row.items() if k not in ('files', 'readbacks')}
            matrix_report = {'route': 'public-api', 'source_commit': producer['source_commit'], 'dirty_files': [],
                             'native_identity': identity, 'test_binary_sha256': evidence.sha(binary),
                             'ledger_sha256': inputs['final_ledger_sha256'], 'rules_sha256': inputs['final_rust_sha256'], 'rows': [original]}
            report = root / 'matrix.json'; report.write_text(json.dumps(matrix_report))
            request = {'route': 'public-api', 'carrier': row['carrier'], 'input': row['seeded'], 'output': row['output'],
                       'key': row['requested_name'], 'scalar': 'bytes', 'value': row['requested_input_hex']}
            (root / 'requests.json').write_text(json.dumps([request]))
            (root / 'results.json').write_text(json.dumps([row['driver_result']]))
            command = [str(binary), matrix.DRIVER, '--exact', '--ignored', '--nocapture']
            call = evidence.completed_transcript(command, subprocess.CompletedProcess(command, 0, b'fixture log', b''))
            (root / 'driver.log').write_bytes(b'fixture log')
            protocol = {**PROTOCOL, 'capture': capture, 'sources': sources}
            observed, _ = evidence.observation(row, WRITERS, protocol, perl, library)
            bundle = {'schema': evidence.SCHEMA, 'route': 'public-api', 'producer': producer, 'inputs': inputs,
                      'build_proof': proof, 'protocol': protocol,
                      'native': {'identity': identity, 'perl': evidence.file_fact(perl), 'cli': evidence.file_fact(cli), 'library': str(library)},
                      'matrix_report': evidence.file_fact(report), 'driver_call': call,
                      'initial_files': {row['id']: {key: row['files'][key] for key in ('seeded', 'native_output')}},
                      'driver_files': {name: evidence.file_fact(root / name) for name in ('requests.json', 'results.json', 'driver.log')},
                      'rows': [row], 'observations': [observed], 'exclusions': [], 'counts': evidence.summarize([observed])}
            with patch.object(evidence.runtime_inputs, 'runtime_input_manifest', return_value='b' * 64), \
                 patch.object(matrix, 'explicit_directory_operands', return_value=('IFD0', 'IFD1')):
                self.assertEqual(evidence.validate_evidence(bundle, WRITERS, inputs, capture), [observed])
                mutations = (
                    lambda x: x.update(route='final-key'),
                    lambda x: x['producer'].update(source_dirty=True),
                    lambda x: x['producer'].update(runtime_input_manifest_sha256='e' * 64),
                    lambda x: x['inputs'].update(public_ledger_sha256='e' * 64),
                    lambda x: x['protocol'].update(explicit_directories=['IFD0']),
                    lambda x: x['counts'].update(successful_write_operations=2),
                    lambda x: x['initial_files'][row['id']]['seeded'].update(sha256='e' * 64),
                    lambda x: x['observations'][0].update(group1_name='IFD1:HostComputer'),
                    lambda x: x['rows'][0].update(operation='delete'),
                    lambda x: x['rows'][0]['readbacks']['output'].update(stdout_sha256='e' * 64),
                    lambda x: x['driver_call']['command'].__setitem__(1, 'another_test'),
                    lambda x: x['native']['identity']['result'].update(exiftool_version='12.64'),
                )
                for mutate in mutations:
                    changed = copy.deepcopy(bundle); mutate(changed)
                    with self.assertRaises((ValueError, AssertionError)):
                        evidence.validate_evidence(changed, WRITERS, inputs, capture)
                Path(row['output']).write_bytes(tiff(b'bad\0'))
                with self.assertRaisesRegex(ValueError, 'bytes changed'):
                    evidence.validate_evidence(bundle, WRITERS, inputs, capture)


if __name__ == '__main__':
    unittest.main()
