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
                 patch.object(remote_build.subprocess,'run',side_effect=run), \
                 patch.object(remote_build,'verify_remote_toolchain',return_value={'channel':'1.97.1'}):
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

    def test_failed_download_preserves_verified_binary(self):
        from lib.remote_build import download_artifact
        from unittest.mock import patch
        import hashlib
        with tempfile.TemporaryDirectory() as directory:
            artifact=Path(directory)/'oxidex';artifact.write_bytes(b'previous verified')
            def download(command, **kwargs):
                Path(command[4]).write_bytes(b'corrupt')
            with patch('lib.remote_build.subprocess.run',side_effect=download):
                with self.assertRaisesRegex(RuntimeError,'checksum'):
                    download_artifact('vm','z','p','/binary',artifact,hashlib.sha256(b'expected').hexdigest())
            self.assertEqual(artifact.read_bytes(),b'previous verified')

    def test_remote_toolchain_refuses_off_pin_before_build(self):
        from lib import remote_build
        from unittest.mock import patch
        pin='a'*40
        expected={'channel':'1.97.1','rustc_commit':pin,'cargo_version':'cargo 1.97.1 (abc)'}
        rustc=f"rustc 1.97.1\ncommit-hash: {pin}\nrelease: 1.97.1\n"
        cargo='rustc 1.97.1\ncargo 1.97.1 (abc)\ncargo 1.97.1 (abc)\n'
        commands=[]
        def ssh(command):
            commands.append(command)
            return [command]
        with patch.object(remote_build,'pinned_toolchain',return_value=expected), \
             patch.object(remote_build.subprocess,'check_output',side_effect=[rustc+rustc,cargo]):
            self.assertEqual(remote_build.verify_remote_toolchain(Path('.'),ssh, 'checkout'),expected)
        self.assertTrue(all('checkout' in command for command in commands))
        off_pin=rustc.replace(pin,'b'*40)
        with patch.object(remote_build,'pinned_toolchain',return_value=expected), \
             patch.object(remote_build.subprocess,'check_output',side_effect=[rustc+off_pin,cargo]):
            with self.assertRaisesRegex(RuntimeError,'does not match'):
                remote_build.verify_remote_toolchain(Path('.'),lambda command:[command], 'checkout')

    def test_prepare_race_is_retryable_without_building(self):
        from lib import remote_build
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            with patch.object(remote_build,'make_snapshot',return_value={'archive_sha256':'0'*64}), \
                 patch.object(remote_build.subprocess,'check_output',return_value='commit\n'), \
                 patch.object(remote_build.subprocess,'run',side_effect=subprocess.CalledProcessError(1,'prepare')) as run:
                with self.assertRaises(subprocess.CalledProcessError):
                    remote_build.main(['--source',str(root),'--instance','vm','--zone','z',
                        '--worktree-id','checkout','--evidence-dir',str(root/'evidence')])
            import json
            receipt=json.loads((root/'evidence'/'remote-build.json').read_text())
            self.assertEqual(receipt['stage'],'prepare')
            self.assertTrue(receipt['retryable'])
            self.assertEqual(run.call_count,1)
            self.assertIn('--ssh-flag=-oServerAliveInterval=15',run.call_args.args[0])
            self.assertIn('--ssh-flag=-oServerAliveCountMax=3',run.call_args.args[0])

    def test_scp_has_keepalives(self):
        from lib import remote_build
        from unittest.mock import patch
        import hashlib
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            def download(command, **kwargs):
                self.assertIn('--scp-flag=-oServerAliveInterval=15',command)
                self.assertIn('--scp-flag=-oServerAliveCountMax=3',command)
                Path(command[4]).write_bytes(b'binary')
            with patch.object(remote_build.subprocess,'run',side_effect=download):
                remote_build.download_artifact('vm','z','p','/binary',root/'oxidex',hashlib.sha256(b'binary').hexdigest())

    def test_toolchain_mismatch_stops_before_fetch(self):
        from lib import remote_build
        from unittest.mock import patch
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            with patch.object(remote_build,'make_snapshot',return_value={'archive_sha256':'0'*64}), \
                 patch.object(remote_build.subprocess,'check_output',return_value='commit\n'), \
                 patch.object(remote_build.subprocess,'run',return_value=SimpleNamespace(returncode=0)) as run, \
                 patch.object(remote_build,'verify_remote_toolchain',side_effect=RuntimeError('off pin')):
                with self.assertRaisesRegex(RuntimeError,'off pin'):
                    remote_build.main(['--source',str(root),'--instance','vm','--zone','z',
                        '--worktree-id','checkout','--evidence-dir',str(root/'evidence')])
            import json
            receipt=json.loads((root/'evidence'/'remote-build.json').read_text())
            self.assertEqual(receipt['stage'],'toolchain')
            self.assertNotIn('verified',receipt)
            self.assertEqual(run.call_count,3)  # prepare, upload, extract only
            self.assertFalse(any('cargo fetch' in value for call in run.call_args_list for value in call.args[0]))

    def test_sync_transport_retries_but_checksum_failure_stops(self):
        from lib.remote_build import retryable_failure
        upload=subprocess.CalledProcessError(1,'scp')
        disconnected=subprocess.CalledProcessError(1,'ssh',stderr='Connection closed')
        mismatch=subprocess.CalledProcessError(66,'ssh',stderr='OXIDEX_SOURCE_CHECKSUM_MISMATCH')
        self.assertTrue(retryable_failure(upload,'sync_upload'))
        self.assertTrue(retryable_failure(disconnected,'sync_extract'))
        self.assertFalse(retryable_failure(mismatch,'sync_extract'))
        self.assertFalse(retryable_failure(subprocess.CalledProcessError(101,'cargo'),'compile'))
