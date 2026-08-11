#!/usr/bin/env python3
"""Focused regression tests for conformance.py's matching rules."""

import importlib.util
import unittest
from collections import Counter
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

        # PDF:CreateDate pairs on group+value. The EXIF/ExifIFD leftover has
        # two ET occurrences and two OxiDex occurrences of "CreateDate" to
        # begin with, so this is NOT the "name unique on both sides" case --
        # cross-group pairing here would be exactly the unprincipled punt
        # that manufactures false VALUE diffs on APE.mpc (see
        # test_group_qualified_matching_on_the_ape_mpc_regression below).
        # Group-qualified matching under-claims instead: EXIF:CreateDate is
        # reported MISSING and ExifIFD:CreateDate is reported EXTRA, rather
        # than guessed as a value_diff pair.
        self.assertEqual(result["matched"], ["CreateDate"])
        self.assertEqual(result["value_diff"], [])
        self.assertEqual(result["missing"], {"EXIF:CreateDate": ("EXIF", "exif value")})
        self.assertEqual(result["extra"], {"ExifIFD:CreateDate": ("ExifIFD", "wrong value")})

    def test_unique_names_remain_group_agnostic(self):
        result = conformance.compare(
            {"EXIF:Make": "Canon"},
            {"IFD0:Make": "Canon"},
        )

        self.assertEqual(result["matched"], ["Make"])
        self.assertEqual(result["value_diff"], [])

    def test_unique_name_cross_group_fallback_still_reports_a_value_diff(self):
        # Tier 4 (the gated last-resort punt) fires only when the name is
        # unique on both sides. Here it genuinely is -- one ET occurrence,
        # one OxiDex occurrence -- so a differing value is still surfaced
        # as a value_diff rather than silently dropped to missing+extra.
        result = conformance.compare(
            {"EXIF:Make": "Canon"},
            {"IFD0:Make": "Nikon"},
        )

        self.assertEqual(result["matched"], [])
        self.assertEqual(
            result["value_diff"],
            [("Make", "Canon", "Nikon", "structural")],
        )
        self.assertEqual(result["missing"], {})
        self.assertEqual(result["extra"], {})

    def test_group_qualified_matching_on_the_ape_mpc_regression(self):
        """The motivating defect for Step 12 (OVERHAUL_OXIDEX_PLAN.md).

        Fixture values are the real output of the pinned ExifTool 13.59
        oracle and OxiDex against t/images/APE.mpc (captured 2026-08-10).
        Before this step, bare-name matching paired OxiDex's MPC:*/APE:*/
        ID3:*/ID3v1:* tags against each other by coincidence -- 10 false
        VALUE diffs plus one false cross-group MATCH -- because the old
        fallback punted to `actual[0]` whenever anything was left over,
        with no check on how many unrelated candidates were competing.

        The correct classification, group-qualified: OxiDex has no MPC or
        APE parser wired up (11 MPC:* + 11 APE:* MISSING, real extraction
        gaps -- see Step 32), it reads the ID3v1 trailer ExifTool's own
        JSON writer drops once ID3v2 outranks it for the same tag names
        (ID3v1:* EXTRA), its ID3v2 reading is byte-for-byte correct
        (16 ID3:* matches + the derived Composite:DateTimeOriginal), and
        there is not one genuine VALUE difference anywhere in the file.
        """
        exiftool = {
            "File:ID3Size": 391,
            "MPC:TotalFrames": 102,
            "MPC:SampleRate": 44100,
            "MPC:Quality": "5 (Standard)",
            "MPC:MaxBand": 28,
            "MPC:ReplayGainTrackPeak": 0,
            "MPC:ReplayGainTrackGain": 0,
            "MPC:ReplayGainAlbumPeak": 0,
            "MPC:ReplayGainAlbumGain": 0,
            "MPC:FastSeek": "No",
            "MPC:Gapless": "Yes",
            "MPC:EncoderVersion": "1.1.5",
            "APE:Track": 4,
            "APE:Year": 2005,
            "APE:Genre": "Electronic",
            "APE:Artist": "Kraftwerk",
            "APE:Album": "Cover Art Test",
            "APE:ToolVersion": "11.1.102",
            "APE:ToolName": "Media Center",
            "APE:Title": "Men Machine Live",
            "APE:MediaJukeboxDate": 38353,
            "APE:CoverArtFrontDesc": r"X:\_kuvat\Kraftwerk - Cover Art Test.jpg",
            "APE:CoverArtFront": "(Binary data 1761 bytes, use -b option to extract)",
            "ID3:Track": "1/5",
            "ID3:PartOfSet": "1/2",
            "ID3:RelativeVolumeAdjustment": "+18.0% Right, +18.0% Left",
            "ID3:Lyrics": "Do-wap she-bang",
            "ID3:PictureFormat": "JPG",
            "ID3:PictureType": "Other",
            "ID3:PictureDescription": "comment",
            "ID3:Picture": "(Binary data 15 bytes, use -b option to extract)",
            "ID3:Title": "ExifTool Test",
            "ID3:Artist": "Phil Harvey",
            "ID3:Composer": "A Composer",
            "ID3:Album": "Phil's Greatest Hits",
            "ID3:Grouping": "This group",
            "ID3:Year": 2005,
            "ID3:Genre": "Testing",
            "ID3:Comment": "My Comments",
            "Composite:DateTimeOriginal": 2005,
        }
        oxidex = {
            "ID3:Album": "Phil's Greatest Hits",
            "ID3:Artist": "Phil Harvey",
            "ID3:Comment": "My Comments",
            "ID3:Composer": "A Composer",
            "ID3:Genre": "Testing",
            "ID3:Grouping": "This group",
            "ID3:Lyrics": "Do-wap she-bang",
            "ID3:PartOfSet": "1/2",
            "ID3:Picture": "(Binary data 15 bytes, use -b option to extract)",
            "ID3:PictureDescription": "comment",
            "ID3:PictureFormat": "JPG",
            "ID3:PictureType": "Other",
            "ID3:RelativeVolumeAdjustment": "+18.0% Right, +18.0% Left",
            "ID3:Title": "ExifTool Test",
            "ID3:Track": "1/5",
            "ID3:Version": "2.2.0",
            "ID3:Year": 2005,
            "ID3Size": 391,
            "ID3TagSize": 128,
            "ID3Version": "ID3 v1",
            "ID3v1:Album": "The Test Album",
            "ID3v1:Artist": "Who Knows",
            "ID3v1:Comment": "a nice comment",
            "ID3v1:Genre": "Funk",
            "ID3v1:Title": "A 4s sample for testing embedd",
            "ID3v1:Year": 2006,
            "MP3:ID3Version": "ID3 v1",
            "Composite:DateTimeOriginal": 2005,
        }

        result = conformance.compare(exiftool, oxidex)

        self.assertEqual(result["value_diff"], [], "zero VALUE diffs")
        self.assertEqual(len(result["missing"]), 22, "11 MPC:* + 11 APE:* MISSING")
        missing_groups = Counter(g for g, _v in result["missing"].values())
        self.assertEqual(missing_groups, Counter({"MPC": 11, "APE": 11}))
        extra_groups = Counter(g for g, _v in result["extra"].values())
        self.assertEqual(extra_groups["ID3v1"], 6, "the full ID3v1 trailer is EXTRA")
        # ID3v2's 16 tags plus the Composite all read correctly and match.
        self.assertEqual(len(result["matched"]), 18)
        self.assertEqual(result["renames"], [])


class SeverityTests(unittest.TestCase):
    def test_date_time(self):
        self.assertEqual(
            conformance.classify_severity(
                "2005:07:18 14:30:45-04:00", "2005:07:18 14:30:45"),
            "date_time",
        )

    def test_binary(self):
        self.assertEqual(
            conformance.classify_severity(
                "(Binary data 15 bytes, use -b option to extract)", "0"),
            "binary",
        )

    def test_identity_is_a_formatting_nit(self):
        self.assertEqual(conformance.classify_severity("Canon", "canon"), "identity")

    def test_numeric(self):
        self.assertEqual(conformance.classify_severity("34", "35"), "numeric")

    def test_display_only_is_a_printconv_gap(self):
        self.assertEqual(conformance.classify_severity("5", "5 (Standard)"), "display_only")

    def test_structural_is_the_fallback(self):
        self.assertEqual(conformance.classify_severity("Canon", "Nikon"), "structural")


if __name__ == "__main__":
    unittest.main()
