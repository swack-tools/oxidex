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
            manifest = root / 'manifest.json'
            manifest.write_text(json.dumps({'probes': {'corpus_files': 4000}}))
            calls = []
            bootstrap = SimpleNamespace(
                perl_path=lambda _: cache / 'perl',
                exiftool_library_path=lambda _: legacy / 'exiftool/lib/Image/ExifTool.pm',
                exiftool_path=lambda _: legacy / 'exiftool/exiftool',
                exiftool_root=lambda _: legacy / 'exiftool',
                write_comparison_wrapper=lambda *args: calls.append(args))
            def probe(argv, **kwargs):
                calls.append(argv)
                return SimpleNamespace(stdout='13.59\n' if '-ver' in argv else 'DOCX\n')
            with patch.object(test_runner, 'ROOT', root), \
                 patch.object(test_runner, 'prepare_generic_recipe_oracle', return_value=manifest), \
                 patch.object(test_runner, 'oracle_cache_root', return_value=cache), \
                 patch.object(test_runner, 'load_oracle_bootstrap', return_value=bootstrap), \
                 patch.object(test_runner.subprocess, 'run', side_effect=probe), \
                 patch.dict(os.environ, {'EXIFTOOL_CACHE_DIR': str(legacy), 'CARGO_HOME': str(root),
                                      'PERL5OPT': '-Mforeign'}):
                self.assertEqual(test_runner.prepare_fleet_recipe_oracle(), manifest)
                self.assertNotIn('PERL5OPT', os.environ)
            self.assertEqual(calls[0][0], legacy / 'exiftool-pinned.sh')
            self.assertEqual(calls[0][1], cache / 'perl')
            self.assertEqual(calls[1][1:], ['-ver'])
            self.assertEqual(calls[2][1:3], ['-s3', '-FileType'])

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
