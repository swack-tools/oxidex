"""Exercise capture compression provisioning without installing host packages."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
ACTION = ROOT / '.github/actions/ensure-zstd/action.yml'


class CaptureCompressionTests(unittest.TestCase):
    def run_setup(self, *, present=False, install=True):
        body = ACTION.read_text().split('      run: |\n', 1)[1]
        script = '\n'.join(line[8:] for line in body.splitlines())
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            zstd = root / 'zstd'
            if present:
                zstd.write_text('#!/bin/sh\necho mock-zstd\n')
                zstd.chmod(0o755)
            sudo = root / 'sudo'
            sudo.write_text('''#!/bin/sh
printf '%s\n' "$*" >> "$TEST_LOG"
if [ "$1 $2" = 'apt-get install' ] && [ "$TEST_INSTALL" = yes ]; then
  printf '#!/bin/sh\necho mock-zstd\n' > "$TEST_BIN/zstd"
  /bin/chmod +x "$TEST_BIN/zstd"
fi
''')
            sudo.chmod(0o755)
            env = dict(os.environ, PATH=str(root), TEST_LOG=str(root / 'calls'),
                       TEST_BIN=str(root), TEST_INSTALL='yes' if install else 'no')
            result = subprocess.run(['/bin/bash', '-c', script], env=env,
                                    capture_output=True, text=True, timeout=10)
            calls = (root / 'calls').read_text().splitlines() if (root / 'calls').exists() else []
            return result, calls

    def test_existing_binary_does_not_install_packages(self):
        result, calls = self.run_setup(present=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, [])

    def test_missing_binary_is_installed_and_verified(self):
        result, calls = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, ['apt-get update -qq', 'apt-get install -y --no-install-recommends zstd'])
        self.assertIn('mock-zstd', result.stdout)

    def test_successful_package_command_without_binary_fails(self):
        result, _ = self.run_setup(install=False)
        self.assertNotEqual(result.returncode, 0)

    def test_producer_and_consumers_provision_before_use(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text()
        capture = workflow.split('  verify-tables-capture:\n', 1)[1].split('  verify-tables-source:', 1)[0]
        consumer = (ROOT / '.github/actions/verify-tables-capture/action.yml').read_text()
        for text in (capture, consumer):
            self.assertLess(text.index('uses: ./.github/actions/ensure-zstd'), text.index('zstd -q'))


if __name__ == '__main__':
    unittest.main()
