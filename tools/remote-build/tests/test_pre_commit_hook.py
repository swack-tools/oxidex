"""Exercise the hook in a normal checkout without the maintainer's Cargo config."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


REPO = Path(__file__).resolve().parents[3]


class HookTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True)
        (self.root / '.githooks').mkdir()
        shutil.copy2(REPO / '.githooks/pre-commit', self.root / '.githooks/pre-commit')
        lib = self.root / 'tools/remote-build/lib'
        lib.parent.mkdir(parents=True)
        lib.symlink_to(REPO / 'tools/remote-build/lib', target_is_directory=True)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.just = self.bin / 'just'
        self.just.write_text('#!/bin/sh\necho invoked > just-invoked\nexit 41\n')
        self.just.chmod(0o755)
        cargo = self.root / 'empty-cargo'
        cargo.mkdir()
        self.env = {k: v for k, v in os.environ.items()
                    if not k.startswith('OXIDEX_REMOTE_')}
        self.env.update(PATH=str(self.bin) + os.pathsep + os.environ['PATH'],
                        CARGO_HOME=str(cargo), GIT_CONFIG_GLOBAL='/dev/null',
                        HOME=str(self.root))

    def hook(self):
        return subprocess.run(['sh', '.githooks/pre-commit'], cwd=self.root,
                              env=self.env, text=True, capture_output=True)

    def test_unconfigured_normal_checkout_warns_and_skips(self):
        result = self.hook()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('skipping pre-commit checks', result.stderr)
        self.assertFalse((self.root / 'just-invoked').exists())

    def test_configured_route_failure_still_blocks(self):
        key = self.root / 'key'
        known = self.root / 'known_hosts'
        key.write_text('synthetic')
        known.write_text('synthetic')
        self.env.update(OXIDEX_REMOTE_SSH_USER='oxidex-uploader',
                        OXIDEX_REMOTE_SSH_KEY=str(key),
                        OXIDEX_REMOTE_SSH_KNOWN_HOSTS=str(known))
        result = self.hook()
        self.assertEqual(result.returncode, 41, result.stderr)
        self.assertTrue((self.root / 'just-invoked').exists())

    def test_partial_remote_configuration_fails_closed(self):
        self.env['OXIDEX_REMOTE_SSH_USER'] = 'oxidex-uploader'
        result = self.hook()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / 'just-invoked').exists())

    def test_chained_hook_runs_even_when_own_checks_are_skipped(self):
        native = self.root / '.git/hooks/pre-commit'
        native.write_text('#!/bin/sh\necho chained > chained-ran\n')
        native.chmod(0o755)
        self.env['OXIDEX_SKIP_HOOKS'] = '1'
        result = self.hook()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.root / 'chained-ran').exists())


if __name__ == '__main__':
    unittest.main()
