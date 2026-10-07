"""Lightweight website recipe controls; no Node installation or site build."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import docs_site_worker
import route

HELPER = Path(__file__).resolve().parents[2] / 'docs' / 'dist-sha256.sh'
REAL_RUN = subprocess.run


class DocsSiteWorkerTests(unittest.TestCase):
    def test_verified_actions_checkout_gets_unique_target_output(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory).resolve() / 'checkout'
            source.mkdir()
            target = source / 'target'
            env = {'GITHUB_SHA': 'a' * 40, 'GITHUB_RUN_ID': '123',
                   'GITHUB_RUN_ATTEMPT': '2', 'GITHUB_JOB': 'docs-build'}
            with patch.dict(docs_site_worker.os.environ, env), \
                 patch.object(docs_site_worker.Path, 'cwd', return_value=source), \
                 patch.object(route, 'local_worker_context', return_value=True), \
                 patch.object(route, 'trusted_marker', return_value=False), \
                 patch.object(route, 'verify_ci_fleet_checkout') as verified, \
                 patch.object(docs_site_worker.secrets, 'token_hex',
                              side_effect=['b' * 16, 'c' * 16]), \
                 patch('test_runner.fleet_recipe_target', return_value=target):
                selected_source, output = docs_site_worker.selected_context()
                _, second_output = docs_site_worker.selected_context()
            self.assertEqual(verified.call_count, 2)
            self.assertEqual(selected_source, source)
            self.assertEqual(output, target / 'docs-site' /
                             '123-2-docs-build-aaaaaaaaaaaa-bbbbbbbbbbbbbbbb')
            self.assertNotEqual(second_output, output)
            output.mkdir(parents=True)
            with patch.object(docs_site_worker.Path, 'cwd', return_value=source):
                with self.assertRaisesRegex(RuntimeError, 'destination must be new'):
                    docs_site_worker.run(selected_source, output)

    def test_builder_context_keeps_retained_target(self):
        with patch.object(docs_site_worker.Path, 'cwd', return_value=docs_site_worker.SOURCE), \
             patch.object(route, 'local_worker_context', return_value=True), \
             patch.object(route, 'trusted_marker', return_value=True), \
             patch.object(route, 'verified_fleet_checkout', return_value=True):
            self.assertEqual(docs_site_worker.selected_context(),
                             (docs_site_worker.SOURCE, docs_site_worker.OUTPUT))

    def test_untrusted_context_refuses_before_checkout_verification(self):
        with patch.object(route, 'local_worker_context', return_value=False), \
             patch.object(route, 'verify_ci_fleet_checkout') as verify:
            with self.assertRaisesRegex(RuntimeError, 'trusted Spot context'):
                docs_site_worker.selected_context()
            verify.assert_not_called()

    def test_requires_node_24_before_build(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / 'source'
            source.mkdir()
            output = Path(directory) / 'site'
            with patch.object(docs_site_worker.Path, 'cwd', return_value=source), \
                 patch.object(docs_site_worker.shutil, 'which', return_value='/fake/bin'), \
                 patch.object(docs_site_worker.subprocess, 'check_output', return_value='v22.0.0\n'), \
                 patch.object(docs_site_worker.subprocess, 'run') as command:
                with self.assertRaisesRegex(RuntimeError, 'requires Node.js 24'):
                    docs_site_worker.run(source, output)
                command.assert_not_called()

    def exercise_snapshot(self, digest_mode):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / 'source'
            source.mkdir()
            output = Path(directory) / 'site'
            head = 'a' * 40
            tree = 'b' * 40
            calls = []

            def output_for(command, **_):
                if command == ['node', '--version']:
                    return 'v24.1.0\n'
                if command == ['git', 'rev-parse', 'HEAD']:
                    return head + '\n'
                if command == ['git', 'rev-parse', 'HEAD^{tree}']:
                    return tree + '\n'
                if command[:2] == ['bash', 'tools/docs/dist-sha256.sh']:
                    return REAL_RUN(['bash', str(HELPER), command[2]],
                                    capture_output=True, text=True, check=True).stdout
                self.fail(f'unexpected command: {command}')

            def run_command(command, **kwargs):
                calls.append((command, kwargs))
                if command[:2] == ['bash', 'tools/docs-local-deploy.sh']:
                    (output / 'dist' / 'assets').mkdir(parents=True)
                    (output / 'dist' / 'index.html').write_text('site')
                    (output / 'dist' / 'assets' / 'app.js').write_text('console.log(1)')
                    actual = REAL_RUN(['bash', str(HELPER), str(output / 'dist')],
                                      capture_output=True, text=True, check=True).stdout.strip()
                    manifest = {
                        'candidate_sha': head, 'source_head_sha': head,
                        'tree_hash': tree, 'source_kind': 'commit',
                        'base_path': '/', 'dist': str(output / 'dist'),
                    }
                    if digest_mode == 'valid':
                        manifest['dist_sha256'] = actual
                    elif digest_mode == 'wrong':
                        manifest['dist_sha256'] = '0' * 64
                    elif digest_mode == 'malformed':
                        manifest['dist_sha256'] = 'not-a-sha256'
                    (output / 'snapshot-manifest.json').write_text(json.dumps(manifest))
                return subprocess.CompletedProcess(command, 0)

            with patch.object(docs_site_worker.Path, 'cwd', return_value=source), \
                 patch.object(docs_site_worker.shutil, 'which', return_value='/fake/bin'), \
                 patch.object(docs_site_worker.subprocess, 'check_output', side_effect=output_for), \
                 patch.object(docs_site_worker.subprocess, 'run', side_effect=run_command):
                if digest_mode == 'valid':
                    docs_site_worker.run(source, output)
                else:
                    with self.assertRaisesRegex(RuntimeError, 'dist_sha256'):
                        docs_site_worker.run(source, output)
            return output, calls

    def test_invokes_snapshot_mirror_and_checks_actual_tiny_dist(self):
        output, calls = self.exercise_snapshot('valid')
        self.assertEqual(calls[1][0], [
            'bash', 'tools/docs-local-deploy.sh', '--ref', 'HEAD',
            '--build-only', '--output', str(output)])
        self.assertEqual(calls[1][1]['env']['DOCS_CHANNEL'], 'stable')
        self.assertEqual(calls[1][1]['env']['DOCS_BASE'], '/')
        self.assertEqual(calls[2][0], [
            'node', 'docs/scripts/check-base-links.mjs', str(output / 'dist'), '/'])

    def test_missing_dist_digest_fails(self):
        self.exercise_snapshot('missing')

    def test_wrong_dist_digest_fails(self):
        self.exercise_snapshot('wrong')

    def test_malformed_dist_digest_fails(self):
        self.exercise_snapshot('malformed')
