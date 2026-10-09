"""Actual Git local-config admission controls before unsigned source reads."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from lib import remote_build


class LocalConfigAdmissionTests(unittest.TestCase):
    def test_fifo_include_refuses_before_git_reads(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'source'
            subprocess.run(['/usr/bin/git', 'init', '-q', str(source)], check=True)
            (source / 'tracked').write_text('committed\n')
            subprocess.run(['/usr/bin/git', '-C', str(source), 'add', 'tracked'], check=True)
            subprocess.run(['/usr/bin/git', '-C', str(source), '-c', 'user.name=Test',
                            '-c', 'user.email=test@example.invalid', 'commit', '-qm', 'base'], check=True)
            head = subprocess.check_output(['/usr/bin/git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
            self.assertEqual(remote_build.source_clean_status(source, head), '')
            fifo = root / 'never-opened'
            os.mkfifo(fifo)
            with (source / '.git/config').open('a') as config:
                config.write(f'\n[include]\n\tpath = {fifo}\n')
            command = [sys.executable, '-B', '-c',
                       'from pathlib import Path; from lib import remote_build; '
                       f'remote_build.source_clean_status(Path({str(source)!r}), {head!r})']
            result = subprocess.run(command, cwd=Path(__file__).resolve().parents[1],
                                    capture_output=True, text=True, timeout=7)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Local Git configuration', result.stderr)


    def test_config_file_fifo_and_symlink_refuse_without_opening_target(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'source'
            subprocess.run(['/usr/bin/git', 'init', '-q', str(source)], check=True)
            config = source / '.git/config'
            backup = root / 'config-backup'
            config.rename(backup)
            os.mkfifo(config)
            with self.assertRaisesRegex(RuntimeError, 'bounded regular file'):
                remote_build._local_config_preflight(source)
            config.unlink()
            config.symlink_to(backup)
            with self.assertRaisesRegex(RuntimeError, 'bounded regular file'):
                remote_build._local_config_preflight(source)

    def test_replaced_include_has_finite_git_deadline(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'source'
            subprocess.run(['/usr/bin/git', 'init', '-q', str(source)], check=True)
            fifo = root / 'later-include'
            os.mkfifo(fifo)
            original = subprocess.check_output
            def replace_before_git(command, **kwargs):
                with (source / '.git/config').open('a') as config:
                    config.write(f'\n[includeIf "gitdir:*"]\n\tpath = {fifo}\n')
                return original(command, **kwargs)
            with patch.object(remote_build.subprocess, 'check_output', side_effect=replace_before_git):
                with self.assertRaises(subprocess.TimeoutExpired):
                    remote_build._git_check_output(
                        ['/usr/bin/git', '-C', str(source), 'rev-parse', 'HEAD'],
                        env=remote_build.source_git_env(), timeout=0.3)
            with self.assertRaisesRegex(RuntimeError, 'Local Git configuration'):
                remote_build._local_config_preflight(source)

    def test_linked_worktree_and_nonincluded_config_remain_usable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'source'
            linked = root / 'linked'
            subprocess.run(['/usr/bin/git', 'init', '-q', str(source)], check=True)
            (source / 'tracked').write_text('committed\n')
            subprocess.run(['/usr/bin/git', '-C', str(source), 'add', 'tracked'], check=True)
            subprocess.run(['/usr/bin/git', '-C', str(source), '-c', 'user.name=Test',
                            '-c', 'user.email=test@example.invalid', 'commit', '-qm', 'base'], check=True)
            subprocess.run(['/usr/bin/git', '-C', str(source), 'worktree', 'add', '-q',
                            '-b', 'linked', str(linked)], check=True)
            head = subprocess.check_output(['/usr/bin/git', '-C', str(linked), 'rev-parse', 'HEAD'], text=True).strip()
            self.assertEqual(remote_build.source_clean_status(linked, head), '')
            self.assertEqual(remote_build._git_check_output(
                ['/usr/bin/git', '-C', str(linked), 'rev-parse', 'HEAD'],
                env=remote_build.source_git_env(), text=True).strip(), head)
            marker = (linked / '.git').read_text().split(': ', 1)[1].strip()
            gitdir = (linked / marker).resolve()
            (gitdir / 'config.worktree').write_text('[include]\npath = /dev/null\n')
            with self.assertRaisesRegex(RuntimeError, 'Local Git configuration'):
                remote_build.source_clean_status(linked, head)



if __name__ == '__main__':
    unittest.main()
