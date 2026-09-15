#!/usr/bin/env python3
"""Observe direct source-generated Keys reads; keep omissions outside credit."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import subprocess

import quicktime_baseline as baseline
import quicktime_keys_specs as specs
import runtime_evidence_inputs
from verify_quicktime_reader import rust_matches

SCHEMA = 'oxidex_quicktime_generated_keys_read_evidence_v1'
MODES = {'print': (), 'no-print-conv': ('-n',)}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def projection(value):
    if isinstance(value, list) and len(value) == 1:
        value = value[0]
    if not isinstance(value, dict):
        raise ValueError('expected one metadata object')
    return {k: v for k, v in value.items()
            if k.startswith(('Keys:', 'QuickTime:GPS')) or k == 'QuickTime:ContentCreateDate'}


def atoms(data):
    offset = 0
    while offset < len(data):
        if len(data) - offset < 8:
            raise ValueError('truncated fixture atom')
        size = int.from_bytes(data[offset:offset + 4], 'big')
        if size < 8 or size > len(data) - offset:
            raise ValueError('invalid fixture atom size')
        yield data[offset + 4:offset + 8], data[offset + 8:offset + size]
        offset += size


def one_child(data, name):
    matches = [body for key, body in atoms(data) if key == name]
    if len(matches) != 1:
        raise ValueError('expected exactly one fixture child')
    return matches[0]


def fixture_identity(data):
    for name in (b'moov', b'udta', b'meta'):
        data = one_child(data, name)
    if len(data) < 4:
        raise ValueError('missing FullBox flags')
    data = data[4:]
    keys = one_child(data, b'keys')
    items = list(atoms(one_child(data, b'ilst')))
    if len(keys) < 8 or len(items) != 1:
        raise ValueError('malformed indexed fixture')
    index = int.from_bytes(items[0][0], 'big')
    one_child(items[0][1], b'data')
    entries = []
    # Native ignores the declared count and walks valid byte-bounded entries.
    for namespace, key in atoms(keys[8:]):
        entries.append({'namespace': namespace.decode('latin1'),
                        'key': key.split(b'\0')[0].decode('latin1')})
    selected = entries[index - 1] if 0 < index <= len(entries) else None
    return {'ordinal': index, 'entries': entries, 'selected': selected}


def resolved_spec(identity, ledger):
    entry = identity['selected']
    if entry is None:
        return None
    full = entry['key']
    short = full
    if entry['namespace'] == 'mdta' and short.startswith('com.'):
        short = short[4:]
        if short.startswith('apple.quicktime.'):
            short = short[len('apple.quicktime.'):]
    by_key = {row['source_key']: row for row in ledger['specs']}
    return by_key.get(short) or by_key.get(full)


def atom(name, payload):
    if len(name) != 4:
        raise ValueError('fixture atom key is not four bytes')
    return (len(payload) + 8).to_bytes(4, 'big') + name + payload


def fixture(*, namespace=b'mdta', key=b'com.apple.quicktime.artist', index=1,
            flags=1, value=b'Ada', count=1):
    keys = b'\0' * 4 + count.to_bytes(4, 'big') + atom(namespace, key)
    item = atom(index.to_bytes(4, 'big'), atom(b'data', flags.to_bytes(4, 'big') + b'\0' * 4 + value))
    handler = atom(b'hdlr', b'\0' * 8 + b'mdta' + b'\0' * 13)
    meta = atom(b'meta', b'\0' * 4 + handler + atom(b'keys', keys) + atom(b'ilst', item))
    return atom(b'ftyp', b'isom\0\0\0\0isom') + atom(b'moov', atom(b'udta', meta))


def cases():
    return {
        'prefix': fixture(),
        'full-retry': fixture(key=b'com.android.model', value=b'Pixel'),
        'non-mdta': fixture(namespace=b'abcd', key=b'artist', value=b'Namespace'),
        'ordinal-miss': fixture(index=2),
        'ordinal-zero': fixture(index=0),
        'count-ignored': fixture(count=0),
        'nul-key': fixture(key=b'com.apple.quicktime.artist\0ignored'),
        'utf8': fixture(value='Ada \u00e9 \u2615'.encode()),
        'numeric-enum': fixture(key=b'player.movie.audio.mute', flags=21, value=b'\1'),
        'unknown': fixture(key=b'not.source.defined', value=b'x'),
        'refused-gps': fixture(key=b'com.apple.quicktime.location.ISO6709', value=b'+1.0-2.0/'),
        'refused-date': fixture(key=b'com.apple.quicktime.creationdate', value=b'2020-01-01T00:00:00Z'),
    }


def authenticated(source, ledger, rust):
    raw = source.read_bytes()
    expected = specs.compile_document(json.loads(raw))
    if (expected != json.loads(ledger.read_bytes())
            or not rust_matches(specs.render_rust(expected), rust.read_text())):
        raise ValueError('Keys source artifacts do not replay')
    return {name: sha(path.read_bytes()) for name, path in
            {'source_sha256': source, 'ledger_sha256': ledger, 'rust_sha256': rust}.items()}


def validate_report(report, ledger):
    if report.get('schema') != SCHEMA:
        raise ValueError('Keys evidence schema differs')
    fixtures = cases()
    required = {(name, mode) for name in fixtures for mode in MODES}
    seen, credited = set(), []
    for row in report['observations']:
        pair = (row.get('fixture'), row.get('mode'))
        if pair not in required or pair in seen:
            raise ValueError('fixture grid is unknown or duplicated')
        seen.add(pair)
        data = fixtures[pair[0]]
        identity = fixture_identity(data)
        if row.get('fixture_sha256') != sha(data) or row.get('fixture_identity') != identity:
            raise ValueError('fixture bytes or identity differ')
        for side, claim in (('native_json', 'expected'), ('oxidex_json', 'actual')):
            raw = row.get(side)
            if not isinstance(raw, str) or sha(raw.encode()) != row.get(side + '_sha256'):
                raise ValueError('transcript hash differs')
            if projection(json.loads(raw)) != row.get(claim):
                raise ValueError('transcript projection claim differs')
        matched = row['expected'] == row['actual']
        if row.get('matched') is not matched:
            raise ValueError('matched claim differs from transcripts')
        spec = resolved_spec(identity, ledger)
        if spec is None:
            # Unknown names, invalid ordinals and source refusals are not generated coverage.
            continue
        key = 'Keys:' + spec['name']
        if set(row['expected']) != {key}:
            raise ValueError('native generated fixture is absent or has a different source identity')
        if matched:
            credited.append({'fixture': pair[0], 'mode': pair[1],
                             'source_identity': spec['source_identity'], 'group1': 'Keys',
                             'tag_name': spec['name']})
    if seen != required:
        raise ValueError('fixture grid is incomplete')
    return credited


def compare(tree, out, source, ledger, rust):
    root = baseline.ROOT
    if out.exists() or out.is_symlink() or out.resolve().is_relative_to(root.resolve()):
        raise ValueError('choose a new evidence directory outside the checkout')
    state = baseline.instrument.git_state(root)
    if state.dirty:
        raise ValueError('clean checkout required')
    inputs = authenticated(source, ledger, rust)
    ledger_doc = json.loads(ledger.read_bytes())
    before = baseline.source_fingerprint(root)
    runtime = runtime_evidence_inputs.runtime_input_manifest(root)
    manifest_path = Path(__file__).parent / 'fixtures/quicktime_oracle_sources_13_59.json'
    manifest = json.loads(manifest_path.read_bytes())
    baseline.verify_oracle_sources(tree, manifest)
    oracle = baseline.exiftool_oracle.resolve_tree(tree.resolve())
    if not oracle.verified or oracle.version != (root / '.exiftool-version').read_text().strip():
        raise ValueError('native oracle capability/version does not match the pin')
    out.mkdir()
    build = subprocess.run(['cargo', 'build', '--bin', 'oxidex', '--message-format=json'],
                           cwd=root, text=True, capture_output=True)
    (out / 'build.jsonl').write_text(build.stdout)
    (out / 'build.stderr').write_text(build.stderr)
    build.check_returncode()
    artifacts = [json.loads(line) for line in build.stdout.splitlines()]
    candidates = [row['executable'] for row in artifacts if row.get('reason') == 'compiler-artifact'
                  and row.get('target', {}).get('name') == 'oxidex' and row.get('executable')]
    if len(candidates) != 1:
        raise ValueError('Cargo did not report exactly one oxidex executable')
    binary = baseline.instrument.resolve_binary(candidates[0])
    binary_hash = sha(binary.path.read_bytes())
    fixtures = cases()
    baseline.instrument.print_header(tool='verify_quicktime_keys_reader.py', git=state,
                                     binary=binary, oracle=oracle, corpus_paths=[out],
                                     file_count=len(fixtures))
    rows = []
    for name, data in fixtures.items():
        path = out / (name + '.mp4')
        path.write_bytes(data)
        for mode, native_flags in MODES.items():
            native = subprocess.run(oracle.command(['-config', '', '-j', '-a', '-G1', '-s',
                                                    *native_flags, str(path)]),
                                    text=True, capture_output=True, check=True)
            ox = subprocess.run([str(binary.path), '-j', '-a', '-G1',
                                 *(['--no-print-conv'] if native_flags else []), str(path)],
                                text=True, capture_output=True, check=True)
            expected, actual = projection(json.loads(native.stdout)), projection(json.loads(ox.stdout))
            rows.append({'fixture': name, 'mode': mode, 'fixture_sha256': sha(data),
                         'fixture_identity': fixture_identity(data), 'expected': expected, 'actual': actual,
                         'native_json': native.stdout, 'native_json_sha256': sha(native.stdout.encode()),
                         'oxidex_json': ox.stdout, 'oxidex_json_sha256': sha(ox.stdout.encode()),
                         'matched': expected == actual})
            (out / 'observations.partial.json').write_text(json.dumps(rows, indent=2) + '\n')
    if (before != baseline.source_fingerprint(root) or runtime != runtime_evidence_inputs.runtime_input_manifest(root)
            or inputs != authenticated(source, ledger, rust) or binary_hash != sha(binary.path.read_bytes())):
        raise ValueError('source, generated artifacts or binary changed during observation')
    baseline.verify_oracle_sources(tree, manifest)
    report = {'schema': SCHEMA, 'instrument': 'verify_quicktime_keys_reader.py',
              'instrument_sha256': sha(Path(__file__).read_bytes()), 'inputs': inputs,
              'producer': {'source_commit': state.commit, 'source_dirty': False,
                           'source_fingerprint': before, 'runtime_input_manifest_sha256': runtime,
                           'runtime_artifact_sha256': binary_hash, 'pin': oracle.version,
                           'oracle_manifest_sha256': sha(manifest_path.read_bytes()),
                           'fixture_manifest_sha256': sha(canonical({n: sha(d) for n, d in fixtures.items()}))},
              'observations': rows, 'scope': 'Direct Keys behavior fixtures; not corpus coverage or observed writing'}
    # Save transcripts even if native fixture admission fails below.
    (out / 'comparison.json').write_text(json.dumps(report, indent=2) + '\n')
    credit = validate_report(report, ledger_doc)
    report['matched_occurrences'] = credit
    report['observed_identities'] = sorted({f"{row['group1']}:{row['tag_name']}" for row in credit})
    report['metric_c'] = {'distinct_group1_tag_identities': len(report['observed_identities']),
                          'fixture_tag_occurrences': len({(row['fixture'], row['group1'], row['tag_name']) for row in credit}),
                          'matched_mode_observations': len(credit)}
    report['unmatched_or_unclaimed'] = [row for row in rows if not row['matched'] or resolved_spec(row['fixture_identity'], ledger_doc) is None]
    (out / 'comparison.json').write_text(json.dumps(report, indent=2) + '\n')
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('exiftool-dir', 'out', 'source', 'ledger', 'rust'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    report = compare(args.exiftool_dir, args.out, args.source, args.ledger, args.rust)
    print(json.dumps(report['metric_c'], sort_keys=True))
    ledger = json.loads(args.ledger.read_bytes())
    if any(not row['matched'] for row in report['observations'] if resolved_spec(row['fixture_identity'], ledger)):
        raise SystemExit(1)


if __name__ == '__main__':
    main()
