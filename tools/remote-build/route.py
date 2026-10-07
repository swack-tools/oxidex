#!/usr/bin/env python3
"""Route normal heavy Just recipes to the ordinary Spot builder."""
import os
import hashlib
from pathlib import Path
import re
import stat
import subprocess
import sys

from lib.remote_build import FLEET_RECIPES

HERE = Path(__file__).resolve().parent
RECIPE = re.compile(r'[a-z][a-z0-9_-]{0,63}\Z')
ORACLE_TEST_RECIPES = frozenset({
    'test', 'test-nextest', 'test-debug', 'test-nocapture', 'test-unit',
    'test-integration', 'test-ffi-c', 'test-comparison', 'test-doc',
    'test-package', 'test-tags', 'test-ignored', 'prepare-ignored-inputs', 'ci',
})
FLEET_SOURCE = Path('/src')
FLEET_CHECKOUT = Path('/target/checkout')
FLEET_CARGO_TARGET = Path('/target/cargo')
BUILDER_MARKER = Path('/run/oxidex-build-container')
RUNNER_MARKER = Path('/run/oxidex-spot-runner')
CI_ORIGIN_URLS = frozenset({'https://github.com/swack-tools/oxidex',
                            'https://github.com/swack-tools/oxidex.git'})


def trusted_marker(path):
    try:
        info = path.lstat()
    except FileNotFoundError:
        return False
    return (stat.S_ISREG(info.st_mode) and info.st_uid == 0
            and stat.S_IMODE(info.st_mode) == 0o444 and info.st_nlink == 1)


def verified_fleet_checkout() -> bool:
    if (Path.cwd() != FLEET_CHECKOUT or FLEET_CHECKOUT.is_symlink()
            or FLEET_CHECKOUT.resolve() != FLEET_CHECKOUT):
        return False
    from qualification_bootstrap import verify_staged_checkout
    bundle = FLEET_SOURCE / 'repository.bundle'
    signers = FLEET_SOURCE / 'maintainer.allowed_signers'
    head_file = FLEET_SOURCE / 'fleet-source-head'
    if any(path.is_symlink() or not path.is_file() for path in (bundle, signers, head_file)):
        return False
    head = head_file.read_text().strip()
    if not re.fullmatch(r'[0-9a-f]{40}', head):
        return False
    verify_staged_checkout(FLEET_CHECKOUT, head, signers,
                           None, hashlib.sha256(bundle.read_bytes()).hexdigest())
    if subprocess.check_output(
            ['git', '-C', str(FLEET_CHECKOUT), 'status', '--porcelain', '--untracked-files=all']):
        return False
    _verify_signed_worktree(FLEET_CHECKOUT, head)
    return True


def prepare_fleet_checkout() -> None:
    if Path.cwd() != FLEET_SOURCE or FLEET_CHECKOUT.exists() or FLEET_CHECKOUT.is_symlink():
        raise RuntimeError('fleet source must start in fresh remote builder context')
    bundle = FLEET_SOURCE / 'repository.bundle'
    if bundle.is_symlink() or not bundle.is_file():
        raise RuntimeError('signed fleet source bundle is unavailable')
    subprocess.run(['git', 'clone', '-q', str(bundle), str(FLEET_CHECKOUT)], check=True)
    head = (FLEET_SOURCE / 'fleet-source-head').read_text().strip()
    if not re.fullmatch(r'[0-9a-f]{40}', head):
        raise RuntimeError('fleet source HEAD is malformed')
    subprocess.run(['git', '-C', str(FLEET_CHECKOUT), 'checkout', '-q', '--detach', head],
                   check=True)
    os.chdir(FLEET_CHECKOUT)
    if not verified_fleet_checkout():
        raise RuntimeError('signed fleet checkout is not the clean selected source')
    print(f'FLEET_SIGNED_SOURCE: head={head} checkout={FLEET_CHECKOUT}', flush=True)


def verify_signed_builder_target() -> None:
    """The selected Cargo path must be a canonical sibling of signed source."""
    if FLEET_CARGO_TARGET == FLEET_CHECKOUT or FLEET_CARGO_TARGET in FLEET_CHECKOUT.parents:
        raise RuntimeError('signed fleet Cargo target contains the source checkout')
    if (FLEET_CARGO_TARGET.is_symlink() or FLEET_CARGO_TARGET.resolve() != FLEET_CARGO_TARGET
            or (FLEET_CARGO_TARGET.exists() and not FLEET_CARGO_TARGET.is_dir())):
        raise RuntimeError('signed fleet Cargo target is not canonical')


def select_signed_builder_target() -> None:
    """Keep Cargo outside its signed checkout so trybuild can normalize source paths."""
    configured = os.environ.get('CARGO_TARGET_DIR')
    if configured not in (None, '/target', str(FLEET_CARGO_TARGET)):
        raise RuntimeError('signed fleet checkout has an unexpected Cargo target')
    verify_signed_builder_target()
    os.environ['CARGO_TARGET_DIR'] = str(FLEET_CARGO_TARGET)


def _read_batch_blob(stream, object_id: str, expected_size: int) -> bytes:
    """Read one framed exact-object response from git cat-file --batch."""
    header = stream.readline()
    match = re.fullmatch(rb"([0-9a-f]{40}) blob ([0-9]+)\n", header)
    if not match or match.group(1).decode('ascii') != object_id:
        raise RuntimeError('fleet CI source has malformed or mismatched Git blob header')
    size = int(match.group(2))
    if size != expected_size:
        raise RuntimeError('fleet CI source has mismatched Git blob size')
    data = stream.read(size)
    if len(data) != size or stream.read(1) != b'\n':
        raise RuntimeError('fleet CI source has truncated Git blob response')
    return data


def _verify_signed_worktree(checkout: Path, head: str) -> None:
    """Compare every included worktree byte and execute bit to exact HEAD."""
    from lib.remote_build import signed_snapshot_files
    signed = signed_snapshot_files(checkout, head)
    env = dict(os.environ, GIT_NO_REPLACE_OBJECTS='1')
    command = ['git', '-C', str(checkout), 'cat-file', '--batch']
    with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          stderr=subprocess.DEVNULL, env=env) as child:
        try:
            assert child.stdin is not None and child.stdout is not None
            for name, (object_id, mode) in signed.items():
                if not re.fullmatch(r'[0-9a-f]{40}', object_id):
                    raise RuntimeError('fleet CI source has malformed Git object ID')
                child.stdin.write((object_id + '\n').encode('ascii'))
                child.stdin.flush()
                path = checkout / name
                info = path.lstat()
                if (not stat.S_ISREG(info.st_mode)
                        or bool(info.st_mode & 0o111) != (mode == 0o755)):
                    raise RuntimeError(f'fleet CI source differs from selected HEAD: {name}')
                try:
                    data = _read_batch_blob(child.stdout, object_id, info.st_size)
                except RuntimeError as exc:
                    raise RuntimeError(
                        f'fleet CI source differs from selected HEAD: {name}: {exc}'
                    ) from exc
                if path.read_bytes() != data:
                    raise RuntimeError(f'fleet CI source differs from selected HEAD: {name}')
            child.stdin.close()
            if child.stdout.read(1) != b'' or child.wait() != 0:
                raise RuntimeError('fleet CI source has extra or failed Git blob response')
        except BaseException:
            child.kill()
            child.wait()
            raise


def verify_ci_fleet_checkout() -> None:
    """Admit the selected Actions checkout and restore history if shallow."""
    checkout = Path.cwd()
    workspace = Path(os.environ.get('GITHUB_WORKSPACE', ''))
    if (not workspace.is_absolute() or workspace.is_symlink()
            or checkout != workspace or checkout.resolve() != checkout):
        raise RuntimeError('fleet CI source must be the canonical Actions workspace')
    if (os.environ.get('GITHUB_SERVER_URL') != 'https://github.com'
            or os.environ.get('GITHUB_REPOSITORY') != 'swack-tools/oxidex'):
        raise RuntimeError('fleet CI source is outside the expected Actions repository')
    def git(*args):
        return subprocess.check_output(
            ['git', '-C', str(checkout), *args],
            env=dict(os.environ, GIT_NO_REPLACE_OBJECTS='1'), text=True).strip()
    if git('rev-parse', '--show-toplevel') != str(checkout):
        raise RuntimeError('fleet CI source must be the checkout root')
    if git('status', '--porcelain', '--untracked-files=all'):
        raise RuntimeError('fleet CI source is not clean')
    head = git('rev-parse', 'HEAD')
    if head != os.environ.get('GITHUB_SHA'):
        raise RuntimeError('fleet CI source is not the selected Actions commit')
    # Status alone misses skip-worktree and assume-unchanged. Check the bytes
    # this recipe can execute against the selected checkout commit.
    _verify_signed_worktree(checkout, head)
    if git('rev-parse', '--is-shallow-repository') == 'true':
        origin = git('config', '--get', 'remote.origin.url')
        if origin not in CI_ORIGIN_URLS or git('remote', 'get-url', 'origin') != origin:
            raise RuntimeError('fleet CI history origin is not the approved repository')
        subprocess.run(['git', '-C', str(checkout), 'fetch', '--no-tags', '--unshallow',
                        'origin', head], check=True,
                       env=dict(os.environ, GIT_NO_REPLACE_OBJECTS='1'))
        if (git('rev-parse', '--is-shallow-repository') != 'false'
                or git('rev-parse', 'HEAD') != head
                or git('status', '--porcelain', '--untracked-files=all')):
            raise RuntimeError('fleet CI history fetch changed the selected checkout')
    elif git('rev-parse', '--is-shallow-repository') != 'false':
        raise RuntimeError('fleet CI history state is unknown')
    print(f'FLEET_CI_SOURCE: head={head} checkout={checkout} history=complete', flush=True)


def local_worker_context(environ=None, marker=BUILDER_MARKER,
                         runner_marker=RUNNER_MARKER):
    environ = os.environ if environ is None else environ
    if sys.platform != 'linux':
        return False
    if Path.cwd() == FLEET_SOURCE and trusted_marker(marker):
        return True
    if Path.cwd() == FLEET_CHECKOUT and trusted_marker(marker):
        return verified_fleet_checkout()
    return (trusted_marker(runner_marker) and environ.get('CI') == 'true'
            and environ.get('GITHUB_ACTIONS') == 'true'
            and environ.get('RUNNER_ENVIRONMENT') == 'self-hosted'
            and environ.get('RUNNER_OS') == 'Linux'
            and bool(environ.get('RUNNER_NAME'))
            and bool(re.fullmatch(r'[0-9]+', environ.get('GITHUB_RUN_ID', ''))))

def main(argv):
    if argv and argv[0] == '--require-local-context':
        if len(argv) not in (1, 2) or (len(argv) == 2 and not RECIPE.fullmatch(argv[1])):
            raise SystemExit('Invalid guarded recipe name')
        if not local_worker_context():
            name = argv[1] if len(argv) == 2 else 'heavy recipe'
            raise SystemExit(f'Refusing local {name}: remote input/output mapping is required')
        builder_checkout = Path.cwd() == FLEET_CHECKOUT and trusted_marker(BUILDER_MARKER)
        # A private Just prerequisite runs in its own process. Selecting a
        # target here would not reach the following Cargo command, so require
        # the public route (or an explicit correctly bound environment) first.
        if builder_checkout and os.environ.get('CARGO_TARGET_DIR') != str(FLEET_CARGO_TARGET):
            raise SystemExit('Refusing signed fleet worker without its separate Cargo target')
        if builder_checkout:
            verify_signed_builder_target()
        if len(argv) == 2 and argv[1] in FLEET_RECIPES and not builder_checkout:
            verify_ci_fleet_checkout()
        marker = BUILDER_MARKER if (Path.cwd() in (FLEET_SOURCE, FLEET_CHECKOUT)
                                    and trusted_marker(BUILDER_MARKER)) else RUNNER_MARKER
        print(f'REMOTE_RECIPE_CONTEXT: root-owned 0444 marker {marker}',flush=True)
        return 0
    if not argv or not RECIPE.fullmatch(argv[0]):
        raise SystemExit('Invalid remote recipe name')
    recipe, *args = argv
    component = recipe == 'prove-linux-perl-component'
    if any(not value or len(value) > 4096 or any(ch in value for ch in '\x00\n\r') for value in args):
        raise SystemExit('Invalid recipe argument')
    worker_context = local_worker_context()
    if component and ((worker_context and (len(args) != 1 or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_-]{0,30}-[0-9a-f]{32}', args[0])))
                      or (not worker_context and (len(args) != 1 or not Path(args[0]).is_absolute()))):
        raise SystemExit('Component proof requires one private run ID or one absolute envelope path')
    if worker_context:
        if recipe in FLEET_RECIPES:
            signed_builder = False
            if Path.cwd() == FLEET_SOURCE and trusted_marker(BUILDER_MARKER):
                prepare_fleet_checkout()
                signed_builder = True
            elif Path.cwd() == FLEET_CHECKOUT and trusted_marker(BUILDER_MARKER):
                if not verified_fleet_checkout():
                    raise RuntimeError('signed fleet checkout is not the clean selected source')
                signed_builder = True
            else:
                verify_ci_fleet_checkout()
            if signed_builder:
                select_signed_builder_target()
        if recipe == 'verify-linux-perl':
            # Bootstrap integration tests require the canonical cache paths.
            for key in ('PERL5LIB', 'PERLLIB', 'PERL5OPT'):
                os.environ.pop(key, None)
            from test_runner import prepare_generic_recipe_oracle
            prepare_generic_recipe_oracle()
        elif (recipe in FLEET_RECIPES
              and recipe not in ('test-ignored', 'freeze-linux-perl',
                                 'prove-linux-perl-component', 'docs-site-build',
                                 'test-remote-build')):
            from test_runner import prepare_fleet_recipe_oracle
            prepare_fleet_recipe_oracle()
        elif recipe in ORACLE_TEST_RECIPES:
            from test_runner import prepare_generic_recipe_oracle
            prepare_generic_recipe_oracle()
        os.execvp('just', ['just', '_' + recipe + '-worker', *args])
        return 0  # execvp does not return outside synthetic controls
    command = [sys.executable, str(HERE/'build.py'), '--just-recipe', recipe]
    if component:
        command.append('--approved-linux-perl-envelope=' + args[0])
    else:
        for value in args:
            command.append('--just-arg=' + value)
    os.execv(sys.executable, command)


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1:]))
