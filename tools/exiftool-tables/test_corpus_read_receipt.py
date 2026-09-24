"""Corpus read receipt: identity folding, strict matching, exact coordinate credit."""
from __future__ import annotations

import copy
import json
import os
import tempfile
import unittest
from pathlib import Path

import corpus_read_receipt as receipt_tool

NATIVE = {"perl": {"path": "/perl", "sha256": "a" * 64}, "script": {"path": "/et/exiftool", "sha256": "b" * 64},
          "library": "/et/lib", "source_capture": {"path": "/tools/capture_corpus_sources.pl", "sha256": "e" * 64}}
PROOF = {"binary": {"path": "/bin/oxidex", "sha256": "c" * 64}}


def fact(command, stdout: bytes, returncode=0):
    return {"command": command, "returncode": returncode,
            "stdout_hex": stdout.hex(), "stdout_sha256": receipt_tool.sha(stdout),
            "stderr_hex": "", "stderr_sha256": receipt_tool.sha(b"")}


def encode(pairs):
    return ("[{" + ",".join(json.dumps(k) + ":" + v for k, v in pairs) + "}]").encode()


def receipt(files, sources):
    """files: {name: {mode: (native_pairs, public_pairs)}}; sources: {name: [[g1, tag, table, id, variant]]}."""
    observations = []
    for name, modes in files.items():
        path = "/corpus/" + name
        for mode, (native_pairs, public_pairs) in modes.items():
            observations.append({
                "file": name, "mode": mode,
                "native": fact(receipt_tool.native_command(NATIVE, mode, path), encode(native_pairs)),
                "oxidex": fact(receipt_tool.public_command(PROOF, mode, path), encode(public_pairs))})
    captures = {name: fact(receipt_tool.source_command(NATIVE, "/corpus/" + name),
                           json.dumps({"/corpus/" + name: rows}).encode()) for name, rows in sources.items()}
    return {"build_proof": PROOF, "native": NATIVE, "observations": observations, "sources": captures,
            "corpus": {"root": "/corpus", "files": {name: "d" * 64 for name in files}}}


MAKE = ["IFD0", "Make", "Image::ExifTool::Exif::Main", "271", 0]
MODEL = ["IFD0", "Model", "Image::ExifTool::Exif::Main", "272", 0]
ORIENTATION = ["IFD0", "Orientation", "Image::ExifTool::Exif::Main", "274", 0]


class IdentityTests(unittest.TestCase):
    def test_copy_segments_and_duplicate_suffixes_fold_to_value_multisets(self):
        native = receipt_tool.identities(encode([
            ("SourceFile", '"x"'), ("ExifTool:ExifToolVersion", "13.59"), ("System:FileSize", '"1 kB"'),
            ("IFD0:Make", '"Canon"'), ("IFD0:Copy1:Make", '"Nikon"')]), "native")
        public = receipt_tool.identities(encode([
            ("System:FileSize", '"1 kB"'), ("IFD0:Make", '"Nikon"'), ("IFD0:Make (2)", '"Canon"')]), "public")
        self.assertEqual(native, public)
        self.assertEqual(list(native), [("IFD0", "Make")])

    def test_numbers_keep_their_literal_text_and_type(self):
        text = receipt_tool.identities(encode([("IFD0:FNumber", '"1.80"')]), "native")
        number = receipt_tool.identities(encode([("IFD0:FNumber", "1.80")]), "public")
        shortest = receipt_tool.identities(encode([("IFD0:FNumber", "1.8")]), "public")
        self.assertNotEqual(text, number)
        self.assertNotEqual(number, shortest)

    def test_duplicate_json_keys_and_unexpected_native_shapes_refuse(self):
        with self.assertRaisesRegex(ValueError, "duplicate"):
            receipt_tool.identities(encode([("IFD0:Make", '"a"'), ("IFD0:Make", '"b"')]), "public")
        with self.assertRaisesRegex(ValueError, "group shape"):
            receipt_tool.identities(encode([("A:B:C", '"a"')]), "native")

    def test_public_names_keep_embedded_colons(self):
        public = receipt_tool.identities(encode([("OOXML:Custom:Division", '"x"')]), "public")
        self.assertEqual(list(public), [("OOXML", "Custom:Division")])


class DeriveTests(unittest.TestCase):
    def files(self):
        return {
            "a.jpg": {"print": ([("IFD0:Make", '"Canon"'), ("IFD0:Orientation", '"Horizontal (normal)"')],
                                [("IFD0:Make", '"Canon"'), ("IFD0:Orientation", '"Horizontal (normal)"')]),
                      "raw": ([("IFD0:Make", '"Canon"'), ("IFD0:Orientation", "1")],
                              [("IFD0:Make", '"Canon"'), ("IFD0:Orientation", '"1"')])},
            "b.jpg": {"print": ([("IFD0:Make", '"Canon"'), ("IFD0:Model", '"R5"')],
                                [("IFD0:Make", '"Canon"'), ("IFD0:Extra", '"1"')]),
                      "raw": ([("IFD0:Make", '"Canon"'), ("IFD0:Model", '"R5"')], [("IFD0:Make", '"Canon"')])},
        }

    def sources(self):
        return {"a.jpg": [MAKE, ORIENTATION], "b.jpg": [MAKE, MODEL]}

    def test_identity_matches_only_when_both_modes_match_and_credit_is_exact(self):
        derived = receipt_tool.derive(receipt(self.files(), self.sources()))
        self.assertEqual(derived["matched_identities"], {"IFD0:Make": ["a.jpg", "b.jpg"]})
        self.assertEqual(derived["credited_coordinates"], [["Image::ExifTool::Exif::Main", "271", 0]])
        metric = derived["metric_c"]
        self.assertEqual((metric["distinct_group1_tag_identities_native"], metric["distinct_group1_tag_identities_matched"],
                          metric["matched_file_identities"], metric["mismatched_file_identities"],
                          metric["missing_file_identities"], metric["extra_file_identities"]),
                         (3, 1, 2, 1, 1, 1))
        # Orientation matched only in default output: reported, not credited.
        self.assertEqual((metric["distinct_group1_tag_identities_print_mode_matched"],
                          metric["print_mode_matched_file_identities"]), (2, 3))

    def test_one_failing_file_withholds_the_coordinate(self):
        files = self.files()
        files["b.jpg"]["raw"] = ([("IFD0:Make", '"Canon"'), ("IFD0:Model", '"R5"')], [("IFD0:Make", '"CANON"')])
        derived = receipt_tool.derive(receipt(files, self.sources()))
        self.assertEqual(derived["credited_coordinates"], [])
        self.assertEqual(derived["metric_c"]["withheld_source_coordinates"], 1)

    def test_unattributable_identity_is_never_credited(self):
        sources = self.sources()
        sources["b.jpg"] = [["IFD0", "Make", None, None, None], MODEL]
        derived = receipt_tool.derive(receipt(self.files(), sources))
        # b.jpg's match cannot name its row; a.jpg alone still credits 271.
        self.assertEqual(derived["credited_coordinates"], [["Image::ExifTool::Exif::Main", "271", 0]])
        self.assertEqual(derived["metric_c"]["unattributable_file_identities"], 1)

    def test_a_failing_file_withholds_rows_even_when_partly_unattributable(self):
        files = self.files()
        files["b.jpg"] = {"print": ([("IFD0:Make", '"Canon"'), ("IFD0:Copy1:Make", '"Canon"')], [("IFD0:Make", '"WRONG"')]),
                          "raw": ([("IFD0:Make", '"Canon"'), ("IFD0:Copy1:Make", '"Canon"')], [("IFD0:Make", '"WRONG"')])}
        sources = {"a.jpg": [MAKE, ORIENTATION], "b.jpg": [MAKE, ["IFD0", "Make", None, None, None]]}
        derived = receipt_tool.derive(receipt(files, sources))
        self.assertEqual(derived["credited_coordinates"], [])
        self.assertEqual(derived["metric_c"]["withheld_source_coordinates"], 1)

    def test_capture_that_differs_from_the_native_tag_set_refuses(self):
        for label, rows in (("extra row", [MAKE, ORIENTATION, MODEL]), ("missing row", [MAKE]),
                            ("count", [MAKE, MAKE, ORIENTATION])):
            sources = self.sources(); sources["a.jpg"] = rows
            with self.subTest(label), self.assertRaisesRegex(ValueError, "differs from the native CLI tag set"):
                receipt_tool.derive(receipt(self.files(), sources))

    def test_failed_public_run_counts_everything_missing(self):
        document = receipt(self.files(), self.sources())
        for row in document["observations"]:
            row["oxidex"] = fact(row["oxidex"]["command"], encode([("IFD0:Make", '"Canon"')]), returncode=101)
        metric = receipt_tool.derive(document)["metric_c"]
        self.assertEqual((metric["public_failed_file_modes"], metric["matched_file_identities"]), (4, 0))

    def test_tampered_failed_or_incomplete_transcripts_refuse(self):
        base = receipt(self.files(), self.sources())
        mutations = (
            lambda d: d["observations"][0]["oxidex"].update(stdout_hex=encode([("IFD0:Make", '"Nikon"')]).hex()),
            lambda d: d["observations"][0]["oxidex"].update(stderr_hex="00"),
            lambda d: d["observations"][0]["oxidex"].update(returncode="timeout"),
            lambda d: d["observations"][0]["native"].update(returncode=1),
            lambda d: d["observations"][0]["native"]["command"].append("-n"),
            lambda d: d["observations"].pop(),
            lambda d: d["observations"].append(copy.deepcopy(d["observations"][0])),
            lambda d: d["sources"]["a.jpg"].update(returncode=2),
            lambda d: d["sources"].pop("a.jpg"),
        )
        for index, mutate in enumerate(mutations):
            document = copy.deepcopy(base); mutate(document)
            with self.subTest(mutation=index), self.assertRaises(ValueError):
                receipt_tool.derive(document)


class NativeIdentityTests(unittest.TestCase):
    def facts(self, version="13.59", capability="DOCX", perl="v5.38.2"):
        facts = {"perl": {"path": "/perl", "sha256": "a" * 64}, "script": {"path": "/et/exiftool", "sha256": "b" * 64},
                 "library": "/et/lib", "exiftool_version": "13.59", "library_fingerprint": {"Image/ExifTool.pm": "f" * 64},
                 "capability_file": "/et/t/images/OOXML.docx"}
        commands = receipt_tool.version_commands(facts)
        facts["version_transcript"] = fact(commands["version"], version.encode() + b"\n")
        facts["capability_transcript"] = fact(commands["capability"], capability.encode() + b"\n")
        facts["perl_transcript"] = fact(commands["perl"], perl.encode())
        return facts

    def test_pin_capability_and_perl_are_checked_against_the_repository(self):
        receipt_tool.validate_native(self.facts(), "13.59")
        for facts, expected in ((self.facts(), "13.55"), (self.facts(version="13.55"), "13.59"),
                                (self.facts(capability="ZIP"), "13.59"), (self.facts(perl="v5.34.1"), "13.59")):
            with self.subTest(expected=expected), self.assertRaises(ValueError):
                receipt_tool.validate_native(facts, expected)


PIN_COMMIT = "8bab26f4f68e0e26f0bb7960be334d5b520ea452"
BREW_COMMIT = "48a229ceaefd4985c50990b14116b6d856af0985"


class BuildToolchainTests(unittest.TestCase):
    """The build proof names the compiler, and only the pinned one passes."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        self.root = self.base / "checkout"
        self.root.mkdir()
        (self.root / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.97.1"\n')
        self.bin = self.base / "bin"
        self.bin.mkdir()

    def script(self, path: Path, body: str) -> Path:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("#!/bin/sh\n" + body)
        path.chmod(0o755)
        return path

    def sysroot(self, release: str, commit: str) -> Path:
        sysroot = self.base / f"toolchains/{release}-{commit[:6]}"
        self.script(sysroot / "bin/rustc", f'printf "rustc {release} (x 2026-01-01)\\nbinary: rustc\\n'
                                           f'commit-hash: {commit}\\nrelease: {release}\\n"\n')
        return sysroot

    def toolchain(self, release="1.97.1", commit=PIN_COMMIT, cargo="1.97.1", pin_commit=PIN_COMMIT,
                  rustup=True) -> dict:
        """A sysroot whose rustc reports ``release``, reached through a proxy, a cargo on PATH, and
        a rustup whose own 1.97.1 toolchain is ``pin_commit`` (``rustup=False``: it cannot resolve it)."""
        sysroot = self.sysroot(release, commit)
        proxy = self.script(self.base / f"proxy-{release}-{commit[:6]}/rustc",
                            f'[ "$1" = --print ] && echo "{sysroot}"\n')
        self.script(self.bin / "cargo", f'echo "cargo {cargo} (x 2026-01-01)"\n')
        pinned = self.sysroot("1.97.1", pin_commit) / "bin/rustc"
        self.script(self.bin / "rustup", (f'[ "$1 $2 $3 $4" = "which --toolchain 1.97.1 rustc" ] && echo "{pinned}" '
                                          '&& exit 0\n' if rustup else "") + "exit 1\n")
        env = {**os.environ, "RUSTC": str(proxy), "PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
               "HOME": str(self.base), "CARGO_HOME": str(self.base / "cargo-home")}
        return receipt_tool.build_toolchain(self.root, env)

    def proof(self, toolchain: dict, commits) -> dict:
        (self.root / "Cargo.toml").write_text("[package]\n")
        binary = self.base / "oxidex"
        binary.write_bytes(b"".join(b"/rustc/" + c.encode() + b"/library/core/src/lib.rs\0" for c in commits))
        artifact = {"reason": "compiler-artifact", "manifest_path": str(self.root / "Cargo.toml"),
                    "target": {"name": "oxidex", "kind": ["bin"]}, "profile": {"test": False},
                    "executable": str(binary)}
        stdout = (json.dumps(artifact) + "\n" + json.dumps({"reason": "build-finished", "success": True})).encode()
        snapshot = {"source_commit": "a" * 40, "source_fingerprint": "b" * 64,
                    "runtime_input_manifest_sha256": "c" * 64, "source_dirty": False}
        toolchain = {**toolchain, "binary_rustc_commits": receipt_tool.instrument.embedded_rustc_commits(binary)}
        return {"schema": receipt_tool.BUILD_SCHEMA, "snapshot": snapshot, "source_root": str(self.root),
                "command": receipt_tool.BUILD_COMMAND, "returncode": 0, "cargo_artifact": artifact,
                "binary": receipt_tool.file_fact(binary), "toolchain": toolchain,
                "cargo_stdout_hex": stdout.hex(), "cargo_stdout_sha256": receipt_tool.sha(stdout),
                "cargo_stderr_hex": "", "cargo_stderr_sha256": receipt_tool.sha(b"")}

    def test_pinned_compiler_is_recorded_by_its_real_executable(self):
        record = self.toolchain()
        self.assertEqual(record["channel"], "1.97.1")
        self.assertTrue(record["rustc"]["path"].endswith("/bin/rustc"))
        self.assertNotIn("proxy", record["rustc"]["path"])
        self.assertEqual(receipt_tool.toolchain_identity(record, "1.97.1"),
                         {"release": "1.97.1", "commit_hash": PIN_COMMIT, "cargo_release": "1.97.1"})

    def test_off_pin_rustc_or_cargo_refuses_before_building(self):
        with self.assertRaisesRegex(ValueError, "rustc 1.98.1 .* not the pinned 1.97.1"):
            self.toolchain(release="1.98.1", commit=BREW_COMMIT)
        with self.assertRaisesRegex(ValueError, "cargo is 1.98.1"):
            self.toolchain(cargo="1.98.1")

    def test_non_rustup_rustc_reporting_the_pinned_release_refuses(self):
        # e.g. a distro or Homebrew build that happens to be 1.97.1: same release, not the pin.
        with self.assertRaisesRegex(ValueError, "is not the rustup-resolved pin"):
            self.toolchain(commit="2" * 40)

    def test_unresolvable_pin_fails_closed(self):
        with self.assertRaisesRegex(ValueError, "rustup cannot resolve the pinned toolchain 1.97.1"):
            self.toolchain(rustup=False)

    def test_rustups_pin_identity_is_recorded_and_replayed(self):
        record = self.toolchain()
        self.assertEqual(record["pin_rustc"]["commit_hash"], PIN_COMMIT)
        self.assertEqual(record["pin_rustc"]["release"], "1.97.1")
        for label, mutate in (("absent", lambda r: r.pop("pin_rustc")),
                              ("other commit", lambda r: r["pin_rustc"].update(commit_hash="3" * 40)),
                              ("other release", lambda r: r["pin_rustc"].update(release="1.98.1"))):
            broken = copy.deepcopy(record); mutate(broken)
            with self.subTest(label), self.assertRaises(ValueError):
                receipt_tool.toolchain_identity(broken, "1.97.1")

    def test_checkout_without_a_numeric_pin_refuses(self):
        (self.root / "rust-toolchain.toml").unlink()
        with self.assertRaisesRegex(ValueError, "no rust-toolchain.toml"):
            self.toolchain()
        (self.root / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "stable"\n')
        with self.assertRaisesRegex(ValueError, "not the pinned stable"):
            self.toolchain()

    def test_build_proof_requires_the_binary_to_carry_the_pinned_compiler(self):
        record = self.toolchain()
        good = self.proof(record, [PIN_COMMIT])
        receipt_tool.validate_build_proof(good, good["snapshot"], expected_channel="1.97.1")
        receipt_tool.check_binary_compiler(good)
        for label, commits in (("built by Homebrew", [BREW_COMMIT]), ("two toolchains", [BREW_COMMIT, PIN_COMMIT]),
                               ("no fingerprint", [])):
            bad = self.proof(record, commits)
            with self.subTest(label), self.assertRaisesRegex(ValueError, "not compiled by the recorded pinned"):
                receipt_tool.validate_build_proof(bad, bad["snapshot"], expected_channel="1.97.1")

    def test_build_proof_is_checked_against_the_validating_checkouts_pin(self):
        good = self.proof(self.toolchain(), [PIN_COMMIT])
        with self.assertRaisesRegex(ValueError, "pinned channel '1.98.1'"):
            receipt_tool.validate_build_proof(good, good["snapshot"], expected_channel="1.98.1")
        mutations = (
            lambda p: p.pop("toolchain"),
            lambda p: p["toolchain"]["rustc_version"].update(stdout_hex=b"rustc 1.97.1\nrelease: 1.97.1\n".hex()),
            lambda p: p["toolchain"]["rustc"].update(path="/opt/homebrew/bin/rustc"),
            lambda p: p["toolchain"]["cargo_version"].update(returncode=1),
            lambda p: p["toolchain"].update(binary_rustc_commits=[BREW_COMMIT]),
        )
        for index, mutate in enumerate(mutations):
            broken = copy.deepcopy(good); mutate(broken)
            with self.subTest(mutation=index), self.assertRaises(ValueError):
                receipt_tool.validate_build_proof(broken, broken["snapshot"], expected_channel="1.97.1")

    def test_on_disk_binary_must_still_carry_the_recorded_fingerprint(self):
        proof = self.proof(self.toolchain(), [PIN_COMMIT])
        Path(proof["binary"]["path"]).write_bytes(b"/rustc/" + BREW_COMMIT.encode() + b"/library\0")
        with self.assertRaisesRegex(ValueError, "fingerprint differs"):
            receipt_tool.check_binary_compiler(proof)


if __name__ == "__main__":
    unittest.main()
