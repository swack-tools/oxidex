import hashlib
import json
import shlex
import subprocess
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
from lib import benchmark_transport as wire


class PrivateTransportTests(unittest.TestCase):
    def test_commands_use_private_prefix_and_source_only_cleanup(self):
        self.assertEqual(shlex.split(wire.prepare_command('project')), [
            'sudo', '-n', wire.LAUNCHER, '--benchmark278', 'project', 'prepare'])
        command = shlex.split(wire.cleanup_command('project'))
        self.assertEqual(command[-1], 'cleanup')
        self.assertNotIn('rm', command)
        self.assertNotIn('/usr/local/bin/oxidex-remote-build', command)

    def test_two_job_json_round_trips_shell_metacharacters_without_interpolation(self):
        jobs = [{'worktree_id': name, 'argv': ['python3', 'job.py', "$(touch x); 'literal'"]}
                for name in ('debug-child', 'release-child')]
        command = shlex.split(wire.supervisor_command('project', jobs))
        self.assertEqual(command[-2], '/usr/local/bin/oxidex-worktree-commands')
        self.assertEqual(json.loads(command[-1]), jobs)
        jobs[1]['worktree_id'] = 'debug-child'
        with self.assertRaisesRegex(ValueError, 'distinct'):
            wire.supervisor_command('project', jobs)

    def test_invalid_paths_and_ids_refuse_before_commands(self):
        for project in ('../root', 'x;touch x', '', 'x' * 65):
            with self.subTest(project=project), self.assertRaises(ValueError):
                wire.cleanup_command(project)
        for name in ('../binary', '/binary', 'a//b', 'a/./b', 'a\x00b'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                wire.export_command('project', 'run-' + '0' * 32, [name])
        with self.assertRaises(ValueError):
            wire.export_command('project', 'run-' + '0' * 32, ['binary', 'binary'])

    def test_archive_is_streamed_on_stdin_and_receipt_is_bound_to_bytes(self):
        data = b'small test source archive'
        checksum = hashlib.sha256(data).hexdigest()
        record = dict(schema='benchmark278-source-staged-v1', project='project',
                      archive_sha256=checksum, archive_bytes=len(data),
                      source=wire.ROOT + '/sources/project', observed_at_ns=1)
        def remote(argv, **kwargs):
            self.assertEqual(kwargs['stdin'].read(), data)
            self.assertEqual(shlex.split(argv[1]), [
                'sudo', '-n', wire.TRANSFER, 'stage', 'project', checksum, str(len(data))])
            self.assertEqual(kwargs['timeout'], 660)
            return SimpleNamespace(stdout=json.dumps(record))
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / 'source.tar'; archive.write_bytes(data)
            with patch.object(wire.subprocess, 'run', side_effect=remote):
                self.assertEqual(wire.stage_archive(lambda cmd: ['ssh', cmd], 'project', archive), record)
                record['archive_sha256'] = '0' * 64
                with self.assertRaisesRegex(ValueError, 'differs'):
                    wire.stage_archive(lambda cmd: ['ssh', cmd], 'project', archive)

    def test_upload_failure_does_not_claim_success_or_run_legacy_cleanup(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / 'source.tar'; archive.write_bytes(b'archive')
            with patch.object(wire.subprocess, 'run', side_effect=subprocess.CalledProcessError(75, 'ssh')) as run:
                with self.assertRaises(subprocess.CalledProcessError):
                    wire.stage_archive(lambda cmd: ['ssh', cmd], 'project', archive)
                self.assertEqual(run.call_count, 1)

    def test_export_requires_exact_requested_files_lengths_and_hashes(self):
        run_id = 'run-' + '0' * 32
        record = dict(schema='benchmark278-immutable-export-v1', project='project', run_id=run_id,
                      observed_at_ns=1, archive_sha256='a' * 64,
                      files=[dict(path='debug/oxidex', bytes=10, sha256='b' * 64)])
        self.assertEqual(wire.validate_export(record, 'project', run_id, ['debug/oxidex']),
                         wire.ROOT + '/exports/project/' + run_id)
        for mutation in ({'path': 'release/oxidex'}, {'bytes': True}, {'bytes': wire.LIMIT + 1},
                         {'sha256': 'bad'}):
            candidate = json.loads(json.dumps(record)); candidate['files'][0].update(mutation)
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                wire.validate_export(candidate, 'project', run_id, ['debug/oxidex'])
