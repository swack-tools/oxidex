#!/usr/bin/env python3
"""Build the website from the authenticated builder checkout, retaining its snapshot."""
import json
import os
import re
import secrets
import shutil
import subprocess
from pathlib import Path

SOURCE = Path('/target/checkout')
OUTPUT = Path('/target/docs-site')


def selected_context():
    """Use only the route-verified builder checkout or exact Actions workspace."""
    import route

    if not route.local_worker_context():
        raise RuntimeError('Docs site worker requires a trusted Spot context')
    source = Path.cwd()
    if source == SOURCE and route.trusted_marker(route.BUILDER_MARKER):
        if not route.verified_fleet_checkout():
            raise RuntimeError('Docs site worker requires the signed builder checkout')
        return source, OUTPUT

    route.verify_ci_fleet_checkout()
    from test_runner import fleet_recipe_target
    target = fleet_recipe_target()
    if (target.is_symlink() or target.resolve() != target
            or target == source or target in source.parents):
        raise RuntimeError('Docs site CI target is not a separate canonical directory')
    run_id = os.environ.get('GITHUB_RUN_ID', '')
    attempt = os.environ.get('GITHUB_RUN_ATTEMPT', '')
    job = os.environ.get('GITHUB_JOB', '')
    if (not re.fullmatch(r'[0-9]+', run_id) or not re.fullmatch(r'[0-9]+', attempt)
            or not re.fullmatch(r'[A-Za-z0-9_-]{1,80}', job)):
        raise RuntimeError('Docs site CI run identity is incomplete')
    head = os.environ['GITHUB_SHA']
    nonce = secrets.token_hex(8)
    return source, target / 'docs-site' / f'{run_id}-{attempt}-{job}-{head[:12]}-{nonce}'


def run(source, output):
    if Path.cwd() != source or source.is_symlink():
        raise RuntimeError('Docs site worker source changed after context verification')
    if output.exists() or output.is_symlink():
        raise RuntimeError(f'Docs site snapshot destination must be new: {output}')
    if shutil.which('node') is None or shutil.which('npm') is None:
        raise RuntimeError('Docs site build requires Node.js 24 and npm on the Spot worker')
    version = subprocess.check_output(['node', '--version'], text=True).strip()
    if not re.fullmatch(r'v24\.[0-9]+\.[0-9]+', version):
        raise RuntimeError(f'Docs site build requires Node.js 24, found {version}')
    subprocess.run(['npm', '--version'], check=True, stdout=subprocess.DEVNULL)
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
    tree = subprocess.check_output(['git', 'rev-parse', 'HEAD^{tree}'], text=True).strip()
    env = dict(DOCS_CHANNEL='stable', DOCS_BASE='/')
    subprocess.run(['bash', 'tools/docs-local-deploy.sh', '--ref', 'HEAD',
                    '--build-only', '--output', str(output)],
                   check=True, env={**os.environ, **env})
    subprocess.run(['node', 'docs/scripts/check-base-links.mjs',
                    str(output / 'dist'), '/'], check=True)
    manifest = json.loads((output / 'snapshot-manifest.json').read_text())
    declared_digest = manifest.get('dist_sha256')
    if not isinstance(declared_digest, str) or not re.fullmatch(r'[0-9a-f]{64}', declared_digest):
        raise RuntimeError('Docs site snapshot has no valid dist_sha256')
    actual_digest = subprocess.check_output(
        ['bash', 'tools/docs/dist-sha256.sh', str(output / 'dist')], text=True).strip()
    if actual_digest != declared_digest:
        raise RuntimeError('Docs site snapshot dist_sha256 differs from retained dist')
    if (manifest.get('candidate_sha') != head or manifest.get('source_head_sha') != head
            or manifest.get('tree_hash') != tree or manifest.get('source_kind') != 'commit'
            or manifest.get('base_path') != '/'
            or manifest.get('dist') != str(output / 'dist')
            or not (output / 'dist' / 'index.html').is_file()):
        raise RuntimeError('Docs site snapshot does not bind the selected signed HEAD and rendered dist')
    print(f'DOCS_SITE_SNAPSHOT: source={head} output={output}', flush=True)


if __name__ == '__main__':
    run(*selected_context())
