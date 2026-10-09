"""Small real-Git regressions for the shared source-byte admission boundary."""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch
import zlib

from lib import remote_build


class SourceIOBoundaryTests(unittest.TestCase):
    def fixture(self, root, members):
        source = root / 'source'
        source.mkdir()
        def git(*args):
            return subprocess.check_output(['/usr/bin/git', '-C', str(source), *args],
                                           text=True).strip()
        git('init', '-q')
        for name, data in members.items():
            path = source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        git('add', '.')
        git('-c', 'user.name=fixture', '-c', 'user.email=fixture@example.invalid',
            '-c', 'commit.gpgsign=false', 'commit', '-qm', 'fixture')
        return source, git, git('rev-parse', 'HEAD')

    def test_status_refuses_oversized_replacement_before_read(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, head = self.fixture(Path(folder), {'tracked': b'hello'})
            with (source / 'tracked').open('r+b') as stream:
                stream.truncate(1024 * 1024 + 1)
            original_read = os.read
            def refuse_tracked(fd, count):
                if os.fstat(fd).st_ino == (source / 'tracked').stat().st_ino:
                    raise AssertionError('read oversized source')
                return original_read(fd, count)
            with patch.object(remote_build, 'MAX_SIGNED_BLOB_BYTES', 1024 * 1024), \
                 patch.object(remote_build.os, 'read', side_effect=refuse_tracked):
                with self.assertRaisesRegex(RuntimeError, 'bound|size'):
                    remote_build.source_clean_status(source, head)

    def test_worktree_deadline_refuses_before_read(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'tracked'
            path.write_bytes(b'hello')
            with patch.object(remote_build.os, 'read', side_effect=AssertionError('read after deadline')):
                with self.assertRaisesRegex(RuntimeError, 'deadline'):
                    remote_build._stream_worktree_hashes(path, deadline=time.monotonic() - 1)

    def test_forged_loose_blob_refused_before_manifest_or_snapshot(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            members = {name: b'signed' for name in remote_build.INFRA_PYTHON_REQUIRED}
            members['tests/test_fixture.py'] = b'original fixture\n'
            source, git, head = self.fixture(root, members)
            with patch.object(remote_build, 'INFRA_PYTHON_MIN_MODULES', 1):
                manifest, _ = remote_build.infra_python_manifest(source, head)
            self.assertEqual(next(row['sha256'] for row in manifest['modules']
                                  if row['path'] == 'tests/test_fixture.py'),
                             hashlib.sha256(b'original fixture\n').hexdigest())
            original = git('rev-parse', 'HEAD:tests/test_fixture.py')
            loose = source / '.git' / 'objects' / original[:2] / original[2:]
            forged = b'forged!! fixture\n'
            loose.chmod(0o644)
            loose.write_bytes(zlib.compress(b'blob ' + str(len(forged)).encode() + b'\0' + forged))
            with patch.object(remote_build, 'INFRA_PYTHON_MIN_MODULES', 1):
                with self.assertRaisesRegex(RuntimeError, 'hash|identity'):
                    remote_build.infra_python_manifest(source, head)
            with self.assertRaisesRegex(RuntimeError, 'hash|identity'):
                remote_build.make_snapshot(source, root / 'snapshot.tar.gz', signed_head=head,
                    source_profile=remote_build.INFRA_PYTHON_PROFILE)


    def test_signed_blob_truncated_output_refused(self):
        with tempfile.TemporaryDirectory() as folder:
            source, git, _ = self.fixture(Path(folder), {'tracked': b'hello'})
            oid = git('rev-parse', 'HEAD:tracked')
            real_command = remote_build.infra_repository_binding._bounded_command
            def short_body(command, environment, limit, seconds, **kwargs):
                if command[-3:] == ['cat-file', 'blob', oid]:
                    return b'hell'
                return real_command(command, environment, limit, seconds, **kwargs)
            with patch.object(remote_build.infra_repository_binding, '_bounded_command',
                              side_effect=short_body):
                with self.assertRaisesRegex(RuntimeError, 'hash or size'):
                    remote_build._authenticated_source_blob(source, oid, 1024 * 1024)

    def test_forged_loose_tree_refused_before_status_manifest_or_snapshot(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            members = {name: b'signed' for name in remote_build.INFRA_PYTHON_REQUIRED}
            members['tests/test_fixture.py'] = b'original fixture\n'
            source, git, head = self.fixture(root, members)
            self.assertEqual(remote_build.source_clean_status(source, head), '')
            with patch.object(remote_build, 'INFRA_PYTHON_MIN_MODULES', 1):
                remote_build.infra_python_manifest(source, head)
            tree_id = git('rev-parse', 'HEAD:tests')
            old_id = git('rev-parse', 'HEAD:tests/test_fixture.py')
            other_id = git('rev-parse', 'HEAD:tests/test_builder_c9_proof_repairs.py')
            raw = subprocess.check_output(['/usr/bin/git', '-C', str(source),
                                           'cat-file', 'tree', tree_id])
            self.assertIn(bytes.fromhex(old_id), raw)
            forged = raw.replace(bytes.fromhex(old_id), bytes.fromhex(other_id))
            loose = source / '.git' / 'objects' / tree_id[:2] / tree_id[2:]
            loose.chmod(0o644)
            loose.write_bytes(zlib.compress(b'tree ' + str(len(forged)).encode() + b'\0' + forged))
            rendered = subprocess.check_output(['/usr/bin/git', '-C', str(source),
                'ls-tree', '-r', head, '--', 'tests/test_fixture.py'])
            self.assertIn(other_id.encode(), rendered,
                          'real Git must render the forged tree under the original commit')
            for operation in (
                    lambda: remote_build.source_clean_status(source, head),
                    lambda: remote_build.infra_python_manifest(source, head),
                    lambda: remote_build.make_snapshot(source, root / 'tree.tar.gz',
                        signed_head=head, source_profile=remote_build.INFRA_PYTHON_PROFILE)):
                with self.assertRaisesRegex(RuntimeError, 'hash'):
                    operation()


if __name__ == '__main__':
    unittest.main()
