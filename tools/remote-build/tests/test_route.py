"""Small control-plane checks: no Cargo, SSH, or provider calls."""
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
import stat
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import route


class RouteTests(unittest.TestCase):
    def test_marker_requires_exact_root_regular_nonwritable_file(self):
        good=SimpleNamespace(st_mode=stat.S_IFREG|0o444,st_uid=0,st_nlink=1)
        with patch.object(route.sys,'platform','linux'), patch.object(Path,'cwd',return_value=Path('/src')):
            with patch.object(Path,'lstat',return_value=good):
                self.assertTrue(route.local_worker_context({},Path('/run/oxidex-build-container'),Path('/absent/runner')))
            for changed in ({'st_uid':1001},{'st_mode':stat.S_IFLNK|0o444},
                            {'st_mode':stat.S_IFREG|0o644},{'st_nlink':2}):
                bad=SimpleNamespace(**{**vars(good),**changed})
                with patch.object(Path,'lstat',return_value=bad):
                    self.assertFalse(route.local_worker_context({},Path('/run/oxidex-build-container'),Path('/absent/runner')))

    def test_self_hosted_ci_context_is_specific(self):
        env={'CI':'true','GITHUB_ACTIONS':'true','RUNNER_ENVIRONMENT':'self-hosted',
             'RUNNER_OS':'Linux','RUNNER_NAME':'spot-runner','GITHUB_RUN_ID':'123'}
        trusted=SimpleNamespace(st_mode=stat.S_IFREG|0o444,st_uid=0,st_nlink=1)
        with patch.object(route.sys,'platform','linux'),patch.object(Path,'cwd',return_value=Path('/work')), \
             patch.object(Path,'lstat',return_value=trusted):
            self.assertTrue(route.local_worker_context(env))
            for key in env:
                changed=dict(env);changed.pop(key)
                self.assertFalse(route.local_worker_context(changed))
        with patch.object(route.sys,'platform','linux'),patch.object(Path,'cwd',return_value=Path('/work')), \
             patch.object(Path,'lstat',return_value=SimpleNamespace(st_mode=stat.S_IFLNK|0o444,st_uid=0,st_nlink=1)):
            self.assertFalse(route.local_worker_context(env))

    def test_laptop_dispatch_preserves_recipe_and_literal_arguments(self):
        with patch.object(route,'local_worker_context',return_value=False),patch.object(os,'execv') as launch:
            route.main(['test-package','some-package'])
        argv=launch.call_args.args[1]
        self.assertEqual(argv[-3:],['--just-recipe','test-package','--just-arg=some-package'])
        with patch.object(route,'local_worker_context',return_value=False),patch.object(os,'execv') as launch:
            route.main(['test-package','$(touch /tmp/should-never-run)'])
        self.assertEqual(launch.call_args.args[1][-1],'--just-arg=$(touch /tmp/should-never-run)')

    def test_rendered_private_package_keeps_one_literal_cargo_argument(self):
        if shutil.which('just') is None:
            self.skipTest('just is unavailable')
        repository = Path(__file__).resolve().parents[3]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fake_bin = root / 'bin'
            fake_bin.mkdir()
            cargo = fake_bin / 'cargo'
            cargo.write_text('#!' + sys.executable + '\n'
                             'import json, os, sys\n'
                             'from pathlib import Path\n'
                             'Path(os.environ["CARGO_ARGV_FILE"]).write_text(json.dumps(sys.argv[1:]))\n')
            cargo.chmod(0o755)
            sentinel = root / 'injection-ran'
            packages = [
                'oxidex-tags-core; touch ' + str(sentinel),
                '$(touch ' + str(sentinel) + ')',
                'pkg with spaces',
                'glob*[abc]',
                'a\'b"c',
            ]
            for index, package in enumerate(packages):
                with self.subTest(package=package):
                    sentinel.unlink(missing_ok=True)
                    argv_file = root / f'argv-{index}.json'
                    rendered = subprocess.run(
                        ['just', '--dry-run', '_test-package-worker', package],
                        cwd=repository, capture_output=True, text=True, check=True)
                    lines = rendered.stderr.splitlines()
                    self.assertEqual(len(lines), 3)
                    self.assertIn('--require-local-context', lines[0])
                    env = {**os.environ, 'PATH': str(fake_bin) + os.pathsep + os.environ['PATH'],
                           'CARGO_ARGV_FILE': str(argv_file)}
                    result = subprocess.run(['/bin/bash', '-c', '\n'.join(lines[1:])],
                                            cwd=repository, env=env, capture_output=True, text=True)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(json.loads(argv_file.read_text()), ['test', '-p', package])
                    self.assertFalse(sentinel.exists())

    def test_worker_runs_private_original_recipe(self):
        with patch.object(route,'local_worker_context',return_value=True),patch.object(os,'execvp') as launch, \
             patch('test_runner.prepare_generic_recipe_oracle'):
            route.main(['test-package','abc'])
        self.assertEqual(launch.call_args.args[1],['just','_test-package-worker','abc'])

    def test_ignored_input_probe_routes_through_oracle_and_guarded_worker(self):
        with patch.object(route, 'local_worker_context', return_value=False), \
             patch.object(os, 'execv') as launch:
            route.main(['prepare-ignored-inputs'])
        self.assertEqual(launch.call_args.args[1][-2:],
                         ['--just-recipe', 'prepare-ignored-inputs'])
        events = []
        with patch.object(route, 'local_worker_context', return_value=True), \
             patch('test_runner.prepare_generic_recipe_oracle',
                   side_effect=lambda: events.append('oracle')), \
             patch.object(os, 'execvp', side_effect=lambda *_: events.append('worker')) as launch:
            route.main(['prepare-ignored-inputs'])
        self.assertEqual(events, ['oracle', 'worker'])
        self.assertEqual(launch.call_args.args[1],
                         ['just', '_prepare-ignored-inputs-worker'])
        if shutil.which('just'):
            repository = Path(__file__).resolve().parents[3]
            rendered = subprocess.run(['just', '--dry-run', '_prepare-ignored-inputs-worker'],
                                      cwd=repository, capture_output=True, text=True, check=True)
            self.assertEqual(rendered.stderr.splitlines(), [
                'python3 tools/remote-build/route.py --require-local-context',
                'python3 tools/remote-build/ignored_fixture_probe.py',
            ])

    def test_ignored_suite_dispatches_to_existing_remote_client(self):
        with patch.object(route, 'local_worker_context', return_value=False), \
             patch.object(os, 'execv') as launch:
            route.main(['test-ignored'])
        self.assertEqual(launch.call_args.args[1][-2:], ['--just-recipe', 'test-ignored'])

    def test_ignored_suite_routes_through_oracle_and_private_worker(self):
        events = []
        with patch.object(route, 'local_worker_context', return_value=True), \
             patch('test_runner.prepare_generic_recipe_oracle', side_effect=lambda: events.append('oracle')), \
             patch.object(os, 'execvp', side_effect=lambda *_: events.append('recipe')) as launch:
            route.main(['test-ignored'])
        self.assertEqual(events, ['oracle', 'recipe'])
        self.assertEqual(launch.call_args.args[1], ['just', '_test-ignored-worker'])

    def test_ignored_suite_renders_guarded_full_workspace_command(self):
        if shutil.which('just') is None:
            self.skipTest('just is unavailable')
        repository = Path(__file__).resolve().parents[3]
        rendered = subprocess.run(['just', '--dry-run', '_test-ignored-worker'],
                                  cwd=repository, capture_output=True, text=True, check=True)
        self.assertEqual(rendered.stderr.splitlines(), [
            'python3 tools/remote-build/route.py --require-local-context',
            'CARGO_PROFILE_RELEASE_PANIC=unwind cargo test --release --workspace --all-features --locked --no-fail-fast -- --include-ignored',
        ])

    def test_test_worker_provisions_oracle_before_original_recipe(self):
        events = []
        with patch.object(route, 'local_worker_context', return_value=True), \
             patch('test_runner.prepare_generic_recipe_oracle', side_effect=lambda: events.append('oracle')), \
             patch.object(os, 'execvp', side_effect=lambda *_: events.append('recipe')) as launch:
            route.main(['test', '--exact', 'literal argument'])
        self.assertEqual(events, ['oracle', 'recipe'])
        self.assertEqual(launch.call_args.args[1], ['just', '_test-worker', '--exact', 'literal argument'])

    def test_oracle_provision_failure_refuses_cargo_recipe(self):
        with patch.object(route, 'local_worker_context', return_value=True), \
             patch('test_runner.prepare_generic_recipe_oracle', side_effect=RuntimeError('invalid oracle')), \
             patch.object(os, 'execvp') as launch:
            with self.assertRaisesRegex(RuntimeError, 'invalid oracle'):
                route.main(['test'])
        launch.assert_not_called()

    def test_bad_names_and_args_refuse_before_dispatch(self):
        with patch.object(os,'execv') as launch:
            for argv in (['-bad'],['build','bad\narg'],['build','']):
                with self.assertRaises(SystemExit):route.main(argv)
            launch.assert_not_called()
