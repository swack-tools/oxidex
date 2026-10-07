#!/usr/bin/env python3
"""Route normal heavy Just recipes to the ordinary Spot builder."""
import os
from pathlib import Path
import re
import stat
import sys

HERE = Path(__file__).resolve().parent
RECIPE = re.compile(r'[a-z][a-z0-9_-]{0,63}\Z')
ORACLE_TEST_RECIPES = frozenset({
    'test', 'test-nextest', 'test-debug', 'test-nocapture', 'test-unit',
    'test-integration', 'test-ffi-c', 'test-comparison', 'test-doc',
    'test-package', 'test-tags', 'test-ignored',
})


def trusted_marker(path):
    try:
        info = path.lstat()
    except FileNotFoundError:
        return False
    return (stat.S_ISREG(info.st_mode) and info.st_uid == 0
            and stat.S_IMODE(info.st_mode) == 0o444 and info.st_nlink == 1)


def local_worker_context(environ=None, marker=Path('/run/oxidex-build-container'),
                         runner_marker=Path('/run/oxidex-spot-runner')):
    environ = os.environ if environ is None else environ
    if sys.platform != 'linux':
        return False
    if Path.cwd() == Path('/src') and trusted_marker(marker):
        return True
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
        marker = ('/run/oxidex-build-container' if Path.cwd() == Path('/src')
                  else '/run/oxidex-spot-runner')
        print(f'REMOTE_RECIPE_CONTEXT: root-owned 0444 marker {marker}',flush=True)
        return 0
    if not argv or not RECIPE.fullmatch(argv[0]):
        raise SystemExit('Invalid remote recipe name')
    recipe, *args = argv
    if any(not value or len(value) > 4096 or any(ch in value for ch in '\x00\n\r') for value in args):
        raise SystemExit('Invalid recipe argument')
    if local_worker_context():
        if recipe in ORACLE_TEST_RECIPES:
            from test_runner import prepare_generic_recipe_oracle
            prepare_generic_recipe_oracle()
        os.execvp('just', ['just', '_' + recipe + '-worker', *args])
        return 0  # execvp does not return outside synthetic controls
    command = [sys.executable, str(HERE/'build.py'), '--just-recipe', recipe]
    for value in args:
        command.append('--just-arg=' + value)
    os.execv(sys.executable, command)


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1:]))
