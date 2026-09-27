"""Exercise the Claude and Codex hook protocols without touching real docs."""
import json
import os
import re
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
HOOK = ROOT / '.claude/hooks/prose-lint.sh'


class ProseHookTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.repo = Path(self.tmp.name)
        (self.repo / '.vale/styles/Google').mkdir(parents=True)
        (self.repo / '.vale.ini').write_text('StylesPath = .vale/styles\n')
        (self.repo / 'doc with spaces.md').write_text('bad prose')
        self.bin = self.repo / 'bin'
        self.bin.mkdir()
        self.log = self.repo / 'calls.jsonl'
        vale = self.bin / 'vale'
        vale.write_text('#!/usr/bin/env python3\nimport json, os, sys\n'
                        'with open(os.environ["VALE_TEST_LOG"], "a") as f: f.write(json.dumps(sys.argv[1:])+"\\n")\n'
                        'print("doc:1:1:error:Google.WordList:bad prose")\nsys.exit(int(os.environ.get("VALE_TEST_EXIT", "1")))\n')
        vale.chmod(0o755)
        self.env = dict(os.environ, PATH=str(self.bin)+os.pathsep+os.environ['PATH'], VALE_TEST_LOG=str(self.log))
        self.env.pop('CLAUDE_PROJECT_DIR', None)
        subprocess.run(['git', 'init', '-q', str(self.repo)], check=True)

    def invoke(self, tool, inputs, **extra):
        payload = dict(cwd=str(self.repo), hook_event_name='PostToolUse', tool_name=tool, tool_input=inputs, **extra)
        return subprocess.run(['bash', str(HOOK)], input=json.dumps(payload), env=self.env,
                              text=True, capture_output=True, cwd=self.repo)

    def test_registration_dispatches_codex_patch_payload(self):
        payload = {'command': '*** Begin Patch\n*** Update File: doc with spaces.md\n*** End Patch'}
        registrations = json.loads((ROOT/'.codex/hooks.json').read_text())['hooks']['PostToolUse']
        dispatched = False
        for registration in registrations:
            # Older Codex reports the raw tool name rather than Edit/Write.
            if re.fullmatch(registration['matcher'], 'apply_patch'):
                result = self.invoke('apply_patch', payload)
                self.assertEqual(result.returncode, 2, result)
                dispatched = True
        self.assertTrue(dispatched, 'real Codex apply_patch payload did not reach Vale')
        self.assertTrue(self.log.exists())

    def test_claude_file_path_feedback(self):
        result = self.invoke('Edit', {'file_path': str(self.repo/'doc with spaces.md')})
        self.assertEqual(result.returncode, 2, result)
        self.assertIn('Google.WordList', result.stderr)

    def test_codex_patch_feedback_without_claude_environment(self):
        result = self.invoke('apply_patch', {'command': '*** Begin Patch\n*** Update File: doc with spaces.md\n@@\n+bad prose\n*** End Patch'}, turn_id='codex-turn')
        self.assertEqual(result.returncode, 2, result)
        self.assertIn('Google.WordList', result.stderr)
        self.assertIn('doc with spaces.md', self.log.read_text())

    def test_shell_edit_checks_untracked_prose(self):
        result = self.invoke('Bash', {'command': 'write document'}, turn_id='codex-turn')
        self.assertEqual(result.returncode, 2, result)
        self.assertIn('doc with spaces.md', self.log.read_text())

    def test_outside_project_file_is_not_linted(self):
        result = self.invoke('Edit', {'file_path': '/outside/doc.md'})
        self.assertEqual(result.returncode, 0, result)
        self.assertFalse(self.log.exists())

    def test_read_only_shell_does_not_repeat_unchanged_feedback(self):
        self.env['VALE_TEST_EXIT'] = '0'
        first = self.invoke('Bash', {'command': 'edit file'}, session_id='same-session')
        self.assertEqual(first.returncode, 0, first)
        before = self.log.read_text()
        second = self.invoke('Bash', {'command': 'true'}, session_id='same-session')
        self.assertEqual(second.returncode, 0, second)
        self.assertEqual(self.log.read_text(), before)
        (self.repo/'doc with spaces.md').write_text('new prose')
        self.env['VALE_TEST_EXIT'] = '1'
        third = self.invoke('Bash', {'command': 'edit file'}, session_id='same-session')
        self.assertEqual(third.returncode, 2, third)
        self.assertNotEqual(self.log.read_text(), before)

    def test_changed_config_rechecks_unchanged_dirty_prose(self):
        self.env['VALE_TEST_EXIT'] = '0'
        self.invoke('Bash', {'command': 'true'}, session_id='config-change')
        before = self.log.read_text()
        (self.repo/'.vale.ini').write_text('StylesPath = .vale/styles\nMinAlertLevel = error\n')
        self.env['VALE_TEST_EXIT'] = '1'
        result = self.invoke('Bash', {'command': 'true'}, session_id='config-change')
        self.assertEqual(result.returncode, 2, result)
        self.assertNotEqual(self.log.read_text(), before)

    def test_changed_style_rechecks_unchanged_dirty_prose(self):
        self.env['VALE_TEST_EXIT'] = '0'
        self.invoke('Bash', {'command': 'true'}, session_id='style-change')
        before = self.log.read_text()
        (self.repo/'.vale/styles/Google/New.yml').write_text('extends: existence\n')
        self.env['VALE_TEST_EXIT'] = '1'
        result = self.invoke('Bash', {'command': 'true'}, session_id='style-change')
        self.assertEqual(result.returncode, 2, result)
        self.assertNotEqual(self.log.read_text(), before)

    def test_failed_vale_run_is_not_cached(self):
        self.env['VALE_TEST_EXIT'] = '2'
        self.invoke('Bash', {'command': 'true'}, session_id='failed-check')
        before = self.log.read_text()
        self.env['VALE_TEST_EXIT'] = '0'
        result = self.invoke('Bash', {'command': 'true'}, session_id='failed-check')
        self.assertEqual(result.returncode, 0, result)
        self.assertNotEqual(self.log.read_text(), before)

    def test_clean_prose_returns_success(self):
        self.env['VALE_TEST_EXIT'] = '0'
        result = self.invoke('apply_patch', {'command': '*** Begin Patch\n*** Update File: doc with spaces.md\n*** End Patch'}, turn_id='codex-turn')
        self.assertEqual(result.returncode, 0, result)
        self.assertEqual(result.stderr, '')
        self.assertTrue(self.log.exists())

    def test_deleted_patch_file_is_not_linted(self):
        result = self.invoke('apply_patch', {'command': '*** Begin Patch\n*** Delete File: gone.md\n*** End Patch'}, turn_id='codex-turn')
        self.assertEqual(result.returncode, 0, result)
        self.assertFalse(self.log.exists())


if __name__ == '__main__':
    unittest.main()
