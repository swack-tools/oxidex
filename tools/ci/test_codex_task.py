"""Exercise model routing and review receipts without spending model tokens."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'codex_task.py'


class CodexTaskTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.repo = self.root / 'repo'
        self.repo.mkdir()
        self.git('init', '-q')
        self.git('config', 'user.name', 'Test')
        self.git('config', 'user.email', 'test@example.invalid')
        self.git('config', 'commit.gpgsign', 'false')
        (self.repo/'file.md').write_text('one\n')
        self.git('add', 'file.md')
        self.git('commit', '-qm', 'base')
        self.base = self.git('rev-parse', 'HEAD').strip()
        (self.repo/'file.md').write_text('two\n')
        self.git('commit', '-qam', 'candidate')
        self.bin = self.root/'bin'
        self.bin.mkdir()
        self.args_log = self.root/'args.json'
        codex = self.bin/'codex'
        codex.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
pathlib.Path(os.environ['ARGS_LOG']).write_text(json.dumps(sys.argv[1:]))
args = sys.argv
pathlib.Path(args[args.index('--output-last-message')+1]).write_text('Review findings require inspection.')
print(json.dumps({'type':'turn.completed','usage':{'input_tokens':10,'output_tokens':2}}))
if os.environ.get('MUTATE'):
    pathlib.Path('file.md').write_text('changed during review')
sys.exit(int(os.environ.get('FAKE_EXIT', '0')))
''')
        codex.chmod(0o755)
        self.env = dict(os.environ, PATH=str(self.bin)+os.pathsep+os.environ['PATH'], ARGS_LOG=str(self.args_log))

    def git(self, *args):
        return subprocess.check_output(['git', '-C', str(self.repo), *args], text=True)

    def run_task(self, *args):
        return subprocess.run([sys.executable, str(SCRIPT), '--cwd', str(self.repo), *args],
                              env=self.env, capture_output=True, text=True)

    def test_review_pins_models_base_and_read_only(self):
        out = self.root/'run'
        result = self.run_task('review', '--base', self.base, '--output-dir', str(out))
        self.assertEqual(result.returncode, 0, result.stderr)
        args = json.loads(self.args_log.read_text())
        self.assertEqual(args[args.index('--model')+1], 'gpt-6-sol')
        self.assertIn('review_model="gpt-6-sol"', args)
        self.assertIn('model_reasoning_effort="medium"', args)
        self.assertEqual(args[args.index('--sandbox')+1], 'read-only')
        self.assertEqual(args[args.index('--base')+1], self.base)
        receipt = json.loads((out/'receipt.json').read_text())
        self.assertEqual(receipt['status'], 'completed')
        self.assertEqual(receipt['approval'], 'not_assessed')
        self.assertEqual(receipt['usage']['input_tokens'], 10)

    def test_inventory_uses_luna_low_without_launching_in_dry_run(self):
        brief = self.root/'brief.txt'
        brief.write_text('Count files, read only.')
        result = self.run_task('inventory', '--brief', str(brief), '--dry-run')
        self.assertEqual(result.returncode, 0, result.stderr)
        config = json.loads(result.stdout)
        self.assertEqual((config['model'], config['effort']), ('gpt-6-luna', 'low'))
        self.assertFalse(self.args_log.exists())

    def test_model_override_requires_reason(self):
        result = self.run_task('review', '--base', self.base, '--model', 'gpt-6-astra', '--dry-run')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('reason', result.stderr)
        self.assertFalse(self.args_log.exists())

    def test_review_refuses_dirty_tree(self):
        (self.repo/'file.md').write_text('uncommitted')
        result = self.run_task('review', '--base', self.base, '--dry-run')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('clean', result.stderr)
        self.assertFalse(self.args_log.exists())

    def test_failed_process_has_failed_receipt(self):
        self.env['FAKE_EXIT'] = '3'
        out = self.root/'run'
        result = self.run_task('review', '--base', self.base, '--output-dir', str(out))
        self.assertEqual(result.returncode, 3, result.stderr)
        receipt = json.loads((out/'receipt.json').read_text())
        self.assertEqual(receipt['status'], 'failed')
        self.assertEqual(receipt['approval'], 'not_assessed')

    def test_mutation_during_review_invalidates_receipt(self):
        self.env['MUTATE'] = '1'
        out = self.root/'run'
        result = self.run_task('review', '--base', self.base, '--output-dir', str(out))
        self.assertNotEqual(result.returncode, 0)
        receipt = json.loads((out/'receipt.json').read_text())
        self.assertEqual(receipt['status'], 'stale')
        self.assertEqual(receipt['approval'], 'not_assessed')


if __name__ == '__main__':
    unittest.main()
