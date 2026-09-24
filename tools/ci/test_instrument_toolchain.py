"""The instrument header names the compiler that built the binary under test.

`rust-toolchain.toml` binds only rustup's proxies, so a Homebrew `rustc` first
on PATH silently builds every local binary with its own release. These pin
`scripts/instrument.py`'s answer to "which compiler built this binary?" -- read
from the binary's own `/rustc/<commit>/` std paths, not from today's PATH --
with fake toolchains, so the result never depends on the host's.
"""

from __future__ import annotations

import contextlib
import io
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "scripts"))
import instrument  # noqa: E402

PIN = "8bab26f4f68e0e26f0bb7960be334d5b520ea452"
BREW = "48a229ceaefd4985c50990b14116b6d856af0985"


def vv(release: str, commit: str, tag: str = "") -> str:
    return (f"rustc {release} ({commit[:9]} 2026-01-01){tag}\nbinary: rustc\ncommit-hash: {commit}\n"
            f"host: aarch64-apple-darwin\nrelease: {release}\n")


def ident(release: str, commit: str, path: str) -> instrument.RustcIdentity:
    fields = instrument.parse_rustc_verbose(vv(release, commit))
    return instrument.RustcIdentity("rustc", path, fields["version"], release, commit)


class ParsingTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_channel_comes_from_the_checkouts_own_file(self):
        self.assertIsNone(instrument.pinned_rust_channel(self.root))
        (self.root / "rust-toolchain.toml").write_text(
            '[toolchain]\nchannel = "1.97.1"  # pinned\ncomponents = ["rustfmt", "clippy"]\n')
        self.assertEqual(instrument.pinned_rust_channel(self.root), "1.97.1")
        (self.root / "rust-toolchain.toml").unlink()
        (self.root / "rust-toolchain").write_text("1.80.0\n")
        self.assertEqual(instrument.pinned_rust_channel(self.root), "1.80.0")

    def test_numeric_pins_match_exactly_and_symbolic_pins_are_unknown(self):
        self.assertIs(instrument.channel_matches("1.97.1", "1.97.1"), True)
        self.assertIs(instrument.channel_matches("1.97.1", "1.98.1"), False)
        self.assertIs(instrument.channel_matches("1.97", "1.97.4"), True)
        self.assertIs(instrument.channel_matches("1.97", "1.970.0"), False)
        self.assertIs(instrument.channel_matches("1.97.1", None), False)
        self.assertIsNone(instrument.channel_matches("stable", "1.97.1"))

    def test_rustc_and_cargo_version_output(self):
        fields = instrument.parse_rustc_verbose(vv("1.98.1", BREW, " (Homebrew)"))
        self.assertEqual((fields["release"], fields["commit-hash"]), ("1.98.1", BREW))
        self.assertEqual(instrument.cargo_release("cargo 1.97.1 (c980f4866 2026-06-30)\n"), "1.97.1")
        self.assertIsNone(instrument.cargo_release("rustc 1.97.1"))

    def test_fingerprint_lists_every_toolchain_embedded_in_the_binary(self):
        binary = self.root / "oxidex"
        binary.write_bytes(f"\0/rustc/{BREW}/library/core/src/panicking.rs\0/rustc/{BREW}/library/std/x\0"
                           f"/rustc/{PIN}/library/alloc/y\0/rustc/not-a-hash/".encode())
        self.assertEqual(instrument.embedded_rustc_commits(binary), sorted([BREW, PIN]))
        binary.write_bytes(b"no fingerprint here")
        self.assertEqual(instrument.embedded_rustc_commits(binary), [])
        self.assertEqual(instrument.embedded_rustc_commits(self.root / "absent"), [])


class VerdictTests(unittest.TestCase):
    pinned = ident("1.97.1", PIN, "/rustup/toolchains/1.97.1/bin/rustc")
    brew = ident("1.98.1", BREW, "/opt/homebrew/bin/rustc")

    def assess(self, commits, *, pinned=pinned, current=brew, channel="1.97.1"):
        return instrument.assess_toolchain(channel=channel, binary_commits=commits, binary_label="oxidex binary",
                                           pinned=pinned, current=current)

    def test_binary_built_by_the_pin_passes_even_when_path_is_skewed(self):
        report = self.assess([PIN])
        self.assertFalse(report.mismatch or report.unverified, report.lines)
        self.assertIn("built by the pinned toolchain", report.lines[0])
        self.assertIn("[!= pin 1.97.1]", report.lines[1])

    def test_binary_built_by_homebrew_is_a_loud_mismatch_naming_it(self):
        report = self.assess([BREW])
        self.assertTrue(report.mismatch)
        self.assertIn("/opt/homebrew/bin/rustc", report.lines[0])
        self.assertIn("TOOLCHAIN MISMATCH", report.lines[1])
        self.assertTrue(self.assess([BREW, PIN]).mismatch)

    def test_unfingerprinted_or_unresolvable_is_unverified_never_passed(self):
        missing = self.assess([])
        self.assertTrue(missing.unverified)
        self.assertFalse(missing.mismatch)
        self.assertIn("UNKNOWN", missing.lines[0])
        unresolved = self.assess([BREW], pinned=None)
        self.assertTrue(unresolved.unverified)
        # A PATH rustc that merely reports the pinned release is NOT the pin:
        # without the pinned toolchain itself resolved, nothing is confirmed.
        via_path = self.assess([PIN], pinned=None, current=ident("1.97.1", PIN, "/usr/local/bin/rustc"))
        self.assertTrue(via_path.unverified, via_path.lines)
        self.assertFalse(via_path.mismatch)
        self.assertNotIn("built by the pinned toolchain", "\n".join(via_path.lines))
        # A "pinned" identity whose release is not the channel is not the pin either.
        wrong = self.assess([BREW], pinned=ident("1.98.1", BREW, "/rustup/toolchains/x/bin/rustc"))
        self.assertTrue(wrong.unverified, wrong.lines)
        self.assertFalse(wrong.mismatch)

    def test_without_a_binary_the_path_resolution_is_what_is_judged(self):
        skewed = self.assess(None)
        self.assertTrue(skewed.mismatch)
        self.assertTrue(skewed.lines[0].startswith("rustc:   rustc on PATH now"))
        self.assertFalse(self.assess(None, current=self.pinned).mismatch)
        unpinned = self.assess(None, channel=None)
        self.assertTrue(unpinned.unverified)
        self.assertFalse(unpinned.mismatch)


class LabelTests(unittest.TestCase):
    def test_a_bare_path_is_labelled_once(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "tool"
            path.write_bytes(f"/rustc/{PIN}/library".encode())
            with patch.object(instrument, "rustc_identity", return_value=None), \
                 patch.object(instrument, "pinned_rustc_identity", return_value=None):
                (Path(directory) / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.97.1"\n')
                bare = instrument.toolchain_report(directory, path)
                named = instrument.toolchain_report(directory, instrument.resolve_binary(str(path)))
        self.assertNotIn("binary binary", "\n".join(bare.lines))
        self.assertIn("binary built by", bare.lines[0])
        self.assertIn("oxidex binary built by", named.lines[0])


class PinnedResolutionTests(unittest.TestCase):
    """The pin's identity comes from rustup's own toolchain, never from PATH."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.toolchain = self.root / "toolchains/1.97.1/bin"
        self.toolchain.mkdir(parents=True)
        rustc = self.toolchain / "rustc"
        rustc.write_text("#!/bin/sh\nprintf '" + vv("1.97.1", PIN).replace("\n", "\\n") + "'\n")
        rustc.chmod(0o755)

    def rustup(self, body: str) -> dict:
        path = self.bin / "rustup"
        path.write_text("#!/bin/sh\n" + body)
        path.chmod(0o755)
        return {"PATH": str(self.bin) + os.pathsep + "/usr/bin:/bin", "HOME": str(self.root)}

    def test_rustup_which_is_an_authoritative_second_source(self):
        env = self.rustup(f'[ "$1" = which ] && echo {self.toolchain / "rustc"} && exit 0\nexit 1\n')
        pinned = instrument.pinned_rustc_identity("1.97.1", cwd=self.root, env=env)
        self.assertIsNotNone(pinned)
        self.assertEqual((pinned.release, pinned.commit_hash), ("1.97.1", PIN))
        self.assertEqual(pinned.path, str(self.toolchain / "rustc"))

    def test_no_rustup_or_no_toolchain_means_unresolved(self):
        self.assertIsNone(instrument.pinned_rustc_identity("1.97.1", cwd=self.root,
                                                           env={"PATH": "/usr/bin:/bin", "HOME": str(self.root)}))
        env = self.rustup("exit 1\n")
        self.assertIsNone(instrument.pinned_rustc_identity("1.97.1", cwd=self.root, env=env))

    def test_a_toolchain_reporting_another_release_is_not_the_pin(self):
        env = self.rustup(f'[ "$1" = which ] && echo {self.toolchain / "rustc"} && exit 0\nexit 1\n')
        self.assertIsNone(instrument.pinned_rustc_identity("1.80.0", cwd=self.root, env=env))


class HeaderTests(unittest.TestCase):
    """End to end: fake rustc/rustup on PATH, a real file as the binary."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.97.1"\n')
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.script("rustc", f'case "$1" in -vV) printf "{self.escaped(vv("1.98.1", BREW, " (Homebrew)"))}";; '
                             '--print) echo /opt/homebrew/Cellar/rust/1.98.1;; esac\n')
        self.script("rustup", 'if [ "$1" = run ] && [ "$2" = 1.97.1 ]; then '
                              f'printf "{self.escaped(vv("1.97.1", PIN))}"; '
                              'elif [ "$1" = which ]; then echo /rustup/toolchains/1.97.1/bin/rustc; '
                              'else exit 1; fi\n')
        self.env = {"PATH": str(self.bin) + os.pathsep + os.environ.get("PATH", ""), "RUSTC": ""}

    @staticmethod
    def escaped(text: str) -> str:
        return text.replace("\n", "\\n")

    def script(self, name: str, body: str) -> None:
        path = self.bin / name
        path.write_text("#!/bin/sh\n" + body)
        path.chmod(0o755)

    def binary(self, commit: str) -> instrument.BinaryIdentity:
        path = self.root / "oxidex"
        path.write_bytes(f"\0/rustc/{commit}/library/core/src/panicking.rs\0".encode())
        return instrument.resolve_binary(str(path))

    def header(self, commit: str) -> str:
        git = instrument.GitState(self.root, "a" * 40, "fixture", False, [], None)
        out = io.StringIO()
        with patch.dict(os.environ, self.env), contextlib.redirect_stdout(out):
            instrument.print_header(tool="fixture", git=git, binary=self.binary(commit))
        return out.getvalue()

    def test_header_flags_a_homebrew_built_binary(self):
        text = self.header(BREW)
        self.assertIn("rustc:   oxidex binary built by rustc 1.98.1", text)
        self.assertIn("TOOLCHAIN MISMATCH: the oxidex binary was NOT compiled by the pin 1.97.1", text)
        self.assertIn(f"commit {PIN[:12]}", text)

    def test_header_passes_a_pin_built_binary_and_still_shows_path_skew(self):
        text = self.header(PIN)
        self.assertIn("built by the pinned toolchain -- rustc 1.97.1", text)
        self.assertNotIn("TOOLCHAIN MISMATCH", text)
        self.assertIn("rustc on PATH now: rustc 1.98.1", text)
        self.assertIn("[!= pin 1.97.1]", text)


if __name__ == "__main__":
    unittest.main()
