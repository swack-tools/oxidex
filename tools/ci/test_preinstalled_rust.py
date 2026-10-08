"""Fixed immutable image selector controls; no local Rust build or download."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

import preinstalled_rust as selected


class PreinstalledRustTests(unittest.TestCase):
    def fixture(self, root):
        source = root / 'source'
        source.mkdir()
        (source / 'rust-toolchain.toml').write_text(
            '[toolchain]\nchannel = "1.99.0"\ncomponents = ["rustfmt", "clippy"]\n')
        image = root / 'runner-rust'
        binary = image / 'cargo/bin'
        tools = image / 'rustup/toolchains/1.99.0-test-host/bin'
        binary.mkdir(parents=True)
        tools.mkdir(parents=True)
        rustup = binary / 'rustup'
        rustup.write_text('''#!/usr/bin/env python3
import os, pathlib, sys
base = pathlib.Path(sys.argv[0]).parents[2] / 'rustup/toolchains/1.99.0-test-host/bin'
if sys.argv[1] == 'which':
    if os.environ.get('MISSING_TOOL') == sys.argv[-1]: sys.exit(1)
    print(base / sys.argv[-1])
elif sys.argv[1:3] == ['component', 'list']:
    print('rustfmt-x86_64-unknown-linux-gnu (installed)')
    if not os.environ.get('MISSING_COMPONENT'): print('clippy-x86_64-unknown-linux-gnu (installed)')
elif sys.argv[1:3] == ['target', 'list']:
    if not os.environ.get('MISSING_TARGET'): print('test-host')
else: sys.exit(2)
''')
        rustup.chmod(0o755)
        for name in selected.TOOLS:
            tool = tools / name
            tool.write_text('''#!/usr/bin/env python3
import os, pathlib, sys
name = pathlib.Path(sys.argv[0]).name
if name == 'rustc':
    print('release: ' + ('1.98.0' if os.environ.get('WRONG_RUSTC') else '1.99.0'))
    print('commit-hash: ' + 'a'*40)
    print('host: test-host')
elif name == 'cargo':
    print('cargo ' + ('1.98.0' if os.environ.get('WRONG_CARGO') else '1.99.0') + ' (fixture)')
else: print(name + ' 1.99.0')
''')
            tool.chmod(0o755)
        return source, image

    def test_absent_uses_legacy_and_present_untrusted_refuses(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            source, image = self.fixture(root)
            with patch.object(selected, 'ROOT', root / 'absent'):
                self.assertFalse(selected.probe(source, root / 'absent'))
            with patch.object(selected, 'ROOT', image):
                with self.assertRaisesRegex(RuntimeError, 'Untrusted|read-only|canonical'):
                    selected.probe(source, image)

    def test_selector_requires_actual_read_only_mount(self):
        with patch.object(selected.os.path, 'ismount', return_value=False):
            with self.assertRaisesRegex(RuntimeError, 'read-only mount'):
                selected._readonly_mount(selected.ROOT)
        with patch.object(selected.os.path, 'ismount', return_value=True), \
             patch.object(selected.os, 'statvfs', return_value=SimpleNamespace(f_flag=0)):
            with self.assertRaisesRegex(RuntimeError, 'read-only mount'):
                selected._readonly_mount(selected.ROOT)
        with patch.object(selected.os.path, 'ismount', return_value=True), \
             patch.object(selected.os, 'statvfs',
                          return_value=SimpleNamespace(f_flag=selected.os.ST_RDONLY)):
            selected._readonly_mount(selected.ROOT)

    def test_fixed_pin_components_targets_and_tool_identities(self):
        with tempfile.TemporaryDirectory() as directory:
            source, image = self.fixture(Path(directory).resolve())
            with patch.object(selected, '_trusted_storage'), patch.object(selected, '_trusted_tool'), \
                 patch.object(selected, '_trusted_proxies'):
                self.assertTrue(selected.probe(source, image))
                for env_name, expected in (('MISSING_COMPONENT', 'component'),
                                           ('MISSING_TARGET', 'target'),
                                           ('MISSING_TOOL', 'rustc'),
                                           ('WRONG_RUSTC', 'rustc'),
                                           ('WRONG_CARGO', 'Cargo')):
                    with self.subTest(env_name=env_name), patch.dict(os.environ, {env_name: 'rustc'}):
                        with self.assertRaisesRegex((RuntimeError, subprocess.CalledProcessError), expected):
                            selected.probe(source, image)
                (source / 'rust-toolchain.toml').write_text(
                    '[toolchain]\nchannel = "1.99.0"\ncomponents = ["rustfmt", "clippy"]\n'
                    'targets = ["x86_64-unknown-linux-musl"]\n')
                with self.assertRaisesRegex(RuntimeError, 'target'):
                    selected.probe(source, image)
                (source / 'rust-toolchain.toml').write_text(
                    '[toolchain]\nchannel = "1.98.0"\ncomponents = ["rustfmt", "clippy"]\n')
                with self.assertRaises((RuntimeError, subprocess.CalledProcessError)):
                    selected.probe(source, image)

    def test_actual_path_proxies_have_immutable_rustup_custody(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            source, image = self.fixture(root)
            proxy_dir = image / 'cargo/bin'
            rustup = proxy_dir / 'rustup'
            original_lstat = Path.lstat
            def root_metadata(path, *args, **kwargs):
                value = original_lstat(path, *args, **kwargs)
                if path.is_relative_to(image):
                    return SimpleNamespace(st_mode=value.st_mode, st_uid=0,
                        st_dev=value.st_dev, st_ino=value.st_ino)
                return value
            with patch.object(selected, 'ROOT', image), \
                 patch.object(Path, 'lstat', root_metadata), \
                 patch.object(selected.os.path, 'ismount', return_value=True), \
                 patch.object(selected.os, 'statvfs',
                              return_value=SimpleNamespace(f_flag=os.ST_RDONLY)):
                with self.assertRaisesRegex(RuntimeError, 'proxy'):
                    selected.probe(source, image)  # Missing actual PATH proxies.
                for name in selected.PROXIES:
                    os.link(rustup, proxy_dir / name)
                self.assertTrue(selected.probe(source, image))  # Real internal hardlinks.
                for name in selected.PROXIES:
                    with self.subTest(missing_proxy=name):
                        path = proxy_dir / name
                        path.unlink()
                        with self.assertRaisesRegex(RuntimeError, 'proxy'):
                            selected.probe(source, image)
                        os.link(rustup, path)
                victim = proxy_dir / 'rustc'
                victim.unlink()
                victim.write_bytes(rustup.read_bytes())
                victim.chmod(0o755)
                with self.assertRaisesRegex(RuntimeError, 'proxy'):
                    selected.probe(source, image)  # Same bytes, different inode.
                victim.unlink()
                marker = root / 'untrusted-proxy-ran'
                external = root / 'job-writable-rustc'
                external.write_text('#!/bin/sh\n/usr/bin/touch '+str(marker)+'\n')
                external.chmod(0o755)
                victim.symlink_to(external)
                with self.assertRaisesRegex(RuntimeError, 'proxy'):
                    selected.probe(source, image)
                self.assertFalse(marker.exists(), 'external wrapper executed before refusal')

    def test_action_output_only_selects_verified_preinstall(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            output = root / 'output'; environment = root / 'environment'; path = root / 'path'
            variables = {'GITHUB_OUTPUT': str(output), 'GITHUB_ENV': str(environment),
                         'GITHUB_PATH': str(path)}
            with patch.dict(os.environ, variables), patch.object(selected, 'probe', return_value=False):
                selected.main()
            self.assertEqual(output.read_text(), 'ready=false\n')
            self.assertFalse(environment.exists())
            self.assertFalse(path.exists())
            with patch.dict(os.environ, variables), patch.object(selected, 'ROOT', root), \
                 patch.object(selected, 'probe', return_value=True):
                selected.main()
            self.assertEqual(output.read_text(), 'ready=false\nready=true\n')
            self.assertEqual(environment.read_text(), f'RUSTUP_HOME={root / "rustup"}\n')
            self.assertEqual(path.read_text(), f'{root / "cargo/bin"}\n')

    def test_action_selects_preinstall_before_mutating_setup(self):
        action = (Path(__file__).resolve().parents[2] /
                  '.github/actions/pinned-rust/action.yml').read_text()
        self.assertLess(action.index('id: preinstalled'), action.index('Bootstrap rustup if missing'))
        self.assertIn("if: runner.os == 'Linux' && steps.preinstalled.outputs.ready != 'true'", action)
        self.assertIn("if: steps.preinstalled.outputs.ready != 'true'", action)
        self.assertIn('run: python3 tools/ci/preinstalled_rust.py', action)
        self.assertEqual(action.count('uses: dtolnay/rust-toolchain@'), 1)


if __name__ == '__main__':
    unittest.main()
