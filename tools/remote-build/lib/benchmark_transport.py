"""Private benchmark278 wire protocol; preparation does not enable a VM.

Derived from source contract 894ddae4574d558f1577d21ea5693994d40ae378.
Callers must separately verify installed pins and authorize the exact VM before
using these commands. The ordinary build selector does not invoke this module.
"""
import hashlib
import json
import re
import shlex
import subprocess
from pathlib import Path

ROOT = '/mnt/runner-data/benchmark-builder-278'
LAUNCHER = '/opt/oxidex-benchmark278/host-launcher.py'
TRANSFER = '/opt/oxidex-benchmark278/benchmark_transfer.py'
LIMIT = 4 * 1024**3


def identifier(value):
    if not isinstance(value, str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_-]{0,63}', value):
        raise ValueError('Invalid private builder project')
    return value


def digest(value):
    if not isinstance(value, str) or not re.fullmatch(r'[0-9a-f]{64}', value):
        raise ValueError('Invalid SHA256')
    return value


def target_paths(paths):
    if not isinstance(paths, list) or not 1 <= len(paths) <= 32:
        raise ValueError('Expected 1..32 target paths')
    for name in paths:
        if (not isinstance(name, str) or not name or len(name) > 1024
                or '\x00' in name or name.startswith('/')
                or len(name.split('/')) > 32
                or any(part in ('', '.', '..') for part in name.split('/'))):
            raise ValueError('Invalid relative target path')
    if len(set(paths)) != len(paths):
        raise ValueError('Duplicate target path')
    return paths


def launcher_command(project, *args):
    return shlex.join(['sudo', '-n', LAUNCHER, '--benchmark278', identifier(project), *args])


def transfer_command(operation, project, *args):
    if operation not in ('probe', 'stage', 'export'):
        raise ValueError('Unknown private transfer operation')
    return shlex.join(['sudo', '-n', TRANSFER, operation, identifier(project), *args])


def prepare_command(project):
    return launcher_command(project, 'prepare')


def cleanup_command(project):
    # Source only; the private launcher retains target/Cargo and immutable exports.
    return launcher_command(project, 'cleanup')


def supervisor_command(project, jobs):
    if not isinstance(jobs, list) or len(jobs) != 2:
        raise ValueError('Exactly two distinct worktree jobs required')
    names = []
    for job in jobs:
        if not isinstance(job, dict) or set(job) != {'worktree_id', 'argv'}:
            raise ValueError('Invalid worktree job')
        names.append(identifier(job['worktree_id']))
        argv = job['argv']
        if (not isinstance(argv, list) or not argv
                or any(not isinstance(arg, str) or '\x00' in arg for arg in argv)):
            raise ValueError('Invalid worktree argument vector')
    if len(set(names)) != 2:
        raise ValueError('Worktrees must be distinct')
    return launcher_command(project, 'python3', '/usr/local/bin/oxidex-worktree-commands',
                            json.dumps(jobs, separators=(',', ':')))


def stream_digest(stream):
    result = hashlib.sha256()
    length = 0
    while data := stream.read(1024 * 1024):
        length += len(data)
        if length > LIMIT:
            raise ValueError('Source archive exceeds 4 GiB')
        result.update(data)
    return result.hexdigest(), length


def stage_archive(ssh, project, archive):
    """Stream one verified archive over SSH stdin; never use a host home file."""
    identifier(project)
    with Path(archive).open('rb') as source:
        expected, length = stream_digest(source)
        if length == 0:
            raise ValueError('Empty source archive')
        source.seek(0)
        command = transfer_command('stage', project, expected, str(length))
        result = subprocess.run(ssh(command), stdin=source, capture_output=True,
                                text=True, check=True, timeout=660)
    record = json.loads(result.stdout)
    if (not isinstance(record, dict)
            or record.get('schema') != 'benchmark278-source-staged-v1'
            or record.get('project') != project or record.get('archive_sha256') != expected
            or type(record.get('archive_bytes')) is not int or record['archive_bytes'] != length
            or record.get('source') != f'{ROOT}/sources/{project}'
            or type(record.get('observed_at_ns')) is not int or record['observed_at_ns'] <= 0):
        raise ValueError('Private source staging receipt differs from upload')
    return record


def export_command(project, run_id, paths):
    if not isinstance(run_id, str) or not re.fullmatch(r'run-[0-9a-f]{32}', run_id):
        raise ValueError('Invalid immutable export ID')
    return transfer_command('export', project, run_id, json.dumps(target_paths(paths)))


def validate_export(record, project, run_id, paths):
    export_command(project, run_id, paths)
    if (not isinstance(record, dict) or record.get('schema') != 'benchmark278-immutable-export-v1'
            or record.get('project') != project or record.get('run_id') != run_id
            or type(record.get('observed_at_ns')) is not int or record['observed_at_ns'] <= 0
            or not isinstance(record.get('files'), list) or len(record['files']) != len(paths)):
        raise ValueError('Invalid immutable export receipt')
    digest(record.get('archive_sha256'))
    total = 0
    actual = []
    for row in record['files']:
        if not isinstance(row, dict) or set(row) != {'path', 'bytes', 'sha256'}:
            raise ValueError('Invalid exported file receipt')
        target_paths([row['path']])
        digest(row['sha256'])
        if type(row['bytes']) is not int or row['bytes'] < 0:
            raise ValueError('Invalid exported file length')
        total += row['bytes']
        actual.append(row['path'])
    if total > LIMIT or actual != paths:
        raise ValueError('Export receipt differs from requested files')
    return f'{ROOT}/exports/{project}/{run_id}'
