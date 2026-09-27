"""Exercise the Claude and Codex hook protocols without touching real docs."""
import json
import os
import re
from pathlib import Path
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch

from tools import prose_lint

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

    def test_snapshot_prunes_owned_stale_orphans_preserving_unknowns(self):
        directory = self.repo/'snapshots'
        directory.mkdir()
        stale = directory/('a'*64+'.json')
        orphan = directory/('b'*64+'.123.tmp')
        unknown = directory/'other.json'
        target = directory/'unknown-target'
        symlink = directory/('c'*64+'.json')
        for path in [stale, orphan, unknown, target]:
            path.write_text('preserve unknown, prune owned')
            os.utime(path, (time.time()-4000, time.time()-4000))
        symlink.symlink_to(target)
        fresh = directory/('d'*64+'.json')
        prose_lint.store_snapshot(directory, fresh, {'files': {}})
        self.assertFalse(stale.exists())
        self.assertFalse(orphan.exists())
        self.assertTrue(fresh.exists())
        self.assertEqual(unknown.read_text(), 'preserve unknown, prune owned')
        self.assertTrue(symlink.is_symlink())
        self.assertTrue(target.exists())

    def test_snapshot_cap_preserves_active_calls_and_recovers_after_consumption(self):
        directory = self.repo/'snapshots'
        first = directory/('a'*64+'.json')
        second = directory/('b'*64+'.json')
        third = directory/('c'*64+'.json')
        with patch.object(prose_lint, 'MAX_PENDING_SNAPSHOTS', 2):
            prose_lint.store_snapshot(directory, first, {'first': 1})
            prose_lint.store_snapshot(directory, second, {'second': 2})
            prose_lint.store_snapshot(directory, third, {'third': 3})
            self.assertFalse(third.exists())
            self.assertEqual(json.loads(first.read_text()), {'first': 1})
            self.assertEqual(json.loads(second.read_text()), {'second': 2})
            prose_lint.store_snapshot(directory, first, {'replaced': True})
            self.assertEqual(json.loads(first.read_text()), {'first': 1})
            first.unlink()
            prose_lint.store_snapshot(directory, third, {'third': 3})
            self.assertTrue(third.exists())

    def test_input_digest_is_deterministic_and_does_not_store_command(self):
        inputs = {'command': 'private shell text', 'workdir': '/project'}
        self.assertEqual(prose_lint.input_digest(inputs),
                         prose_lint.input_digest(dict(reversed(list(inputs.items())))))
        self.assertNotEqual(prose_lint.input_digest(inputs),
                            prose_lint.input_digest({**inputs, 'command': 'different'}))
        self.shell('PreToolUse')
        state = next((self.repo/'.git/prose-lint-snapshots').glob('*.json'))
        stored = json.loads(state.read_text())
        self.assertNotIn('input', stored)
        self.assertEqual(stored['input_digest'], prose_lint.input_digest({'command': 'command'}))
        self.assertNotIn('command', state.read_text())

    def test_stale_snapshot_skips_post_and_is_consumed(self):
        self.shell('PreToolUse')
        state = next((self.repo/'.git/prose-lint-snapshots').glob('*.json'))
        stored = json.loads(state.read_text())
        stored['created'] = time.time()-4000
        state.write_text(json.dumps(stored))
        (self.repo/'doc with spaces.md').write_text('changed')
        self.assertEqual(self.shell('PostToolUse').returncode, 0)
        self.assertFalse(state.exists())
        self.assertFalse(self.log.exists())

    def test_tracked_ignored_example_remains_eligible(self):
        doc = self.repo/'oxidex-tags/examples/render_domain.rs'
        doc.parent.mkdir(parents=True)
        doc.write_text('// bad prose')
        subprocess.run(['git', '-C', str(self.repo), 'add', str(doc)], check=True)
        (self.repo/'.gitignore').write_text('examples/\n')
        self.assertEqual(self.invoke('Edit', {'file_path': str(doc)}).returncode, 2)
        self.shell('PreToolUse')
        doc.write_text('// changed bad prose')
        self.assertEqual(self.shell('PostToolUse').returncode, 2)
        self.assertIn(str(doc), self.log.read_text())

    def test_quoted_generated_header_is_prose(self):
        doc = self.repo/'header_test.py'
        doc.write_text('HEADER = "//! generated -- DO NOT EDIT\\n"\n# bad prose\n')
        self.assertEqual(self.invoke('Edit', {'file_path': str(doc)}).returncode, 2)
        self.shell('PreToolUse')
        doc.write_text(doc.read_text()+'# changed prose\n')
        self.assertEqual(self.shell('PostToolUse').returncode, 2)

    def test_noneligible_inventory_does_not_exhaust_prose_cap(self):
        for n in range(8):
            (self.repo/f'corpus{n}.bin').write_bytes(b'\0')
        payload = dict(tool_input={'command': 'command'}, session_id='session',
                       tool_use_id='bulk', hook_event_name='PreToolUse')
        with patch.object(prose_lint, 'MAX_SNAPSHOT_FILES', 2):
            snapshot = prose_lint.shell_snapshot(self.repo)
            self.assertIsNotNone(snapshot)
            self.assertIn('doc with spaces.md', snapshot)
            prose_lint.shell_paths(payload, self.repo, self.repo)
            (self.repo/'doc with spaces.md').write_text('changed alongside corpus')
            payload['hook_event_name'] = 'PostToolUse'
            self.assertEqual(prose_lint.shell_paths(payload, self.repo, self.repo),
                             [self.repo/'doc with spaces.md'])

    def test_overlapping_shell_completions_claim_changes_once(self):
        for order in [('A', 'B'), ('B', 'A')]:
            with self.subTest(order=order):
                self.log.unlink(missing_ok=True)
                self.shell('PreToolUse', 'A')
                self.shell('PreToolUse', 'B')
                (self.repo/'doc with spaces.md').write_text(str(order))
                self.assertEqual(self.shell('PostToolUse', order[0]).returncode, 2)
                self.assertEqual(self.shell('PostToolUse', order[1]).returncode, 0)
                self.assertEqual(len(self.log.read_text().splitlines()), 1)

    def test_simultaneous_posts_claim_changes_once(self):
        self.shell('PreToolUse', 'A')
        self.shell('PreToolUse', 'B')
        (self.repo/'doc with spaces.md').write_text('changed during overlap')
        processes = []
        for call in ['A', 'B']:
            payload = dict(cwd=str(self.repo), hook_event_name='PostToolUse',
                           tool_name='Bash', tool_input={'command': 'command'},
                           session_id='session', tool_use_id=call)
            process = subprocess.Popen(['bash', str(HOOK)], stdin=subprocess.PIPE,
                                       stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                       text=True, env=self.env, cwd=self.repo)
            process.stdin.write(json.dumps(payload))
            process.stdin.close()
            process.stdin = None
            processes.append(process)
        for process in processes:
            process.communicate(timeout=10)
        self.assertEqual(sorted(p.returncode for p in processes), [0, 2])
        self.assertEqual(len(self.log.read_text().splitlines()), 1)

    def test_overlap_later_edit_is_still_reported(self):
        self.shell('PreToolUse', 'A')
        self.shell('PreToolUse', 'B')
        doc = self.repo/'doc with spaces.md'
        doc.write_text('first change')
        self.assertEqual(self.shell('PostToolUse', 'A').returncode, 2)
        doc.write_text('second change')
        self.assertEqual(self.shell('PostToolUse', 'B').returncode, 2)
        self.assertEqual(len(self.log.read_text().splitlines()), 2)

    def test_generated_header_boundaries_through_shipped_adapter(self):
        cases = [
            ('suffix.rs', '/* ordinary comment */ const HEADER: &str = "DO NOT EDIT";\n', False),
            ('heading.md', '# Do not edit the original photograph\n', False),
            ('ordinary.rs', '// Do not edit the original photograph\n', False),
            ('multisuffix.rs', '/* ordinary\n * comment */ const HEADER: &str = "DO NOT EDIT";\n', False),
            ('mixeddirective.rs', '/* @generated by generator */ const X: u8 = 1;\n', False),
            ('wrongform.py', '// @generated by generator\n', False),
            ('unclosed.rs', '/* @generated by generator\n', False),
            ('nested.rs', '/* /* @generated by generator */ */\n', False),
            ('hash.py', '# Do not edit the original photograph\n', False),
            ('html.md', '<!-- Auto-generated. -->\n', True),
            ('multihtml.md', '<!--\nAuto-generated by generator\n-->\n', True),
            ('multi.rs', '/*\n * @generated by generator\n */\n', True),
            ('docblock.rs', '/*! @generated by generator */\n', True),
            ('line.rs', '//! DO NOT EDIT. Regenerate with a generator.\n', True),
            ('python.py', '#!/usr/bin/env python3\n# @generated by generator\n', True),
            ('shell.sh', '# Auto-generated by generator\n', True),
            ('perl.pl', '# Autogenerated.\n', True),
        ]
        for name, header, generated in cases:
            with self.subTest(name=name):
                self.log.unlink(missing_ok=True)
                doc = self.repo/name
                doc.write_text(header+'bad prose\n')
                expected = 0 if generated else 2
                self.assertEqual(self.invoke('Edit', {'file_path': str(doc)}).returncode, expected)
                self.assertEqual(self.log.exists(), not generated)
                self.shell('PreToolUse', name)
                doc.write_text(header+'changed bad prose\n')
                self.assertEqual(self.shell('PostToolUse', name).returncode, expected)
                self.assertEqual(self.log.exists(), not generated)

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
