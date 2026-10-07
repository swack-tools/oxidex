"""Lightweight website recipe controls; no Node installation or site build."""
import json
import os
from pathlib import Path
import subprocess
import tarfile
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

    def test_symlinked_ci_docs_site_parent_refuses_before_creation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            source = root / 'checkout'
            source.mkdir()
            target = source / 'target'
            target.mkdir()
            outside = root / 'outside'
            outside.mkdir()
            (target / 'docs-site').symlink_to(outside, target_is_directory=True)
            env = {'GITHUB_SHA': 'a' * 40, 'GITHUB_RUN_ID': '123',
                   'GITHUB_RUN_ATTEMPT': '1', 'GITHUB_JOB': 'docs'}
            with patch.dict(docs_site_worker.os.environ, env), \
                 patch.object(docs_site_worker.Path, 'cwd', return_value=source), \
                 patch.object(route, 'local_worker_context', return_value=True), \
                 patch.object(route, 'trusted_marker', return_value=False), \
                 patch.object(route, 'verify_ci_fleet_checkout'), \
                 patch('test_runner.fleet_recipe_target', return_value=target):
                with self.assertRaisesRegex(RuntimeError, 'not canonical'):
                    docs_site_worker.selected_context()
            self.assertEqual(list(outside.iterdir()), [])

    def test_child_environment_scrubs_git_and_site_overrides(self):
        hostile = {'GIT_DIR': '/tmp/other-repo', 'GIT_WORK_TREE': '/tmp/other-tree',
                   'GIT_CONFIG_COUNT': '1', 'GIT_CONFIG_KEY_0': 'core.bare',
                   'GIT_CONFIG_VALUE_0': 'true', 'GIT_REPLACE_REF_BASE': 'refs/evil',
                   'DOCS_STABLE_URL': 'https://attacker.invalid/',
                   'DOCS_PREVIEW_URL': 'https://evil.invalid/',
                   'DOCS_PREVIEW_SHA': '0' * 40}
        with patch.dict(os.environ, hostile):
            child = docs_site_worker.build_environment()
            self.assertEqual({key: os.environ[key] for key in hostile}, hostile)
        self.assertTrue(all(key not in child for key in hostile))
        self.assertEqual(child['GIT_NO_REPLACE_OBJECTS'], '1')
        self.assertEqual(child['GIT_CONFIG_GLOBAL'], os.devnull)
        self.assertEqual(child['GIT_CONFIG_SYSTEM'], os.devnull)
        self.assertEqual((child['DOCS_CHANNEL'], child['DOCS_BASE']), ('stable', '/'))

    def test_git_replacement_does_not_change_archived_head(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            env = docs_site_worker.build_environment()

            def git(*args):
                return subprocess.check_output(['git', '-C', str(root), *args],
                                               env=env, text=True).strip()

            (root / 'page').write_text('verified')
            git('add', 'page')
            subprocess.run(['git', '-C', str(root), '-c', 'user.name=T',
                            '-c', 'user.email=t@x', 'commit', '-qm', 'verified'],
                           env=env, check=True)
            good = git('rev-parse', 'HEAD')
            good_tree = git('rev-parse', 'HEAD^{tree}')
            (root / 'page').write_text('replacement')
            git('add', 'page')
            subprocess.run(['git', '-C', str(root), '-c', 'user.name=T',
                            '-c', 'user.email=t@x', 'commit', '-qm', 'replacement'],
                           env=env, check=True)
            bad = git('rev-parse', 'HEAD')
            git('replace', good, bad)
            git('checkout', '-q', good)
            self.assertNotEqual(subprocess.check_output(
                ['git', '-C', str(root), 'rev-parse', 'HEAD^{tree}'], text=True).strip(),
                good_tree)
            self.assertEqual(git('rev-parse', 'HEAD^{tree}'), good_tree)
            archive = subprocess.check_output(['git', '-C', str(root), 'archive', 'HEAD'],
                                              env=env)
            import io
            with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
                self.assertEqual(tar.extractfile('page').read(), b'verified')

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
            source = Path(directory).resolve() / 'source'
            source.mkdir()
            output = Path(directory).resolve() / 'site'
            with patch.object(docs_site_worker.Path, 'cwd', return_value=source), \
                 patch.object(docs_site_worker.shutil, 'which', return_value='/fake/bin'), \
                 patch.object(docs_site_worker.subprocess, 'check_output', return_value='v22.0.0\n'), \
                 patch.object(docs_site_worker.subprocess, 'run') as command:
                with self.assertRaisesRegex(RuntimeError, 'requires Node.js 24'):
                    docs_site_worker.run(source, output)
                command.assert_not_called()

    def exercise_snapshot(self, digest_mode):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory).resolve() / 'source'
            source.mkdir()
            output = Path(directory).resolve() / 'site'
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
                 patch.object(docs_site_worker.subprocess, 'run', side_effect=run_command), \
                 patch.dict(os.environ, {'GIT_DIR': '/wrong/repo',
                                      'DOCS_STABLE_URL': 'https://attacker.invalid/'}):
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
        self.assertEqual(calls[1][1]['env']['GIT_NO_REPLACE_OBJECTS'], '1')
        self.assertNotIn('GIT_DIR', calls[1][1]['env'])
        self.assertNotIn('DOCS_STABLE_URL', calls[1][1]['env'])
        self.assertEqual(calls[2][0], [
            'node', 'docs/scripts/check-base-links.mjs', str(output / 'dist'), '/'])

    def test_missing_dist_digest_fails(self):
        self.exercise_snapshot('missing')

    def test_wrong_dist_digest_fails(self):
        self.exercise_snapshot('wrong')

    def test_malformed_dist_digest_fails(self):
        self.exercise_snapshot('malformed')
