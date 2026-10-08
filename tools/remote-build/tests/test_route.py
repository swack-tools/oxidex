"""Small control-plane checks: no Cargo, SSH, or provider calls."""
import io
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
import ignored_suite


class RouteTests(unittest.TestCase):
    def test_batch_blob_framing_refuses_wrong_type_identity_size_and_truncation(self):
        oid = 'a' * 40
        valid = (oid + ' blob 3\n').encode() + b'a\0b\n'
        self.assertEqual(route._read_batch_blob(io.BytesIO(valid), oid, 3), b'a\0b')
        bad = (
            (oid + ' tree 3\n').encode() + b'a\0b\n',
            (('b' * 40) + ' blob 3\n').encode() + b'a\0b\n',
            (oid + ' blob 4\n').encode() + b'a\0b\n',
            (oid + ' blob 3\n').encode() + b'a\0',
            (oid + ' blob 3\n').encode() + b'a\0bX',
            b'',
        )
        for response in bad:
            with self.subTest(response=response), self.assertRaises(RuntimeError):
                route._read_batch_blob(io.BytesIO(response), oid, 3)

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

    def test_fleet_checkout_requires_root_marker_and_source_verification(self):
        with patch.object(route.sys, 'platform', 'linux'), \
             patch.object(Path, 'cwd', return_value=route.FLEET_CHECKOUT), \
             patch.object(route, 'verified_fleet_checkout', return_value=True) as verify, \
             patch.object(route, 'trusted_marker', return_value=False):
            self.assertFalse(route.local_worker_context({}))
            verify.assert_not_called()
        with patch.object(route.sys, 'platform', 'linux'), \
             patch.object(Path, 'cwd', return_value=route.FLEET_CHECKOUT), \
             patch.object(route, 'verified_fleet_checkout', return_value=False), \
             patch.object(route, 'trusted_marker', return_value=True):
            self.assertFalse(route.local_worker_context({}))

    def test_laptop_dispatch_preserves_recipe_and_literal_arguments(self):
        with patch.object(route,'local_worker_context',return_value=False),patch.object(os,'execv') as launch:
            route.main(['test-package','some-package'])
        argv=launch.call_args.args[1]
        self.assertEqual(argv[-3:],['--just-recipe','test-package','--just-arg=some-package'])
        with patch.object(route,'local_worker_context',return_value=False),patch.object(os,'execv') as launch:
            route.main(['test-package','$(touch /tmp/should-never-run)'])
        self.assertEqual(launch.call_args.args[1][-1],'--just-arg=$(touch /tmp/should-never-run)')

    def test_mac_fleet_recipe_still_dispatches_remote(self):
        with patch.object(route.sys, 'platform', 'darwin'), \
             patch.object(os, 'execv') as launch, \
             patch.object(route, 'prepare_fleet_checkout') as stage, \
             patch.object(route, 'verify_ci_fleet_checkout') as ci:
            route.main(['fleet-test'])
        self.assertEqual(launch.call_args.args[1][-2:], ['--just-recipe', 'fleet-test'])
        stage.assert_not_called()
        ci.assert_not_called()

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
             patch.object(route, 'verify_ci_fleet_checkout', side_effect=lambda: events.append('signed-source')), \
             patch('test_runner.prepare_generic_recipe_oracle', side_effect=lambda: events.append('oracle')), \
             patch.object(os, 'execvp', side_effect=lambda *_: events.append('recipe')) as launch:
            route.main(['test-ignored'])
        self.assertEqual(events, ['signed-source', 'oracle', 'recipe'])
        self.assertEqual(launch.call_args.args[1], ['just', '_test-ignored-worker'])

    def test_ignored_suite_renders_guarded_full_workspace_command(self):
        if shutil.which('just') is None:
            self.skipTest('just is unavailable')
        repository = Path(__file__).resolve().parents[3]
        rendered = subprocess.run(['just', '--dry-run', '_test-ignored-worker'],
                                  cwd=repository, capture_output=True, text=True, check=True)
        self.assertEqual(rendered.stderr.splitlines(), [
            'python3 tools/remote-build/route.py --require-local-context',
            'python3 tools/remote-build/ignored_suite.py',
        ])

    def test_test_worker_provisions_oracle_before_original_recipe(self):
        events = []
        with patch.object(route, 'local_worker_context', return_value=True), \
             patch('test_runner.prepare_generic_recipe_oracle', side_effect=lambda: events.append('oracle')), \
             patch.object(os, 'execvp', side_effect=lambda *_: events.append('recipe')) as launch:
            route.main(['test', '--exact', 'literal argument'])
        self.assertEqual(events, ['oracle', 'recipe'])
        self.assertEqual(launch.call_args.args[1], ['just', '_test-worker', '--exact', 'literal argument'])

    def test_ci_and_docs_public_recipes_dispatch_before_worker_commands(self):
        if shutil.which('just') is None:
            self.skipTest('just is unavailable')
        repository = Path(__file__).resolve().parents[3]
        for recipe in ('ci', 'ci-standard', 'pre-commit', 'docs-build'):
            with self.subTest(recipe=recipe):
                rendered = subprocess.run(['just', '--dry-run', recipe], cwd=repository,
                                          capture_output=True, text=True, check=True).stderr
                self.assertEqual(rendered.strip(),
                                 'python3 tools/remote-build/route.py ' + recipe)
        ci = subprocess.run(['just', '--dry-run', '_ci-worker'], cwd=repository,
                            capture_output=True, text=True, check=True).stderr
        steps = ('cargo fmt --all -- --check', 'just cbindgen-check',
                 'cargo clippy --release --all-features -- -D warnings',
                 'cargo build --release --all-features --all-targets',
                 'cargo test --doc --release --all-features',
                 'cargo nextest run --release --all-features --no-fail-fast',
                 'cargo test --test ffi_c_integration -- --nocapture')
        self.assertEqual([ci.index(step) for step in steps],
                         sorted(ci.index(step) for step in steps))
        docs = subprocess.run(['just', '--dry-run', '_docs-build-worker'], cwd=repository,
                              capture_output=True, text=True, check=True).stderr
        self.assertIn('cargo doc --workspace --no-deps', docs)
        for recipe, ordered in (
            ('ci-standard', ('cargo fmt --all -- --check',
                             'Checking C header is up-to-date',
                             'tools/remote-build/route.py lint-release',
                             'tools/remote-build/route.py build-release-local',
                             'tools/remote-build/route.py test\n',
                             'tools/remote-build/route.py test-ffi-c')),
            ('pre-commit', ('cargo fmt --all -- --check',
                            'Checking C header is up-to-date',
                            'tools/remote-build/route.py lint\n',
                            'tools/remote-build/route.py test\n')),
        ):
            worker = subprocess.run(['just', '--dry-run', '_' + recipe + '-worker'],
                                    cwd=repository, capture_output=True, text=True, check=True).stderr
            self.assertEqual([worker.index(step) for step in ordered],
                             sorted(worker.index(step) for step in ordered))

    def test_ci_worker_prepares_oracle_before_original_compound_recipe(self):
        events = []
        with patch.object(route, 'local_worker_context', return_value=True), \
             patch('test_runner.prepare_generic_recipe_oracle', side_effect=lambda: events.append('oracle')), \
             patch.object(os, 'execvp', side_effect=lambda *_: events.append('recipe')) as launch:
            route.main(['ci'])
        self.assertEqual(events, ['oracle', 'recipe'])
        self.assertEqual(launch.call_args.args[1], ['just', '_ci-worker'])

    def test_oracle_provision_failure_refuses_cargo_recipe(self):
        with patch.object(route, 'local_worker_context', return_value=True), \
             patch('test_runner.prepare_generic_recipe_oracle', side_effect=RuntimeError('invalid oracle')), \
             patch.object(os, 'execvp') as launch:
            with self.assertRaisesRegex(RuntimeError, 'invalid oracle'):
                route.main(['test'])
        launch.assert_not_called()

    def test_fleet_worker_verifies_checkout_and_fixtures_before_suite(self):
        events = []
        with patch.object(route, 'local_worker_context', return_value=True), \
             patch.object(Path, 'cwd', return_value=route.FLEET_SOURCE), \
             patch.object(route, 'trusted_marker', return_value=True), \
             patch.object(route, 'prepare_fleet_checkout', side_effect=lambda: events.append('signed checkout')), \
             patch('test_runner.prepare_fleet_recipe_oracle', side_effect=lambda: events.append('locked fixtures')), \
             patch.object(os, 'execvp', side_effect=lambda *_: events.append('suite')) as launch:
            route.main(['fleet-test'])
        self.assertEqual(events, ['signed checkout', 'locked fixtures', 'suite'])
        self.assertEqual(launch.call_args.args[1], ['just', '_fleet-test-worker'])

    def test_ci_standard_public_worker_enters_verified_git_checkout(self):
        events = []
        with patch.object(route, 'local_worker_context', return_value=True), \
             patch.object(Path, 'cwd', return_value=route.FLEET_SOURCE), \
             patch.object(route, 'trusted_marker', return_value=True), \
             patch.object(route, 'prepare_fleet_checkout', side_effect=lambda: events.append('checkout')), \
             patch.object(route, 'select_signed_builder_target', side_effect=lambda: events.append('target')), \
             patch('test_runner.prepare_generic_recipe_oracle', side_effect=lambda: events.append('oracle')), \
             patch('test_runner.prepare_fleet_recipe_oracle', side_effect=AssertionError('fleet-only corpus contract')), \
             patch.object(os, 'execvp', side_effect=lambda *_: events.append('recipe')) as launch:
            route.main(['ci-standard'])
        self.assertEqual(events, ['checkout', 'target', 'oracle', 'recipe'])
        self.assertEqual(launch.call_args.args[1], ['just', '_ci-standard-worker'])

    def test_signed_builder_target_is_sibling_and_generic_source_stays_separate(self):
        self.assertNotIn(route.FLEET_CARGO_TARGET, route.FLEET_CHECKOUT.parents)
        self.assertNotEqual(route.FLEET_CARGO_TARGET, route.FLEET_CHECKOUT)
        self.assertNotIn(Path('/target'), Path('/src').parents)
        with patch.dict(os.environ, {'CARGO_TARGET_DIR': '/target'}):
            route.select_signed_builder_target()
            self.assertEqual(os.environ['CARGO_TARGET_DIR'], '/target/cargo')
        with patch.dict(os.environ, {'CARGO_TARGET_DIR': '/unexpected'}), \
             self.assertRaisesRegex(RuntimeError, 'unexpected Cargo target'):
            route.select_signed_builder_target()
        with patch.object(route, 'FLEET_CARGO_TARGET', Path('/target')), \
             patch.dict(os.environ, {'CARGO_TARGET_DIR': '/target'}), \
             self.assertRaisesRegex(RuntimeError, 'contains the source checkout'):
            route.select_signed_builder_target()

    def test_public_signed_fleet_selects_target_before_oracle_and_worker(self):
        events = []
        def oracle():
            events.append(('oracle', os.environ.get('CARGO_TARGET_DIR')))
        with patch.dict(os.environ, {'CARGO_TARGET_DIR': '/target'}), \
             patch.object(route, 'local_worker_context', return_value=True), \
             patch.object(Path, 'cwd', return_value=route.FLEET_SOURCE), \
             patch.object(route, 'trusted_marker', return_value=True), \
             patch.object(route, 'prepare_fleet_checkout'), \
             patch('test_runner.prepare_generic_recipe_oracle', side_effect=oracle), \
             patch.object(os, 'execvp', side_effect=lambda *_: events.append(('worker', os.environ.get('CARGO_TARGET_DIR')))):
            route.main(['test-ignored'])
        self.assertEqual(events, [('oracle', '/target/cargo'), ('worker', '/target/cargo')])

    def test_ordinary_just_test_keeps_separate_generic_target(self):
        seen = []
        with patch.dict(os.environ, {'CARGO_TARGET_DIR': '/target'}), \
             patch.object(route, 'local_worker_context', return_value=True), \
             patch('test_runner.prepare_generic_recipe_oracle',
                   side_effect=lambda: seen.append(('oracle', os.environ['CARGO_TARGET_DIR']))), \
             patch.object(os, 'execvp',
                          side_effect=lambda *_: seen.append(('worker', os.environ['CARGO_TARGET_DIR']))):
            route.main(['test'])
        self.assertEqual(seen, [('oracle', '/target'), ('worker', '/target')])

    def test_private_signed_workers_refuse_unbound_target_before_prerequisite_build(self):
        with patch.object(route.sys, 'platform', 'linux'), \
             patch.object(route.Path, 'cwd', return_value=route.FLEET_CHECKOUT), \
             patch.object(route, 'trusted_marker', return_value=True), \
             patch.object(route, 'verified_fleet_checkout', return_value=True), \
             patch.object(route, 'verify_ci_fleet_checkout') as ci:
            for recipe in (None, 'fleet-test', 'fleet-tests-both', 'test-ignored'):
                args = ['--require-local-context', *([recipe] if recipe else [])]
                with self.subTest(recipe=recipe), patch.dict(os.environ, {'CARGO_TARGET_DIR': '/target'}), \
                     self.assertRaisesRegex(SystemExit, 'separate Cargo target'):
                    route.main(args)
            with patch.dict(os.environ, {'CARGO_TARGET_DIR': str(route.FLEET_CARGO_TARGET)}):
                self.assertEqual(route.main(['--require-local-context']), 0)
                self.assertEqual(route.main(['--require-local-context', 'test-ignored']), 0)
            ci.assert_not_called()
        if shutil.which('just') is not None:
            repo = Path(__file__).resolve().parents[3]
            for recipe in ('_fleet-test-worker', '_fleet-tests-both-worker', '_test-ignored-worker'):
                rendered = subprocess.run(['just', '--dry-run', recipe], cwd=repo,
                                          capture_output=True, text=True, check=True)
                self.assertEqual(rendered.stderr.splitlines()[0],
                                 'python3 tools/remote-build/route.py --require-local-context')

    def test_public_and_private_signed_targets_reject_aliases_and_ancestor(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            checkout = root / 'checkout'
            checkout.mkdir()
            target = root / 'cargo'
            other = root / 'other'
            other.mkdir()
            with patch.object(route, 'FLEET_CHECKOUT', checkout), \
                 patch.object(route, 'FLEET_CARGO_TARGET', target), \
                 patch.object(route.Path, 'cwd', return_value=checkout), \
                 patch.object(route, 'local_worker_context', return_value=True), \
                 patch.object(route, 'trusted_marker', return_value=True):
                with patch.dict(os.environ, {'CARGO_TARGET_DIR': str(target)}):
                    self.assertEqual(route.main(['--require-local-context']), 0)
                    route.select_signed_builder_target()
                    for destination in (checkout, other):
                        with self.subTest(destination=destination):
                            target.symlink_to(destination, target_is_directory=True)
                            try:
                                with self.assertRaisesRegex(RuntimeError, 'not canonical'):
                                    route.main(['--require-local-context'])
                                with self.assertRaisesRegex(RuntimeError, 'not canonical'):
                                    route.select_signed_builder_target()
                            finally:
                                target.unlink()
                    target.write_text('not a directory')
                    with self.assertRaisesRegex(RuntimeError, 'not canonical'):
                        route.main(['--require-local-context'])
                    target.unlink()
            with patch.object(route, 'FLEET_CHECKOUT', checkout), \
                 patch.object(route, 'FLEET_CARGO_TARGET', root), \
                 patch.object(route.Path, 'cwd', return_value=checkout), \
                 patch.object(route, 'local_worker_context', return_value=True), \
                 patch.object(route, 'trusted_marker', return_value=True), \
                 patch.dict(os.environ, {'CARGO_TARGET_DIR': str(root)}):
                with self.assertRaisesRegex(RuntimeError, 'contains the source checkout'):
                    route.main(['--require-local-context'])
                with self.assertRaisesRegex(RuntimeError, 'contains the source checkout'):
                    route.select_signed_builder_target()

    def test_both_hub_suite_requires_signed_checkout_and_locked_oracle(self):
        events = []
        with patch.object(route, 'local_worker_context', return_value=True), \
             patch.object(Path, 'cwd', return_value=route.FLEET_SOURCE), \
             patch.object(route, 'trusted_marker', return_value=True), \
             patch.object(route, 'prepare_fleet_checkout', side_effect=lambda: events.append('signed checkout')), \
             patch('test_runner.prepare_fleet_recipe_oracle', side_effect=lambda: events.append('locked fixtures')), \
             patch.object(os, 'execvp', side_effect=lambda *_: events.append('suite')) as launch:
            route.main(['fleet-tests-both'])
        self.assertEqual(events, ['signed checkout', 'locked fixtures', 'suite'])
        self.assertEqual(launch.call_args.args[1], ['just', '_fleet-tests-both-worker'])

    def test_fleet_fixture_refusal_prevents_suite_dispatch(self):
        for recipe in ('fleet-test', 'fleet-tests-both'):
            with self.subTest(recipe=recipe), \
                 patch.object(route, 'local_worker_context', return_value=True), \
                 patch.object(Path, 'cwd', return_value=route.FLEET_SOURCE), \
                 patch.object(route, 'trusted_marker', return_value=True), \
                 patch.object(route, 'prepare_fleet_checkout'), \
                 patch('test_runner.prepare_fleet_recipe_oracle', side_effect=RuntimeError('corpus missing')), \
                 patch.object(os, 'execvp') as launch:
                with self.assertRaisesRegex(RuntimeError, 'corpus missing'):
                    route.main([recipe])
                launch.assert_not_called()

    def test_staged_bundle_supplies_history_to_fleet_checkout(self):
        import qualification_bootstrap
        repo_root = Path(__file__).resolve().parents[3]
        sys.path.insert(0, str(repo_root / 'tools/fleet'))
        from intent import check_history
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            source, checkout = root / 'source', root / 'checkout'
            source.mkdir()
            subprocess.run(['git', 'init', '-q', str(source)], check=True)
            required = ('justfile', 'rust-toolchain.toml',
                        'tools/remote-build/route.py',
                        'tools/remote-build/qualification_bootstrap.py',
                        'tools/remote-build/qualification_source.py',
                        'tools/remote-build/test_runner.py',
                        'tools/release/bootstrap_oracle.py')
            for name in required:
                path = source / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(name + '\n')
            (source / 'sample').write_text('source')
            subprocess.run(['git', '-C', str(source), 'add', '.'], check=True)
            subprocess.run(['git', '-C', str(source), '-c', 'user.name=Fixture',
                            '-c', 'user.email=fixture@example.invalid', 'commit', '-q',
                            '-m', 'Route SWF'], check=True)
            head = subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
            subprocess.run(['git', '-C', str(source), 'bundle', 'create',
                            str(source / 'repository.bundle'), 'HEAD'], check=True)
            (source / 'fleet-source-head').write_text(head + '\n')
            (source / 'maintainer.allowed_signers').write_text('fixture signer\n')
            old_cwd = Path.cwd()
            try:
                os.chdir(source)
                with patch.object(route, 'FLEET_SOURCE', source), \
                     patch.object(route, 'FLEET_CHECKOUT', checkout), \
                     patch.object(qualification_bootstrap, 'verify_staged_checkout') as verify:
                    route.prepare_fleet_checkout()
                    self.assertTrue(check_history(checkout, {'formats': ['SWF']}).hit)
                    self.assertEqual(verify.call_args.args[0:2], (checkout, head))
                    subprocess.run(['git', '-C', str(checkout), 'update-index', '--assume-unchanged', 'sample'], check=True)
                    (checkout / 'sample').write_text('hidden bytes')
                    self.assertEqual(subprocess.check_output(
                        ['git', '-C', str(checkout), 'status', '--porcelain']).strip(), b'')
                    with self.assertRaisesRegex(RuntimeError, 'differs from selected HEAD: sample'):
                        route.verified_fleet_checkout()
                    (checkout / 'sample').write_text('source')
                    subprocess.run(['git', '-C', str(checkout), 'update-index', '--no-assume-unchanged', 'sample'], check=True)
                    subprocess.run(['git', '-C', str(checkout), 'update-index', '--skip-worktree', 'sample'], check=True)
                    (checkout / 'sample').chmod(0o755)
                    self.assertEqual(subprocess.check_output(
                        ['git', '-C', str(checkout), 'status', '--porcelain']).strip(), b'')
                    with self.assertRaisesRegex(RuntimeError, 'differs from selected HEAD: sample'):
                        route.verified_fleet_checkout()
            finally:
                os.chdir(old_cwd)

    def test_actions_fleet_routes_in_place_and_private_guard_rechecks_source(self):
        events = []
        ci_env = {'CI': 'true', 'GITHUB_ACTIONS': 'true',
                  'RUNNER_ENVIRONMENT': 'self-hosted', 'RUNNER_OS': 'Linux',
                  'RUNNER_NAME': 'spot-runner', 'GITHUB_RUN_ID': '123'}
        with patch.object(route.sys, 'platform', 'linux'), \
             patch.dict(os.environ, ci_env), \
             patch.object(route, 'trusted_marker', side_effect=lambda path: path == route.RUNNER_MARKER), \
             patch.object(route, 'verify_ci_fleet_checkout', side_effect=lambda: events.append('verified CI checkout')), \
             patch.object(route, 'prepare_fleet_checkout') as stage, \
             patch('test_runner.prepare_fleet_recipe_oracle', side_effect=lambda: events.append('locked fixtures')), \
             patch.object(os, 'execvp', side_effect=lambda *_: events.append('suite')):
            route.main(['fleet-test'])
            self.assertEqual(events, ['verified CI checkout', 'locked fixtures', 'suite'])
            stage.assert_not_called()
            events.clear()
            route.main(['fleet-tests-both'])
            self.assertEqual(events, ['verified CI checkout', 'locked fixtures', 'suite'])
            stage.assert_not_called()
            events.clear()
            route.main(['--require-local-context', 'fleet-test'])
            self.assertEqual(events, ['verified CI checkout'])

    def test_perl_producer_uses_signed_builder_route_without_oracle_bootstrap(self):
        head='a'*40;tree='b'*40
        with patch.object(route,'local_worker_context',return_value=True), \
             patch.object(route.Path,'cwd',return_value=route.FLEET_SOURCE), \
             patch.object(route,'trusted_marker',return_value=True), \
             patch.object(route,'prepare_fleet_checkout') as prepare, \
             patch.object(route,'select_signed_builder_target') as target, \
             patch('test_runner.prepare_fleet_recipe_oracle') as oracle, \
             patch.object(route.os,'execvp') as execute:
            route.main(['freeze-linux-perl',head,tree])
        prepare.assert_called_once()
        target.assert_called_once()
        oracle.assert_not_called()
        execute.assert_called_once_with('just',['just','_freeze-linux-perl-worker',head,tree])

    def test_component_route_uses_signed_builder_without_oracle_or_public_path(self):
        envelope='/approved/data/approved-linux-perl.json'
        with patch.object(route,'local_worker_context',return_value=False), \
             patch.object(os,'execv') as launch:
            route.main(['prove-linux-perl-component',envelope])
        self.assertEqual(launch.call_args.args[1][-3:],
                         ['--just-recipe','prove-linux-perl-component',
                          '--approved-linux-perl-envelope='+envelope])
        with patch.object(route,'local_worker_context',return_value=False):
            with self.assertRaisesRegex(SystemExit,'absolute envelope'):
                route.main(['prove-linux-perl-component','relative.json'])
        run_id='component-'+'a'*32
        with patch.object(route,'local_worker_context',return_value=True), \
             patch.object(route.Path,'cwd',return_value=route.FLEET_SOURCE), \
             patch.object(route,'trusted_marker',return_value=True), \
             patch.object(route,'prepare_fleet_checkout') as prepare, \
             patch.object(route,'select_signed_builder_target') as target, \
             patch('test_runner.prepare_generic_recipe_oracle') as generic, \
             patch('test_runner.prepare_fleet_recipe_oracle') as fleet, \
             patch.object(route.os,'execvp') as worker:
            route.main(['prove-linux-perl-component',run_id])
        prepare.assert_called_once()
        target.assert_called_once()
        generic.assert_not_called()
        fleet.assert_not_called()
        worker.assert_called_once_with('just',['just','_prove-linux-perl-component-worker',run_id])
        with patch.object(route,'local_worker_context',return_value=True):
            with self.assertRaisesRegex(SystemExit,'private run ID'):
                route.main(['prove-linux-perl-component',envelope])
        if shutil.which('just'):
            repository=Path(__file__).resolve().parents[3]
            public=subprocess.run(['just','--dry-run','prove-linux-perl-component-remote',envelope],
                                  cwd=repository,capture_output=True,text=True,check=True)
            self.assertIn('route.py prove-linux-perl-component',public.stderr)
            private=subprocess.run(['just','--dry-run','_prove-linux-perl-component-worker',run_id],
                                   cwd=repository,capture_output=True,text=True,check=True)
            self.assertIn('--require-local-context prove-linux-perl-component',private.stderr)
            self.assertIn('prove_linux_perl_component.py',private.stderr)

    def test_linux_perl_full_suite_prepares_locked_oracle_before_worker(self):
        events = []
        def prepared():
            self.assertTrue(all(key not in os.environ for key in
                                ('PERL5LIB', 'PERLLIB', 'PERL5OPT')))
            events.append('oracle')
        with patch.object(route,'local_worker_context',return_value=True), \
             patch.object(route.Path,'cwd',return_value=route.FLEET_SOURCE), \
             patch.object(route,'trusted_marker',return_value=True), \
             patch.object(route,'prepare_fleet_checkout',side_effect=lambda: events.append('signed')), \
             patch.object(route,'select_signed_builder_target',side_effect=lambda: events.append('target')), \
             patch.dict(os.environ, {'PERL5LIB':'/untrusted/lib', 'PERLLIB':'/untrusted/lib',
                                      'PERL5OPT':'-MHostile'}), \
             patch('test_runner.prepare_generic_recipe_oracle', side_effect=prepared), \
             patch('test_runner.prepare_fleet_recipe_oracle',side_effect=AssertionError('fleet wrapper changed canonical cache')), \
             patch.object(route.os,'execvp',side_effect=lambda *_: events.append('worker')) as execute:
            route.main(['verify-linux-perl'])
        self.assertEqual(events, ['signed', 'target', 'oracle', 'worker'])
        execute.assert_called_once_with('just',['just','_verify-linux-perl-worker'])

    def test_linux_perl_generic_preparation_preserves_canonical_cache_environment(self):
        import test_runner
        with tempfile.TemporaryDirectory() as directory:
            cargo = Path(directory) / 'cargo'
            lock = test_runner.ROOT / 'tools/release/oracle-lock.json'
            pin = (test_runner.ROOT / '.exiftool-version').read_text().strip()
            cache = test_runner.oracle_cache_root(lock, cargo)
            manifest = Path(directory) / 'manifest.json'
            manifest.write_text(json.dumps({
                'lock_sha256': test_runner.file_sha(lock),
                'artifacts': {
                    'perl_executable': {'sha256': 'a' * 64},
                    'exiftool_tree': {'sha256': 'b' * 64},
                },
                'probes': {'docx': 'DOCX'},
            }))
            def load(root):
                self.assertEqual(root, cache)
                self.assertEqual(os.environ['OXIDEX_OPS_DIR'], str(cache))
                self.assertEqual(os.environ['EXIFTOOL_CACHE_DIR'],
                                 str(cache / 'cache/exiftool' / pin))
                self.assertEqual(os.environ['EXIFTOOL_PERL'],
                                 str(cache / 'toolchains/perl-5.38.2/prefix/bin/perl5.38.2'))
                self.assertEqual(os.environ['OXIDEX_RELEASE_REQUIRE_PINNED_FIXTURES'], '1')
                self.assertNotIn('EXIFTOOL', os.environ)
                self.assertNotIn('OXIDEX_ALLOW_EXIFTOOL_SKEW', os.environ)
                return object()
            with patch.dict(os.environ, {
                'CARGO_HOME': str(cargo), 'OXIDEX_OPS_DIR': '/target/ops',
                'EXIFTOOL_CACHE_DIR': '/target/cargo/fleet-oracle-receipts/hostile',
                'EXIFTOOL': '/untrusted/oracle', 'OXIDEX_ALLOW_EXIFTOOL_SKEW': '1',
            }), patch.object(test_runner, 'load_oracle_bootstrap', side_effect=load), \
                 patch.object(test_runner, 'provision_cached_oracle',
                              return_value=(cache, manifest)) as provision:
                self.assertEqual(test_runner.prepare_generic_recipe_oracle(), manifest)
                provision.assert_called_once()
                self.assertEqual(provision.call_args.args[1:], (lock, cargo))
                self.assertEqual(os.environ['EXIFTOOL_CACHE_DIR'],
                                 str(cache / 'cache/exiftool' / pin))

    def test_linux_perl_public_and_private_just_commands_are_guarded(self):
        if shutil.which('just') is None:
            self.skipTest('just is unavailable')
        repository = Path(__file__).resolve().parents[3]
        public = subprocess.run(['just','--dry-run','verify-linux-perl-remote'],
                                cwd=repository,capture_output=True,text=True,check=True)
        self.assertEqual(public.stderr.splitlines(),
                         ['python3 tools/remote-build/route.py verify-linux-perl'])
        worker_entry = subprocess.run(['just','--dry-run','verify-linux-perl'],
                                      cwd=repository,capture_output=True,text=True,check=True)
        self.assertEqual(worker_entry.stderr.splitlines(),
                         ['python3 tools/remote-build/route.py verify-linux-perl'])
        private = subprocess.run(['just','--dry-run','_verify-linux-perl-worker'],
                                 cwd=repository,capture_output=True,text=True,check=True)
        lines = private.stderr.splitlines()
        self.assertEqual(lines[0],
                         'python3 tools/remote-build/route.py --require-local-context verify-linux-perl')
        self.assertEqual(len(lines),7)
        self.assertIn('test_bootstrap_oracle',lines[1])
        self.assertIn('test_approved_linux_perl',lines[1])
        self.assertIn('test_freeze_linux_perl',lines[1])
        self.assertIn('test_prove_linux_perl_component',lines[1])
        self.assertIn("test_qualification*.py",lines[2])
        self.assertIn("test_generic_recipe.py",lines[3])
        self.assertIn("test_route.py",lines[4])
        self.assertIn("test_qualification_bootstrap_boundary.py",lines[5])
        self.assertIn("test_version_transition_qualification.PlatformPerlIdentityTests",lines[6])

    def test_actions_fleet_checkout_accepts_full_and_shallow_merge_history(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            source = root / 'source'
            source.mkdir()
            def git(*args, cwd=source):
                return subprocess.run(['git', '-C', str(cwd), *args], check=True,
                                      capture_output=True, text=True).stdout.strip()
            def commit(message):
                git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                    'commit', '-qm', message)
            git('init', '-q')
            required = ('justfile', 'rust-toolchain.toml',
                        'tools/remote-build/route.py',
                        'tools/remote-build/qualification_bootstrap.py',
                        'tools/remote-build/qualification_source.py',
                        'tools/remote-build/test_runner.py',
                        'tools/release/bootstrap_oracle.py')
            for name in required:
                path = source / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(name + '\n')
            git('add', '.')
            commit('SWF base')
            base_branch = git('branch', '--show-current')
            git('checkout', '-qb', 'feature')
            (source / 'feature.txt').write_text('feature\n')
            git('add', '.')
            commit('feature')
            git('checkout', '-q', base_branch)
            (source / 'justfile').write_text('second\n')
            git('add', 'justfile')
            commit('base advance')
            git('-c', 'user.name=GitHub', '-c', 'user.email=noreply@github.com',
                'merge', '-q', '--no-ff', 'feature', '-m', 'synthetic PR merge')
            merge_head = git('rev-parse', 'HEAD')
            self.assertEqual(len(git('rev-list', '--parents', '-n', '1', 'HEAD').split()), 3)
            ci_env = {'GITHUB_SERVER_URL': 'https://github.com',
                      'GITHUB_REPOSITORY': 'swack-tools/oxidex',
                      'GITHUB_WORKSPACE': str(source), 'GITHUB_SHA': merge_head,
                      'CI': 'true', 'GITHUB_ACTIONS': 'true',
                      'RUNNER_ENVIRONMENT': 'self-hosted', 'RUNNER_OS': 'Linux',
                      'RUNNER_NAME': 'spot-runner', 'GITHUB_RUN_ID': '123',
                      'CARGO_TARGET_DIR': str(source / 'target')}
            old_cwd = Path.cwd()
            try:
                os.chdir(source)
                with patch.dict(os.environ, ci_env):
                    route.verify_ci_fleet_checkout()
                    with patch.object(route.sys, 'platform', 'linux'), \
                         patch.object(route, 'trusted_marker',
                                      side_effect=lambda path: path == route.RUNNER_MARKER), \
                         patch.object(ignored_suite, 'ROOT', source):
                        self.assertEqual(route.main(['--require-local-context', 'test-ignored']), 0)
                        self.assertEqual(ignored_suite.admitted_context(),
                                         ('actions-scratch', source / 'target'))
                        (source / 'justfile').chmod(0o755)
                        with self.assertRaisesRegex(RuntimeError, 'not clean|differs from selected HEAD'):
                            ignored_suite.admitted_context()
                        (source / 'justfile').chmod(0o644)
                        with patch.dict(os.environ, {'GITHUB_REPOSITORY': 'other/repo'}):
                            with self.assertRaisesRegex(RuntimeError, 'expected Actions repository'):
                                ignored_suite.admitted_context()
                    with patch.dict(os.environ, {'GITHUB_WORKSPACE': str(root)}):
                        with self.assertRaisesRegex(RuntimeError, 'canonical Actions workspace'):
                            route.verify_ci_fleet_checkout()
                    with patch.dict(os.environ, {'GITHUB_SHA': '0' * 40}):
                        with self.assertRaisesRegex(RuntimeError, 'selected Actions commit'):
                            route.verify_ci_fleet_checkout()
                    with patch.dict(os.environ, {'GITHUB_REPOSITORY': 'other/repo'}):
                        with self.assertRaisesRegex(RuntimeError, 'expected Actions repository'):
                            route.verify_ci_fleet_checkout()
                    (source / 'justfile').write_text('dirty\n')
                    with self.assertRaisesRegex(RuntimeError, 'not clean'):
                        route.verify_ci_fleet_checkout()
                    (source / 'justfile').write_text('second\n')
                    git('update-index', '--skip-worktree', 'justfile')
                    (source / 'justfile').write_text('unsigned change\n')
                    self.assertEqual(git('status', '--porcelain'), '')
                    with self.assertRaisesRegex(RuntimeError, 'differs from selected HEAD: justfile'):
                        route.verify_ci_fleet_checkout()
                    with patch.object(route.sys, 'platform', 'linux'), \
                         patch.object(route, 'trusted_marker',
                                      side_effect=lambda path: path == route.RUNNER_MARKER), \
                         patch.object(ignored_suite, 'ROOT', source):
                        with self.assertRaisesRegex(RuntimeError, 'differs from selected HEAD: justfile'):
                            ignored_suite.admitted_context()
                shallow = root / 'shallow'
                subprocess.run(['git', 'clone', '-q', '--depth=1', source.as_uri(), str(shallow)], check=True)
                os.chdir(shallow)
                shallow_head = git('rev-parse', 'HEAD', cwd=shallow)
                self.assertEqual(shallow_head, merge_head)
                self.assertEqual(git('rev-parse', '--is-shallow-repository', cwd=shallow), 'true')
                shallow_env = {**ci_env, 'GITHUB_WORKSPACE': str(shallow)}
                with patch.dict(os.environ, shallow_env), \
                     patch.object(route, 'CI_ORIGIN_URLS', frozenset({source.as_uri()})):
                    with patch.object(route, 'CI_ORIGIN_URLS', frozenset()):
                        with self.assertRaisesRegex(RuntimeError, 'origin is not the approved repository'):
                            route.verify_ci_fleet_checkout()
                    route.verify_ci_fleet_checkout()
                    self.assertEqual(git('rev-parse', '--is-shallow-repository', cwd=shallow), 'false')
                    self.assertEqual(git('rev-parse', 'HEAD', cwd=shallow), merge_head)
                    self.assertEqual(git('rev-list', '--count', 'HEAD', cwd=shallow), '4')
                    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'fleet'))
                    from intent import check_history
                    self.assertTrue(check_history(shallow, {'formats': ['SWF']}).hit)
                    self.assertEqual(git('status', '--porcelain', cwd=shallow), '')
            finally:
                os.chdir(old_cwd)

    def test_bad_names_and_args_refuse_before_dispatch(self):
        with patch.object(os,'execv') as launch:
            for argv in (['-bad'],['build','bad\narg'],['build','']):
                with self.assertRaises(SystemExit):route.main(argv)
            launch.assert_not_called()

class FleetBuilderCallFlowControls(unittest.TestCase):
    def test_public_builder_moves_to_signed_checkout_before_private_guard(self):
        state = {'cwd': route.FLEET_SOURCE}
        events = []
        def prepare():
            self.assertEqual(state['cwd'], route.FLEET_SOURCE)
            state['cwd'] = route.FLEET_CHECKOUT
            events.append('signed-checkout')
        with patch.object(route, 'local_worker_context', return_value=True), \
             patch.object(route.Path, 'cwd', side_effect=lambda: state['cwd']), \
             patch.object(route, 'trusted_marker', return_value=True), \
             patch.object(route, 'prepare_fleet_checkout', side_effect=prepare), \
             patch.object(route, 'verified_fleet_checkout', return_value=True), \
             patch.object(route, 'verify_ci_fleet_checkout', side_effect=AssertionError('CI guard on builder')) as ci, \
             patch('test_runner.prepare_fleet_recipe_oracle', side_effect=lambda: events.append('oracle')), \
             patch.object(os, 'execvp', side_effect=lambda *_: events.append('private-recipe')):
            route.main(['fleet-test'])
            self.assertEqual(state['cwd'], route.FLEET_CHECKOUT)
            route.main(['--require-local-context', 'fleet-test'])
        ci.assert_not_called()
        self.assertEqual(events, ['signed-checkout', 'oracle', 'private-recipe'])

    def test_direct_private_fleet_guard_from_unsigned_source_is_refused(self):
        with patch.object(route, 'local_worker_context', return_value=True), \
             patch.object(route.Path, 'cwd', return_value=route.FLEET_SOURCE), \
             patch.object(route, 'trusted_marker', return_value=True), \
             patch.object(route, 'verify_ci_fleet_checkout', side_effect=RuntimeError('not CI')) as ci:
            with self.assertRaisesRegex(RuntimeError, 'not CI'):
                route.main(['--require-local-context', 'fleet-test'])
        ci.assert_called_once()

    def test_public_fleet_just_recipes_enter_signed_route_before_private_guard(self):
        if shutil.which('just') is None:
            self.skipTest('just is unavailable')
        repository = Path(__file__).resolve().parents[3]
        for recipe in ('fleet-test', 'fleet-tests-both'):
            with self.subTest(recipe=recipe):
                public = subprocess.run(['just', '--dry-run', recipe], cwd=repository,
                                        capture_output=True, text=True, check=True)
                self.assertEqual(public.stderr.splitlines(),
                                 [f'python3 tools/remote-build/route.py {recipe}'])
                private = subprocess.run(['just', '--dry-run', '_' + recipe + '-worker'], cwd=repository,
                                         capture_output=True, text=True, check=True)
                self.assertIn(f'python3 tools/remote-build/route.py --require-local-context {recipe}',
                              private.stderr)
                self.assertIn('cargo build --bin oxidex --release', private.stderr)
                self.assertNotIn('route.py build-bin-release', private.stderr)
