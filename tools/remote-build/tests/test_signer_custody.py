"""Real Git controls for the signer bytes used at source admission."""
import hashlib
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

from lib import remote_build
import qualification_source
from scripts import ops_paths


class SignerCustodyTests(unittest.TestCase):
    def test_swap_after_approved_key_inspection_cannot_admit_attacker_commit(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            source = root / 'source'
            source.mkdir()
            approved, attacker = root / 'approved', root / 'attacker'
            for key in (approved, attacker):
                subprocess.run(['/usr/bin/ssh-keygen', '-q', '-t', 'ed25519', '-N', '',
                                '-f', str(key)], check=True)
            public = ' '.join(approved.with_suffix('.pub').read_text().split()[:2])
            evil_public = ' '.join(attacker.with_suffix('.pub').read_text().split()[:2])
            fingerprint = subprocess.check_output(
                ['/usr/bin/ssh-keygen', '-lf', str(approved) + '.pub'], text=True).split()[1]
            signers = root / 'signers'
            good_bytes = (qualification_source.PRINCIPAL + ' ' + public + '\n').encode()
            evil_bytes = (qualification_source.PRINCIPAL + ' ' + evil_public + '\n').encode()
            signers.write_bytes(good_bytes)
            def git(*args):
                return subprocess.check_output(['/usr/bin/git', '-C', str(source), *args],
                                               text=True).strip()
            git('init', '-q')
            git('config', 'user.name', 'swackhamer')
            git('config', 'user.email', qualification_source.PRINCIPAL)
            git('config', 'gpg.format', 'ssh')
            git('config', 'user.signingkey', str(attacker))
            git('config', 'gpg.ssh.allowedSignersFile', str(signers))
            (source / 'payload').write_text('signed source\n')
            git('add', 'payload')
            git('commit', '-q', '-S', '-m', 'Attacker signed')
            head = git('rev-parse', 'HEAD')
            real_check = qualification_source._trusted_key
            inspected = []
            def swap_after_check(path):
                real_check(path)
                inspected.append(Path(path))
                signers.write_bytes(evil_bytes)
            with patch.object(ops_paths, 'ops_root', return_value=root / 'ops'), \
                 patch.object(qualification_source, 'KEY', public), \
                 patch.object(qualification_source, 'FINGERPRINT', fingerprint), \
                 patch.object(qualification_source, '_trusted_key', side_effect=swap_after_check):
                with self.assertRaisesRegex(RuntimeError, 'signed maintainer HEAD'):
                    remote_build.verify_signed_source(source, head, signers)
            self.assertEqual(signers.read_bytes(), evil_bytes)
            self.assertEqual(len(inspected), 1)
            self.assertNotEqual(inspected[0], signers)

    def test_frozen_copy_is_the_packet_member_despite_original_swap(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            evidence = root / 'evidence'
            evidence.mkdir(mode=0o700)
            original = root / 'signers'
            approved = qualification_source.PRINCIPAL.encode() + b' ssh-ed25519 approved\n'
            original.write_bytes(approved)
            frozen, digest = remote_build.freeze_signer(original, evidence)
            original.write_bytes(b'attacker replacement\n')
            remote_build.assert_frozen_signer(frozen, digest)
            self.assertEqual(frozen.read_bytes(), approved)
            self.assertEqual(digest, hashlib.sha256(approved).hexdigest())
            source = root / 'source'
            source.mkdir()
            (source / 'payload').write_text('source\n')
            subprocess.run(['/usr/bin/git', '-C', str(source), 'init', '-q'], check=True)
            archive = evidence / 'snapshot.tar.gz'
            snapshot = remote_build.make_snapshot(source, archive,
                                                   extra_files={'maintainer.allowed_signers': frozen})
            with tarfile.open(archive, 'r:gz') as packet:
                self.assertEqual(packet.extractfile('maintainer.allowed_signers').read(), approved)
            self.assertEqual(next(row['sha256'] for row in snapshot['files']
                                  if row['path'] == 'maintainer.allowed_signers'), digest)
            frozen.chmod(0o600)
            frozen.write_bytes(b'changed\n')
            with self.assertRaisesRegex(RuntimeError, 'custody changed'):
                remote_build.assert_frozen_signer(frozen, digest)

    def test_short_read_and_symlink_refuse_before_custody(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            evidence = root / 'evidence'
            evidence.mkdir(mode=0o700)
            original = root / 'signers'
            original.write_bytes(b'approved\n')
            alias = root / 'alias'
            alias.symlink_to(original)
            with self.assertRaisesRegex(RuntimeError, 'unavailable'):
                remote_build.freeze_signer(alias, evidence)
            with patch.object(remote_build.os, 'read', return_value=b''):
                with self.assertRaisesRegex(RuntimeError, 'short-read'):
                    remote_build.freeze_signer(original, evidence)
            real_read = os.read
            # A same-size write can share a filesystem timestamp tick. Replacing
            # the path changes its inode on both Linux and macOS.
            replacement = root / 'replacement'
            replacement.write_bytes(b'attacker\n')
            swaps = []
            def mutate_during_read(fd, count):
                if not swaps:
                    os.replace(replacement, original)
                    swaps.append(True)
                return real_read(fd, count)
            with patch.object(remote_build.os, 'read', side_effect=mutate_during_read):
                with self.assertRaisesRegex(RuntimeError, 'changed or was short-read'):
                    remote_build.freeze_signer(original, evidence)
            self.assertEqual(original.read_bytes(), b'attacker\n')
            self.assertEqual(len(swaps), 1)
            self.assertFalse(list(evidence.iterdir()))

    def test_main_packages_approved_bytes_after_external_path_changes(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            source = root / 'source'
            source.mkdir()
            key = root / 'approved'
            subprocess.run(['/usr/bin/ssh-keygen', '-q', '-t', 'ed25519', '-N', '',
                            '-f', str(key)], check=True)
            public = ' '.join(key.with_suffix('.pub').read_text().split()[:2])
            fingerprint = subprocess.check_output(
                ['/usr/bin/ssh-keygen', '-lf', str(key) + '.pub'], text=True).split()[1]
            signers = root / 'signers'
            approved = (qualification_source.PRINCIPAL + ' ' + public + '\n').encode()
            signers.write_bytes(approved)
            def git(*args):
                return subprocess.check_output(['/usr/bin/git', '-C', str(source), *args],
                                               text=True).strip()
            git('init', '-q')
            git('config', 'user.name', 'swackhamer')
            git('config', 'user.email', qualification_source.PRINCIPAL)
            git('config', 'gpg.format', 'ssh')
            git('config', 'user.signingkey', str(key))
            git('config', 'gpg.ssh.allowedSignersFile', str(signers))
            (source / 'payload').write_text('signed\n')
            for name in ('justfile', 'rust-toolchain.toml',
                         'tools/remote-build/route.py',
                         'tools/remote-build/qualification_bootstrap.py',
                         'tools/remote-build/qualification_source.py',
                         'tools/remote-build/test_runner.py',
                         'tools/release/bootstrap_oracle.py'):
                path = source / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('signed fixture\n')
            git('add', '.')
            git('commit', '-q', '-S', '-m', 'Approved source')
            expected = git('rev-parse', 'HEAD')
            real_check = qualification_source._trusted_key
            def swap_after_check(path):
                real_check(path)
                signers.write_bytes(b'attacker replacement\n')
            transport = SimpleNamespace(instance_id='2', host='192.0.2.1',
                                        ssh=lambda command: ['ssh', command])
            evidence = root / 'evidence'
            real_run = subprocess.run
            packet_bytes = []
            with patch.dict(os.environ, {'OXIDEX_REMOTE_SSH_KNOWN_HOSTS': str(root / 'known')}), \
                 patch.object(qualification_source, 'KEY', public), \
                 patch.object(qualification_source, 'FINGERPRINT', fingerprint), \
                 patch.object(qualification_source, '_trusted_key', side_effect=swap_after_check), \
                 patch.object(remote_build.ssh_transport, 'identity', return_value=('uploader', 'key')), \
                 patch.object(remote_build.ssh_transport, 'DirectTransport', return_value=transport), \
                 patch.object(remote_build, 'verify_builder_admission', return_value={'admission_passed': True}), \
                 patch.object(remote_build, 'pinned_toolchain', return_value={'channel': '1.99.0'}), \
                 patch('qualification_source.verify_source', return_value={}), \
                 patch.object(remote_build.subprocess, 'run') as run:
                def stop_at_prepare(command, **kwargs):
                    if command[0] == 'ssh':
                        packet_bytes.append((evidence / 'remote-source.tar.gz').read_bytes())
                        raise RuntimeError('stopped after local packet')
                    return real_run(command, **kwargs)
                run.side_effect = stop_at_prepare
                with self.assertRaisesRegex(RuntimeError, 'stopped after local packet'):
                    remote_build.main(['--source', str(source), '--instance', 'builder-vm',
                        '--zone', 'zone', '--instance-id', '2', '--worktree-id', 'fixture',
                        '--evidence-dir', str(evidence), '--just-recipe', 'fleet-test'])
            self.assertEqual(expected, git('rev-parse', 'HEAD'))
            self.assertGreaterEqual(len(packet_bytes), 1)
            with tarfile.open(fileobj=io.BytesIO(packet_bytes[0]), mode='r:gz') as packet:
                self.assertEqual(packet.extractfile('maintainer.allowed_signers').read(), approved)
            self.assertEqual(signers.read_bytes(), b'attacker replacement\n')


if __name__ == '__main__':
    unittest.main()
