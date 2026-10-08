"""Focused signed-source controls for the infrastructure Python route."""
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from types import SimpleNamespace
import json
import os
from unittest.mock import patch

from lib import remote_build
import qualification_source


class InfraPythonProfileTests(unittest.TestCase):
    def test_recipe_gate(self):
        with tempfile.TemporaryDirectory() as folder:
            base = ['--source', folder, '--instance', 'builder-vm', '--zone', 'zone',
                    '--instance-id', '2', '--worktree-id', 'fixture',
                    '--evidence-dir', folder, '--just-recipe', 'infra-python-tests']
            for suffix in ([], ['--source-profile', 'infra-python-v1', '--just-arg', 'extra'],
                           ['--source-profile', 'infra-python-v1', '--artifact-dir', folder]):
                with self.subTest(suffix=suffix), self.assertRaises(SystemExit):
                    remote_build.main(base + suffix)

    def test_signed_snapshot_hidden_mutation_and_missing_required_file(self):
        if shutil.which('ssh-keygen') is None:
            self.skipTest('ssh-keygen unavailable')
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            source = root / 'source'
            source.mkdir()
            key = root / 'key'
            subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(key)], check=True)
            public = ' '.join((root / 'key.pub').read_text().split()[:2])
            fingerprint = subprocess.check_output(['ssh-keygen', '-lf', str(key) + '.pub'], text=True).split()[1]
            signers = root / 'signers'
            signers.write_text(qualification_source.PRINCIPAL + ' ' + public + '\n')
            def git(*args):
                return subprocess.check_output(['git', '-C', str(source), *args], text=True).strip()
            git('init', '-q')
            git('remote', 'add', 'origin', 'git@github.com:swack-tools/spot-github-runners.git')
            members = {name: b'signed\n' for name in remote_build.INFRA_PYTHON_REQUIRED}
            members['justfile'] = b'infra-python-tests:\n    echo signed\n'
            members['rust-toolchain.toml'] = b'[toolchain]\nchannel = "1.99.0"\n'
            for name, contents in members.items():
                path = source / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(contents)
            git('config', 'user.name', 'swackhamer')
            git('config', 'user.email', qualification_source.PRINCIPAL)
            git('config', 'gpg.format', 'ssh')
            git('config', 'user.signingkey', str(key))
            git('config', 'gpg.ssh.allowedSignersFile', str(signers))
            git('add', '.')
            git('commit', '-q', '-S', '-m', 'Signed infra source')
            head = git('rev-parse', 'HEAD')
            with patch.object(qualification_source, 'KEY', public), \
                 patch.object(qualification_source, 'FINGERPRINT', fingerprint):
                remote_build.verify_signed_source(source, head, signers)
                archive = root / 'snapshot.tar.gz'
                snapshot = remote_build.make_snapshot(source, archive, signed_head=head,
                    source_profile=remote_build.INFRA_PYTHON_PROFILE)
                self.assertEqual({row['path'] for row in snapshot['files']},
                                 remote_build.INFRA_PYTHON_REQUIRED)
                with tarfile.open(archive, 'r:gz') as stream:
                    self.assertEqual(stream.extractfile('justfile').read(), members['justfile'])
                actual_run = subprocess.run
                remote_commands = []
                transport = SimpleNamespace(instance_id='2', host='192.0.2.1',
                    ssh=lambda command: ['ssh', command],
                    scp=lambda local, remote, download=False: ['scp', str(local), remote])
                def fake_run(command, **kwargs):
                    if command[0] in ('git', 'ssh-keygen'):
                        return actual_run(command, **kwargs)
                    remote_commands.append(command)
                    if 'stdout' in kwargs and hasattr(kwargs['stdout'], 'write'):
                        kwargs['stdout'].write('synthetic recipe result\n')
                    return SimpleNamespace(returncode=0)
                evidence = root / 'evidence'
                with patch.dict(os.environ, {'OXIDEX_REMOTE_SSH_KNOWN_HOSTS': str(root / 'known')}), \
                     patch.object(remote_build.ssh_transport, 'identity', return_value=('uploader', 'key')), \
                     patch.object(remote_build.ssh_transport, 'DirectTransport', return_value=transport), \
                     patch.object(remote_build, 'verify_builder_admission', return_value={'admission_passed': True}), \
                     patch.object(remote_build, 'pinned_toolchain', return_value={'channel': '1.99.0', 'rustc_commit': 'a'*40}), \
                     patch.object(remote_build, 'verify_remote_toolchain'), \
                     patch.object(remote_build.subprocess, 'run', side_effect=fake_run):
                    remote_build.main(['--source', str(source), '--instance', 'builder-vm',
                        '--zone', 'zone', '--instance-id', '2', '--worktree-id', 'fixture',
                        '--evidence-dir', str(evidence), '--source-profile', 'infra-python-v1',
                        '--just-recipe', 'infra-python-tests'])
                receipt = json.loads((evidence / 'remote-build.json').read_text())
                self.assertEqual(receipt['source_commit'], head)
                self.assertEqual(receipt['source_repository'], 'swack-tools/spot-github-runners')
                self.assertEqual(receipt['source_tree'], git('rev-parse', 'HEAD^{tree}'))
                self.assertEqual(receipt['source_provenance'], 'signed_exact_head_infra_python_v1')
                self.assertEqual(receipt['validation_scope'], 'infra_python_unittest_only')
                self.assertTrue(receipt['remote_toolchain_verified'])
                self.assertEqual(receipt['recipe_exit_code'], 0)
                self.assertTrue(receipt['remote_command'].endswith('just infra-python-tests'))
                self.assertTrue(any('just infra-python-tests' in ' '.join(command) for command in remote_commands))
                git('remote', 'set-url', 'origin', 'git@github.com:someone/else.git')
                with self.assertRaisesRegex(RuntimeError, 'approved repository'):
                    with patch.dict(os.environ, {'OXIDEX_REMOTE_SSH_KNOWN_HOSTS': str(root / 'known')}), \
                         patch.object(remote_build.ssh_transport, 'identity', return_value=('uploader', 'key')), \
                         patch.object(remote_build.ssh_transport, 'DirectTransport', return_value=transport), \
                         patch.object(remote_build, 'verify_builder_admission', return_value={'admission_passed': True}), \
                         patch.object(remote_build, 'pinned_toolchain', return_value={'channel': '1.99.0'}):
                        remote_build.main(['--source', str(source), '--instance', 'builder-vm',
                            '--zone', 'zone', '--instance-id', '2', '--worktree-id', 'fixture',
                            '--evidence-dir', str(root / 'wrong-origin'), '--source-profile', 'infra-python-v1',
                            '--just-recipe', 'infra-python-tests'])
                git('remote', 'set-url', 'origin', 'git@github.com:swack-tools/spot-github-runners.git')
                git('update-index', '--skip-worktree', 'justfile')
                (source / 'justfile').write_text('infra-python-tests:\n    echo unsigned\n')
                self.assertEqual(git('status', '--porcelain', '--untracked-files=all'), '')
                with self.assertRaisesRegex(RuntimeError, 'differs from signed HEAD: justfile'):
                    remote_build.make_snapshot(source, archive, signed_head=head,
                        source_profile=remote_build.INFRA_PYTHON_PROFILE)
                (source / 'justfile').write_bytes(members['justfile'])
                git('update-index', '--no-skip-worktree', 'justfile')
                git('rm', '-q', 'tests/test_builder_c9_proof_repairs.py')
                git('commit', '-q', '-S', '-m', 'Remove required test')
                with self.assertRaisesRegex(RuntimeError, 'incomplete'):
                    remote_build.signed_snapshot_files(source, git('rev-parse', 'HEAD'),
                        source_profile=remote_build.INFRA_PYTHON_PROFILE)
                with self.assertRaises(Exception):
                    remote_build.verify_signed_source(source, head, root / 'missing-signers')
                git('commit', '--amend', '-q', '--no-gpg-sign', '-m', 'Unsigned infra source')
                with self.assertRaisesRegex(RuntimeError, 'signed maintainer HEAD'):
                    remote_build.verify_signed_source(source, git('rev-parse', 'HEAD'), signers)
