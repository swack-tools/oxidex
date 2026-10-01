"""Exercise the action's actual pin reader and CI installation wiring."""
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[2]
ACTION = ROOT / '.github/actions/pinned-rust/action.yml'


class PinnedRustActionTests(unittest.TestCase):
    def resolve(self, document):
        source = ACTION.read_text().split("        python3 - <<'PYTHON'\n", 1)[1].split('        PYTHON\n', 1)[0]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'rust-toolchain.toml').write_text(document)
            output = root / 'output'
            result = subprocess.run(['python3', '-c', textwrap.dedent(source)], cwd=root,
                                    env={**os.environ, 'GITHUB_OUTPUT': str(output)},
                                    capture_output=True, text=True)
            return result, output.read_text() if output.exists() else ''

    def test_reads_changed_pin_and_components_without_a_duplicate_version(self):
        result, output = self.resolve('[toolchain]\nchannel="1.96.0"\ncomponents=["rustfmt", "clippy"]\n')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(output, 'channel=1.96.0\ncomponents=rustfmt,clippy\n')

    def test_refuses_unpinned_channel_or_output_injection(self):
        for document in ('[toolchain]\nchannel="stable"\n',
                         '[toolchain]\nchannel="1.97.1"\ncomponents=["rustfmt\\nother=value"]\n'):
            with self.subTest(document=document):
                result, output = self.resolve(document)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(output, '')

    def test_rust_action_change_triggers_actual_read_gate_filter(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text()
        gate = workflow.split('  read-regression-gate:', 1)[1].split('\n  verify-tables-', 1)[0]
        expression = gate.split("grep -qE '", 1)[1].split("'", 1)[0]
        result = subprocess.run(['grep', '-qE', expression],
                                input='.github/actions/pinned-rust/action.yml\n', text=True)
        self.assertEqual(result.returncode, 0)

    def test_readiness_rejects_shadowed_tools_and_compiler_override(self):
        source = textwrap.dedent(ACTION.read_text().rsplit('      run: |\n', 1)[1])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = root / 'tool'
            script.write_text("""#!/usr/bin/env python3
import os, sys
from pathlib import Path
name = Path(sys.argv[0]).name
if name == 'rustup':
    if sys.argv[1] == 'show':
        print('1.97.1-test-host (override)')
    elif sys.argv[1] == 'which':
        print(Path(sys.argv[0]).parent / ('pin-' + sys.argv[-1]))
    else:
        print('release: 1.97.1\\ncommit-hash: pinned\\nhost: test-host')
else:
    tool = name.removeprefix('pin-')
    version = '1.98.1' if name == os.environ.get('WRONG_VERSION') else '1.97.1'
    commit = 'wrong' if name == os.environ.get('WRONG_COMMIT') else 'pinned'
    if tool in ('rustc', 'override'):
        print(f'release: {version}\\ncommit-hash: {commit}\\nhost: test-host')
    else:
        print(f'{tool} {version}')
""")
            script.chmod(0o755)
            for name in ('rustup', 'rustc', 'cargo', 'rustfmt', 'override',
                         'pin-rustc', 'pin-cargo', 'pin-rustfmt'):
                (root / name).symlink_to(script)
            env = {**os.environ, 'PATH': str(root) + os.pathsep + os.environ['PATH'],
                   'PINNED_RUST': '1.97.1'}
            env.pop('RUSTC', None)
            cases = ({}, {'WRONG_VERSION': 'rustc'}, {'WRONG_COMMIT': 'rustc'},
                     {'WRONG_VERSION': 'cargo'}, {'WRONG_VERSION': 'rustfmt'},
                     {'RUSTC': str(root / 'override'), 'WRONG_COMMIT': 'override'})
            for changes in cases:
                with self.subTest(changes=changes):
                    result = subprocess.run(['bash', '-c', source], env={**env, **changes},
                                            capture_output=True, text=True)
                    self.assertEqual(result.returncode == 0, not changes, result.stderr)

    def test_all_ci_rust_installations_use_the_repository_pin(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text()
        self.assertNotIn('uses: dtolnay/rust-toolchain@', workflow)
        self.assertEqual(workflow.count('uses: ./.github/actions/pinned-rust'), 8)
        action = ACTION.read_text()
        self.assertIn('toolchain: ${{ steps.pin.outputs.channel }}', action)
        self.assertIn('components: ${{ steps.pin.outputs.components }}', action)
        self.assertLess(action.index('toolchain: ${{'), action.index('rustfmt --version'))


if __name__ == '__main__':
    unittest.main()
