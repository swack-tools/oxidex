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

    def test_status_aggregate_budget_refuses_fourth_small_file_before_read(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, head = self.fixture(Path(folder),
                {f'tracked-{index}': bytes([index]) * 20 for index in range(4)})
            fourth = (source / 'tracked-3').stat().st_ino
            original_read = os.read
            def no_fourth_body(fd, count):
                if os.fstat(fd).st_ino == fourth:
                    raise AssertionError('fourth tracked body requested')
                return original_read(fd, count)
            with patch.object(remote_build, 'MAX_SOURCE_STATUS_BYTES', 65, create=True), \
                 patch.object(remote_build.os, 'read', side_effect=no_fourth_body):
                with self.assertRaisesRegex(RuntimeError, 'aggregate byte bound'):
                    remote_build.source_clean_status(source, head)

    def test_snapshot_aggregate_budget_refuses_fourth_small_file_before_read(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            source, _, _ = self.fixture(root,
                {f'tracked-{index}': bytes([index]) * 20 for index in range(4)})
            fourth = (source / 'tracked-3').stat().st_ino
            original_read = os.read
            def no_fourth_body(fd, count):
                if os.fstat(fd).st_ino == fourth:
                    raise AssertionError('fourth snapshot body requested')
                return original_read(fd, count)
            with patch.object(remote_build, 'MAX_SOURCE_SNAPSHOT_READ_BYTES', 65, create=True), \
                 patch.object(remote_build.os, 'read', side_effect=no_fourth_body):
                with self.assertRaisesRegex(RuntimeError, 'aggregate byte bound'):
                    remote_build.make_snapshot(source, root / 'snapshot.tar.gz')

    def test_signed_snapshot_counts_object_and_worktree_reads_together(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            members = {name: b'' for name in ('justfile', 'rust-toolchain.toml',
                'tools/remote-build/route.py', 'tools/remote-build/qualification_bootstrap.py',
                'tools/remote-build/qualification_source.py',
                'tools/remote-build/test_runner.py', 'tools/release/bootstrap_oracle.py')}
            members.update({f'tracked-{index}': bytes([index]) * 20 for index in range(4)})
            source, _, head = self.fixture(root, members)
            with patch.object(remote_build, 'MAX_SOURCE_SNAPSHOT_READ_BYTES', 65):
                with self.assertRaisesRegex(RuntimeError, 'aggregate read budget'):
                    remote_build.make_snapshot(source, root / 'signed.tar.gz', signed_head=head)

    def test_source_budget_deadline_does_not_renew_per_file(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, head = self.fixture(Path(folder), {'first': b'a', 'second': b'b'})
            original_stream = remote_build._stream_worktree_hashes
            seen_budgets = []
            def expire_after_first(path, **kwargs):
                seen_budgets.append(kwargs['budget'])
                result = original_stream(path, **kwargs)
                if len(seen_budgets) == 1:
                    kwargs['budget'].deadline = time.monotonic() - 1
                return result
            with patch.object(remote_build, '_stream_worktree_hashes', side_effect=expire_after_first):
                with self.assertRaisesRegex(RuntimeError, 'aggregate deadline'):
                    remote_build.source_clean_status(source, head)
            self.assertEqual(len(seen_budgets), 1,
                             'expired shared deadline must stop before the next file')

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


    def test_manifest_module_size_bound_precedes_body(self):
        with tempfile.TemporaryDirectory() as folder:
            members = {name: b'signed' for name in remote_build.INFRA_PYTHON_REQUIRED}
            members['tests/test_fixture.py'] = b'original fixture\n'
            source, git, head = self.fixture(Path(folder), members)
            oid = git('rev-parse', 'HEAD:tests/test_fixture.py')
            real_command = remote_build.infra_repository_binding._bounded_command
            body_limits = []
            declared_size = 0
            def declared_object(command, environment, limit, seconds, **kwargs):
                if command[-2:] == ['cat-file', '--batch-check=%(objectname) %(objecttype) %(objectsize)'] \
                        and kwargs.get('data') == (oid + '\n').encode():
                    return f'{oid} blob {declared_size}\n'.encode()
                if command[-3:] == ['cat-file', 'blob', oid]:
                    body_limits.append(limit)
                    raise AssertionError('module body requested')
                return real_command(command, environment, limit, seconds, **kwargs)
            with patch.object(remote_build, 'INFRA_PYTHON_MIN_MODULES', 1), \
                 patch.object(remote_build.infra_repository_binding, '_bounded_command',
                              side_effect=declared_object):
                for declared_size in (remote_build.MAX_INFRA_PYTHON_MODULE_BYTES + 1,
                                      remote_build.MAX_SIGNED_BLOB_BYTES):
                    with self.assertRaisesRegex(RuntimeError, 'fixed bound'):
                        remote_build.infra_python_manifest(source, head)
                    self.assertEqual(body_limits, [], 'oversized module body was requested')
                declared_size = remote_build.MAX_INFRA_PYTHON_MODULE_BYTES
                with self.assertRaisesRegex(AssertionError, 'module body requested'):
                    remote_build.infra_python_manifest(source, head)
                self.assertEqual(body_limits, [remote_build.MAX_INFRA_PYTHON_MODULE_BYTES])

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
