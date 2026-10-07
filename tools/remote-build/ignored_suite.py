#!/usr/bin/env python3
"""Run every executable ignored test against a signed checkout and real inputs."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import sys
from datetime import datetime, timezone

import ignored_fixture_probe as inputs

ROOT = Path(__file__).resolve().parents[2]
EXECUTABLE_COMMAND = ['cargo', 'test', '--release', '--workspace', '--all-features', '--locked',
                      '--no-fail-fast', '--lib', '--bins', '--tests', '--message-format=json',
                      '--', '--include-ignored']
LIBRARY_BUILD_COMMAND = [*EXECUTABLE_COMMAND[:-2], '--no-run']
DOC_COMMAND = ['cargo', 'test', '--doc', '--workspace', '--all-features', '--locked']


def digest(path: Path) -> str:
    return inputs.digest(path)


def clean_identity(repo: Path) -> tuple[str, str]:
    if subprocess.check_output(['git', '-C', str(repo), 'status', '--porcelain', '--untracked-files=all'], text=True).strip():
        raise RuntimeError('ignored-suite checkout is not clean')
    return tuple(subprocess.check_output(['git', '-C', str(repo), 'rev-parse', arg], text=True).strip()
                 for arg in ('HEAD', 'HEAD^{tree}'))


def stage_inputs(repo: Path, receipt: dict, manifest: dict, staged: list[dict] | None = None,
                 on_copy=None) -> list[dict]:
    """Copy only authenticated, absent fixture targets into the owned checkout."""
    if receipt.get('status') != 'PASS' or len(receipt.get('inputs', [])) != 25:
        raise RuntimeError('all 25 ignored inputs must pass pinned native probing')
    if receipt.get('manifest_sha256') != digest(inputs.MANIFEST):
        raise RuntimeError('ignored input receipt does not bind current source manifest')
    by_id = {row['id']: row for row in receipt['inputs']}
    if len(by_id) != 25:
        raise RuntimeError('duplicate ignored input receipt identity')
    staged = [] if staged is None else staged
    for row in manifest['inputs']:
        evidence = by_id[row['id']]
        if (evidence.get('status') != 'PASS' or evidence.get('target') != row['target']
                or evidence.get('native', {}).get('compared_count', 0) <= 0):
            raise RuntimeError(f'ignored input lacks positive native proof: {row["id"]}')
        relative = inputs.relative_path(row['target'])
        target = repo / relative
        if row['source']['kind'] == 'repo':
            if evidence.get('verified') != inputs.verify_file(target, row):
                raise RuntimeError(f'checked-in ignored input receipt differs: {row["id"]}')
            continue
        source = Path(evidence['path'])
        verified = inputs.verify_file(source, row)
        if evidence.get('verified') != verified:
            raise RuntimeError(f'ignored input receipt differs from verified bytes: {row["id"]}')
        if target.exists() or target.is_symlink():
            raise RuntimeError(f'ignored target already exists: {relative}')
        parent = target.parent
        while parent != repo:
            if parent.is_symlink():
                raise RuntimeError(f'ignored fixture parent is a symlink: {parent}')
            parent = parent.parent
        target.parent.mkdir(parents=True, exist_ok=True)
        with source.open('rb') as src, target.open('xb') as dst:
            owned = {'id': row['id'], 'path': str(target),
                     'sha256': verified['sha256'], 'state': 'copying'}
            staged.append(owned)
            if on_copy is not None:
                on_copy()
            try:
                shutil.copyfileobj(src, dst)
            finally:
                dst.flush()
        if inputs.verify_file(target, row) != verified:
            raise RuntimeError(f'owned ignored copy differs from verified input: {row["id"]}')
        owned['state'] = 'verified'
        if on_copy is not None:
            on_copy()
    if len(staged) != 24:
        raise RuntimeError('expected 24 owned staged fixtures and one checked-in fixture')
    return staged


def remove_owned_inputs(staged: list[dict]) -> None:
    for row in staged:
        path = Path(row['path'])
        if not path.is_file() or path.is_symlink() or digest(path) != row['sha256']:
            raise RuntimeError(f'owned ignored fixture changed; retaining input for investigation: {path}')
    for row in staged:
        Path(row['path']).unlink()


def retained_owned_inputs(staged: list[dict]) -> list[dict]:
    retained = []
    for row in staged:
        path = Path(row['path'])
        item = {**row, 'exists': path.exists() or path.is_symlink(),
                'is_symlink': path.is_symlink(), 'actual_sha256': None}
        if path.is_file() and not path.is_symlink():
            try:
                item['actual_sha256'] = digest(path)
            except OSError as error:
                item['read_error'] = f'{type(error).__name__}: {error}'
        retained.append(item)
    return retained


def write_receipt_atomic(path: Path, report: dict) -> None:
    temporary = path.with_name(path.name + '.' + secrets.token_hex(8) + '.tmp')
    try:
        with temporary.open('x', encoding='utf-8') as stream:
            stream.write(json.dumps(report, indent=2, sort_keys=True) + '\n')
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def run_logged(command: list[str], log: Path, *, env: dict | None = None) -> int:
    with log.open('xb') as stream:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=stream, stderr=subprocess.STDOUT)
    return result.returncode


def release_lib_artifact(log: Path) -> Path:
    paths = []
    for line in log.read_text().splitlines():
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (record.get('reason') == 'compiler-artifact' and record.get('target', {}).get('name') == 'oxidex'
                and 'lib' in record.get('target', {}).get('kind', []) and record.get('profile', {}).get('test')
                and record.get('executable')):
            paths.append(Path(record['executable']))
    if len(paths) != 1 or not paths[0].is_file():
        raise RuntimeError('Cargo did not name one exact release library-test executable')
    return paths[0]


def suite_environment(oracle_root: Path) -> dict:
    if not oracle_root.is_dir() or any(not (oracle_root / name).is_file() for name in (
            'exiftool', 'lib/Image/ExifTool/Real.pm', 'lib/Image/ExifTool/Canon.pm')):
        raise RuntimeError('pinned ignored-suite ExifTool source directory is unavailable')
    return dict(os.environ, CARGO_PROFILE_RELEASE_PANIC='unwind',
                OXIDEX_PINNED_EXIFTOOL=str(oracle_root),
                OXIDEX_REQUIRE_EXIFTOOL_ORACLE='1',
                OXIDEX_RELEASE_REQUIRE_PINNED_FIXTURES='1')


def verify_full_suite_artifact(log: Path, binary: Path, report: dict, save) -> None:
    full_binary = release_lib_artifact(log)
    report['full_suite_library_artifact'] = {'path': str(full_binary), 'sha256': digest(full_binary)}
    save()
    if full_binary != binary or report['full_suite_library_artifact']['sha256'] != report['library_binary']['sha256']:
        raise RuntimeError('full-suite Cargo artifact differs from scored library driver binary')


def validate_driver_manifests(scalar: dict, raw: dict) -> dict:
    # The owner and RAW source cohort define the current denominator. Historical
    # counts are reported for comparison, never used to silently narrow it.
    sys.path.insert(0, str(ROOT / 'tools/exiftool-tables'))
    import raw_scoped_native_adapter as raw_owner
    owner_report = json.loads(Path(scalar['owner_report']).read_text())
    scalar_count = owner_report['declared']
    raw_count = len(raw_owner.TARGETS) * len(raw_owner.OPS)
    if (scalar_count <= 0 or len(scalar.get('rows', [])) != scalar_count
            or len(raw.get('rows', [])) != raw_count or raw.get('unsupported')):
        raise RuntimeError('driver population differs from current owner/source cohort')
    for name, manifest, expected in (('scalar', scalar, scalar_count), ('raw', raw, raw_count)):
        requests, results = Path(manifest['requests']), Path(manifest['results'])
        if (not requests.is_file() or requests.is_symlink() or requests.stat().st_size == 0
                or results.exists() or results.is_symlink()):
            raise RuntimeError(f'{name} driver requests are absent/empty or results are stale')
        raw_requests = (json.loads(requests.read_text()) if name == 'scalar' else
                        [json.loads(line) for line in requests.read_text().splitlines() if line])
        if not isinstance(raw_requests, list) or len(raw_requests) != expected:
            raise RuntimeError(f'{name} request population differs from owner manifest')
        outputs = [row.get('output') for row in raw_requests]
        if any(not isinstance(path, str) or not path for path in outputs) or len(set(outputs)) != expected:
            raise RuntimeError(f'{name} request outputs are absent or duplicated')
        if name == 'scalar':
            identities = [row['id'] for row in manifest['rows']]
        else:
            identities = [(row['target'], row['op']) for row in manifest['rows']]
        if len(set(identities)) != expected:
            raise RuntimeError(f'{name} owner rows are duplicated')
    return {'scalar_owner_declared': scalar_count, 'raw_source_cells': raw_count,
            'historical_scalar_reference': 1530, 'historical_raw_reference': 15}


def admitted_context() -> tuple[str, Path]:
    """Recheck the exact builder or Actions source before using an owned target."""
    import route
    if not route.local_worker_context() or Path.cwd() != ROOT:
        raise RuntimeError('ignored suite requires a guarded checkout root')
    if ROOT == route.FLEET_CHECKOUT and route.trusted_marker(route.BUILDER_MARKER):
        if not route.verified_fleet_checkout():
            raise RuntimeError('ignored suite requires the clean signed builder checkout')
        if os.environ.get('CARGO_TARGET_DIR') != str(route.FLEET_CARGO_TARGET):
            raise RuntimeError('ignored suite requires the separate signed-builder Cargo target')
        return 'signed-builder', route.FLEET_CARGO_TARGET
    route.verify_ci_fleet_checkout()
    configured = os.environ.get('CARGO_TARGET_DIR')
    target = Path(configured) if configured else ROOT / 'target'
    if (not target.is_absolute() or target == ROOT or target == route.FLEET_SOURCE
            or route.FLEET_SOURCE in target.parents or target.is_symlink()
            or target.resolve() != target
            or (ROOT in target.parents and ROOT / 'target' != target
                and ROOT / 'target' not in target.parents)):
        raise RuntimeError('ignored suite requires a canonical owned Cargo target')
    os.environ.setdefault('CARGO_TARGET_DIR', str(target))
    return 'actions-scratch', target


def main() -> int:
    component, target = admitted_context()
    source_identity = clean_identity(ROOT)
    pin = (ROOT / '.exiftool-version').read_text().strip()
    if pin != '13.59' or 'EXIFTOOL_CACHE_DIR' not in os.environ or 'EXIFTOOL_PERL' not in os.environ:
        raise SystemExit('pinned ignored-suite oracle environment is unavailable')
    oracle_root = Path(os.environ['EXIFTOOL_CACHE_DIR']) / 'exiftool'
    perl = Path(os.environ['EXIFTOOL_PERL'])
    if not perl.is_file():
        raise SystemExit('pinned ignored-suite oracle files are unavailable')
    env = suite_environment(oracle_root)
    round_dir = target / 'ignored-suite' / (datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S%fZ') + '-' + secrets.token_hex(4))
    round_dir.mkdir(parents=True)
    receipt_path = round_dir / 'receipt.json'
    report = {'schema': 1, 'status': 'FAILED', 'source_head': source_identity[0],
              'source_tree': source_identity[1], 'oracle_pin': pin, 'round': str(round_dir),
              'component': component, 'target': str(target)}
    def save():
        write_receipt_atomic(receipt_path, report)
    save()
    staged = []
    try:
        manifest = json.loads(inputs.MANIFEST.read_text())
        _, input_receipt = inputs.run(manifest, inputs.ops_root())
        report['inputs'] = input_receipt
        save()
        if input_receipt['status'] != 'PASS':
            raise RuntimeError('ignored input probe failed')
        report['library_build_command'] = LIBRARY_BUILD_COMMAND
        report['library_build_log'] = str(round_dir / 'library-build.jsonl')
        save()
        report['library_build_exit_code'] = run_logged(LIBRARY_BUILD_COMMAND,
                                                       round_dir / 'library-build.jsonl', env=env)
        save()
        if report['library_build_exit_code']:
            raise RuntimeError('full-workspace release library-test build failed')
        binary = release_lib_artifact(round_dir / 'library-build.jsonl')
        report['library_binary'] = {'path': str(binary), 'sha256': digest(binary)}
        save()
        tools = ROOT / 'tools/exiftool-tables'
        scalar_report = round_dir / 'scalar-owner-report.json'
        owner_cmd = [sys.executable, str(tools / 'generated_tiff_write_matrix.py'),
                     '--test-binary', str(binary), '--perl', str(perl), '--lib', str(oracle_root / 'lib'),
                     '--output', str(scalar_report), '--jpeg-base', str(oracle_root / 't/images/GPS.jpg'),
                     '--route', 'public-api']
        if run_logged(owner_cmd, round_dir / 'scalar-owner.log', env=env):
            raise RuntimeError('public scalar owner preparation/scoring failed')
        owner_requests = round_dir / 'generated-tiff-matrix-files/requests.json'
        scalar_round = round_dir / 'scalar-fresh'
        scalar_cmd = [sys.executable, str(tools / 'scalar_fresh_owner_adapter.py'), 'prepare',
                      '--repo', str(ROOT), '--owner-report', str(scalar_report),
                      '--owner-requests', str(owner_requests), '--owner-binary', str(binary),
                      '--source-root', str(oracle_root), '--perl', str(perl), '--round-dir', str(scalar_round)]
        if run_logged(scalar_cmd, round_dir / 'scalar-prepare.log', env=env):
            raise RuntimeError('fresh scalar request preparation failed')
        raw_round = round_dir / 'raw-fresh'
        raw_cmd = [sys.executable, str(tools / 'raw_scoped_native_adapter.py'), 'prepare',
                   '--repo', str(ROOT), '--source-root', str(oracle_root), '--perl', str(perl),
                   '--round-dir', str(raw_round)]
        if run_logged(raw_cmd, round_dir / 'raw-prepare.log', env=env):
            raise RuntimeError('fresh RAW request preparation failed')
        scalar_manifest = json.loads((scalar_round / 'manifest.json').read_text())
        raw_manifest = json.loads((raw_round / 'manifest.json').read_text())
        report['driver_population'] = validate_driver_manifests(scalar_manifest, raw_manifest)
        save()
        env.update(OXIDEX_SCALAR_WRITE_REQUESTS=scalar_manifest['requests'],
                   OXIDEX_SCALAR_WRITE_RESULTS=scalar_manifest['results'],
                   OXIDEX_RAW_EDIT_REQUESTS=raw_manifest['requests'],
                   OXIDEX_RAW_EDIT_RESULTS=raw_manifest['results'])
        report['staged_inputs'] = staged
        save()
        stage_inputs(ROOT, input_receipt, manifest, staged, on_copy=save)
        command = EXECUTABLE_COMMAND
        report['executable_command'] = command
        report['executable_log'] = str(round_dir / 'executable.log')
        save()
        report['executable_exit_code'] = run_logged(command, round_dir / 'executable.log', env=env)
        save()
        verify_full_suite_artifact(round_dir / 'executable.log', binary, report, save)
        remove_owned_inputs(staged)
        staged = []
        if clean_identity(ROOT) != source_identity:
            raise RuntimeError('ignored suite changed signed checkout after owned fixture cleanup')
        for kind, script, manifest_path in (('scalar', 'scalar_fresh_owner_adapter.py', scalar_round / 'manifest.json'),
                                            ('raw', 'raw_scoped_native_adapter.py', raw_round / 'manifest.json')):
            score_cmd = [sys.executable, str(tools / script), 'score', '--repo', str(ROOT),
                         '--manifest', str(manifest_path), '--score', str(manifest_path.parent / 'score.json'),
                         '--driver-binary', str(binary), '--driver-log', str(round_dir / 'executable.log')]
            report[kind + '_score_exit_code'] = run_logged(score_cmd, round_dir / (kind + '-score.log'), env=env)
            save()
        report['doc_command'] = DOC_COMMAND
        report['doc_exit_code'] = run_logged(report['doc_command'], round_dir / 'doc.log', env=env)
        if clean_identity(ROOT) != source_identity:
            raise RuntimeError('signed checkout changed after owner scores or docs')
        report['final_source_identity'] = {'head': source_identity[0], 'tree': source_identity[1]}
        report['rustdoc_ignored_examples'] = 'intentionally noncompiled by ordinary cargo test --doc'
        report['status'] = ('PASS' if all(report.get(k) == 0 for k in
                            ('executable_exit_code', 'scalar_score_exit_code', 'raw_score_exit_code', 'doc_exit_code'))
                            else 'FAILED')
    except Exception as error:
        report['error'] = f'{type(error).__name__}: {error}'
    finally:
        if staged:
            try:
                remove_owned_inputs(staged)
                if clean_identity(ROOT) != source_identity:
                    raise RuntimeError('checkout changed after fixture cleanup')
            except Exception as error:
                report['cleanup_error'] = f'{type(error).__name__}: {error}'
                report['retained_inputs'] = retained_owned_inputs(staged)
                report['status'] = 'FAILED'
        save()
    print(f'IGNORED_SUITE_RECEIPT {receipt_path} status={report["status"]}', flush=True)
    return 0 if report['status'] == 'PASS' else 1


if __name__ == '__main__':
    raise SystemExit(main())
