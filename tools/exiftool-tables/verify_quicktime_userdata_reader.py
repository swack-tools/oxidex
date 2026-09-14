#!/usr/bin/env python3
"""Bounded UserData direct-format byte fixtures and pinned native transcripts.

Requires an explicit lock and canonical Perl/library. Native-only mode produces
portable decoder fixtures; --oxidex additionally compares the real public read
route. Native-only results never claim Rust runtime coverage.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import fcntl
import runtime_evidence_inputs as runtime_inputs
import quicktime_userdata_specs as compiler
from quicktime_baseline import atom


def cases(specs):
    rows=[]
    def add(label,s,hex_value,repeat=1,suffix=''):
        rows.append({'case':label,'raw_fourcc':s['raw_fourcc'],
                     'target':s['group']+':'+s['name'],
                     'payload':{'hex':hex_value,'repeat':repeat,'suffix_hex':suffix}})
    for s in specs:
        fmt=s['source_format']
        payload='c3a9' if fmt['kind']=='string' else (2).to_bytes(fmt['width']//8,'big').hex()
        add('declaration-'+s['raw_fourcc'],s,payload)
        if fmt['kind']=='unsigned':
            w=fmt['width']//8
            for label,payload in [('empty',''),('short','ff'*(w-1)),('max','ff'*w),
                                  ('array',(1).to_bytes(w,'big').hex()+(2).to_bytes(w,'big').hex()),
                                  ('partial-tail',(1).to_bytes(w,'big').hex()+'ff'*(w-1))]:
                add(s['raw_fourcc']+'-'+label,s,payload)
    strings=[s for s in specs if s['source_format']['kind']=='string']
    for bypass in (False,True):
        candidates=[s for s in strings if (bytes.fromhex(s['raw_fourcc'])[0]==169)==bypass]
        if not candidates:continue
        s=candidates[0]
        for label,payload in [('empty',''),('ascii','4142'),('nul','41008e'),('macroman','8e'),
                              ('invalid-lead','ff'),('overlong','c080'),('surrogate','eda080'),
                              ('noncharacter','efbfbe'),('scalar','f09f9880'),('partial-utf8','e282')]:
            add(s['raw_fourcc']+'-'+label,s,payload)
        for count in (65536,65537):add(s['raw_fourcc']+'-limit-'+str(count),s,'8e',count)
        add(s['raw_fourcc']+'-nul-before-limit',s,'8e',1,'00'+'8e'*65537)
    return rows


def payload(row):
    p=row['payload'];return bytes.fromhex(p['hex'])*p['repeat']+bytes.fromhex(p['suffix_hex'])

def fixture(row):
    return atom(b'ftyp',b'qt  \0\0\0\0qt  ')+atom(b'moov',atom(b'udta',atom(bytes.fromhex(row['raw_fourcc']),payload(row))))

def value_text(value):
    if isinstance(value,(str,int,float)) and not isinstance(value,bool):return str(value)
    raise ValueError('native target is not a scalar: '+repr(value))

# The native-fixture schema remains backward compatible. Public observations
# have a separate schema and cannot be inferred from fixture declarations.
SCHEMA = 'oxidex_quicktime_generated_userdata_read_evidence_v1'
BUILD_SCHEMA = 'oxidex_quicktime_userdata_cli_build_proof_v1'
MODES = {'print': (), 'raw': ('-n',)}
BUILD_COMMAND = ['cargo', 'build', '--bin', 'oxidex', '--all-features', '--message-format=json', '--jobs', '4']
ROOT = compiler.ROOT
ORACLE_MANIFEST = ROOT / 'tools/exiftool-tables/fixtures/quicktime_oracle_sources_13_59.json'


def sha(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False, allow_nan=False).encode()


def file_fact(path):
    path = Path(path).resolve()
    with path.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    return {'path': str(path), 'sha256': digest}


def check_file(fact):
    if not isinstance(fact, dict) or set(fact) != {'path', 'sha256'} or file_fact(fact['path']) != fact:
        raise ValueError('evidence file path or hash differs')
    return Path(fact['path'])


def tool_inputs(root=ROOT):
    names = ('verify_quicktime_userdata_reader.py', 'quicktime_userdata_specs.py',
             'quicktime_atom_tables.py', 'quicktime_generated_specs.py',
             'capture_quicktime_baseline.py', 'runtime_evidence_inputs.py',
             'quicktime_baseline.py', 'native_write_matrix.py',
             'verify_quicktime_reader.py', 'join_catalog_hydrated.py')
    return {name: file_fact(root / 'tools/exiftool-tables' / name)['sha256'] for name in names}


def clean_snapshot(root=ROOT):
    import quicktime_baseline as baseline
    state = baseline.instrument.git_state(root)
    if state.dirty or not state.commit:
        raise ValueError('UserData public evidence requires a clean committed checkout; overrides do not grant credit')
    return {'source_commit': state.commit, 'source_dirty': False,
            'source_fingerprint': baseline.source_fingerprint(root),
            'runtime_input_manifest_sha256': runtime_inputs.runtime_input_manifest(root),
            'instrument_inputs': tool_inputs(root)}


def cargo_artifact(log, root):
    rows = []
    for line in log.decode('utf8').splitlines():
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(row, dict):
            rows.append(row)
    finishes = [row for row in rows if row.get('reason') == 'build-finished']
    if len(finishes) != 1 or finishes[0].get('success') is not True:
        raise ValueError('Cargo transcript has no successful terminal build')
    artifacts = [row for row in rows if row.get('reason') == 'compiler-artifact'
                 and row.get('target', {}).get('name') == 'oxidex'
                 and 'bin' in row.get('target', {}).get('kind', []) and row.get('executable')]
    if len(artifacts) != 1:
        raise ValueError('Cargo must identify exactly one oxidex CLI executable')
    artifact = artifacts[0]
    if (Path(artifact.get('manifest_path', '')).resolve() != root.resolve() / 'Cargo.toml'
            or artifact.get('profile', {}).get('test') is not False):
        raise ValueError('Cargo CLI artifact belongs to another checkout or test profile')
    return artifact


def build_proof(output, root=ROOT):
    """Explicit opt-in build wrapper; never run implicitly during observation."""
    output = Path(output).resolve()
    if output.is_relative_to(root.resolve()) or output.exists():
        raise ValueError('choose a new build evidence directory outside the checkout')
    before = clean_snapshot(root)
    output.mkdir(parents=True)
    with (output / 'cargo.jsonl').open('wb') as stdout, (output / 'cargo.stderr').open('wb') as stderr:
        run = subprocess.run(BUILD_COMMAND, cwd=root, stdout=stdout, stderr=stderr)
    if run.returncode != 0:
        raise ValueError('Cargo CLI build failed; transcripts retained')
    artifact = cargo_artifact((output / 'cargo.jsonl').read_bytes(), root)
    proof = {'schema': BUILD_SCHEMA, 'snapshot': before, 'source_root': str(root.resolve()), 'command': BUILD_COMMAND,
             'returncode': run.returncode, 'cargo_artifact': artifact,
             'binary': file_fact(artifact['executable']),
             'cargo_stdout': file_fact(output / 'cargo.jsonl'),
             'cargo_stderr': file_fact(output / 'cargo.stderr'),
             'cargo_stdout_hex': (output / 'cargo.jsonl').read_bytes().hex(),
             'cargo_stderr_hex': (output / 'cargo.stderr').read_bytes().hex()}
    if clean_snapshot(root) != before:
        raise ValueError('source changed during Cargo CLI build')
    validate_build(proof, proof['binary']['path'], root)
    (output / 'build-proof.json').write_text(json.dumps(proof, sort_keys=True, indent=2) + '\n')
    return proof


def validate_build_receipt(proof, expected_snapshot):
    """Replay an embedded build receipt without resolving producer-host paths.

    The caller supplies its authenticated expected runtime snapshot. Live
    observation additionally checks all persisted files with validate_build.
    """
    if (not isinstance(proof, dict) or proof.get('schema') != BUILD_SCHEMA
            or proof.get('command') != BUILD_COMMAND or type(proof.get('returncode')) is not int
            or proof['returncode'] != 0 or proof.get('snapshot') != expected_snapshot
            or not isinstance(expected_snapshot, dict) or expected_snapshot.get('source_dirty') is not False):
        raise ValueError('build proof is stale, dirty, or belongs to different source inputs')
    import re
    for field, width in [('source_commit',40), ('source_fingerprint',64), ('runtime_input_manifest_sha256',64)]:
        if not re.fullmatch('[0-9a-f]{%d}' % width, str(expected_snapshot.get(field,''))):
            raise ValueError('build snapshot content identity is malformed')
    tools = expected_snapshot.get('instrument_inputs')
    if not isinstance(tools, dict) or not tools or any(not re.fullmatch('[0-9a-f]{64}',str(x)) for x in tools.values()):
        raise ValueError('build instrument inputs are missing or malformed')
    for field in ('binary', 'cargo_stdout', 'cargo_stderr'):
        fact = proof.get(field)
        if (not isinstance(fact,dict) or set(fact) != {'path','sha256'}
                or not Path(fact['path']).is_absolute() or not re.fullmatch('[0-9a-f]{64}',str(fact['sha256']))):
            raise ValueError('build file identity is malformed')
    raw = {}
    for field in ('cargo_stdout', 'cargo_stderr'):
        try:
            raw[field] = bytes.fromhex(proof[field + '_hex'])
        except (KeyError,TypeError,ValueError) as error:
            raise ValueError('build transcript bytes are absent or malformed') from error
        if sha(raw[field]) != proof[field]['sha256']:
            raise ValueError('build transcript hash differs')
    source_root = Path(proof.get('source_root',''))
    if not source_root.is_absolute():
        raise ValueError('build source checkout is missing')
    artifact = cargo_artifact(raw['cargo_stdout'], source_root)
    if artifact != proof.get('cargo_artifact') or Path(artifact['executable']).resolve() != Path(proof['binary']['path']):
        raise ValueError('build proof artifact differs from Cargo transcript')
    return expected_snapshot


def validate_build(proof, binary, root=ROOT):
    snapshot = validate_build_receipt(proof, clean_snapshot(root))
    if Path(proof['source_root']) != root.resolve():
        raise ValueError('build receipt belongs to another source checkout')
    if check_file(proof['binary']) != Path(binary).resolve():
        raise ValueError('build proof binary differs from requested executable')
    check_file(proof['cargo_stdout']); check_file(proof['cargo_stderr'])
    return snapshot


def authenticated(source, ledger, rust, *, compiled_rust=None):
    from verify_quicktime_reader import rust_matches
    source_doc = json.loads(Path(source).read_bytes())
    expected = compiler.compile_document(source_doc)
    if not expected['protocol']['eligible'] or not expected['specs']:
        raise ValueError('UserData source protocol has no admitted declarations')
    if (expected != json.loads(Path(ledger).read_bytes())
            or not rust_matches(compiler.render_rust(expected), Path(rust).read_text())):
        raise ValueError('UserData source ledger/Rust artifacts do not replay')
    if compiled_rust is not None and Path(rust).read_bytes() != Path(compiled_rust).read_bytes():
        raise ValueError('supplied Rust differs from the artifact compiled into the public reader')
    return ({name: file_fact(path) for name, path in
             {'source': source, 'ledger': ledger, 'rust': rust}.items()}, expected)


def json_object(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError('duplicate JSON identity')
            result[key] = value
        return result
    def invalid_constant(value):
        raise ValueError('non-finite JSON number: ' + value)
    value = json.loads(raw, object_pairs_hook=pairs, parse_constant=invalid_constant)
    if not isinstance(value, list) or len(value) != 1 or not isinstance(value[0], dict):
        raise ValueError('expected exactly one metadata JSON object')
    return value[0]


def json_equal(left, right):
    """JSON numbers compare numerically; strings/bools never coerce to numbers."""
    if isinstance(left, bool) or isinstance(right, bool):
        return type(left) is type(right) and left == right
    if isinstance(left, (int, float)) and isinstance(right, (int, float)):
        import math
        finite = lambda value: isinstance(value, int) or math.isfinite(value)
        return finite(left) and finite(right) and left == right
    if type(left) is not type(right):
        return False
    if isinstance(left, dict):
        return left.keys() == right.keys() and all(json_equal(left[k], right[k]) for k in left)
    if isinstance(left, list):
        return len(left) == len(right) and all(json_equal(a, b) for a, b in zip(left, right))
    return left == right


def projection(raw, ledger):
    groups = {s['group'] for s in ledger['specs']}
    return {k: v for k, v in json_object(raw).items() if k.partition(':')[0] in groups}


def transcript(command, *, env=None):
    run = subprocess.run(command, capture_output=True, env=env, timeout=30)
    return {'command': command, 'returncode': run.returncode,
            **{key + suffix: value for key, raw in [('stdout', run.stdout), ('stderr', run.stderr)]
               for suffix, value in [('_hex', raw.hex()), ('_sha256', sha(raw))]}}


def read_transcript(fact, command):
    if not isinstance(fact, dict) or fact.get('command') != command or type(fact.get('returncode')) is not int or fact['returncode'] != 0:
        raise ValueError('transcript command differs or process failed')
    streams = {}
    for key in ('stdout', 'stderr'):
        try:
            raw = bytes.fromhex(fact[key + '_hex'])
        except (KeyError, TypeError, ValueError) as error:
            raise ValueError('malformed transcript bytes') from error
        if sha(raw) != fact.get(key + '_sha256'):
            raise ValueError('transcript hash differs')
        streams[key] = raw
    return streams['stdout']


def native_command(native, mode, path):
    return [native['perl']['path'], '-I' + native['library'], native['script']['path'],
            '-config', '', '-j', '-a', '-G1', '-s', *MODES[mode], str(path)]


def public_command(proof, mode, path):
    return [proof['binary']['path'], '-j', '-a', '-G1',
            *(['--no-print-conv'] if mode == 'raw' else []), str(path)]


def native_identity(perl, lib, source):
    import quicktime_baseline as baseline
    from native_write_matrix import clean_env
    lib = Path(lib).resolve()
    manifest = json.loads(ORACLE_MANIFEST.read_bytes())
    baseline.verify_oracle_sources(lib.parent, manifest)
    facts = {'perl': file_fact(perl), 'script': file_fact(lib.parent / 'exiftool'),
             'library': str(lib), 'manifest': file_fact(ORACLE_MANIFEST),
             'files': {name: file_fact(lib.parent / name) for name in manifest['files']},
             'exiftool_version': source['exiftool_version'], 'perl_version': 'v5.38.2'}
    facts['version_transcript'] = transcript([facts['perl']['path'], '-I' + str(lib), facts['script']['path'], '-config', '', '-ver'], env=clean_env())
    facts['perl_transcript'] = transcript([facts['perl']['path'], '-e', 'print $^V'], env=clean_env())
    validate_native(facts, source)
    return facts


def validate_native(facts, source):
    import quicktime_baseline as baseline
    lib = Path(facts['library'])
    check_file(facts['perl']); script = check_file(facts['script'])
    manifest_path = check_file(facts['manifest'])
    if script != (lib.parent / 'exiftool').resolve():
        raise ValueError('native script is outside selected source tree')
    if file_fact(manifest_path)['sha256'] != file_fact(ORACLE_MANIFEST)['sha256']:
        raise ValueError('native oracle manifest differs from repository pin')
    manifest = json.loads(manifest_path.read_bytes())
    baseline.verify_oracle_sources(lib.parent, manifest)
    expected_files = {name: file_fact(lib.parent / name) for name in manifest['files']}
    if facts.get('files') != expected_files:
        raise ValueError('native source file facts changed')
    protocol = source['quicktime_userdata_reader_protocol']
    proc = source['modules']['QuickTime']['tables']['UserData']['meta']['PROCESS_PROC']
    for fact in [proc, protocol['charset_map'], *protocol['dependencies'].values()]:
        if expected_files['lib/' + fact['source_file']]['sha256'] != fact['source_sha256']:
            raise ValueError('native reader source differs from captured protocol')
    version = read_transcript(facts['version_transcript'], [facts['perl']['path'], '-I' + str(lib), str(script), '-config', '', '-ver']).decode().strip()
    perl_version = read_transcript(facts['perl_transcript'], [facts['perl']['path'], '-e', 'print $^V']).decode()
    if (version != source['exiftool_version'] or facts.get('exiftool_version') != version
            or perl_version != 'v5.38.2' or facts.get('perl_version') != perl_version):
        raise ValueError('native version or canonical Perl differs')


def fixture_grid(ledger):
    rows = cases(ledger['specs'])
    by_key = {row['raw_fourcc']: row for row in ledger['specs']}
    if len(by_key) != len(ledger['specs']):
        raise ValueError('ambiguous source FourCC')
    grid = {}
    for row in rows:
        if row['case'] in grid:
            raise ValueError('duplicate generated fixture name')
        spec = by_key[row['raw_fourcc']]
        data = fixture(row)
        grid[row['case']] = {'case': row, 'bytes': data, 'spec': spec}
    return grid


def derived_observations(report, ledger):
    if report.get('schema') != SCHEMA or type(report.get('rust_public_route_checked')) is not bool:
        raise ValueError('UserData read evidence schema or route differs')
    public = report['rust_public_route_checked']
    if public:
        validate_build_receipt(report.get('build_proof'), report.get('producer'))
        if report.get('instrument_inputs') != report['producer']['instrument_inputs']:
            raise ValueError('observation instrument differs from build receipt')
    elif report.get('build_proof') is not None or report.get('producer') is not None:
        raise ValueError('native-only evidence cannot claim a public producer')
    grid = fixture_grid(ledger)
    required = {(name, mode) for name in grid for mode in MODES}
    expected_manifest = {name: sha(row['bytes']) for name, row in grid.items()}
    if report.get('fixture_manifest_sha256') != sha(canonical(expected_manifest)):
        raise ValueError('source-derived fixture manifest differs')
    seen, credited, failures = set(), [], []
    for row in report.get('observations', []):
        pair = row.get('fixture'), row.get('mode')
        if pair not in required or pair in seen:
            raise ValueError('fixture/mode grid is unknown or duplicated')
        seen.add(pair)
        expected_fixture = grid[pair[0]]
        spec = expected_fixture['spec']
        key = spec['group'] + ':' + spec['name']
        if (row.get('fixture_sha256') != sha(expected_fixture['bytes'])
                or row.get('source_identity') != spec['source_identity'] or row.get('target') != key):
            raise ValueError('fixture bytes or source coordinate differ')
        path = Path(report['output']) / (pair[0] + '.mov')
        if row.get('fixture_path') != str(path):
            raise ValueError('fixture path differs from report output')
        raw = read_transcript(row.get('native'), native_command(report['native'], pair[1], path))
        expected = projection(raw, ledger)
        if set(expected) != {key} or not json_equal(row.get('expected'), expected):
            raise ValueError('native generated target is absent, changed or wrongly projected')
        if public:
            actual_raw = read_transcript(row.get('oxidex'), public_command(report['build_proof'], pair[1], path))
            actual = projection(actual_raw, ledger)
            matched = json_equal(expected, actual)
            if not json_equal(row.get('actual'), actual) or row.get('matched') is not matched:
                raise ValueError('actual projection or parity claim differs from typed transcripts')
            if matched:
                credited.append({'fixture': pair[0], 'mode': pair[1], 'source_identity': spec['source_identity'],
                                 'group1': spec['group'], 'tag_name': spec['name']})
            else:
                failures.append({'fixture': pair[0], 'mode': pair[1]})
        elif row.get('oxidex') is not None or row.get('actual') is not None or row.get('matched') is not None:
            raise ValueError('native-only fixture cannot claim a public observation')
    if seen != required:
        raise ValueError('fixture/mode grid is incomplete')
    matched_pairs = {(x['fixture'], x['mode']) for x in credited}
    complete = [name for name in grid if all((name, mode) in matched_pairs for mode in MODES)]
    names = sorted({grid[name]['spec']['group'] + ':' + grid[name]['spec']['name'] for name in complete})
    coordinates = {canonical(grid[name]['spec']['source_identity']) for name in complete}
    counts = {'fixture_count': len(grid), 'native_mode_operations': len(required),
              'public_mode_operations': len(required) if public else 0,
              'matched_mode_observations': len(credited),
              'matched_modes': {mode: sum(x['mode'] == mode for x in credited) for mode in MODES},
              'fixture_tag_occurrences': len({x['fixture'] for x in credited}),
              'fully_matched_fixture_tag_occurrences': len(complete),
              'distinct_group1_tag_identities': len(names),
              'distinct_source_identities': len(coordinates)}
    return {'matched_occurrences': credited, 'observed_identities': names,
            'metric_c': counts, 'failures': failures}


def validate_report(report, ledger):
    derived = derived_observations(report, ledger)
    for key, value in derived.items():
        if not json_equal(report.get(key), value):
            raise ValueError('derived observation/count claim differs: ' + key)
    return derived['matched_occurrences']


def validate_evidence(report, source, ledger, rust, root=ROOT):
    """Import boundary: authenticate artifacts/build/native before any credit."""
    public = report.get('rust_public_route_checked') is True
    inputs, compiled = authenticated(source, ledger, rust,
        compiled_rust=root / 'src/parsers/quicktime/generated_userdata_specs.rs' if public else None)
    if report.get('inputs') != inputs or report.get('instrument_inputs') != tool_inputs(root):
        raise ValueError('evidence generated or instrument inputs differ')
    validate_native(report['native'], json.loads(Path(source).read_bytes()))
    if public:
        snapshot = validate_build(report['build_proof'], report['build_proof']['binary']['path'], root)
        if report.get('producer') != snapshot:
            raise ValueError('observation producer differs from immutable build')
    elif report.get('build_proof') is not None or report.get('producer') is not None:
        raise ValueError('native-only evidence cannot carry public producer claims')
    grid = fixture_grid(compiled)
    for name, row in grid.items():
        path = Path(report['output']) / (name + '.mov')
        if path.read_bytes() != row['bytes']:
            raise ValueError('persisted fixture bytes differ')
    return validate_report(report, compiled)


def compare(args):
    import quicktime_baseline as baseline
    from native_write_matrix import clean_env
    output = args.output.resolve()
    if output.exists() or output.is_relative_to(ROOT.resolve()):
        raise ValueError('choose a new evidence directory outside the checkout')
    public = args.oxidex is not None
    inputs, compiled = authenticated(args.source, args.ledger, args.rust,
        compiled_rust=compiler.RUST if public else None)
    proof = json.loads(args.build_proof.read_bytes()) if public else None
    snapshot = validate_build(proof, args.oxidex) if public else None
    native = native_identity(args.perl, args.lib, json.loads(args.source.read_bytes()))
    instruments = tool_inputs()
    grid = fixture_grid(compiled)
    output.mkdir(parents=True)
    report = {'schema': SCHEMA, 'instrument': 'verify_quicktime_userdata_reader.py',
              'rust_public_route_checked': public, 'inputs': inputs, 'instrument_inputs': instruments,
              'producer': snapshot, 'build_proof': proof, 'native': native, 'output': str(output),
              'fixture_manifest_sha256': sha(canonical({n: sha(x['bytes']) for n, x in grid.items()})),
              'observations': [], 'scope': 'direct UserData fixture reads; not corpus coverage or observed writes'}
    baseline.instrument.print_header(tool='verify_quicktime_userdata_reader.py',
        git=baseline.instrument.git_state(ROOT),
        extra=['pinned native: ' + native['script']['path'], 'Rust public route: ' + str(public)])
    portable = []
    for name, entry in grid.items():
        path = output / (name + '.mov'); path.write_bytes(entry['bytes'])
        spec = entry['spec']; target = spec['group'] + ':' + spec['name']
        native_case = {**entry['case'], 'fixture_sha256': sha(entry['bytes']), 'expected': {}}
        for mode in MODES:
            row = {'fixture': name, 'mode': mode, 'fixture_path': str(path),
                   'fixture_sha256': sha(entry['bytes']), 'source_identity': spec['source_identity'], 'target': target,
                   'native': transcript(native_command(native, mode, path), env=clean_env()),
                   'oxidex': transcript(public_command(proof, mode, path), env=clean_env()) if public else None}
            # Persist raw process output before parsing/admission can fail.
            report['observations'].append(row)
            (output / 'observations.partial.json').write_text(json.dumps(report['observations'], indent=2) + '\n')
            expected_raw = read_transcript(row['native'], native_command(native, mode, path))
            row['expected'] = projection(expected_raw, compiled)
            actual_raw = read_transcript(row['oxidex'], public_command(proof, mode, path)) if public else None
            row['actual'] = projection(actual_raw, compiled) if public else None
            row['matched'] = json_equal(row['expected'], row['actual']) if public else None
            if target in row['expected']:
                text = value_text(row['expected'][target])
                native_case['expected'][mode] = {'utf8_sha256': sha(text.encode()), 'utf8_length': len(text.encode()),
                                                'transcript_sha256': sha(expected_raw)}
        portable.append(native_case)
        (output / 'status.json').write_text(json.dumps({'stage': 'observe', 'completed': len(portable), 'total': len(grid)}))
    (output / 'comparison.json').write_text(json.dumps(report, indent=2) + '\n')
    report.update(derived_observations(report, compiled))
    validate_evidence(report, args.source, args.ledger, args.rust)
    (output / 'comparison.json').write_text(json.dumps(report, indent=2) + '\n')
    # Legacy native fixture consumers receive the same case and scalar-hash
    # shape. They cannot grant credit without the separate comparison receipt.
    (output / 'fixtures.json').write_text(json.dumps({'schema': 'quicktime_userdata_native_fixtures_v1',
        'source_hashes': {fact['path']: fact['sha256'] for fact in [*inputs.values(), native['perl'], native['script'], *native['files'].values()]},
        'exiftool_version': native['exiftool_version'], 'rust_public_route_checked': public,
        'runtime_input_manifest_sha256': snapshot['runtime_input_manifest_sha256'] if public else None,
        'cases': portable, 'failures': report['failures']}, indent=2) + '\n')
    (output / 'status.json').write_text(json.dumps({'stage': 'done', **report['metric_c'], 'failures': report['failures']}, indent=2))
    return report


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--build-proof-dir', type=Path)
    for name in ('perl', 'lib', 'lock', 'output', 'oxidex', 'build-proof'):
        p.add_argument('--' + name, type=Path)
    p.add_argument('--source', type=Path, default=compiler.SNAPSHOT)
    p.add_argument('--ledger', type=Path, default=compiler.LEDGER)
    p.add_argument('--rust', type=Path, default=compiler.RUST)
    args = p.parse_args()
    if args.build_proof_dir:
        if any(getattr(args, name) is not None for name in ('perl', 'lib', 'lock', 'output', 'oxidex', 'build_proof')):
            p.error('build-proof mode cannot be combined with an observation')
        build_proof(args.build_proof_dir)
        return
    if any(getattr(args, name) is None for name in ('perl', 'lib', 'lock', 'output')):
        p.error('observation requires --perl, --lib, --lock and --output')
    if bool(args.oxidex) != bool(args.build_proof):
        p.error('public observation requires both --oxidex and --build-proof')
    with args.lock.open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        report = compare(args)
    print(json.dumps(report['metric_c'], sort_keys=True))
    if report['failures']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
