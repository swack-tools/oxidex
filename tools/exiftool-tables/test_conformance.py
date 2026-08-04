#!/usr/bin/env python3
"""Focused regression tests for conformance.py's matching rules."""

import importlib.util
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("conformance.py")
SPEC = importlib.util.spec_from_file_location("conformance", MODULE_PATH)
conformance = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(conformance)


class CompareTests(unittest.TestCase):
    def test_duplicate_names_match_equal_values_before_counting_differences(self):
        exiftool = {
            "PDF:CreateDate": "2005:07:18 14:30:45-04:00",
            "EXIF:CreateDate": "2001:05:19 18:36:41",
        }
        oxidex = {
            "ExifIFD:CreateDate": "2001:05:19 18:36:41",
            "PDF:CreateDate": "2005:07:18 14:30:45-04:00",
        }

        result = conformance.compare(exiftool, oxidex)

        self.assertEqual(result["matched"], ["CreateDate", "CreateDate"])
        self.assertEqual(result["value_diff"], [])
        self.assertEqual(result["missing"], {})
        self.assertEqual(result["extra"], {})

    def test_duplicate_names_still_report_a_real_value_difference(self):
        exiftool = {
            "PDF:CreateDate": "pdf value",
            "EXIF:CreateDate": "exif value",
        }
        oxidex = {
            "PDF:CreateDate": "pdf value",
            "ExifIFD:CreateDate": "wrong value",
        }

        result = conformance.compare(exiftool, oxidex)

        self.assertEqual(result["matched"], ["CreateDate"])
        self.assertEqual(
            result["value_diff"],
            [("CreateDate", "exif value", "wrong value")],
        )

    def test_unique_names_remain_group_agnostic(self):
        result = conformance.compare(
            {"EXIF:Make": "Canon"},
            {"IFD0:Make": "Canon"},
        )

        self.assertEqual(result["matched"], ["Make"])
        self.assertEqual(result["value_diff"], [])


if __name__ == "__main__":
    unittest.main()
