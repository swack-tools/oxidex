"""Corpus read receipt: identity folding, strict matching, exact coordinate credit."""
from __future__ import annotations

import copy
import json
import unittest

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
    capture = json.dumps({"/corpus/" + name: rows for name, rows in sources.items()}).encode()
    return {"build_proof": PROOF, "native": NATIVE, "observations": observations,
            "sources": fact(receipt_tool.source_command(NATIVE, ["/corpus/" + name for name in files]), capture),
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
        sources["a.jpg"] = [MAKE, MAKE, ORIENTATION]   # count differs from the one JSON value
        sources["b.jpg"] = [["IFD0", "Make", None, None, None], MODEL]
        derived = receipt_tool.derive(receipt(self.files(), sources))
        self.assertEqual(derived["credited_coordinates"], [])
        self.assertEqual(derived["metric_c"]["unattributable_file_identities"], 2)

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
            lambda d: d["sources"].update(returncode=2),
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


if __name__ == "__main__":
    unittest.main()
