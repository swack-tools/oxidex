"""Bound the untrusted commit identity read before signature verification."""
import hashlib
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from lib import remote_build
import qualification_source


class IdentityOutputBoundTests(unittest.TestCase):
    def test_unsigned_oversize_author_refuses_during_bounded_read(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            source = root / 'source'
            source.mkdir()
            subprocess.run(['/usr/bin/git', 'init', '-q', str(source)], check=True)
            (source / 'payload').write_text('unsigned\n')
            subprocess.run(['/usr/bin/git', '-C', str(source), 'add', 'payload'], check=True)
            env = remote_build.source_git_env()
            env.update(GIT_AUTHOR_NAME='A' * (32 * 1024),
                       GIT_AUTHOR_EMAIL='author@example.invalid',
                       GIT_COMMITTER_NAME='swackhamer',
                       GIT_COMMITTER_EMAIL=qualification_source.PRINCIPAL)
            subprocess.run(['/usr/bin/git', '-C', str(source), '-c', 'commit.gpgsign=false',
                            'commit', '-qm', 'unsigned'], env=env, check=True)
            head = subprocess.check_output(['/usr/bin/git', '-C', str(source),
                                            'rev-parse', 'HEAD'], text=True).strip()
            signer = root / 'signer'
            signer.write_text('fixed signer bytes\n')
            signer.chmod(0o400)
            digest = hashlib.sha256(signer.read_bytes()).hexdigest()
            with patch.object(qualification_source, '_trusted_key'):
                with self.assertRaisesRegex(RuntimeError, 'output limit exceeded'):
                    remote_build._verify_signed_source_with_frozen_signer(
                        source, head, signer, digest)


    def test_truncated_identity_cannot_reach_verify_commit(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            source = root / 'source'
            subprocess.run(['/usr/bin/git', 'init', '-q', str(source)], check=True)
            signer = root / 'signer'
            signer.write_text('fixed signer bytes\n')
            signer.chmod(0o400)
            digest = hashlib.sha256(signer.read_bytes()).hexdigest()
            with patch.object(qualification_source, '_trusted_key'), \
                 patch.object(remote_build.infra_repository_binding, '_bounded_command',
                              return_value=b'swackhamer|truncated'), \
                 patch.object(remote_build, '_git_run',
                              side_effect=AssertionError('verify-commit reached')):
                with self.assertRaisesRegex(RuntimeError, 'signed maintainer HEAD'):
                    remote_build._verify_signed_source_with_frozen_signer(
                        source, 'a' * 40, signer, digest)

    def test_pre_signature_config_scalars_have_byte_limits(self):
        with tempfile.TemporaryDirectory() as folder:
            source = Path(folder) / 'source'
            subprocess.run(['/usr/bin/git', 'init', '-q', str(source)], check=True)
            def config(key, value):
                subprocess.run(['/usr/bin/git', '-C', str(source), 'config', key, value],
                               check=True)
            config('gpg.ssh.allowedSignersFile', '/' + 'a' * 8192)
            with self.assertRaisesRegex(RuntimeError, 'output limit exceeded'):
                remote_build.configured_signer_path(source)
            config('remote.origin.url', 'x' * 2048)
            with self.assertRaisesRegex(RuntimeError, 'output limit exceeded'):
                remote_build._bounded_source_git(
                    source, ['config', '--local', '--get-all', 'remote.origin.url'], 1024)


if __name__ == '__main__':
    unittest.main()
