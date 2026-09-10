"""scripts/exiftool_oracle.py's key helpers are verbatim mirrors of the corpus
gate's (tools/exiftool-tables/conformance.py). This suite pins the two copies
to identical behaviour and the mirrored argv to the gate's real argv, so a
drift in either shows up here rather than as a silently different grading
surface in scripts/."""

import sys
import unittest
from pathlib import Path
from unittest import mock

import exiftool_oracle

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools" / "exiftool-tables"))
import conformance  # noqa: E402

# Every shape the two docstrings name, plus the -a repeat spellings.
KEYS = [
    "EXIF:IFD0:Make", "File:FileType", "EXIF:IFD0:Copy1:Make", "File:Copy1:Comment",
    "Make", "SourceFile", "MPF:MPImage2:MPImageLength", "XMP:XMP-tiff:Make",
    "IFD0:Make", "OOXML:Custom:Division", "PNG:tEXt:comment", "IPTC:Keywords (2)",
    "XMP-tiff:Make", "", ":", "a:b:c:d",
]


class SplitHelpersMirrorConformanceTests(unittest.TestCase):
    def test_split_oracle_key_matches_conformance_on_every_shape(self):
        for k in KEYS:
            with self.subTest(key=k):
                self.assertEqual(exiftool_oracle.split_oracle_key(k),
                                 conformance.split_oracle_key(k))

    def test_split_oxidex_key_matches_conformance_on_every_shape(self):
        for k in KEYS:
            with self.subTest(key=k):
                self.assertEqual(exiftool_oracle.split_oxidex_key(k),
                                 conformance.split_oxidex_key(k))

    def test_oracle_rule_is_first_and_last_segment(self):
        self.assertEqual(exiftool_oracle.split_oracle_key("EXIF:IFD0:Copy1:Make"), ("EXIF", "Make"))
        self.assertEqual(exiftool_oracle.split_oracle_key("File:FileType"), ("File", "FileType"))
        self.assertEqual(exiftool_oracle.split_oracle_key("SourceFile"), ("", "SourceFile"))

    def test_oxidex_rule_is_first_segment_then_the_rest(self):
        self.assertEqual(exiftool_oracle.split_oxidex_key("OOXML:Custom:Division"),
                         ("OOXML", "Custom:Division"))
        self.assertEqual(exiftool_oracle.split_oxidex_key("IPTC:Keywords (2)"),
                         ("IPTC", "Keywords (2)"))
        self.assertEqual(exiftool_oracle.split_oxidex_key("Make"), ("", "Make"))

    def test_census_flags_are_the_gates_own_argv(self):
        # conformance.run_exiftool is the corpus gate's oracle read; the
        # mirrored constant must be exactly its flags.
        class Oracle:
            def command(self, extra):
                return ["exiftool", *extra]

        seen = {}

        class Done:
            stdout = "[{}]"

        def fake_run(argv, **_kwargs):
            seen["argv"] = argv
            return Done()

        with mock.patch.object(conformance.subprocess, "run", fake_run):
            conformance.run_exiftool(Oracle(), "x.jpg")
        self.assertEqual(seen["argv"][1:-1], list(exiftool_oracle.CENSUS_ORACLE_FLAGS))


if __name__ == "__main__":
    unittest.main()
