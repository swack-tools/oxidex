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
import hashlib
from unittest.mock import patch

from lib import remote_build
from lib import infra_repository_binding
import qualification_source


class InfraPythonProfileTests(unittest.TestCase):
    def setUp(self):
        # No fixture may resolve a real credential or contact GitHub. Positives
        # replace only these two seams; real source/graph binding still executes.
        token = patch.object(infra_repository_binding, '_maintainer_token',
                             side_effect=AssertionError('unexpected credential lookup'))
        api = patch.object(infra_repository_binding, '_api',
                           side_effect=AssertionError('unexpected GitHub request'))
        token.start(); self.addCleanup(token.stop)
        api.start(); self.addCleanup(api.stop)

    def test_missing_promised_tree_cannot_run_local_transport_helper(self):
        with tempfile.TemporaryDirectory() as folder:
            base = Path(folder).resolve()
            source = base / 'source'
            source.mkdir()
            def git(*args):
                return subprocess.check_output(['/usr/bin/git', '-C', str(source), *args],
                    text=True, env=remote_build.source_git_env(), stderr=subprocess.PIPE).strip()
            git('init', '-q')
            git('config', 'user.name', 'Unsigned Fixture')
            git('config', 'user.email', 'fixture@example.invalid')
            (source / 'nested').mkdir()
            (source / 'nested/payload').write_text('unsigned fixture\n')
            git('add', '.')
            git('-c', 'commit.gpgsign=false', 'commit', '-qm', 'Unsigned source')
            head = git('rev-parse', 'HEAD')
            tree = git('rev-parse', 'HEAD:nested')
            marker = base / 'helper-ran'
            helper = base / 'local-helper'
            helper.write_text(f'#!/bin/sh\n/usr/bin/touch {marker}\nexit 1\n')
            helper.chmod(0o755)
            git('config', 'remote.origin.url', 'ext::' + str(helper))
            git('config', 'remote.origin.promisor', 'true')
            git('config', 'extensions.partialClone', 'origin')
            git('config', 'remote.origin.partialclonefilter', 'blob:none')
            git('config', 'protocol.ext.allow', 'always')
            (source / '.git/objects' / tree[:2] / tree[2:]).unlink()
            with self.assertRaises(subprocess.CalledProcessError):
                remote_build.source_clean_status(source, head)
            self.assertFalse(marker.exists(), 'pre-admission Git started local transport helper')
            transport = subprocess.run(['/usr/bin/git', '-C', str(source), 'ls-remote',
                'ext::' + str(helper)], env=remote_build.source_git_env(),
                stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            self.assertNotEqual(transport.returncode, 0)
            self.assertFalse(marker.exists(), 'source Git environment allowed explicit transport')

    def test_unsigned_filter_config_cannot_execute_before_infra_admission(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder).resolve()
            source = root / 'source'
            source.mkdir()
            key = root / 'key'
            subprocess.run(['/usr/bin/ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(key)], check=True)
            public = ' '.join((root / 'key.pub').read_text().split()[:2])
            fingerprint = subprocess.check_output(['/usr/bin/ssh-keygen', '-lf', str(key) + '.pub'],
                                                  text=True).split()[1]
            signers = root / 'signers'
            signers.write_text(qualification_source.PRINCIPAL + ' ' + public + '\n')
            def git(*args):
                return subprocess.check_output(['/usr/bin/git', '-C', str(source), *args],
                                               text=True, env=remote_build.source_git_env()).strip()
            git('init', '-q')
            git('config', 'user.name', 'swackhamer')
            git('config', 'user.email', qualification_source.PRINCIPAL)
            git('config', 'gpg.ssh.allowedSignersFile', str(signers))
            git('remote', 'add', 'origin', 'git@github.com:swack-tools/spot-github-runners.git')
            (source / '.gitattributes').write_text('payload filter=marker\n')
            payload = source / 'payload'
            payload.write_text('unsigned bytes\n')
            git('add', '.')
            git('-c', 'commit.gpgsign=false', 'commit', '-qm', 'Unsigned unadmitted source')
            head = git('rev-parse', 'HEAD')
            marker = root / 'filter-ran'
            driver = root / 'clean-filter'
            driver.write_text(f'#!/bin/sh\n/usr/bin/touch {marker}\n/bin/cat\n')
            driver.chmod(0o755)
            git('config', 'filter.marker.clean', str(driver))
            git('config', 'filter.marker.required', 'true')
            before = payload.stat()
            os.utime(payload, ns=(before.st_atime_ns, before.st_mtime_ns + 2_000_000_000))
            self.assertEqual(remote_build.source_clean_status(source, head), '')
            self.assertFalse(marker.exists(), 'clean filter ran during source inspection')
            transport = SimpleNamespace(instance_id='2', host='192.0.2.1',
                ssh=lambda command: ['ssh', command],
                scp=lambda *args, **kwargs: self.fail('unsigned source transfer started'))
            with patch.dict(os.environ, {'OXIDEX_REMOTE_SSH_KNOWN_HOSTS': str(root / 'known')}), \
                 patch.object(qualification_source, 'KEY', public), \
                 patch.object(qualification_source, 'FINGERPRINT', fingerprint), \
                 patch.object(remote_build.ssh_transport, 'identity', return_value=('uploader', 'key')), \
                 patch.object(remote_build.ssh_transport, 'DirectTransport', return_value=transport), \
                 patch.object(remote_build, 'verify_builder_admission', return_value={'admission_passed': True}), \
                 patch.object(remote_build, 'pinned_toolchain', return_value={'channel': '1.99.0'}), \
                 patch.object(remote_build, 'make_snapshot', side_effect=AssertionError('snapshot started')):
                with self.assertRaisesRegex(RuntimeError, 'signed maintainer HEAD'):
                    remote_build.main(['--source', str(source), '--instance', 'builder-vm',
                        '--zone', 'zone', '--instance-id', '2', '--worktree-id', 'fixture',
                        '--evidence-dir', str(root / 'evidence'), '--source-profile', 'infra-python-v1',
                        '--just-recipe', 'infra-python-tests'])
            self.assertFalse(marker.exists(), 'unsigned source filter ran before rejection')
            receipt = json.loads((root / 'evidence/remote-build.json').read_text())
            self.assertEqual(receipt['source_commit'], head)
            # Rejected unsigned external source has not had working files read.
            self.assertIsNone(receipt['source_status'])
            self.assertNotIn('source_provenance', receipt)
            payload.write_text('changed bytes\n')
            self.assertIn('payload', remote_build.source_clean_status(source, head))
            self.assertFalse(marker.exists(), 'clean filter ran during dirty-source refusal')
            sibling = root / 'sibling'
            sibling.mkdir()
            git('config', 'core.worktree', str(sibling))
            with self.assertRaisesRegex(RuntimeError, 'worktree differs'):
                remote_build.source_clean_status(source, head)
            self.assertFalse(marker.exists())

    def test_attacker_git_executable_config_does_not_run_during_source_admission(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder).resolve()
            source = root / 'source'
            source.mkdir()
            key = root / 'key'
            subprocess.run(['/usr/bin/ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(key)], check=True)
            public = ' '.join((root / 'key.pub').read_text().split()[:2])
            fingerprint = subprocess.check_output(['/usr/bin/ssh-keygen', '-lf', str(key) + '.pub'], text=True).split()[1]
            signers = root / 'signers'
            signers.write_text(qualification_source.PRINCIPAL + ' ' + public + '\n')
            def git(*args, env=None):
                return subprocess.check_output(['/usr/bin/git', '-C', str(source), *args],
                                               text=True, env=env).strip()
            git('init', '-q')
            git('config', 'user.name', 'swackhamer')
            git('config', 'user.email', qualification_source.PRINCIPAL)
            git('config', 'gpg.format', 'ssh')
            git('config', 'user.signingkey', str(key))
            git('config', 'gpg.ssh.allowedSignersFile', str(signers))
            (source / 'payload').write_text('signed\n')
            git('add', 'payload')
            git('commit', '-q', '-S', '-m', 'Signed source')
            head = git('rev-parse', 'HEAD')
            fsmonitor = root / 'fsmonitor.sh'
            fsmonitor_marker = root / 'fsmonitor-ran'
            fsmonitor.write_text(f'#!/bin/sh\ntouch {fsmonitor_marker}\n')
            fsmonitor.chmod(0o755)
            verifier = root / 'verifier.sh'
            verifier_marker = root / 'verifier-ran'
            verifier.write_text(f'#!/bin/sh\ntouch {verifier_marker}\nexec /usr/bin/ssh-keygen "$@"\n')
            verifier.chmod(0o755)
            git('config', 'core.fsmonitor', str(fsmonitor))
            git('config', 'gpg.ssh.program', str(verifier))
            fakebin = root / 'fakebin'
            fakebin.mkdir()
            path_markers = []
            for name in ('git', 'ssh-keygen'):
                marker = root / f'path-{name}-ran'
                path_markers.append(marker)
                wrapper = fakebin / name
                wrapper.write_text(f'#!/bin/sh\n/usr/bin/touch {marker}\nexec /usr/bin/{name} "$@"\n')
                wrapper.chmod(0o755)
            with patch.dict(os.environ, {'PATH': str(fakebin) + ':/usr/bin:/bin'}), \
                 patch.object(qualification_source, 'KEY', public), \
                 patch.object(qualification_source, 'FINGERPRINT', fingerprint):
                self.assertEqual(remote_build.source_clean_status(source, head), '')
                remote_build.verify_signed_source(source, head, signers)
            self.assertFalse(fsmonitor_marker.exists(), 'repository fsmonitor executed')
            self.assertFalse(verifier_marker.exists(), 'repository SSH verifier executed')
            self.assertTrue(all(not marker.exists() for marker in path_markers),
                            'ambient PATH supplied source admission tools')

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
            members['justfile'] = b"infra-python-tests:\n    echo 'Ran 854 tests'\n    echo OK\n"
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
                with patch.object(remote_build, 'INFRA_PYTHON_MIN_MODULES', 1):
                    manifest, manifest_raw = remote_build.infra_python_manifest(source, head)
                self.assertEqual(manifest['modules'], [{'path':'tests/test_builder_c9_proof_repairs.py',
                    'sha256':hashlib.sha256(members['tests/test_builder_c9_proof_repairs.py']).hexdigest(),
                    'module':'test_builder_c9_proof_repairs'}])
                self.assertEqual(hashlib.sha256(manifest_raw).hexdigest(),
                    hashlib.sha256(json.dumps(manifest,sort_keys=True,separators=(',',':')).encode()).hexdigest())
                actual_run = subprocess.run
                remote_commands = []
                transport = SimpleNamespace(instance_id='2', host='192.0.2.1',
                    ssh=lambda command: ['ssh', command],
                    scp=lambda local, remote, download=False: ['scp', str(local), remote])
                def fake_run(command, **kwargs):
                    if command[0] in ('git', '/usr/bin/git', '/usr/bin/ssh-keygen'):
                        return actual_run(command, **kwargs)
                    remote_commands.append(command)
                    if 'stdout' in kwargs and hasattr(kwargs['stdout'], 'write'):
                        kwargs['stdout'].write('Ran 854 tests in 1.0s\n\nOK\n')
                    return SimpleNamespace(returncode=0)
                evidence = root / 'evidence'
                def github_response(endpoint, token):
                    return json.dumps({
                        '/user': {'login':'swackhamer'},
                        infra_repository_binding.REPO_ENDPOINT: {
                            'id':1397480856, 'full_name':infra_repository_binding.REPOSITORY,
                            'default_branch':'main', 'fork':False, 'archived':False},
                        infra_repository_binding.REF_ENDPOINT: {
                            'ref':'refs/heads/main', 'object':{'type':'commit','sha':head}},
                    }[endpoint]).encode()
                with patch.object(infra_repository_binding, '_maintainer_token', return_value='fixture'), \
                     patch.object(infra_repository_binding, '_api', side_effect=github_response), \
                     patch.dict(os.environ, {'OXIDEX_REMOTE_SSH_KNOWN_HOSTS': str(root / 'known')}), \
                     patch.object(remote_build.ssh_transport, 'identity', return_value=('uploader', 'key')), \
                     patch.object(remote_build.ssh_transport, 'DirectTransport', return_value=transport), \
                     patch.object(remote_build, 'verify_builder_admission', return_value={'admission_passed': True}), \
                     patch.object(remote_build, 'pinned_toolchain', return_value={'channel': '1.99.0', 'rustc_commit': 'a'*40}), \
                     patch.object(remote_build, 'verify_remote_toolchain'), \
                     patch.object(remote_build.subprocess, 'run', side_effect=fake_run), \
                     patch.object(remote_build, 'INFRA_PYTHON_MIN_MODULES', 1):
                    with self.assertRaises(Exception):
                        remote_build.main(['--source', str(source), '--instance', 'builder-vm',
                        '--zone', 'zone', '--instance-id', '2', '--worktree-id', 'fixture',
                        '--evidence-dir', str(evidence), '--source-profile', 'infra-python-v1',
                        '--just-recipe', 'infra-python-tests'])
                receipt = json.loads((evidence / 'remote-build.json').read_text())
                self.assertEqual(receipt['source_commit'], head)
                self.assertEqual(receipt['infra_repository_binding']['witness']['path'], [head])
                self.assertEqual(receipt['infra_repository_binding']['packet']['bundle_sha256'],
                                 receipt['fleet_source_bundle_sha256'])
                self.assertEqual(receipt['infra_repository_binding']['packet']['manifest_sha256'],
                                 receipt['infra_python_manifest_sha256'])
                self.assertEqual(receipt['infra_repository_binding']['packet']['run_id'],receipt['run_id'])
                self.assertEqual(receipt['source_origin_configured'],
                                 'git@github.com:swack-tools/spot-github-runners.git')
                self.assertEqual(receipt['source_origin_check'], 'raw_local_config_allowlist')
                self.assertEqual(receipt['source_tree'], git('rev-parse', 'HEAD^{tree}'))
                self.assertEqual(receipt['source_provenance'], 'signed_exact_head_infra_python_v1')
                self.assertNotIn('validation_scope', receipt)
                self.assertEqual(receipt['infra_python_manifest_sha256'],hashlib.sha256(manifest_raw).hexdigest())
                self.assertIn('infra-python-test-manifest.json',
                    {row['path'] for row in receipt['snapshot']['files']})
                self.assertNotIn('verified', receipt)
                self.assertEqual(receipt['infra_python_proof_state'], 'PENDING')
                self.assertTrue(receipt['remote_retained'])
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
                self.assertIn('justfile', remote_build.source_clean_status(source, head))
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


class InfraPythonProofControls(unittest.TestCase):
    def fixture(self):
        head = 'a'*40
        manifest = {'schema':1, 'kind':'infra_python_test_manifest_v1',
                    'source_commit':head,
                    'modules':[{'path':'tests/test_one.py','sha256':'b'*64,'module':'test_one'},
                               {'path':'tests/test_two.py','sha256':'c'*64,'module':'test_two'}]}
        raw = json.dumps(manifest,sort_keys=True,separators=(',',':')).encode()
        manifest_hash = hashlib.sha256(raw).hexdigest()
        log = b'two actual unittest cases\n'
        proof = {'schema':1,'kind':'infra_python_unittest_v1','source_commit':head,
                 'manifest_sha256':manifest_hash,
                 'runner_argv':remote_build.INFRA_PYTHON_RUNNER_ARGV,
                 'unittest_argv':remote_build.INFRA_PYTHON_UNITTEST_ARGV,
                 'discovery':remote_build.INFRA_PYTHON_DISCOVERY,
                 'python_executable':'/opt/build-tools/python3','discovered_tests':2,
                 'tests_run':2,'module_counts':{'test_one':1,'test_two':1},
                 'failures':0,'errors':0,'skipped':0,'expected_failures':0,
                 'unexpected_successes':0,'success':True,'exit_code':0,'status':'PASS',
                 'diagnostic_log_sha256':hashlib.sha256(log).hexdigest()}
        return head,manifest,manifest_hash,log,proof

    def test_strict_result_refuses_noop_partial_wrong_head_and_forgery(self):
        head,manifest,manifest_hash,log,proof = self.fixture()
        def check(value):
            return remote_build.validate_infra_python_proof(
                json.dumps(value).encode(),manifest,head,manifest_hash,
                hashlib.sha256(log).hexdigest())
        with patch.object(remote_build,'INFRA_PYTHON_MIN_TESTS',2), \
             patch.object(remote_build,'INFRA_PYTHON_MIN_MODULES',2):
            self.assertEqual(check(proof)['tests_run'],2)
            mutations=[{'source_commit':'d'*40},{'manifest_sha256':'e'*64},
                       {'runner_argv':['echo','signed']},{'unittest_argv':['echo','Ran 854 tests']},
                       {'discovery':{'start_dir':'tests','pattern':'other*.py','top_level_dir':None}},
                       {'discovered_tests':0,'tests_run':0,'module_counts':{}},
                       {'discovered_tests':1},{'module_counts':{'test_one':2}},
                       {'module_counts':{'test_one':1,'test_two':True}},
                       {'failures':1},{'skipped':1},{'errors':1},
                       {'expected_failures':1},{'unexpected_successes':1},
                       {'success':False},{'exit_code':1},{'status':'FAIL'},
                       {'diagnostic_log_sha256':'0'*64}]
            for changes in mutations:
                with self.subTest(changes=changes),self.assertRaises(RuntimeError):
                    check({**proof,**changes})
            for raw in (b'{}',b'{"schema":1,"schema":1}',b'{"schema":NaN}',
                        b' '* (remote_build.INFRA_PYTHON_PROOF_LIMIT+1)):
                with self.subTest(raw=raw[:30]),self.assertRaises(RuntimeError):
                    remote_build.validate_infra_python_proof(raw,manifest,head,
                        manifest_hash,hashlib.sha256(log).hexdigest())
        with self.assertRaises(RuntimeError):
            check(proof)  # Production 854/82 floors never relax through the CLI.

    def test_fixed_remote_artifacts_are_hashed_and_actual_log_downloaded(self):
        head,manifest,manifest_hash,log,proof = self.fixture()
        proof_raw=json.dumps(proof,sort_keys=True).encode()
        run_id='infra-'+'a'*32
        base='/mnt/runner-data/remote-build/targets/'+run_id+'/'
        remote={base+'infra-python-test-proof.json':proof_raw,
                base+'infra-python-test.log':log}
        def ssh(command):return ['ssh',command]
        def scp(local,path,download=False):
            self.assertTrue(download)
            return ['scp',path,str(local)]
        transport=SimpleNamespace(scp=scp)
        def checksum(command,**kwargs):
            path=command[1].removeprefix('sha256sum ')
            return hashlib.sha256(remote[path]).hexdigest()+'  '+path+'\n'
        def copy(command,**kwargs):
            Path(command[2]).write_bytes(remote[command[1]])
            return SimpleNamespace(returncode=0)
        with tempfile.TemporaryDirectory() as folder, \
             patch.object(remote_build,'INFRA_PYTHON_MIN_TESTS',2), \
             patch.object(remote_build,'INFRA_PYTHON_MIN_MODULES',2), \
             patch.object(remote_build.subprocess,'check_output',side_effect=checksum), \
             patch.object(remote_build.subprocess,'run',side_effect=copy):
            root=Path(folder)
            summary, artifacts=remote_build.retrieve_infra_python_proof(
                transport,ssh,'builder','zone','project',run_id,root,head,manifest,manifest_hash)
            self.assertEqual(summary['module_count'],2)
            self.assertEqual(artifacts['infra-python-test.log']['sha256'],hashlib.sha256(log).hexdigest())
            self.assertEqual((root/'infra-python-test.log').read_bytes(),log)
            remote[base+'infra-python-test.log']=b'forged diagnostic log\n'
            rejected=root/'rejected';rejected.mkdir()
            with self.assertRaisesRegex(RuntimeError,'identity or result differs'):
                remote_build.retrieve_infra_python_proof(transport,ssh,'builder','zone','project',
                    run_id,rejected,head,manifest,manifest_hash)
            # The remote stat/hash can precede a changed SCP payload; local bytes
            # must still match the previously observed digest.
            transfer=root/'changed-transfer';transfer.mkdir()
            remote[base+'infra-python-test.log']=log
            with patch.object(remote_build.subprocess,'check_output',return_value=(
                    hashlib.sha256(proof_raw).hexdigest()+'  '+base+'infra-python-test-proof.json\n')):
                remote[base+'infra-python-test-proof.json']=proof_raw+b'changed'
                with self.assertRaisesRegex(RuntimeError,'checksum differs'):
                    remote_build.retrieve_infra_python_proof(transport,ssh,'builder','zone','project',
                        run_id,transfer,head,manifest,manifest_hash)
