"""Small control-plane checks: no Cargo, SSH, or provider calls."""
import os
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

    def test_worker_runs_private_original_recipe(self):
        with patch.object(route,'local_worker_context',return_value=True),patch.object(os,'execvp') as launch, \
             patch('test_runner.prepare_generic_recipe_oracle'):
            route.main(['test-package','abc'])
        self.assertEqual(launch.call_args.args[1],['just','_test-package-worker','abc'])

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
