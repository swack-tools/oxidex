#!/usr/bin/env python3
"""Build the website from the authenticated builder checkout, retaining its snapshot."""
import json
import os
import re
import secrets
import shutil
import stat
import subprocess
from pathlib import Path

SOURCE = Path('/target/checkout')
OUTPUT = Path('/target/docs-site')
# The approved Spot image provides Node/npm here; CI must provide the same runtime.
RUNTIME_PATH = '/opt/node/bin:/usr/bin:/bin'
CI_DURABLE_ROOT = Path('/mnt/runner-data/remote-build/targets')
SYSTEM_TEMP_ROOTS = (Path('/tmp'), Path('/var/tmp'), Path('/private/tmp'),
                     Path('/dev/shm'), Path('/run'))


def _root_owned_path(path):
    """Refuse writable or non-root-owned lexical and resolved path components."""
    try:
        resolved = path.resolve(strict=True)
        for current_path in (path, resolved):
            for component in (current_path, *current_path.parents):
                info = component.lstat()
                if info.st_uid != 0 or (not stat.S_ISLNK(info.st_mode)
                                        and info.st_mode & 0o022):
                    return False
        return True
    except OSError:
        return False


def verify_runtime_tools():
    """Require the approved root-owned runtime before invoking any child."""
    for directory in RUNTIME_PATH.split(os.pathsep):
        path = Path(directory)
        if not path.is_dir() or not _root_owned_path(path):
            raise RuntimeError(f'Docs site requires a root-owned approved runtime directory: {path}')
    for name in ('node', 'npm', 'git', 'bash'):
        resolved = shutil.which(name, path=RUNTIME_PATH)
        if not resolved or not _root_owned_path(Path(resolved)):
            raise RuntimeError(f'Docs site requires root-owned approved runtime tool: {name}')
        info = Path(resolved).stat()
        if not stat.S_ISREG(info.st_mode) or not info.st_mode & 0o111:
            raise RuntimeError(f'Docs site requires executable approved runtime tool: {name}')


def build_environment(home):
    """Pass only runtime tool lookup plus fixed Git, npm and site settings."""
    return {'PATH': RUNTIME_PATH, 'HOME': str(home), 'TMPDIR': '/tmp',
            'LANG': 'C', 'LC_ALL': 'C', 'TZ': 'UTC',
            'GIT_NO_REPLACE_OBJECTS': '1', 'GIT_ATTR_NOSYSTEM': '1',
            'GIT_CONFIG_GLOBAL': os.devnull,
            'GIT_CONFIG_SYSTEM': os.devnull, 'DOCS_CHANNEL': 'stable',
            'DOCS_BASE': '/', 'NPM_CONFIG_USERCONFIG': str(home / 'npm-user.npmrc'),
            'NPM_CONFIG_GLOBALCONFIG': str(home / 'npm-global.npmrc'),
            'NPM_CONFIG_CACHE': str(home / 'npm-cache')}


def prepare_npm_configs(home):
    """Give npm two distinct, empty, private configs without ambient fallback."""
    for filename in ('npm-user.npmrc', 'npm-global.npmrc'):
        descriptor = os.open(home / filename,
                             os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        try:
            os.fchmod(descriptor, 0o600)
        finally:
            os.close(descriptor)


def check_output_path(output):
    if not output.is_absolute() or output.resolve() != output:
        raise RuntimeError(f'Docs site output path is not canonical: {output}')


def selected_context():
    """Use only the route-verified builder checkout or exact Actions workspace."""
    import route

    if not route.local_worker_context():
        raise RuntimeError('Docs site worker requires a trusted Spot context')
    source = Path.cwd()
    if source == SOURCE and route.trusted_marker(route.BUILDER_MARKER):
        if not route.verified_fleet_checkout():
            raise RuntimeError('Docs site worker requires the signed builder checkout')
        check_output_path(OUTPUT)
        return source, OUTPUT

    route.verify_ci_fleet_checkout()
    from test_runner import fleet_recipe_target
    if not os.environ.get('CARGO_TARGET_DIR'):
        raise RuntimeError('Docs site CI requires an explicit durable CARGO_TARGET_DIR')
    target = fleet_recipe_target()
    if (target.is_symlink() or target.resolve() != target
            or target == source or target in source.parents):
        raise RuntimeError('Docs site CI target is not a separate canonical directory')
    runner_temp = Path(os.environ.get('RUNNER_TEMP', '/nonexistent-runner-temp'))
    temporary_roots = (*SYSTEM_TEMP_ROOTS, runner_temp)
    if any(root.is_absolute() and (target == root or target.is_relative_to(root))
           for root in temporary_roots):
        raise RuntimeError('Docs site CI target is temporary and cannot retain a snapshot')
    if (not CI_DURABLE_ROOT.is_dir() or CI_DURABLE_ROOT.resolve() != CI_DURABLE_ROOT
            or target == CI_DURABLE_ROOT or not target.is_relative_to(CI_DURABLE_ROOT)):
        raise RuntimeError('Docs site CI requires an explicit target under the durable '
                           '/mnt/runner-data/remote-build/targets mount')
    run_id = os.environ.get('GITHUB_RUN_ID', '')
    attempt = os.environ.get('GITHUB_RUN_ATTEMPT', '')
    job = os.environ.get('GITHUB_JOB', '')
    if (not re.fullmatch(r'[0-9]+', run_id) or not re.fullmatch(r'[0-9]+', attempt)
            or not re.fullmatch(r'[A-Za-z0-9_-]{1,80}', job)):
        raise RuntimeError('Docs site CI run identity is incomplete')
    head = os.environ['GITHUB_SHA']
    nonce = secrets.token_hex(8)
    output = target / 'docs-site' / f'{run_id}-{attempt}-{job}-{head[:12]}-{nonce}'
    check_output_path(output)
    return source, output


def run(source, output):
    if Path.cwd() != source or source.is_symlink():
        raise RuntimeError('Docs site worker source changed after context verification')
    check_output_path(output)
    if output.exists() or output.is_symlink():
        raise RuntimeError(f'Docs site snapshot destination must be new: {output}')
    output.parent.mkdir(parents=True, exist_ok=True)
    check_output_path(output)
    output.mkdir()
    check_output_path(output)
    home = output.with_name(output.name + '-home')
    check_output_path(home)
    home.mkdir(mode=0o700)
    home_identity = home.lstat()
    try:
        check_output_path(home)
        prepare_npm_configs(home)
        env = build_environment(home)
        verify_runtime_tools()
        version = subprocess.check_output(['node', '--version'], text=True, env=env).strip()
        if not re.fullmatch(r'v24\.[0-9]+\.[0-9]+', version):
            raise RuntimeError(f'Docs site build requires Node.js 24, found {version}')
        subprocess.run(['npm', '--version'], check=True, stdout=subprocess.DEVNULL, env=env)
        head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True, env=env).strip()
        tree = subprocess.check_output(['git', 'rev-parse', 'HEAD^{tree}'], text=True, env=env).strip()
        subprocess.run(['bash', 'tools/docs-local-deploy.sh', '--ref', 'HEAD',
                        '--build-only', '--output', str(output)],
                       check=True, env=env)
        subprocess.run(['node', 'docs/scripts/check-base-links.mjs',
                        str(output / 'dist'), '/'], check=True, env=env)
        manifest = json.loads((output / 'snapshot-manifest.json').read_text())
        declared_digest = manifest.get('dist_sha256')
        if not isinstance(declared_digest, str) or not re.fullmatch(r'[0-9a-f]{64}', declared_digest):
            raise RuntimeError('Docs site snapshot has no valid dist_sha256')
        actual_digest = subprocess.check_output(
            ['bash', 'tools/docs/dist-sha256.sh', str(output / 'dist')],
            text=True, env=env).strip()
        if actual_digest != declared_digest:
            raise RuntimeError('Docs site snapshot dist_sha256 differs from retained dist')
        if (manifest.get('candidate_sha') != head or manifest.get('source_head_sha') != head
                or manifest.get('tree_hash') != tree or manifest.get('source_kind') != 'commit'
                or manifest.get('base_path') != '/'
                or manifest.get('dist') != str(output / 'dist')
                or not (output / 'dist' / 'index.html').is_file()):
            raise RuntimeError('Docs site snapshot does not bind the selected signed HEAD and rendered dist')
    finally:
        try:
            current_identity = home.lstat()
        except FileNotFoundError as exc:
            raise RuntimeError('Docs site private npm home changed before cleanup') from exc
        if (not stat.S_ISDIR(current_identity.st_mode)
                or current_identity.st_dev != home_identity.st_dev
                or current_identity.st_ino != home_identity.st_ino
                or current_identity.st_uid != os.getuid()
                or home.resolve() != home):
            raise RuntimeError('Docs site private npm home changed before cleanup')
        shutil.rmtree(home)
    print(f'DOCS_SITE_SNAPSHOT: source={head} output={output}', flush=True)


if __name__ == '__main__':
    run(*selected_context())
