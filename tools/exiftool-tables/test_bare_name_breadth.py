"""Controls for bare_name_breadth.py's parsing and verdicts (no oracle)."""

import importlib.util
import sys
import unittest
from pathlib import Path


def load():
    path = Path(__file__).with_name("bare_name_breadth.py")
    spec = importlib.util.spec_from_file_location("bare_name_breadth", path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


class BareNameBreadthTests(unittest.TestCase):
    def setUp(self):
        self.tool = load()

    def test_grouped_rows_and_continuations(self):
        rows = self.tool.parse_grouped(
            "[ExifIFD]       MeteringMode                    : Average\n"
            "[Kodak]         MeteringMode                    : Multi-segment\n"
            "[XMP-dc]        Description                     : line one\n"
            "line two\n"
        )
        self.assertEqual(rows[0], ("ExifIFD", "MeteringMode", "Average"))
        self.assertEqual(rows[1], ("Kodak", "MeteringMode", "Multi-segment"))
        self.assertEqual(rows[2], ("XMP-dc", "Description", "line one\nline two"))

    def test_only_names_in_two_groups_are_measured(self):
        rows = [
            ("ExifIFD", "MeteringMode", "Average"),
            ("Kodak", "MeteringMode", "Multi-segment"),
            ("IFD0", "Make", "A"),
            ("IFD0", "Make", "B"),  # one group twice is not a group conflict
            ("System", "FileName", "x.jpg"),
            ("File", "FileName", "x.jpg"),  # ignored name
        ]
        self.assertEqual(set(self.tool.multi_group_names(rows)), {"MeteringMode"})

    def test_verdicts(self):
        oracle_rows = [("ExifIFD", "Average"), ("Kodak", "Multi-segment")]
        classify = self.tool.classify
        self.assertEqual(classify("MeteringMode", "Multi-segment", "Multi-segment", oracle_rows, []), "MATCH")
        self.assertEqual(classify("MeteringMode", "Multi-segment", "", oracle_rows, []), "NOT_EMITTED")
        self.assertEqual(classify("MeteringMode", "Multi-segment", "Spot", oracle_rows, []), "VALUE")
        # oxidex has the winning group but chose the other copy
        both = [("ExifIFD", "MeteringMode", "Average"), ("Kodak", "MeteringMode", "Multi-segment")]
        self.assertEqual(classify("MeteringMode", "Multi-segment", "Average", oracle_rows, both), "WRONG_WINNER")
        # the winning group is absent from oxidex's -a -G1 output
        only_exif = [("ExifIFD", "MeteringMode", "Average")]
        self.assertEqual(classify("MeteringMode", "Multi-segment", "Average", oracle_rows, only_exif), "WINNER_ABSENT")


if __name__ == "__main__":
    unittest.main()
