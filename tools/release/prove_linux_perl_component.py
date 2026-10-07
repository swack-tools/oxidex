#!/usr/bin/env python3
"""Record only a fresh approved Linux Perl cold/warm component proof."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
FLEET_CHECKOUT = Path('/target/checkout')
SOURCE = Path('/src')
TARGET = Path('/target')
OPS = TARGET / 'ops'
ENVELOPE = SOURCE / 'approved-linux-perl.json'
PROOF = TARGET / 'linux-perl-component-proof.json'
RUN_ID = re.compile(r'[A-Za-z0-9][A-Za-z0-9_-]{0,30}-[0-9a-f]{32}\Z')


def sha(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def git(*args: str) -> str:
    return subprocess.check_output(['git', '-C', str(ROOT), *args], text=True).strip()


def main(run_id: str) -> None:
    if not RUN_ID.fullmatch(run_id) or Path.cwd() != ROOT or ROOT != FLEET_CHECKOUT:
        raise RuntimeError('component proof requires a signed private builder project')
    sys.path.insert(0, str(ROOT / 'tools/remote-build'))
    import route
    if not route.verified_fleet_checkout():
        raise RuntimeError('component proof requires the exact signed fleet checkout')
    if (TARGET.is_symlink() or not TARGET.is_dir()
            or not TARGET.stat().st_mode & stat.S_IXOTH
            or OPS.exists() or OPS.is_symlink() or PROOF.exists() or PROOF.is_symlink()):
        raise RuntimeError('component proof requires a fresh private target and ops root')
    head = (SOURCE / 'fleet-source-head').read_text().strip()
    if git('rev-parse', 'HEAD') != head or git('status', '--porcelain', '--untracked-files=all'):
        raise RuntimeError('component proof source changed after signed admission')
    if ENVELOPE.is_symlink() or not ENVELOPE.is_file():
        raise RuntimeError('component proof packet lacks a regular approved envelope')
    for key in ('PERL5LIB', 'PERLLIB', 'PERL5OPT'):
        os.environ.pop(key, None)
    os.environ['OXIDEX_OPS_DIR'] = str(OPS)
    from tools.release import approved_linux_perl, bootstrap_oracle as bootstrap
    # The descriptor is from the verified Git checkout; the envelope is data.
    approval = approved_linux_perl.load(
        ROOT / 'tools/release/oracle-lock.json', envelope=ENVELOPE)
    envelope_sha = sha(ENVELOPE)
    OPS.mkdir(mode=0o700)
    info = OPS.lstat()
    if (not stat.S_ISDIR(info.st_mode) or info.st_uid != os.geteuid()
            or stat.S_IMODE(info.st_mode) != 0o700):
        raise RuntimeError('component proof ops root is not privately worker owned')
    prefix = bootstrap.perl_prefix(OPS)
    if (prefix.exists() or prefix.is_symlink() or bootstrap.manifest_path(OPS).exists()):
        raise RuntimeError('component proof refuses an existing installation or manifest')
    cold = bootstrap._materialize_perl(OPS, None, None, approval)
    if cold != 'installed':
        raise RuntimeError('component cold phase did not install approved Perl')
    warm = bootstrap._materialize_perl(OPS, None, None, approval)
    if warm != 'reused':
        raise RuntimeError('component warm phase did not reuse approved Perl')
    approved_linux_perl.check_tree(approval[0], prefix, bootstrap.sha256_tree)
    perl = prefix / 'bin/perl5.38.2'
    environment = bootstrap.staged_perl_environment(prefix)
    config_prefix = bootstrap.run([str(perl), '-MConfig', '-e', 'print $Config{prefix}'],
                                  env=environment)
    zip_version = bootstrap.run([str(perl), '-MArchive::Zip', '-e',
                                 'print $Archive::Zip::VERSION'], env=environment)
    if config_prefix != str(prefix) or zip_version != '1.68':
        raise RuntimeError('component Perl capability changed after warm verification')
    descriptor = approval[0]
    prefix_info = prefix.lstat()
    proof = {
        'schema': 1, 'kind': 'linux_perl_component_proof',
        'status': 'COMPONENT_ONLY_PASS', 'run_id': run_id,
        'source_head': head, 'source_tree': git('rev-parse', 'HEAD^{tree}'),
        'bundle_sha256': sha(SOURCE / 'repository.bundle'),
        'descriptor_sha256': sha(ROOT / 'tools/release/oracle-linux-perl-identity.json'),
        'lock_sha256': sha(ROOT / 'tools/release/oracle-lock.json'),
        'envelope_sha256': envelope_sha,
        'archive_sha256': descriptor['archive_sha256'],
        'archive_bytes': descriptor['archive_bytes'],
        'tree_sha256': bootstrap.sha256_tree(prefix),
        'exe_sha256': approved_linux_perl.sha(perl),
        'zip_sha256': approved_linux_perl.sha(prefix / descriptor['zip_relative_path']),
        'prefix': str(prefix), 'prefix_mode': stat.S_IMODE(prefix_info.st_mode),
        'prefix_uid': prefix_info.st_uid, 'config_prefix': config_prefix,
        'zip_version': zip_version, 'cold': cold, 'warm': warm,
    }
    body=(json.dumps(proof, sort_keys=True, separators=(',', ':'))+'\n').encode()
    if len(body)>64*1024:
        raise RuntimeError('component proof exceeds transport bound')
    with PROOF.open('xb') as output:
        output.write(body)
        output.flush()
        os.fsync(output.fileno())
    PROOF.chmod(0o644)


if __name__ == '__main__':
    if len(sys.argv) != 2:
        raise SystemExit('usage: prove_linux_perl_component.py RUN_ID')
    main(sys.argv[1])
