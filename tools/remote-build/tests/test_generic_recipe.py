"""Bounded generic Just dispatch controls; all external processes are mocked."""
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import os

from lib import remote_build


class GenericRecipeTests(unittest.TestCase):
    def exercise(self, code, recipe="test-package"):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);commands=[]
            def snapshot(source,archive):
                archive.write_bytes(b'synthetic source')
                return {'archive_sha256':'0'*64}
            def run(command, **kwargs):
                commands.append(command)
                if 'stdout' in kwargs and hasattr(kwargs['stdout'],'write'):
                    kwargs['stdout'].write('synthetic recipe result\n')
                return SimpleNamespace(returncode=code if ' just ' in ' '.join(command) else 0)
            transport=SimpleNamespace(instance_id='2',host='192.0.2.1',ssh=lambda command:[command],
                                      scp=lambda local,remote,download=False:['scp',str(local),remote])
            def output(command, **kwargs):
                if 'sha256sum' in ' '.join(command):
                    return 'b'*64+'  /target/debug/oxidex\n'
                return 'a'*40+'\n'
            def download(instance,zone,project,binary,artifact,digest,**kwargs):
                artifact.write_bytes(b'verified synthetic binary')
            with patch.dict(os.environ,{'OXIDEX_REMOTE_SSH_KNOWN_HOSTS':'/synthetic/known'}), \
                 patch.object(remote_build.ssh_transport,'identity',return_value=('oxidex-uploader','/synthetic/key')), \
                 patch.object(remote_build.ssh_transport,'DirectTransport',return_value=transport), \
                 patch.object(remote_build,'verify_builder_admission',return_value={'admission_passed':True}), \
                 patch.object(remote_build,'pinned_toolchain',return_value={'channel':'1.99.0'}), \
                 patch.object(remote_build,'make_snapshot',side_effect=snapshot), \
                 patch.object(remote_build,'verify_remote_toolchain'), \
                 patch.object(remote_build.subprocess,'check_output',side_effect=output), \
                 patch.object(remote_build,'download_artifact',side_effect=download), \
                 patch.object(remote_build.subprocess,'run',side_effect=run):
                argv=['--source',str(root),'--instance','builder-vm','--zone','z',
                      '--instance-id','2','--worktree-id','checkout',
                      '--evidence-dir',str(root/'evidence'),'--just-recipe',recipe]
                if recipe=='test-package':
                    argv += ['--just-arg=package with spaces']
                if code:
                    with self.assertRaisesRegex(RuntimeError,'recipe failed'):
                        remote_build.main(argv)
                else:
                    self.assertEqual(remote_build.main(argv),0)
            receipt=json.loads((root/'evidence/remote-build.json').read_text())
            return receipt,commands,root

    def test_success_runs_exact_recipe_without_fetch_or_cleanup_and_retains_targets(self):
        receipt,commands,_=self.exercise(0)
        joined='\n'.join(map(str,commands))
        self.assertIn("just test-package 'package with spaces'",joined)
        self.assertNotIn('cargo fetch',joined)
        self.assertNotIn('cleanup',joined)
        self.assertEqual(receipt['stage'],'complete_retained')
        self.assertEqual(receipt['recipe_state'],'DIRECT_SUCCESS_RETAINED')
        self.assertTrue(receipt['remote_retained'])
        self.assertEqual(receipt['recipe_log_bytes'],24)
        self.assertIn('source',receipt['remote_paths'])
        self.assertIn('target',receipt['remote_paths'])

    def test_docs_build_receipt_names_retained_remote_rustdoc_directory(self):
        receipt, commands, _ = self.exercise(0, recipe='docs-build')
        self.assertEqual(receipt['rustdoc_path'],
                         receipt['remote_paths']['target'] + '/doc')
        self.assertEqual(receipt['recipe_state'], 'DIRECT_SUCCESS_RETAINED')
        self.assertFalse(any('cleanup' in ' '.join(command) for command in commands))

    def test_ssh_255_retains_unknown_and_never_retries_or_cleans(self):
        receipt,commands,_=self.exercise(255)
        self.assertEqual(receipt['recipe_state'],'UNKNOWN_RETAINED')
        self.assertEqual(receipt['recipe_exit_code'],255)
        self.assertFalse(receipt['retryable'])
        self.assertTrue(receipt['remote_retained'])
        self.assertEqual(sum(' just ' in ' '.join(c) for c in commands),1)
        self.assertFalse(any('cleanup' in ' '.join(c) for c in commands))

    def test_missing_explicit_uploader_refuses_before_provider_or_process(self):
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(remote_build.ssh_transport,'identity',return_value=None), \
             patch.object(remote_build.subprocess,'run') as run, \
             patch.object(remote_build.subprocess,'check_output') as output:
            root=Path(directory)
            with self.assertRaisesRegex(ValueError,'explicit uploader'):
                remote_build.main(['--source',str(root),'--instance','builder-vm','--zone','z',
                    '--instance-id','2','--worktree-id','checkout','--evidence-dir',str(root/'evidence'),
                    '--just-recipe','build'])
            run.assert_not_called();output.assert_not_called()

    def test_build_downloads_checked_linux_binary_and_retains_full_target(self):
        receipt,commands,root=self.exercise(0,'build')
        self.assertEqual(receipt['binary_sha256'],'b'*64)
        self.assertIn('/target/remote-linux/debug/',receipt['artifact'])
        self.assertTrue(receipt['remote_retained'])
        self.assertFalse(any('cleanup' in ' '.join(c) for c in commands))
