#!/usr/bin/env python3
"""Authenticated public-write observations; declarations and deletes earn no credit.

The optional matrix sidecar retains native JSON transcripts and all wire files.
Import replays the wire comparisons and source joins; it never trusts a passed
flag or an observed-name list. Paths in evidence are durable local artifacts.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

import native_write_matrix as native
import runtime_evidence_inputs as runtime_inputs

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
from instrument import git_state, resolve_binary, staleness_note

SCHEMA = 'oxidex_public_write_readback_evidence_v1'
BUILD_SCHEMA = 'oxidex_public_write_build_proof_v1'
SOURCE_FILES = {'main_source_sha256': 'Image/ExifTool.pm',
                'exif_source_sha256': 'Image/ExifTool/Exif.pm',
                'writer_source_sha256': 'Image/ExifTool/Writer.pl',
                'write_exif_source_sha256': 'Image/ExifTool/WriteExif.pl'}
PUBLIC_LEDGER = ROOT / 'tools/exiftool-tables/setnewvalue_public_migration_ledger.json'
PUBLIC_RUST = ROOT / 'src/writers/generated_setnewvalue_public_migration_rules.rs'

def tool_inputs():
    names = ('write_readback_evidence.py', 'generated_tiff_write_matrix.py', 'native_write_matrix.py', 'runtime_evidence_inputs.py')
    return {name: sha(ROOT / 'tools/exiftool-tables' / name) for name in names}



def sha(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True).encode()).hexdigest()


def file_fact(path):
    path = Path(path).resolve()
    return {'path': str(path), 'sha256': sha(path)}


def check_file(fact):
    if not isinstance(fact, dict) or set(fact) != {'path', 'sha256'} or sha(fact['path']) != fact['sha256']:
        raise ValueError('evidence file hash differs')
    return Path(fact['path'])


def clean_snapshot(root=ROOT):
    state = git_state(root)
    if state.dirty:
        raise ValueError('write readback requires clean source; dirty override cannot grant observed credit')
    return {'source_commit': state.commit, 'source_dirty': False,
            'runtime_input_manifest_sha256': runtime_inputs.runtime_input_manifest(root), 'instrument_inputs': tool_inputs()}


def build_proof(output: Path):
    """Explicit build wrapper: bind Cargo's actual executable to unchanged inputs."""
    before = clean_snapshot()
    output.mkdir(parents=True, exist_ok=False)
    command = ['cargo', 'test', '--lib', '--all-features', '--no-run', '--message-format=json', '--jobs', '4']
    with (output / 'cargo.jsonl').open('wb') as log:
        result = subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT)
    result.check_returncode()
    if clean_snapshot() != before:
        raise ValueError('source changed during public write evidence build')
    artifacts = []
    for line in (output / 'cargo.jsonl').read_text().splitlines():
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (row.get('reason') == 'compiler-artifact' and row.get('target', {}).get('name') == 'oxidex'
                and 'lib' in row.get('target', {}).get('kind', []) and row.get('profile', {}).get('test') is True
                and row.get('executable')):
            if Path(row['manifest_path']).resolve() != ROOT / 'Cargo.toml':
                raise ValueError('Cargo artifact belongs to another checkout')
            artifacts.append(row)
    if len(artifacts) != 1:
        raise ValueError('Cargo did not identify exactly one lib test executable')
    proof = {'schema': BUILD_SCHEMA, **before, 'binary': file_fact(artifacts[0]['executable']),
             'cargo_artifact': artifacts[0], 'command': command, 'cargo_log': file_fact(output / 'cargo.jsonl')}
    (output / 'build-proof.json').write_text(json.dumps(proof, sort_keys=True, indent=2) + '\n')
    return proof


def validate_build(proof, binary, root=ROOT):
    if (proof.get('schema') != BUILD_SCHEMA or proof.get('source_dirty') is not False
            or not re.fullmatch('[0-9a-f]{40}', str(proof.get('source_commit', '')))
            or proof.get('runtime_input_manifest_sha256') != runtime_inputs.runtime_input_manifest(root)):
        raise ValueError('build proof runtime inputs differ or source was dirty')
    if check_file(proof['binary']).resolve() != Path(binary).resolve():
        raise ValueError('build proof binary differs')
    artifact = proof['cargo_artifact']
    if (artifact.get('reason') != 'compiler-artifact' or artifact.get('target', {}).get('name') != 'oxidex'
            or 'lib' not in artifact.get('target', {}).get('kind', [])
            or artifact.get('profile', {}).get('test') is not True
            or Path(artifact.get('executable', '')).resolve() != Path(binary).resolve()):
        raise ValueError('build proof is not a Cargo lib test artifact')
    lines = check_file(proof['cargo_log']).read_text().splitlines()
    if json.dumps(artifact, sort_keys=True) not in [json.dumps(json.loads(line), sort_keys=True) for line in lines if line.startswith('{')]:
        raise ValueError('build artifact is absent from Cargo transcript')


def input_paths(source, final_ledger, final_rust):
    return {'source_sha256': Path(source), 'final_ledger_sha256': Path(final_ledger),
            'final_rust_sha256': Path(final_rust), 'public_ledger_sha256': PUBLIC_LEDGER,
            'public_rust_sha256': PUBLIC_RUST}


def replay_inputs(paths):
    from join_catalog_hydrated import writer_implementation
    return writer_implementation(paths['source_sha256'].read_bytes(),
                                 json.loads(paths['final_ledger_sha256'].read_text()),
                                 paths['final_rust_sha256'].read_text(),
                                 json.loads(paths['public_ledger_sha256'].read_text()),
                                 paths['public_rust_sha256'].read_text())


def source_protocol(paths, library):
    from generated_tiff_write_matrix import explicit_directory_operands
    capture = json.loads(paths['public_ledger_sha256'].read_text())['source']['capture']
    helper = json.loads((ROOT / 'tools/exiftool-tables/scalar_helper_ledger.json').read_text())['helpers']['explicit_directories']
    if helper.get('source_capture_identity') != capture:
        raise ValueError('directory and public source capture differ')
    sources = {field: file_fact(library / name) for field, name in SOURCE_FILES.items()}
    for field, fact in sources.items():
        if not Path(fact['path']).is_relative_to(library.resolve()) or fact['sha256'] != capture[field]:
            raise ValueError('native readback source differs from public write source closure')
    return {'capture': capture, 'sources': sources, 'explicit_directories': list(explicit_directory_operands())}


def read_command(perl, library, path):
    return [str(perl), '-I' + str(library), str(library.parent / 'exiftool'),
            '-config', '', '-j', '-a', '-G1', '-s', str(path)]


def completed_transcript(command, call):
    stdout = call.stdout.encode() if isinstance(call.stdout, str) else call.stdout
    stderr = call.stderr.encode() if isinstance(call.stderr, str) else call.stderr
    return {'command': command, 'returncode': call.returncode,
            'stdout_hex': stdout.hex(), 'stdout_sha256': hashlib.sha256(stdout).hexdigest(),
            'stderr_hex': stderr.hex(), 'stderr_sha256': hashlib.sha256(stderr).hexdigest()}

def transcript(command):
    return completed_transcript(command, subprocess.run(command, env=native.clean_env(), capture_output=True, timeout=30))


def read_json(call, command):
    if call.get('command') != command or call.get('returncode') != 0:
        raise ValueError('native Group1 read command failed or changed')
    raw = {}
    for stream in ('stdout', 'stderr'):
        raw[stream] = bytes.fromhex(call[stream + '_hex'])
        if hashlib.sha256(raw[stream]).hexdigest() != call[stream + '_sha256']:
            raise ValueError('native read transcript hash differs')
    if raw['stderr']:
        raise ValueError('native read emitted diagnostic stderr')
    def pairs(items):
        out = {}
        for key, value in items:
            if key in out:
                raise ValueError('ambiguous duplicate native JSON key')
            out[key] = value
        return out
    value = json.loads(raw['stdout'], object_pairs_hook=pairs)
    if not isinstance(value, list) or len(value) != 1 or not isinstance(value[0], dict):
        raise ValueError('native read did not return exactly one object')
    if value[0].get('SourceFile') != command[-1] or any(k.rsplit(':', 1)[-1] in ('Error', 'Warning') for k in value[0]):
        raise ValueError('native read source file or diagnostics differ')
    return value[0]


def target_identity(row, writers, directories):
    from generated_tiff_write_matrix import GeneratedTarget, selected_qualifiers, directory_path
    target = GeneratedTarget(**row['target'])
    identity = ('Image::ExifTool::Exif::Main', str(target.raw_tag_id), 0)
    source = writers.get(identity)
    if (source is None or source['name'] != target.name or source['write_group'] != target.physical_write_group
            or source['group0'] != target.table_group0 or source['wire_format'] != target.wire_format
            or row['requested_name'] not in selected_qualifiers(target, tuple(directories))
            or tuple(row['target_directory']) != directory_path(target, row['requested_name'], tuple(directories))):
        raise ValueError('readback target is absent from authenticated source/directory operands')
    requested_group = row['requested_name'].split(':', 1)[0]
    group = requested_group if requested_group in directories else source['write_group']
    return identity, group + ':' + source['name']


def observation(row, writers, protocol, perl, library, *, capture=False):
    """Replay physical mutation and actual read transcripts, not summary flags."""
    from generated_tiff_write_matrix import at_directory, compare_carrier
    identity, group_name = target_identity(row, writers, protocol['explicit_directories'])
    if row.get('state') != 'passed':
        return None, 'wire_comparison_failed'
    if row.get('public_scalar') == 'undefined' or row.get('requested_input_hex') is None or row.get('operation') == 'delete':
        return None, 'deletion'
    if row.get('effective_state') != 'mutated':
        return None, 'no_mutation'
    result = row.get('driver_result', {})
    if result.get('output') != row['output'] or result.get('ok') is not True or result.get('warnings'):
        raise ValueError('public writer did not report success')
    native.assert_native(row['native_call'], row['id'])
    call = row['native_call']
    if (json.loads(call['stdout']) != call['result']
            or call['command'][:7] != [str(perl), '-I' + str(library), '-e', '<native-write-program>', str(library), row['seeded'], row['native_output']]):
        raise ValueError('native write transcript/command differs')
    facts = {key: file_fact(row[key]) for key in ('seeded', 'native_output', 'output')}
    if not capture and facts != row.get('files'):
        raise ValueError('write observation fixture/output bytes changed')
    seed, expected, actual = [native.inspect(Path(row[key]), row['carrier']) for key in facts]
    path = tuple(row['target_directory'])
    tag = str(row['target']['raw_tag_id'])
    values = [at_directory(doc, row['carrier'], path)['tags'].get(tag) for doc in (seed, expected, actual)]
    if values[1] is None or values[2] is None:
        return None, 'target_absent'
    if native.entry_storage(values[0]) == native.entry_storage(values[1]):
        return None, 'no_mutation'
    compare_carrier(seed, expected, actual, row['carrier'], int(tag), target_directory=path)
    calls = {}
    for key in ('native_output', 'output'):
        command = read_command(perl, library, row[key])
        call = transcript(command) if capture else row['readbacks'][key]
        calls[key] = call
    documents = [read_json(calls[key], read_command(perl, library, row[key])) for key in calls]
    if capture:
        row.update(files=facts, readbacks=calls)
    if any(group_name not in doc for doc in documents):
        return None, 'group1_target_absent'
    if digest(documents[0][group_name]) != digest(documents[1][group_name]):
        raise ValueError('native/generated Group1 target values differ')
    return {'source_identity': {'table': identity[0], 'raw_key': identity[1], 'variant_index': identity[2]},
            'group1_name': group_name, 'case_id': row['id']}, None


def summarize(observations):
    names = sorted({row['group1_name'] for row in observations})
    return {'successful_write_operations': len(observations), 'distinct_group1_names': len(names),
            'group1_names': names}


class Collector:
    def __init__(self, source, proof_path, binary, final_ledger, final_rust, perl, library, identity):
        self.before = clean_snapshot()
        self.driver_call = None
        self.paths = input_paths(source, final_ledger, final_rust)
        self.writers = replay_inputs(self.paths)
        self.inputs = {key: sha(path) for key, path in self.paths.items()}
        if self.inputs['final_rust_sha256'] != sha(ROOT / 'src/writers/generated_tiff_scalar_final_rules.rs'):
            raise ValueError('readback final rules differ from compiled runtime inputs')
        self.proof = json.loads(Path(proof_path).read_text())
        validate_build(self.proof, binary)
        if staleness_note(resolve_binary(str(binary), 'oxidex-lib-test'), git_state(ROOT)):
            raise ValueError('readback binary is stale')
        self.perl, self.library = perl, library
        self.protocol = source_protocol(self.paths, library)
        self.native = {'identity': identity, 'perl': file_fact(perl), 'cli': file_fact(library.parent / 'exiftool'), 'library': str(library)}
        pin = (ROOT / '.exiftool-version').read_text().strip()
        if identity['result']['exiftool_version'] != pin or identity['result']['config_file'] != '':
            raise ValueError('readback native identity differs from pin/config')
        self.pin = pin

    def capture_inputs(self, rows):
        # Capture before the public process starts, so mutating its input cannot
        # redefine the baseline used by the subsequent wire comparison.
        self.initial_files = {row['id']: {key: file_fact(row[key]) for key in ('seeded', 'native_output')} for row in rows}

    def finish(self, report, output):
        if report['route'] != 'public-api':
            raise ValueError('only actual public-api writes may produce observed write evidence')
        for original in report['rows']:
            for key, fact in self.initial_files[original['id']].items():
                if file_fact(original[key]) != fact:
                    raise ValueError('public driver changed its seeded input or native expected output')
        observations, exclusions, rows = [], [], []
        for original in report['rows']:
            row = json.loads(json.dumps(original))
            observed, reason = observation(row, self.writers, self.protocol, self.perl, self.library, capture=True)
            if observed:
                observations.append(observed)
            else:
                exclusions.append({'case_id': row['id'], 'reason': reason})
            rows.append(row)
        if clean_snapshot() != self.before or {k: sha(p) for k, p in self.paths.items()} != self.inputs:
            raise ValueError('readback inputs changed during measurement')
        validate_build(self.proof, self.proof['binary']['path'])
        if source_protocol(self.paths, self.library) != self.protocol:
            raise ValueError('native source changed during readback')
        for key in ('perl', 'cli'):
            check_file(self.native[key])
        evidence = {'schema': SCHEMA, 'route': 'public-api', 'producer': {**self.before, 'pin': self.pin},
                    'inputs': self.inputs, 'build_proof': self.proof, 'protocol': self.protocol,
                    'native': self.native, 'rows': rows, 'observations': observations,
                    'exclusions': exclusions, 'counts': summarize(observations),
                    'matrix_report': file_fact(output), 'driver_call': self.driver_call, 'initial_files': self.initial_files,
                    'driver_files': {name: file_fact(Path(report['rows'][0]['seeded']).parent / name)
                                     for name in ('requests.json', 'results.json', 'driver.log')}}
        return evidence


def validate_evidence(evidence, writers, input_digests, capture):
    """Import sidecar only after caller independently replayed writer compilers."""
    from generated_tiff_write_matrix import explicit_directory_operands
    if evidence.get('schema') != SCHEMA or evidence.get('route') != 'public-api':
        raise ValueError('unsupported observed write evidence schema/route')
    producer = evidence['producer']
    if (producer.get('source_dirty') is not False or producer.get('pin') != (ROOT / '.exiftool-version').read_text().strip()
            or producer.get('runtime_input_manifest_sha256') != runtime_inputs.runtime_input_manifest(ROOT)
            or producer.get('instrument_inputs') != tool_inputs()
            or not re.fullmatch('[0-9a-f]{40}', str(producer.get('source_commit', '')))):
        raise ValueError('observed write producer inputs differ')
    if (input_digests is None or input_digests.get('final_rust_sha256') != sha(ROOT / 'src/writers/generated_tiff_scalar_final_rules.rs')
            or input_digests.get('public_rust_sha256') != sha(PUBLIC_RUST)):
        raise ValueError('observed write artifacts differ from compiled runtime inputs')
    if evidence.get('inputs') != input_digests:
        raise ValueError('observed write source/artifact inputs differ')
    validate_build(evidence['build_proof'], evidence['build_proof']['binary']['path'])
    protocol = evidence['protocol']
    if protocol.get('capture') != capture or protocol.get('explicit_directories') != list(explicit_directory_operands()):
        raise ValueError('observed write source/directory protocol differs')
    selected_native = evidence['native']
    perl, library = check_file(selected_native['perl']), Path(selected_native['library'])
    if check_file(selected_native['cli']).resolve() != (library / '..' / 'exiftool').resolve():
        raise ValueError('native read CLI is outside selected source')
    for field, name in SOURCE_FILES.items():
        fact = protocol['sources'][field]
        if check_file(fact).resolve() != (library / name).resolve() or fact['sha256'] != capture[field]:
            raise ValueError('native read source closure differs')
    native_identity = selected_native['identity']
    if json.loads(native_identity['stdout']) != native_identity['result']:
        raise ValueError('native identity transcript differs')
    if (native_identity['returncode'] != 0 or native_identity['result']['exiftool_version'] != producer['pin']
            or native_identity['result']['config_file'] != '' or Path(native_identity['result']['perl']).resolve() != perl.resolve()):
        raise ValueError('native read identity differs')
    matrix = json.loads(check_file(evidence['matrix_report']).read_text())
    if (matrix.get('route') != 'public-api' or matrix.get('source_commit') != producer['source_commit']
            or matrix.get('dirty_files') or matrix.get('test_binary_sha256') != evidence['build_proof']['binary']['sha256']
            or matrix.get('native_identity') != native_identity
            or matrix.get('ledger_sha256') != input_digests['final_ledger_sha256']
            or matrix.get('rules_sha256') != input_digests['final_rust_sha256']):
        raise ValueError('observed write matrix producer differs')
    driver = {name: check_file(fact) for name, fact in evidence['driver_files'].items()}
    requests = json.loads(driver['requests.json'].read_text())
    results = json.loads(driver['results.json'].read_text())
    if len(requests) != len(matrix['rows']) or len(results) != len(matrix['rows']):
        raise ValueError('public driver population differs')
    if set(evidence['initial_files']) != {row['id'] for row in matrix['rows']}:
        raise ValueError('initial fixture manifest population differs')
    for row, request, result in zip(matrix['rows'], requests, results, strict=True):
        facts = evidence['initial_files'][row['id']]
        if set(facts) != {'seeded', 'native_output'} or any(file_fact(row[key]) != fact for key, fact in facts.items()):
            raise ValueError('initial fixture/expected output hashes differ')
        value = row['requested_input_hex']
        if value is not None and row['public_scalar'] != 'bytes':
            value = bytes.fromhex(value).decode('utf-8')
        expected_request = {'route': 'public-api', 'carrier': row['carrier'], 'input': row['seeded'],
                            'output': row['output'], 'key': row['requested_name'], 'scalar': row['public_scalar'], 'value': value}
        if request != expected_request or result != row['driver_result']:
            raise ValueError('public driver request/result differs from observed row')
    from generated_tiff_write_matrix import DRIVER
    driver_call = evidence['driver_call']
    if driver_call['returncode'] != 0 or driver_call['command'] != [evidence['build_proof']['binary']['path'], DRIVER, '--exact', '--ignored', '--nocapture']:
        raise ValueError('public fixture driver command failed or changed')
    streams = []
    for stream in ('stdout', 'stderr'):
        content = bytes.fromhex(driver_call[stream + '_hex'])
        if hashlib.sha256(content).hexdigest() != driver_call[stream + '_sha256']:
            raise ValueError('public fixture driver transcript hash differs')
        streams.append(content)
    if b''.join(streams) != driver['driver.log'].read_bytes():
        raise ValueError('public fixture driver transcript differs from saved log')
    by_id = {row['id']: row for row in matrix['rows']}
    if len(by_id) != len(matrix['rows']):
        raise ValueError('duplicate matrix case identity')
    observed, exclusions = [], []
    for row in evidence['rows']:
        original = {k: v for k, v in row.items() if k not in ('files', 'readbacks')}
        if by_id.pop(row['id'], None) != original:
            raise ValueError('observed write row differs from matrix')
        result, reason = observation(row, writers, protocol, perl, library)
        if result is None:
            exclusions.append({'case_id': row['id'], 'reason': reason})
        else:
            observed.append(result)
    if by_id or evidence.get('exclusions') != exclusions:
        raise ValueError('observed write population/exclusions differ')
    if evidence.get('observations') != observed or evidence.get('counts') != summarize(observed):
        raise ValueError('observed write counts/identities differ from transcripts')
    return observed


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build-proof-dir', type=Path, required=True,
                        help='explicitly build the lib test binary and capture immutable runtime proof')
    arguments = parser.parse_args()
    build_proof(arguments.build_proof_dir)
