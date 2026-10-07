"""Lightweight website recipe controls; no Node installation or site build."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import docs_site_worker


class DocsSiteWorkerTests(unittest.TestCase):
    def test_requires_node_24_before_build(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / 'source'
            source.mkdir()
            output = Path(directory) / 'site'
            with patch.object(docs_site_worker.Path, 'cwd', return_value=source), \
                 patch.object(docs_site_worker.shutil, 'which', return_value='/fake/bin'), \
                 patch.object(docs_site_worker.subprocess, 'check_output', return_value='v22.0.0\n'), \
                 patch.object(docs_site_worker.subprocess, 'run') as command:
                with self.assertRaisesRegex(RuntimeError, 'requires Node.js 24'):
                    docs_site_worker.run(source, output)
                command.assert_not_called()

    def test_invokes_snapshot_mirror_and_checks_exact_commit(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / 'source'
            source.mkdir()
            output = Path(directory) / 'site'
            head = 'a' * 40
            tree = 'b' * 40
            calls = []

            def output_for(command, **_):
                if command == ['node', '--version']:
                    return 'v24.1.0\n'
                if command == ['git', 'rev-parse', 'HEAD']:
                    return head + '\n'
                if command == ['git', 'rev-parse', 'HEAD^{tree}']:
                    return tree + '\n'
                self.fail(f'unexpected command: {command}')

            def run_command(command, **kwargs):
                calls.append((command, kwargs))
                if command[:2] == ['bash', 'tools/docs-local-deploy.sh']:
                    (output / 'dist').mkdir(parents=True)
                    (output / 'dist' / 'index.html').write_text('site')
                    (output / 'snapshot-manifest.json').write_text(json.dumps({
                        'candidate_sha': head, 'source_head_sha': head,
                        'tree_hash': tree, 'source_kind': 'commit',
                        'base_path': '/', 'dist': str(output / 'dist'),
                    }))
                return subprocess.CompletedProcess(command, 0)

            with patch.object(docs_site_worker.Path, 'cwd', return_value=source), \
                 patch.object(docs_site_worker.shutil, 'which', return_value='/fake/bin'), \
                 patch.object(docs_site_worker.subprocess, 'check_output', side_effect=output_for), \
                 patch.object(docs_site_worker.subprocess, 'run', side_effect=run_command):
                docs_site_worker.run(source, output)
            self.assertEqual(calls[1][0], [
                'bash', 'tools/docs-local-deploy.sh', '--ref', 'HEAD',
                '--build-only', '--output', str(output)])
            self.assertEqual(calls[1][1]['env']['DOCS_CHANNEL'], 'stable')
            self.assertEqual(calls[1][1]['env']['DOCS_BASE'], '/')
            self.assertEqual(calls[2][0], [
                'node', 'docs/scripts/check-base-links.mjs', str(output / 'dist'), '/'])
