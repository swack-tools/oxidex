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
