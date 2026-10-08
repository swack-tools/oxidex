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
    def test_relative_allowed_signers_cannot_switch_keys_between_caller_and_git(self):
        if shutil.which('ssh-keygen') is None:
            self.skipTest('ssh-keygen unavailable')
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder).resolve()
            caller = root / 'caller'
            source = root / 'source'
            caller.mkdir()
            source.mkdir()
            approved = root / 'approved'
            attacker = root / 'attacker'
            for key in (approved, attacker):
                subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(key)], check=True)
            approved_public = ' '.join(approved.with_suffix('.pub').read_text().split()[:2])
            attacker_public = ' '.join(attacker.with_suffix('.pub').read_text().split()[:2])
            fingerprint = subprocess.check_output(['ssh-keygen', '-lf', str(approved) + '.pub'], text=True).split()[1]
            (caller / 'signers').write_text(qualification_source.PRINCIPAL + ' ' + approved_public + '\n')
            (source / 'signers').write_text(qualification_source.PRINCIPAL + ' ' + attacker_public + '\n')
            def git(*args):
                return subprocess.check_output(['git', '-C', str(source), *args], text=True).strip()
            git('init', '-q')
            git('config', 'user.name', 'swackhamer')
            git('config', 'user.email', qualification_source.PRINCIPAL)
            git('config', 'gpg.format', 'ssh')
            git('config', 'user.signingkey', str(attacker))
            git('config', 'gpg.ssh.allowedSignersFile', 'signers')
            (source / 'payload').write_text('attacker-controlled\n')
            git('add', 'payload')
            git('commit', '-q', '-S', '-m', 'Attacker signed source')
            head = git('rev-parse', 'HEAD')
            original_cwd = Path.cwd()
            try:
                os.chdir(caller)
                with patch.object(qualification_source, 'KEY', approved_public), \
                     patch.object(qualification_source, 'FINGERPRINT', fingerprint):
                    with self.assertRaisesRegex(RuntimeError, 'absolute'):
                        remote_build.verify_signed_source(source, head, Path('signers'))
            finally:
                os.chdir(original_cwd)

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
                self.assertEqual(receipt['source_origin_configured'],
                                 'git@github.com:swack-tools/spot-github-runners.git')
                self.assertEqual(receipt['source_origin_check'], 'raw_local_config_allowlist')
                self.assertEqual(receipt['source_tree'], git('rev-parse', 'HEAD^{tree}'))
                self.assertEqual(receipt['source_provenance'], 'signed_exact_head_infra_python_v1')
                self.assertEqual(receipt['validation_scope'], 'infra_python_unittest_only')
                self.assertTrue(receipt['remote_toolchain_verified'])
                self.assertEqual(receipt['recipe_exit_code'], 0)
                self.assertTrue(receipt['remote_command'].endswith('just infra-python-tests'))
                self.assertTrue(any('just infra-python-tests' in ' '.join(command) for command in remote_commands))
                git('remote', 'set-url', 'origin', 'ssh://malicious.invalid/other')
                git('config', 'url.git@github.com:swack-tools/spot-github-runners.git.insteadOf',
                    'ssh://malicious.invalid/other')
                self.assertEqual(git('remote', 'get-url', 'origin'),
                                 'git@github.com:swack-tools/spot-github-runners.git')
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

    def test_unsigned_original_cannot_borrow_signed_replacement(self):
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
            def git(*args, env=None):
                return subprocess.check_output(['git', '-C', str(source), *args],
                    text=True, env=env).strip()
            git('init', '-q')
            git('remote', 'add', 'origin', 'git@github.com:swack-tools/spot-github-runners.git')
            git('config', 'user.name', 'swackhamer')
            git('config', 'user.email', qualification_source.PRINCIPAL)
            git('config', 'gpg.format', 'ssh')
            git('config', 'user.signingkey', str(key))
            git('config', 'gpg.ssh.allowedSignersFile', str(signers))
            for name in remote_build.INFRA_PYTHON_REQUIRED:
                path = source / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('payload=bad\n')
            git('add', '.')
            git('commit', '-q', '--no-gpg-sign', '-m', 'Unsigned original')
            unsigned = git('rev-parse', 'HEAD')
            (source / 'justfile').write_text('payload=good\n')
            git('add', 'justfile')
            git('commit', '-q', '-S', '-m', 'Signed replacement')
            signed = git('rev-parse', 'HEAD')
            git('reset', '--hard', unsigned)
            git('replace', unsigned, signed)
            git('read-tree', signed + '^{tree}')
            git('update-index', '--skip-worktree', 'justfile')
            self.assertEqual(git('status', '--porcelain', '--untracked-files=all'), '')
            self.assertEqual((source / 'justfile').read_text(), 'payload=bad\n')
            self.assertEqual(git('log', '-1', '--format=%G?', unsigned), 'G')
            self.assertNotEqual(git('log', '-1', '--format=%G?', unsigned,
                                    env=remote_build.source_git_env()), 'G')
            with patch.object(qualification_source, 'KEY', public), \
                 patch.object(qualification_source, 'FINGERPRINT', fingerprint):
                with self.assertRaisesRegex(RuntimeError, 'signed maintainer HEAD'):
                    remote_build.verify_signed_source(source, unsigned, signers)
                transport = SimpleNamespace(instance_id='2', host='192.0.2.1',
                    ssh=lambda command: ['ssh', command],
                    scp=lambda *args, **kwargs: self.fail('source transfer started'))
                with patch.dict(os.environ, {'OXIDEX_REMOTE_SSH_KNOWN_HOSTS': str(root / 'known')}), \
                     patch.object(remote_build.ssh_transport, 'identity', return_value=('uploader', 'key')), \
                     patch.object(remote_build.ssh_transport, 'DirectTransport', return_value=transport), \
                     patch.object(remote_build, 'verify_builder_admission', return_value={'admission_passed': True}), \
                     patch.object(remote_build, 'pinned_toolchain', return_value={'channel': '1.99.0'}), \
                     patch.object(remote_build, 'make_snapshot', side_effect=AssertionError('snapshot started')):
                    with self.assertRaisesRegex(RuntimeError, 'clean exact-HEAD|signed maintainer HEAD'):
                        remote_build.main(['--source', str(source), '--instance', 'builder-vm',
                            '--zone', 'zone', '--instance-id', '2', '--worktree-id', 'fixture',
                            '--evidence-dir', str(root / 'rejected'), '--source-profile', 'infra-python-v1',
                            '--just-recipe', 'infra-python-tests'])
                receipt = json.loads((root / 'rejected/remote-build.json').read_text())
                self.assertNotIn('source_provenance', receipt)
                self.assertEqual(receipt['source_commit'], unsigned)
                self.assertEqual(receipt['source_tree'], git('rev-parse', 'HEAD^{tree}',
                    env=remote_build.source_git_env()))
