import tempfile
import unittest
import subprocess
import tarfile
from pathlib import Path
from lib.remote_build import make_snapshot


class ClientTests(unittest.TestCase):
    def test_snapshot_uses_working_files_without_caches_or_credentials(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)/'src';root.mkdir()
            subprocess.run(['git','init','-q',str(root)],check=True)
            for name, data in [('Cargo.toml','source'),('.env','secret'),('target/build','cache'),('.codex/config.toml','private')]:
                path=root/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_text(data)
            subprocess.run(['git','-C',str(root),'add','.'],check=True)
            (root/'Cargo.toml').write_text('working change')
            archive=Path(directory)/'source.tar.gz'
            receipt=make_snapshot(root,archive)
            with tarfile.open(archive) as tar:
                self.assertEqual(tar.getnames(), ['Cargo.toml'])
                self.assertEqual(tar.extractfile('Cargo.toml').read(), b'working change')
            self.assertEqual(receipt['file_count'],1)

    def test_failed_remote_header_validation_prevents_release_compilation(self):
        from lib import remote_build
        from unittest.mock import patch
        from types import SimpleNamespace
        calls=[]
        def run(command, **kwargs):
            calls.append(command)
            failed=any('just cbindgen-check' in value for value in command)
            return SimpleNamespace(returncode=1 if failed else 0)
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            with patch.object(remote_build,'make_snapshot',return_value={'archive_sha256':'0'*64}), \
                 patch.object(remote_build.subprocess,'check_output',return_value='commit\n'), \
                 patch.object(remote_build.subprocess,'run',side_effect=run):
                with self.assertRaisesRegex(RuntimeError,'header failed'):
                    remote_build.main(['--source',str(root),'--instance','vm','--zone','z',
                        '--worktree-id','checkout','--evidence-dir',str(root/'evidence')])
        self.assertFalse(any('cargo build' in value for command in calls for value in command))

    def test_worker_interruption_is_retryable_but_compiler_errors_are_not(self):
        from lib.remote_build import retryable_exit
        self.assertTrue(retryable_exit(255))
        self.assertTrue(retryable_exit(137))
        self.assertTrue(retryable_exit(143))
        self.assertFalse(retryable_exit(101))
        self.assertFalse(retryable_exit(1))
