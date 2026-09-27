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
        self.repo = Path(self.tmp.name).resolve()
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
        payload = dict(cwd=str(self.repo), hook_event_name='PostToolUse', tool_name=tool, tool_input=inputs)
        payload.update(extra)
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

    def test_outside_project_file_is_not_linted(self):
        result = self.invoke('Edit', {'file_path': '/outside/doc.md'})
        self.assertEqual(result.returncode, 0, result)
        self.assertFalse(self.log.exists())

    def shell(self, event, call='call', workdir=None, **extra):
        return self.invoke('Bash', {'command': 'command', **({'cwd': str(workdir)} if workdir else {})},
                           hook_event_name=event, tool_use_id=call, session_id='session', **extra)

    def test_readonly_never_lints_existing_bad_untracked_prose(self):
        self.assertEqual(self.shell('PreToolUse').returncode, 0)
        self.assertEqual(self.shell('PostToolUse').returncode, 0)
        self.assertFalse(self.log.exists())

    def test_shell_new_and_edited_prose_then_readonly_after_failure(self):
        for name in ['doc with spaces.md', 'new café doc.md']:
            self.assertEqual(self.shell('PreToolUse', name).returncode, 0)
            (self.repo/name).write_text('new bad prose')
            result = self.shell('PostToolUse', name)
            self.assertEqual(result.returncode, 2, result)
            self.assertIn(str(self.repo/name), json.loads(self.log.read_text().splitlines()[-1]))
        before = self.log.read_text()
        self.shell('PreToolUse', 'readonly')
        self.assertEqual(self.shell('PostToolUse', 'readonly').returncode, 0)
        self.assertEqual(before, self.log.read_text())

    def test_missing_duplicate_and_wrong_call_snapshots_skip(self):
        self.assertEqual(self.shell('PostToolUse').returncode, 0)
        self.shell('PreToolUse')
        (self.repo/'doc with spaces.md').write_text('changed')
        self.assertEqual(self.shell('PostToolUse', 'other').returncode, 0)
        self.assertEqual(self.shell('PostToolUse').returncode, 2)
        before = self.log.read_text()
        self.assertEqual(self.shell('PostToolUse').returncode, 0)
        self.assertEqual(before, self.log.read_text())

    def test_malformed_snapshot_skips(self):
        self.shell('PreToolUse')
        for state in (self.repo/'.git/prose-lint-snapshots').glob('*.json'):
            state.write_text('{"files":[]}')
        (self.repo/'doc with spaces.md').write_text('changed')
        self.assertEqual(self.shell('PostToolUse').returncode, 0)
        self.assertFalse(self.log.exists())

    def test_shell_without_call_identity_does_not_guess(self):
        self.invoke('Bash', {'command': 'command'}, hook_event_name='PreToolUse')
        (self.repo/'doc with spaces.md').write_text('changed')
        self.assertEqual(self.invoke('Bash', {'command': 'command'}).returncode, 0)
        self.assertFalse(self.log.exists())

    def test_explicit_workdir_targets_second_worktree(self):
        subprocess.run(['git', '-C', str(self.repo), 'add', '.vale.ini', 'doc with spaces.md'], check=True)
        subprocess.run(['git', '-C', str(self.repo), '-c', 'user.name=Test', '-c', 'user.email=test@example.com',
                        '-c', 'commit.gpgsign=false', 'commit', '-qm', 'initial'], check=True)
        second = self.repo/'second tree'
        subprocess.run(['git', '-C', str(self.repo), 'worktree', 'add', '-qb', 'second', str(second)], check=True)
        (second/'.vale/styles/Google').mkdir(parents=True)
        self.shell('PreToolUse', workdir=second)
        (second/'doc with spaces.md').write_text('second changed')
        (self.repo/'doc with spaces.md').write_text('unrelated changed')
        self.assertEqual(self.shell('PostToolUse', workdir=second).returncode, 2)
        args = json.loads(self.log.read_text().splitlines()[-1])
        self.assertIn(str(second/'doc with spaces.md'), args)
        self.assertNotIn(str(self.repo/'doc with spaces.md'), args)
        result = self.invoke('Edit', {'file_path': str(second/'doc with spaces.md')})
        self.assertEqual(result.returncode, 2, result)
        args = json.loads(self.log.read_text().splitlines()[-1])
        self.assertIn(str(second/'.vale.ini'), args)
        result = self.invoke('apply_patch', {'command': f'*** Update File: {second}/doc with spaces.md'})
        self.assertEqual(result.returncode, 2, result)

    def test_installed_codex_normalization_does_not_guess_second_worktree(self):
        # CLI 0.157.1 loses exec_command.workdir: cwd is the session root,
        # tool_name is Bash, and only command survives in tool_input.
        second = self.repo/'other'
        second.mkdir()
        (second/'new.md').write_text('old')
        subprocess.run(['git', 'init', '-q', str(second)], check=True)
        self.shell('PreToolUse')
        (second/'new.md').write_text('changed')
        self.assertEqual(self.shell('PostToolUse').returncode, 0)
        self.assertFalse(self.log.exists())

    def test_out_of_order_and_changed_inputs_skip(self):
        self.shell('PreToolUse')
        (self.repo/'doc with spaces.md').write_text('changed')
        result = self.invoke('Bash', {'command': 'different'}, session_id='session', tool_use_id='call')
        self.assertEqual(result.returncode, 0)
        self.assertFalse(self.log.exists())

    def test_ignored_and_generated_files_excluded(self):
        (self.repo/'.gitignore').write_text('ignored.md\n')
        self.shell('PreToolUse')
        (self.repo/'ignored.md').write_text('bad')
        generated = self.repo/'src/exiftool_tables/a.rs'
        generated.parent.mkdir(parents=True)
        generated.write_text('bad')
        (self.repo/'generated.md').write_text('<!-- Auto-generated. -->\nbad')
        self.assertEqual(self.shell('PostToolUse').returncode, 0)
        self.assertFalse(self.log.exists())
        self.assertEqual(self.invoke('Edit', {'file_path': str(self.repo/'ignored.md')}).returncode, 0)
        self.assertFalse(self.log.exists())

    def test_unrelated_repo_and_symlink_file_are_not_linted(self):
        unrelated = self.repo/'unrelated'
        unrelated.mkdir()
        subprocess.run(['git', 'init', '-q', str(unrelated)], check=True)
        doc = unrelated/'doc.md'
        doc.write_text('bad')
        self.assertEqual(self.invoke('Edit', {'file_path': str(doc)}).returncode, 0)
        link = self.repo/'link.md'
        link.symlink_to(self.repo/'doc with spaces.md')
        self.assertEqual(self.invoke('Edit', {'file_path': str(link)}).returncode, 0)
        self.assertFalse(self.log.exists())

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
