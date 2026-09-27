"""Exercise model routing and review receipts without spending model tokens."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'codex_task.py'


class CodexTaskTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='.oxidex-codex-task-', dir=Path.home())
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
import json, os, pathlib, sys, time
pathlib.Path(os.environ['ARGS_LOG']).write_text(json.dumps(sys.argv[1:]))
if os.environ.get('SLEEP_PID_PATH'):
    pathlib.Path(os.environ['SLEEP_PID_PATH']).write_text(str(os.getpid()))
    time.sleep(30)
args = sys.argv
pathlib.Path(args[args.index('--output-last-message')+1]).write_text('Review findings require inspection.')
print(json.dumps({'type':'turn.completed','usage':{'input_tokens':0 if os.environ.get('ZERO_USAGE') else 10,'output_tokens':0 if os.environ.get('ZERO_USAGE') else 2}}))
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

    def test_temporary_evidence_override_is_refused(self):
        out = Path(tempfile.gettempdir()) / self.root.name
        result = self.run_task('review', '--base', self.base, '--output-dir', str(out))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('durable', result.stderr)
        self.assertFalse(self.args_log.exists())

    def test_default_evidence_checks_appended_subdirectory(self):
        ops = self.root/'ops'
        ops.mkdir()
        (ops/'evidence').symlink_to(tempfile.gettempdir(), target_is_directory=True)
        self.env['OXIDEX_OPS_DIR'] = str(ops)
        result = self.run_task('review', '--base', self.base, '--dry-run')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('durable', result.stderr)
        self.assertFalse(self.args_log.exists())

    def test_zero_reported_usage_is_unknown(self):
        self.env['ZERO_USAGE'] = '1'
        out = self.root/'run'
        result = self.run_task('review', '--base', self.base, '--output-dir', str(out))
        self.assertEqual(result.returncode, 0, result.stderr)
        receipt = json.loads((out/'receipt.json').read_text())
        self.assertIsNone(receipt['usage'])

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

    def assert_stopped_run(self, stop_signal=None):
        out = self.root / ('timeout' if stop_signal is None else f'signal-{stop_signal}')
        pid_path = self.root / (out.name + '.pid')
        self.env['SLEEP_PID_PATH'] = str(pid_path)
        process = subprocess.Popen(
            [sys.executable, str(SCRIPT), '--cwd', str(self.repo), 'review',
             '--base', self.base, '--output-dir', str(out),
             '--timeout', '1' if stop_signal is None else '20'],
            env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        child_pid = None
        try:
            deadline = time.monotonic() + 10
            while not pid_path.exists() and time.monotonic() < deadline:
                time.sleep(0.02)
            self.assertTrue(pid_path.exists(), 'fake Codex did not start')
            child_pid = int(pid_path.read_text())
            if stop_signal is not None:
                process.send_signal(stop_signal)
            stdout, stderr = process.communicate(timeout=10)
            self.assertEqual(process.returncode, 124 if stop_signal is None else 128 + stop_signal,
                             stdout + stderr)
            receipt = json.loads((out / 'receipt.json').read_text())
            self.assertEqual(receipt['status'], 'failed' if stop_signal is None else 'interrupted')
            self.assertEqual(receipt['approval'], 'not_assessed')
            self.assertIn('finished', receipt)
            with self.assertRaises(ProcessLookupError):
                os.kill(child_pid, 0)
        finally:
            if process.poll() is None:
                process.kill()
            process.communicate()
            if child_pid is not None:
                try:
                    os.killpg(child_pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass

    def test_termination_stops_child_and_finalizes_receipt(self):
        for stop_signal in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
            with self.subTest(signal=stop_signal):
                self.assert_stopped_run(stop_signal)

    def test_timeout_stops_child_and_finalizes_receipt(self):
        self.assert_stopped_run()


if __name__ == '__main__':
    unittest.main()
