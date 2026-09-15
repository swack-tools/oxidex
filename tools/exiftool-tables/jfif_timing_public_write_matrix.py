#!/usr/bin/env python3
"""Compare public JPEG writes against native output across raw-property timing.

Run under the shared validation lock with a lib-test executable built from
--repo. This tool performs no build and uses the running repo's existing
fresh-JPEG fixture builder, ledger joins, public driver, and full comparator.
"""
from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass
import hashlib
import importlib
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Any

ROOT = Path(__file__).resolve().parents[2]


def load_tools(repo: Path):
    directory = repo.resolve() / 'tools/exiftool-tables'
    sys.path.insert(0, str(directory))
    modules = tuple(importlib.import_module(name) for name in (
        'fresh_jpeg_public_write_matrix', 'native_write_matrix', 'generated_tiff_write_matrix'))
    if any(Path(module.__file__).resolve().parent != directory for module in modules):
        raise ValueError('matrix dependencies do not belong to the selected running repo')
    return modules


@dataclass(frozen=True)
class TimingCase:
    label: str
    segments: tuple[bytes, ...]
    empty_ifd0: bool = False


def timing_cases(fresh) -> tuple[TimingCase, ...]:
    def jfif(label, unit, x, y):
        return fresh._segment(0xe0, fresh.raw_jfif_payload(fresh.RawJfif(label, unit, x, y)))
    a = jfif('a', 1, 72, 96)
    b = jfif('b', 2, 300, 600)
    other = fresh._segment(0xe2, b'unrelated APP2')
    exif = fresh._segment(0xe1, b'Exif\0\0' + fresh.empty_ifd0_tiff('little'))
    # Truncate B before its Y field: preceding Y remains present natively.
    partial = fresh._segment(0xe0, fresh.raw_jfif_payload(fresh.RawJfif('b', 2, 300, 600))[:10])
    return (
        TimingCase('fresh-a-b', (a, b)),
        TimingCase('fresh-a-other-b', (a, other, b)),
        TimingCase('fresh-other-a', (other, a)),
        TimingCase('existing-exif-a', (exif, a), True),
        TimingCase('existing-a-exif-b', (a, exif, b), True),
        TimingCase('existing-a-b-exif', (a, b, exif), True),
        TimingCase('existing-other-a-exif-b', (other, a, exif, b), True),
        TimingCase('existing-exif-other-a', (exif, other, a), True),
        TimingCase('fresh-a-partial-b', (a, partial)),
        TimingCase('fresh-partial-b', (partial,)),
        TimingCase('fresh-a-zero', (a, jfif('zero', 0, 0, 0))),
        TimingCase('fresh-nonjfif-app0-a', (fresh._segment(0xe0, b'other'), a)),
    )


def write_carrier(fresh, path: Path, case: TimingCase, base: Path) -> None:
    path.write_bytes(b'\xff\xd8' + b''.join(case.segments) + fresh._base_tail(base.read_bytes()))


def assert_actual_native_set(fresh, call: dict[str, Any], label: str) -> None:
    fresh.assert_native_oracle(call, label, 'fresh-insert')
    calls = call['result'].get('set_calls')
    if not isinstance(calls, list) or len(calls) != 1 or calls[0].get('return') != 1:
        raise AssertionError(f'{label}: native did not accept exactly one actual set: {call}')


def run_matrix(*, repo: Path, test_binary: Path, perl: Path, library: Path,
               output: Path, base: Path, ledger: Path, rules: Path) -> dict[str, Any]:
    fresh, native, generated = load_tools(repo)
    targets = generated.generated_targets(ledger, rules)
    if len(targets) != 9:
        raise ValueError(f'expected source cohort of 9, captured {len(targets)}')
    cases = timing_cases(fresh)
    directory = output.parent / (output.stem + '-files')
    directory.mkdir(parents=True, exist_ok=False)
    report = {'instrument': 'jfif_timing_public_write_matrix_v1', 'state': 'native',
              'declared': len(cases) * sum(len(t.qualifiers) for t in targets),
              'passed': 0, 'cases': len(cases), 'targets': [asdict(t) for t in targets],
              'native_rows': [], 'limitations': ['Defined insert through public API only; selected source-ledger cohort.']}
    def save():
        output.write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
    save()
    requests = []
    for case in cases:
        source = directory / (case.label + '-source.jpg')
        write_carrier(fresh, source, case, base)
        source_doc = native.parse_jpeg(source)
        if (source_doc['exif'] is not None) != case.empty_ifd0:
            raise AssertionError('timing fixture EXIF presence differs from declaration')
        if case.empty_ifd0 and (source_doc['exif']['tags'] or source_doc['exif']['byte_order'] != 'little'):
            raise AssertionError('timing fixture must start with empty little-endian IFD0')
        for target in targets:
            for qualifier in target.qualifiers:
                stem = f'{case.label}-{target.raw_tag_id:04x}-{qualifier.replace(":", "_")}'
                expected = directory / (stem + '-native.jpg')
                actual = directory / (stem + '-public.jpg')
                row = {'id': stem, 'case': case.label, 'source': str(source), 'native_output': str(expected),
                       'output': str(actual), 'target': asdict(target), 'qualifier': qualifier}
                report['native_rows'].append(row)
                save()
                value = generated.target_text(target, 'insert-value')
                call = (native.run_native(perl, library, source, expected, 'insert', qualifier)
                        if target.case_family == 'native_string_scalar' else
                        generated.run_typed_native(perl, library, source, expected, qualifier, value))
                row['native_call'] = call
                save()
                assert_actual_native_set(fresh, call, stem)
                # Require actual target presence before launching the public driver.
                fresh._assert_native_target_transition(source_doc, native.parse_jpeg(expected), target, 'fresh-insert')
                requests.append(fresh._request(source, actual, target, qualifier, 'utf8', value))
    request_path, result_path = directory / 'requests.json', directory / 'results.json'
    request_path.write_text(json.dumps(requests, indent=2) + '\n')
    report['state'] = 'public-driver'
    save()
    env = os.environ.copy()
    env.update(OXIDEX_SCALAR_WRITE_REQUESTS=str(request_path), OXIDEX_SCALAR_WRITE_RESULTS=str(result_path))
    driver = subprocess.run([str(test_binary), generated.DRIVER, '--exact', '--ignored', '--nocapture'],
                            env=env, text=True, capture_output=True, timeout=120)
    (directory / 'driver.log').write_text(driver.stdout + driver.stderr)
    driver.check_returncode()
    results = json.loads(result_path.read_text())
    if not isinstance(results, list) or len(results) != report['declared']:
        raise AssertionError('public driver result population differs from declared timing matrix')
    for row, result in zip(report['native_rows'], results, strict=True):
        row['driver_result'] = result
        try:
            if not isinstance(result, dict) or result.get('output') != row['output'] or result.get('ok') is not True:
                raise AssertionError(f'public write failed: {result}')
            # Includes full TIFF type/count/value and byte order, target presence,
            # ordered raw JFIF bytes, non-EXIF bytes, and SOS-to-end preservation.
            fresh.compare_jpeg(Path(row['source']), Path(row['native_output']), Path(row['output']),
                               generated.GeneratedTarget(**row['target']), 'fresh-insert')
            row['state'] = 'passed'
            report['passed'] += 1
        except (AssertionError, OSError, ValueError) as error:
            row.update(state='failed', error=str(error))
        save()
    report['state'] = 'complete'
    save()
    return report


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=ROOT)
    parser.add_argument('--test-binary', type=Path, required=True)
    parser.add_argument('--perl', type=Path, required=True)
    parser.add_argument('--lib', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--jpeg-base', type=Path)
    parser.add_argument('--ledger', type=Path)
    parser.add_argument('--rules', type=Path)
    args = parser.parse_args(argv)
    repo = args.repo.resolve()
    fresh, native, generated = load_tools(repo)
    state = fresh.git_state(repo)
    overridden = fresh.refuse_if_dirty(state, 'jfif_timing_public_write_matrix')
    binary = fresh.resolve_binary(args.test_binary, 'oxidex-lib-test')
    if note := fresh.staleness_note(binary, state):
        raise RuntimeError(note)
    perl, library = native.resolve_perl(args.perl), native.resolve_library(args.lib)
    identity = native.native_identity(perl, library)
    native.assert_contract_version(identity)
    ledger, rules = args.ledger or fresh.LEDGER, args.rules or fresh.RULES
    fresh.print_header(tool='jfif_timing_public_write_matrix_v1', git=state, binary=binary,
                       dirty_overridden=overridden, extra=[f'native: {identity}', '12 timing carriers; complete source-format cohort; both qualifiers'])
    report = run_matrix(repo=repo, test_binary=binary.path, perl=perl, library=library,
                        output=args.output, base=args.jpeg_base or fresh.DEFAULT_JPEG, ledger=ledger, rules=rules)
    report.update(source_commit=state.commit, native_identity=identity,
                  test_binary_sha256=hashlib.sha256(binary.path.read_bytes()).hexdigest(),
                  ledger_sha256=hashlib.sha256(ledger.read_bytes()).hexdigest(),
                  rules_sha256=hashlib.sha256(rules.read_bytes()).hexdigest())
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
    print(f"Public JFIF timing matched: {report['passed']}/{report['declared']}")
    return 0 if report['passed'] == report['declared'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
