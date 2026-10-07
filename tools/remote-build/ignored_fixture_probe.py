#!/usr/bin/env python3
"""Authenticate and probe the inputs needed by executable ignored tests on Spot.

This prepares an owned cache and a receipt. It deliberately does not stage files
at their test paths or claim that the ignored suite has executed.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import secrets
import stat
import subprocess
import sys
from datetime import datetime, timezone
from urllib.error import URLError
from urllib.parse import urljoin, urlparse
from urllib.request import HTTPRedirectHandler, build_opener
import zipfile

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = Path(__file__).with_name('ignored-inputs.json')
DOWNLOAD_DEADLINE_SECONDS = 180


def digest(path: Path, algorithm: str = 'sha256') -> str:
    result = hashlib.new(algorithm)
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(block)
    return result.hexdigest()


def relative_path(value: str) -> Path:
    path = PurePosixPath(value)
    if not value or value == '.' or path.is_absolute() or '..' in path.parts or str(path) != value:
        raise ValueError(f'unsafe relative path: {value!r}')
    return Path(*path.parts)


def verify_file(path: Path, row: dict, *, archive: bool = False) -> dict:
    if not path.is_file() or path.is_symlink():
        raise ValueError(f'missing or nonregular fixture: {path}')
    size = path.stat().st_size
    expected_size = row.get('archive_bytes' if archive else 'bytes')
    if expected_size is not None and size != expected_size:
        raise ValueError(f'{row["id"]}: size {size}, expected {expected_size}')
    limit = row.get('max_bytes' if archive else 'max_member_bytes', row.get('max_bytes'))
    if limit is not None and size > limit:
        raise ValueError(f'{row["id"]}: size {size} exceeds limit {limit}')
    sha256 = digest(path)
    expected_sha = row.get('archive_sha256' if archive else 'sha256')
    if expected_sha is not None and sha256 != expected_sha:
        raise ValueError(f'{row["id"]}: SHA-256 mismatch')
    expected_sha1 = None if archive else row.get('sha1')
    if expected_sha1 is not None and digest(path, 'sha1') != expected_sha1:
        raise ValueError(f'{row["id"]}: publisher SHA-1 mismatch')
    return {'bytes': size, 'sha256': sha256, 'sha1': digest(path, 'sha1')}


class HTTPSOnlyRedirect(HTTPRedirectHandler):
    """Reject a plaintext hop before urllib can open the redirected URL."""

    def redirect_request(self, request, fp, code, message, headers, newurl):
        target = urljoin(request.full_url, newurl)
        if urlparse(target).scheme != 'https':
            raise URLError('redirect outside HTTPS refused')
        return super().redirect_request(request, fp, code, message, headers, target)


def download_child(url: str, destination: Path, max_bytes: int) -> None:
    """Run only in the disposable child; the parent enforces total wall time."""
    opener = build_opener(HTTPSOnlyRedirect)
    with opener.open(url, timeout=30) as response, destination.open('xb') as output:
        if urlparse(response.geturl()).scheme != 'https':
            raise ValueError('source redirected outside HTTPS')
        advertised = response.headers.get('Content-Length')
        if advertised is not None and int(advertised) > max_bytes:
            raise ValueError('source exceeds bounded download size')
        total = 0
        while block := response.read(min(1024 * 1024, max_bytes - total + 1)):
            total += len(block)
            if total > max_bytes:
                raise ValueError('source exceeds bounded download size')
            output.write(block)


def download(url: str, destination: Path, max_bytes: int) -> None:
    if urlparse(url).scheme != 'https' or max_bytes <= 0:
        raise ValueError('source must be bounded HTTPS')
    temporary = destination.with_name(destination.name + '.' + secrets.token_hex(8) + '.part')
    try:
        # A socket timeout alone does not bound a slow trickle or DNS. Keep
        # the whole open/redirect/read operation in a killable child process.
        command = [sys.executable, str(Path(__file__).resolve()), '--download-child',
                   url, str(temporary), str(max_bytes)]
        try:
            result = subprocess.run(command, capture_output=True, text=True, timeout=DOWNLOAD_DEADLINE_SECONDS)
        except subprocess.TimeoutExpired as error:
            raise TimeoutError('HTTPS download exceeded total deadline') from error
        if result.returncode:
            raise ValueError('HTTPS download refused: ' + result.stderr[-1000:])
        if not temporary.is_file() or temporary.is_symlink():
            raise ValueError('HTTPS download child produced no regular file')
        temporary.replace(destination)
    finally:
        temporary.unlink(missing_ok=True)


def materialize(row: dict, cache: Path) -> tuple[Path, dict]:
    source = row['source']
    kind = source['kind']
    if kind in ('pinned', 'repo'):
        base = Path(os.environ['EXIFTOOL_CACHE_DIR']) if kind == 'pinned' else ROOT
        path = base / relative_path(source['path'])
        return path, verify_file(path, row)
    if kind not in ('download', 'zip_member'):
        raise ValueError(f'{row["id"]}: unknown source kind {kind}')
    url = source['url']
    cache.mkdir(parents=True, exist_ok=True)
    archive = kind == 'zip_member'
    cached = cache / (row['id'] + ('.zip' if archive else '.source'))
    if cached.exists():
        verify_file(cached, row, archive=archive)
    else:
        download(url, cached, row['max_bytes'])
        verify_file(cached, row, archive=archive)
    if not archive:
        return cached, verify_file(cached, row)
    member_name = source['member']
    relative_path(member_name)
    extracted = cache / (row['id'] + '.exe')
    with zipfile.ZipFile(cached) as bundle:
        members = [member for member in bundle.infolist() if member.filename == member_name]
        if len(members) != 1 or members[0].is_dir() or \
                stat.S_IFMT(members[0].external_attr >> 16) == stat.S_IFLNK:
            raise ValueError(f'{row["id"]}: expected one regular {member_name} ZIP member')
        member = members[0]
        if member.file_size == 0 or member.file_size > row['max_member_bytes']:
            raise ValueError(f'{row["id"]}: ZIP member exceeds bounds')
        temporary = extracted.with_name(extracted.name + '.' + secrets.token_hex(8) + '.part')
        try:
            with bundle.open(member) as src, temporary.open('xb') as dst:
                total = 0
                for block in iter(lambda: src.read(1024 * 1024), b''):
                    total += len(block)
                    if total > row['max_member_bytes']:
                        raise ValueError(f'{row["id"]}: ZIP member exceeds bounds')
                    dst.write(block)
            if temporary.stat().st_size != member.file_size:
                raise ValueError(f'{row["id"]}: ZIP member size changed')
            temporary.replace(extracted)
        finally:
            temporary.unlink(missing_ok=True)
    return extracted, verify_file(extracted, row)


def native_probe(path: Path, row: dict) -> dict:
    oracle_root = Path(os.environ['EXIFTOOL_CACHE_DIR'])
    perl = Path(os.environ['EXIFTOOL_PERL'])
    exiftool = oracle_root / 'exiftool' / 'exiftool'
    if not perl.is_file() or not exiftool.is_file():
        raise ValueError('pinned Perl or ExifTool missing after bootstrap')
    result = subprocess.run([str(perl), str(exiftool), '-G0', '-json', str(path)],
                            capture_output=True, text=True, timeout=60, check=True)
    parsed = json.loads(result.stdout)
    if len(parsed) != 1 or not isinstance(parsed[0], dict):
        raise ValueError(f'{row["id"]}: invalid native JSON shape')
    fields = parsed[0]
    nonempty = {key: value for key, value in fields.items()
                if key != 'SourceFile' and value is not None and value != ''}
    required = row['required_tags']
    if row['kind'] == 'media':
        compared = {key: nonempty[key] for key in required if key in nonempty}
        if not compared:
            raise ValueError(f'{row["id"]}: zero native fields from {required}')
        for key, value in row.get('expected_values', {}).items():
            if compared.get(key) != value:
                raise ValueError(f'{row["id"]}: native {key} is {compared.get(key)!r}, expected {value!r}')
    else:
        def find(tag: str):
            if tag.endswith(':*'):
                return {k: v for k, v in nonempty.items() if k.startswith(tag[:-1])}
            if ':' in tag:
                return {tag: nonempty[tag]} if tag in nonempty else {}
            return {k: v for k, v in nonempty.items() if k == tag or k.endswith(':' + tag)}
        missing = [tag for tag in required if not find(tag)]
        if missing:
            raise ValueError(f'{row["id"]}: missing native fields {missing}')
        compared = {key: value for tag in required for key, value in find(tag).items()}
        if row['kind'] == 'raw':
            model = str(next(value for key, value in compared.items() if key.endswith(':Model')))
            make = str(next(value for key, value in compared.items() if key.endswith(':Make')))
            if source_model := row['source'].get('publisher_model'):
                if source_model.casefold() not in model.casefold():
                    raise ValueError(f'{row["id"]}: native model {model!r} differs from publisher {source_model!r}')
            if source_make := row['source'].get('publisher_make'):
                if source_make.casefold() not in make.casefold():
                    raise ValueError(f'{row["id"]}: native make {make!r} differs from publisher {source_make!r}')
    return {'compared_fields': compared, 'compared_count': len(compared),
            'native_field_count': len(nonempty)}


def run(manifest: dict, target: Path) -> tuple[Path, dict]:
    if manifest.get('schema') != 1 or manifest.get('oracle_pin') != '13.59':
        raise ValueError('unsupported ignored-input manifest')
    rows = manifest['inputs']
    if len(rows) != 25 or len({row['id'] for row in rows}) != 25:
        raise ValueError('ignored-input manifest must name 25 distinct cases')
    for row in rows:
        relative_path(row['target'])
    namespace = target / 'ignored-inputs'
    runs = namespace / 'runs'
    runs.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S%fZ')
    run_dir = runs / (stamp + '-' + secrets.token_hex(4))
    run_dir.mkdir()
    receipt = {'schema': 1, 'status': 'FAILED', 'manifest_sha256': digest(MANIFEST),
               'oracle_pin': manifest['oracle_pin'], 'run_dir': str(run_dir), 'inputs': []}
    for row in rows:
        item = {'id': row['id'], 'kind': row['kind'], 'target': row['target'],
                'source': row['source'], 'status': 'FAILED'}
        try:
            path, verified = materialize(row, namespace / 'cache')
            item.update({'path': str(path), 'verified': verified,
                         'native': native_probe(path, row), 'status': 'PASS'})
        except Exception as error:
            item['error'] = f'{type(error).__name__}: {error}'
        receipt['inputs'].append(item)
        print(f'IGNORED_INPUT {row["id"]} {item["status"]}', flush=True)
    receipt['status'] = 'PASS' if all(item['status'] == 'PASS' for item in receipt['inputs']) else 'FAILED'
    output = run_dir / 'receipt.json'
    output.write_text(json.dumps(receipt, indent=2, sort_keys=True) + '\n')
    print(f'IGNORED_INPUT_RECEIPT {output} status={receipt["status"]}', flush=True)
    return output, receipt


def main() -> int:
    from route import local_worker_context
    if not local_worker_context():
        raise SystemExit('refusing ignored input probe outside the guarded Spot worker')
    if len(sys.argv) == 5 and sys.argv[1] == '--download-child':
        url, destination, maximum = sys.argv[2:]
        if urlparse(url).scheme != 'https' or not maximum.isdecimal() or int(maximum) <= 0:
            raise SystemExit('invalid bounded HTTPS child request')
        download_child(url, Path(destination), int(maximum))
        return 0
    if len(sys.argv) != 1:
        raise SystemExit('invalid ignored input probe arguments')
    manifest = json.loads(MANIFEST.read_text())
    target = Path(os.environ.get('CARGO_TARGET_DIR', '/target'))
    if not target.is_absolute() or target == Path('/src') or str(target).startswith('/src/'):
        raise SystemExit('ignored input evidence requires an absolute target outside /src')
    _, receipt = run(manifest, target)
    return 0 if receipt['status'] == 'PASS' else 1


if __name__ == '__main__':
    sys.exit(main())
