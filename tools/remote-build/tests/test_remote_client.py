import tempfile
import unittest
import subprocess
import tarfile
from pathlib import Path
from lib.remote_build import make_snapshot


class ClientTests(unittest.TestCase):
    def test_direct_approved_vm_cannot_bypass_busy_admission(self):
        import json
        from lib import remote_build
        from unittest.mock import patch
        approved={'name':'approved-bench','id':'2','project':'homelab-424523','zone':'z','enabled':True}
        inventory=[{'name':'approved-bench','id':'2','zone':'zones/z','status':'RUNNING'}]
        def output(command, **kwargs):
            return json.dumps({'id':'2','status':'RUNNING'} if 'describe' in command else inventory)
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(remote_build,'approved_instances',return_value=[approved]), \
             patch('lib.worker_selection.approved_instances',return_value=[approved]), \
             patch.object(remote_build.subprocess,'check_output',side_effect=output), \
             patch('lib.worker_selection.read_utilization',return_value=[(.9,.2)]), \
             patch.object(remote_build.subprocess,'run') as run:
            with self.assertRaisesRegex(RuntimeError,'No available'):
                remote_build.main(['--source',directory,'--instance','approved-bench','--zone','z',
                                   '--worktree-id','checkout','--evidence-dir',str(Path(directory)/'evidence')])
            run.assert_not_called()
            receipt=json.loads((Path(directory)/'evidence'/'remote-build.json').read_text())
            self.assertEqual(receipt['stage'],'admission')
            self.assertTrue(receipt['retryable'])

    def test_direct_exception_refuses_replacement_before_remote_mutation(self):
        import json
        from lib import remote_build
        from unittest.mock import patch
        approved={'name':'approved-bench','id':'2','project':'homelab-424523','zone':'z','enabled':True}
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(remote_build,'approved_instances',return_value=[approved]), \
             patch.object(remote_build.subprocess,'check_output',return_value=json.dumps({'id':'3','status':'RUNNING'})), \
             patch.object(remote_build.subprocess,'run') as run:
            with self.assertRaisesRegex(RuntimeError,'identity changed'):
                remote_build.main(['--source',directory,'--instance','approved-bench','--zone','z',
                                   '--worktree-id','checkout','--evidence-dir',str(Path(directory)/'evidence')])
            run.assert_not_called()

    def test_direct_build_rejects_runner_vm_before_any_remote_work(self):
        from lib import remote_build
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(remote_build.subprocess,'run') as run, \
             patch.object(remote_build.subprocess,'check_output') as check_output:
            with self.assertRaisesRegex(ValueError,'dedicated builder-'):
                remote_build.main(['--source',directory,'--instance','oxidex-runners-a',
                                   '--zone','z','--worktree-id','checkout',
                                   '--evidence-dir',str(Path(directory)/'evidence')])
            run.assert_not_called()
            check_output.assert_not_called()

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
            with patch.object(remote_build,'pinned_toolchain',return_value={'channel':'1.97.1'}), \
                 patch.object(remote_build,'make_snapshot',return_value={'archive_sha256':'0'*64}), \
                 patch.object(remote_build.subprocess,'check_output',return_value='commit\n'), \
                 patch.object(remote_build.subprocess,'run',side_effect=run), \
                 patch.object(remote_build,'verify_remote_toolchain',return_value={'channel':'1.97.1'}):
                with self.assertRaisesRegex(RuntimeError,'header failed'):
                    remote_build.main(['--source',str(root),'--instance','builder-vm','--zone','z',
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
        with patch.object(remote_build.subprocess,'check_output',side_effect=[rustc+rustc,cargo]):
            self.assertEqual(remote_build.verify_remote_toolchain(expected,ssh, 'checkout'),expected)
        self.assertTrue(all('checkout' in command for command in commands))
        off_pin=rustc.replace(pin,'b'*40)
        with patch.object(remote_build.subprocess,'check_output',side_effect=[rustc+off_pin,cargo]):
            with self.assertRaisesRegex(RuntimeError,'does not match'):
                remote_build.verify_remote_toolchain(expected,lambda command:[command], 'checkout')

    def test_prepare_race_is_retryable_without_building(self):
        from lib import remote_build
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            with patch.object(remote_build,'pinned_toolchain',return_value={'channel':'1.97.1'}), \
                 patch.object(remote_build,'make_snapshot',return_value={'archive_sha256':'0'*64}), \
                 patch.object(remote_build.subprocess,'check_output',return_value='commit\n'), \
                 patch.object(remote_build.subprocess,'run',side_effect=subprocess.CalledProcessError(1,'prepare')) as run:
                with self.assertRaises(subprocess.CalledProcessError):
                    remote_build.main(['--source',str(root),'--instance','builder-vm','--zone','z',
                        '--worktree-id','checkout','--evidence-dir',str(root/'evidence')])
            import json
            receipt=json.loads((root/'evidence'/'remote-build.json').read_text())
            self.assertEqual(receipt['stage'],'prepare')
            self.assertTrue(receipt['retryable'])
            self.assertEqual(receipt['worktree_namespace'],'checkout')
            self.assertEqual(len(receipt['run_id']),41)
            self.assertIn(receipt['run_id'], ' '.join(run.call_args_list[0].args[0]))
            self.assertEqual(run.call_count,2)  # failed prepare and best-effort cleanup
            self.assertIn('--ssh-flag=-oServerAliveInterval=15',run.call_args_list[0].args[0])
            self.assertIn('--ssh-flag=-oServerAliveCountMax=3',run.call_args_list[0].args[0])

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
            with patch.object(remote_build,'pinned_toolchain',return_value={'channel':'1.97.1'}), \
                 patch.object(remote_build,'make_snapshot',return_value={'archive_sha256':'0'*64}), \
                 patch.object(remote_build.subprocess,'check_output',return_value='commit\n'), \
                 patch.object(remote_build.subprocess,'run',return_value=SimpleNamespace(returncode=0)) as run, \
                 patch.object(remote_build,'verify_remote_toolchain',side_effect=RuntimeError('off pin')):
                with self.assertRaisesRegex(RuntimeError,'off pin'):
                    remote_build.main(['--source',str(root),'--instance','builder-vm','--zone','z',
                        '--worktree-id','checkout','--evidence-dir',str(root/'evidence')])
            import json
            receipt=json.loads((root/'evidence'/'remote-build.json').read_text())
            self.assertEqual(receipt['stage'],'toolchain')
            self.assertNotIn('verified',receipt)
            self.assertEqual(run.call_count,4)  # prepare, upload, extract, cleanup
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

    def test_sync_failure_keeps_remote_diagnostics(self):
        from lib import remote_build
        from unittest.mock import patch
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            failed=subprocess.CalledProcessError(66,'ssh',stderr='OXIDEX_SOURCE_CHECKSUM_MISMATCH\n')
            with patch.object(remote_build,'pinned_toolchain',return_value={'channel':'1.97.1'}), \
                 patch.object(remote_build,'make_snapshot',return_value={'archive_sha256':'0'*64}), \
                 patch.object(remote_build.subprocess,'check_output',return_value='commit\n'), \
                 patch.object(remote_build.subprocess,'run',side_effect=[SimpleNamespace(returncode=0),
                     SimpleNamespace(returncode=0),failed]):
                with self.assertRaises(subprocess.CalledProcessError):
                    remote_build.main(['--source',str(root),'--instance','builder-vm','--zone','z',
                        '--worktree-id','checkout','--evidence-dir',str(root/'evidence')])
            import json
            receipt=json.loads((root/'evidence'/'remote-build.json').read_text())
            self.assertFalse(receipt['retryable'])
            self.assertIn('sync-extract.log',receipt['error'])
            self.assertIn('OXIDEX_SOURCE_CHECKSUM_MISMATCH',
                          (root/'evidence'/'sync-extract.log').read_text())

    def test_new_snapshot_removes_deleted_source_files(self):
        from lib.remote_build import source_sync_command
        import hashlib
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            source=root/'remote source';source.mkdir()
            for index,files in enumerate(({'old.rs':b'old'}, {'new.rs':b'new'})):
                upload=f'source-{index}.tar.gz'
                archive=root/upload
                with tarfile.open(archive,'w:gz') as tar:
                    for name,data in files.items():
                        local=root/name;local.write_bytes(data)
                        tar.add(local,arcname=name)
                digest=hashlib.sha256(archive.read_bytes()).hexdigest()
                command=source_sync_command(digest,upload,str(source))
                subprocess.run(['sh','-c',command],cwd=root,check=True,capture_output=True)
                self.assertFalse(archive.exists())
            self.assertEqual([path.name for path in source.iterdir()],['new.rs'])
            archive.write_bytes(b'corrupt')
            command=source_sync_command('0'*64,upload,str(source))
            failed=subprocess.run(['sh','-c',command],cwd=root,capture_output=True,text=True)
            self.assertEqual(failed.returncode,66)
            self.assertIn('OXIDEX_SOURCE_CHECKSUM_MISMATCH',failed.stderr)
            self.assertEqual([path.name for path in source.iterdir()],['new.rs'])

    def test_post_build_transport_retries_but_digest_error_stops(self):
        from lib.remote_build import retryable_failure
        dropped=subprocess.CalledProcessError(1,'gcloud')
        for stage in ('verify','download'):
            self.assertTrue(retryable_failure(dropped,stage))
        self.assertFalse(retryable_failure(subprocess.CalledProcessError(65,'ssh',
            stderr='OXIDEX_SOURCE_ARCHIVE_INVALID'),'sync_extract'))

    def test_run_id_is_unique_across_clients_with_same_namespace(self):
        from lib import remote_build
        from unittest.mock import patch
        with patch.object(remote_build.secrets,'token_hex',side_effect=['a'*32,'b'*32]):
            first=remote_build.unique_run_id('checkout')
            second=remote_build.unique_run_id('checkout')
        self.assertNotEqual(first,second)
        self.assertEqual(len(first),41)
        self.assertTrue(first.startswith('checkout-'))
        self.assertEqual(len(remote_build.unique_run_id('x'*64)),64)

    def test_cleanup_is_scoped_to_generated_run_paths(self):
        from lib.remote_build import cleanup_command
        run_id='checkout-'+'a'*32
        command=cleanup_command(run_id)
        self.assertIn('/remote-build/sources/'+run_id,command)
        self.assertIn('/remote-build/targets/'+run_id,command)
        with self.assertRaisesRegex(ValueError,'invalid remote run identifier'):
            cleanup_command('../checkout')

    def test_missing_local_pin_stops_before_remote_work(self):
        from lib import remote_build
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            (root/'rust-toolchain.toml').write_text('[toolchain]\nchannel = "1.97.1"\n')
            with patch.object(remote_build.subprocess,'check_output',side_effect=subprocess.CalledProcessError(1,'rustup')), \
                 patch.object(remote_build.subprocess,'run') as run:
                with self.assertRaisesRegex(RuntimeError,'Local rustup cannot resolve repository toolchain pin'):
                    remote_build.main(['--source',str(root),'--instance','builder-vm','--zone','z',
                        '--worktree-id','checkout','--evidence-dir',str(root/'evidence')])
            import json
            receipt=json.loads((root/'evidence'/'remote-build.json').read_text())
            self.assertEqual(receipt['stage'],'local_toolchain')
            self.assertFalse(receipt.get('retryable',False))
            run.assert_not_called()

    def test_success_downloads_before_exact_run_cleanup(self):
        from lib import remote_build
        from unittest.mock import patch
        from types import SimpleNamespace
        import json
        events=[]
        digest='a'*64
        def output(command, **kwargs):
            if command[0]=='gcloud':
                return f'oxidex 2.0\n{digest}  /binary\n'
            return 'commit\n'
        def run(command, **kwargs):
            if 'sudo rm -rf' in ' '.join(command):
                events.append('cleanup')
            return SimpleNamespace(returncode=0)
        def download(instance,zone,project,binary,artifact,sha):
            events.append('download')
            artifact.write_bytes(b'verified binary')
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            with patch.object(remote_build,'pinned_toolchain',return_value={'channel':'1.97.1'}), \
                 patch.object(remote_build,'make_snapshot',side_effect=lambda source,archive: (archive.write_bytes(b'upload'),{'archive_sha256':'0'*64})[1]), \
                 patch.object(remote_build,'verify_remote_toolchain'), \
                 patch.object(remote_build,'download_artifact',side_effect=download), \
                 patch.object(remote_build.subprocess,'check_output',side_effect=output), \
                 patch.object(remote_build.subprocess,'run',side_effect=run), \
                 patch.object(remote_build.secrets,'token_hex',side_effect=['a'*32,'b'*32]):
                self.assertEqual(remote_build.main(['--source',str(root),'--instance','builder-vm','--zone','z',
                    '--worktree-id','checkout','--evidence-dir',str(root/'evidence')]),0)
                self.assertEqual(remote_build.main(['--source',str(root),'--instance','builder-vm','--zone','z',
                    '--worktree-id','checkout','--evidence-dir',str(root/'evidence2')]),0)
            receipt=json.loads((root/'evidence'/'remote-build.json').read_text())
            second=json.loads((root/'evidence2'/'remote-build.json').read_text())
            self.assertEqual(events,['download','cleanup','download','cleanup'])
            self.assertTrue(receipt['verified'])
            self.assertEqual(receipt['remote_cleanup'],'complete')
            self.assertFalse((root/'evidence'/'remote-source.tar.gz').exists())
            self.assertEqual(Path(receipt['artifact']).read_bytes(),b'verified binary')
            self.assertNotEqual(receipt['artifact'],second['artifact'])
            self.assertEqual(Path(second['artifact']).read_bytes(),b'verified binary')
            self.assertEqual(Path(receipt['artifact']),root.resolve()/'target'/'remote-linux'/'release'/receipt['run_id']/'oxidex')

    def test_cleanup_failure_after_verified_download_is_not_success(self):
        from lib import remote_build
        from unittest.mock import patch
        from types import SimpleNamespace
        import json
        events=[]
        digest='a'*64
        def output(command, **kwargs):
            if command[0]=='gcloud':
                return f'oxidex 2.0\n{digest}  /binary\n'
            return 'commit\n'
        def run(command, **kwargs):
            if 'sudo rm -rf' in ' '.join(command):
                raise subprocess.CalledProcessError(255,command)
            return SimpleNamespace(returncode=0)
        def download(instance,zone,project,binary,artifact,sha):
            events.append('download')
            artifact.write_bytes(b'verified binary')
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            with patch.object(remote_build,'pinned_toolchain',return_value={'channel':'1.97.1'}), \
                 patch.object(remote_build,'make_snapshot',return_value={'archive_sha256':'0'*64}), \
                 patch.object(remote_build,'verify_remote_toolchain'), \
                 patch.object(remote_build,'download_artifact',side_effect=download), \
                 patch.object(remote_build.subprocess,'check_output',side_effect=output), \
                 patch.object(remote_build.subprocess,'run',side_effect=run), \
                 patch.object(remote_build.secrets,'token_hex',side_effect=['a'*32,'b'*32]):
                with self.assertRaises(subprocess.CalledProcessError):
                    remote_build.main(['--source',str(root),'--instance','builder-vm','--zone','z',
                        '--worktree-id','checkout','--evidence-dir',str(root/'evidence')])
            receipt=json.loads((root/'evidence'/'remote-build.json').read_text())
            self.assertTrue(receipt['verified'])
            self.assertEqual(receipt['remote_cleanup'],'failed')
            self.assertFalse(receipt.get('retryable',False))
            self.assertEqual(Path(receipt['artifact']).read_bytes(),b'verified binary')

    def test_snapshot_manifest_uses_exact_archived_bytes(self):
        import hashlib
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);source=root/'src';source.mkdir()
            file=source/'code.rs';file.write_bytes(b'original')
            subprocess.run(['git','init','-q',str(source)],check=True)
            subprocess.run(['git','-C',str(source),'add','.'],check=True)
            original=Path.read_bytes
            def changing_read(path):
                data=original(path)
                if path==file:
                    file.write_bytes(b'edited during packaging')
                return data
            archive=root/'source.tar.gz'
            with patch.object(Path,'read_bytes',changing_read):
                receipt=make_snapshot(source,archive)
            with tarfile.open(archive) as tar:
                data=tar.extractfile('code.rs').read()
            self.assertEqual(receipt['files'][0]['sha256'],hashlib.sha256(data).hexdigest())
            self.assertEqual(receipt['files'][0]['bytes'],len(data))
