"""Exercise the action's actual pin reader and CI installation wiring."""
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest
import hashlib

ROOT = Path(__file__).resolve().parents[2]
ACTION = ROOT / '.github/actions/pinned-rust/action.yml'


class PinnedRustActionTests(unittest.TestCase):
    def bootstrap(self, *, failures='', arch='x86_64', checksum='valid', installer_exit=0,
                  rustup_on_path=False, rustup_in_cargo=False):
        action = ACTION.read_text()
        source = textwrap.dedent(action.split('    - name: Bootstrap rustup if missing on Linux\n', 1)[1]
                                 .split('      run: |\n', 1)[1]
                                 .split('    - uses: dtolnay/rust-toolchain@', 1)[0])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / 'bin'
            binary.mkdir()
            cargo = root / 'cargo'
            (cargo / 'bin').mkdir(parents=True)
            runner_temp = root / 'temp'
            runner_temp.mkdir()
            path_file = root / 'github-path'
            calls = root / 'calls'
            installer = root / 'installer'
            installer.write_text('#!/bin/sh\nprintf "install\\n" >> "$CALLS"\n'
                                 'mkdir -p "$CARGO_HOME/bin"\n'
                                 'printf "#!/bin/sh\\nexit 0\\n" > "$CARGO_HOME/bin/rustup"\n'
                                 'chmod +x "$CARGO_HOME/bin/rustup"\n'
                                 f'exit {installer_exit}\n')
            installer.chmod(0o755)
            digest = hashlib.sha256(installer.read_bytes()).hexdigest()
            curl = binary / 'curl'
            curl.write_text('#!/usr/bin/env python3\n'
                            'import os, pathlib, sys\n'
                            'args = sys.argv[1:]\n'
                            'url = next(arg for arg in args if arg.startswith("https://"))\n'
                            'dest = pathlib.Path(args[args.index("--output") + 1])\n'
                            'kind = "checksum" if url.endswith(".sha256") else "binary"\n'
                            'expected = "https://static.rust-lang.org/rustup/dist/" + os.environ["ARCH"] + "-unknown-linux-gnu/rustup-init"\n'
                            'assert url == expected + (".sha256" if kind == "checksum" else "")\n'
                            'assert args[args.index("--proto") + 1] == "=https"\n'
                            'assert args[args.index("--proto-redir") + 1] == "=https"\n'
                            'calls = pathlib.Path(os.environ["CALLS"])\n'
                            'previous = calls.read_text().splitlines() if calls.exists() else []\n'
                            'attempt = sum(x == kind for x in previous) + 1\n'
                            'with calls.open("a") as output: output.write(kind + "\\n")\n'
                            'failure = os.environ["FAILURES"]\n'
                            'if failure == kind + ":once" and attempt == 1: dest.write_text("partial"); sys.exit(56)\n'
                            'if dest.exists(): sys.exit(99)\n'
                            'if failure == kind + ":always": sys.exit(56)\n'
                            'if failure == kind + ":404": sys.exit(22)\n'
                            'if kind == "binary": dest.write_bytes(pathlib.Path(os.environ["INSTALLER"]).read_bytes())\n'
                            'else:\n'
                            '    digest = os.environ["DIGEST"] if os.environ["CHECKSUM"] == "valid" else "0" * 64\n'
                            '    dest.write_text("invalid\\n" if os.environ["CHECKSUM"] == "malformed" else digest + " *./rustup-init\\n")\n')
            curl.chmod(0o755)
            (binary / 'uname').write_text(f'#!/bin/sh\necho {arch}\n')
            (binary / 'uname').chmod(0o755)
            (binary / 'sleep').write_text('#!/bin/sh\nexit 0\n')
            (binary / 'sleep').chmod(0o755)
            if rustup_on_path:
                (binary / 'rustup').write_text('#!/bin/sh\nexit 0\n')
                (binary / 'rustup').chmod(0o755)
            if rustup_in_cargo:
                (cargo / 'bin' / 'rustup').write_text('#!/bin/sh\nexit 0\n')
                (cargo / 'bin' / 'rustup').chmod(0o755)
            # macOS uses shasum; Linux CI uses sha256sum. Keep the fixture host independent.
            (binary / 'sha256sum').write_text('#!/usr/bin/env python3\n'
                                             'import hashlib, pathlib, sys\n'
                                             'line = pathlib.Path(sys.argv[-1]).read_text().strip()\n'
                                             'digest, name = line.split(" *", 1)\n'
                                             'actual = hashlib.sha256(pathlib.Path(name).read_bytes()).hexdigest()\n'
                                             'sys.exit(0 if digest == actual else 1)\n')
            (binary / 'sha256sum').chmod(0o755)
            env = {**os.environ, 'PATH': f'{binary}:/usr/bin:/bin', 'RUNNER_TEMP': str(runner_temp),
                   'CARGO_HOME': str(cargo), 'GITHUB_PATH': str(path_file), 'CALLS': str(calls),
                   'FAILURES': failures, 'CHECKSUM': checksum, 'DIGEST': digest,
                   'INSTALLER': str(installer), 'ARCH': arch}
            result = subprocess.run(['bash', '-c', source], env=env, capture_output=True, text=True, timeout=15)
            return result, calls.read_text().splitlines() if calls.exists() else [], (
                path_file.read_text().splitlines() if path_file.exists() else [])

    def test_bootstrap_retries_only_failed_download_and_installs_once(self):
        result, calls, paths = self.bootstrap(failures='binary:once')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, ['binary', 'binary', 'checksum', 'install'])
        self.assertEqual(len(paths), 1)

    def test_bootstrap_refuses_failures_without_running_installer(self):
        for changes, expected in (
            ({'failures': 'binary:always'}, ['binary'] * 3),
            ({'failures': 'binary:404'}, ['binary']),
            ({'failures': 'checksum:404'}, ['binary', 'checksum']),
            ({'checksum': 'bad'}, ['binary', 'checksum']),
            ({'checksum': 'malformed'}, ['binary', 'checksum']),
            ({'arch': 's390x'}, []),
        ):
            with self.subTest(changes=changes):
                result, calls, paths = self.bootstrap(**changes)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(calls, expected)
                self.assertEqual(paths, [])

    def test_bootstrap_checksum_retry_and_installer_failure(self):
        result, calls, paths = self.bootstrap(failures='checksum:once')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, ['binary', 'checksum', 'checksum', 'install'])
        self.assertEqual(len(paths), 1)
        result, calls, paths = self.bootstrap(installer_exit=1)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls, ['binary', 'checksum', 'install'])
        self.assertEqual(paths, [])

    def test_bootstrap_existing_installation_bypasses_downloads(self):
        for changes, path_count in (({'rustup_on_path': True}, 0),
                                    ({'rustup_in_cargo': True}, 1)):
            with self.subTest(changes=changes):
                result, calls, paths = self.bootstrap(**changes)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(calls, [])
                self.assertEqual(len(paths), path_count)

    def test_bootstrap_supports_arm64(self):
        result, calls, paths = self.bootstrap(arch='aarch64')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, ['binary', 'checksum', 'install'])
        self.assertEqual(len(paths), 1)

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
    elif tool == 'cargo' and '-vV' in sys.argv:
        print(f'cargo {version}\\ncommit-hash: {commit}')
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
                     {'WRONG_VERSION': 'cargo'}, {'WRONG_COMMIT': 'cargo'},
                     {'WRONG_VERSION': 'rustfmt'},
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
