"""Fleet fixture adapter controls; no downloads, Cargo, or real oracle calls."""
import json
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import test_runner


class FleetRecipeOracleTests(unittest.TestCase):
    def test_locked_fixtures_become_legacy_ledger_paths_and_are_probed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / '.exiftool-version').write_text('13.59\n')
            cache = root / 'cache'
            legacy = cache / 'cache/exiftool/13.59'
            (legacy / 'combined-samples').mkdir(parents=True)
            (legacy / 'exiftool').mkdir()
            manifest = root / 'manifest.json'
            manifest.write_text(json.dumps({'probes': {'corpus_files': 4000},
                                            'artifacts': {'perl_executable': {'sha256': 'c' * 64},
                                                          'exiftool_tree': {'sha256': 'a' * 64},
                                                          'corpus_tree': {'sha256': 'b' * 64}}}))
            calls = []
            target = root / "run-target"
            bootstrap = SimpleNamespace(
                perl_path=lambda _: cache / 'perl',
                exiftool_library_path=lambda _: legacy / 'exiftool/lib/Image/ExifTool.pm',
                exiftool_path=lambda _: legacy / 'exiftool/exiftool',
                exiftool_root=lambda _: legacy / 'exiftool',
                write_comparison_wrapper=lambda *args, **kwargs: calls.append((args, kwargs)),
                reject_symlink_components=lambda _: None)
            def probe(argv, **kwargs):
                calls.append(argv)
                return SimpleNamespace(stdout='13.59\n' if '-ver' in argv else 'DOCX\n')
            with patch.object(test_runner, 'ROOT', root), \
                 patch.object(test_runner, 'TARGET', target), \
                 patch.object(test_runner, 'prepare_generic_recipe_oracle', return_value=manifest), \
                 patch.object(test_runner, 'oracle_cache_root', return_value=cache), \
                 patch.object(test_runner, 'load_oracle_bootstrap', return_value=bootstrap), \
                 patch.object(test_runner.subprocess, 'run', side_effect=probe), \
                 patch.dict(os.environ, {'EXIFTOOL_CACHE_DIR': str(legacy), 'CARGO_HOME': str(root),
                                      'PERL5OPT': '-Mforeign', 'CARGO_TARGET_DIR': str(target)}):
                self.assertEqual(test_runner.prepare_fleet_recipe_oracle(), manifest)
                first_overlay = Path(os.environ['EXIFTOOL_CACHE_DIR'])
                os.environ['EXIFTOOL_CACHE_DIR'] = str(legacy)
                self.assertEqual(test_runner.prepare_fleet_recipe_oracle(), manifest)
                second_overlay = Path(os.environ['EXIFTOOL_CACHE_DIR'])
                self.assertNotIn('PERL5OPT', os.environ)
            first, second = calls[0][0], calls[3][0]
            self.assertEqual(first[0], first_overlay / 'exiftool-pinned.sh')
            self.assertEqual(first[1], cache / 'perl')
            self.assertEqual(calls[0][1]['marker_root'], target)
            self.assertEqual(first[4].parent, first_overlay)
            self.assertEqual(first_overlay.parent, target / 'fleet-oracle-receipts')
            self.assertEqual(second[0], second_overlay / 'exiftool-pinned.sh')
            self.assertNotEqual(first_overlay, second_overlay)
            self.assertEqual((first_overlay / 'exiftool').resolve(), (legacy / 'exiftool').resolve())
            self.assertEqual((first_overlay / 'combined-samples').resolve(),
                             (legacy / 'combined-samples').resolve())
            self.assertNotEqual(first[4], second[4])
            self.assertTrue(first[4].is_dir())
            self.assertTrue(second[4].is_dir())
            self.assertEqual(calls[1][1:], ['-ver'])
            self.assertEqual(calls[2][1:3], ['-s3', '-FileType'])

    def test_actions_default_target_uses_verified_workspace_when_cargo_target_unset(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            env = {'GITHUB_ACTIONS': 'true', 'GITHUB_REPOSITORY': 'swack-tools/oxidex',
                   'GITHUB_SERVER_URL': 'https://github.com', 'GITHUB_WORKSPACE': str(root)}
            with patch.object(test_runner, 'ROOT', root), \
                 patch.object(Path, 'cwd', return_value=root), \
                 patch.dict(os.environ, env, clear=True):
                self.assertEqual(test_runner.fleet_recipe_target(), root / 'target')
                self.assertEqual(os.environ['CARGO_TARGET_DIR'], str(root / 'target'))

    def test_corpus_below_floor_refuses_before_wrapper_or_suite(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / '.exiftool-version').write_text('13.59\n')
            manifest = root / 'manifest.json'
            manifest.write_text(json.dumps({'probes': {'corpus_files': 3999}}))
            with patch.object(test_runner, 'ROOT', root), \
                 patch.object(test_runner, 'prepare_generic_recipe_oracle', return_value=manifest), \
                 patch.object(test_runner, 'load_oracle_bootstrap') as bootstrap:
                with self.assertRaisesRegex(RuntimeError, 'file floor'):
                    test_runner.prepare_fleet_recipe_oracle()
            bootstrap.assert_not_called()
