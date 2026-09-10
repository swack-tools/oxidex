#!/usr/bin/env python3
"""Focused regression tests for conformance.py's matching rules."""

import importlib.util
import unittest
from collections import Counter
from pathlib import Path
from unittest import mock


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


class OracleKeyShapeTests(unittest.TestCase):
    """The oracle is asked for `-G0:1:4` keys, not `-G`.

    ExifTool's JSON writer keeps ONE entry per key. Under family-0 keys the
    three MPF sub-images of combined-samples/Apple/Apple_iPhone11.jpg all
    spell `MPF:MPImageLength`, so two of them were gone before compare()
    ran and OxiDex's two correct rows scored EXTRA (census at 25a2109e:
    MPImage1/2/3 EXTRA 3456/1488/166 over 4,238 files under conformance.py).
    Family-1 qualification (`MPF:MPImage1:MPImageLength`) keeps the keys
    distinct; split_oracle_key takes the group from the first segment and
    the name from the last, so the report still speaks family-0 groups.
    OxiDex keys are NOT split that way -- see split_oxidex_key and
    OxidexKeyShapeTests.
    """

    THREE_MPF_ROWS_FROM_OXIDEX = {
        "MPImage1:MPImageLength": 1001,
        "MPImage2:MPImageLength": 2002,
        "MPImage3:MPImageLength": 3003,
    }

    def test_split_oracle_key_takes_first_segment_as_group_and_last_as_name(self):
        split = conformance.split_oracle_key
        self.assertEqual(split("EXIF:IFD0:Make"), ("EXIF", "Make"))
        self.assertEqual(split("File:System:FileName"), ("File", "FileName"))
        # Family 1 == family 0: ExifTool prints one segment, not "File:File".
        self.assertEqual(split("File:FileType"), ("File", "FileType"))
        # A family-4 copy segment, should a caller ever ask -G0:1:4, is
        # just another middle segment.
        self.assertEqual(
            split("MPF:MPImage2:Copy2:MPImageLength"), ("MPF", "MPImageLength"),
        )
        self.assertEqual(split("EXIF:IFD0:Copy1:Make"), ("EXIF", "Make"))
        self.assertEqual(split("Make"), ("", "Make"))

    def test_family_1_qualified_oracle_keys_match_every_mpf_sub_image(self):
        exiftool = {
            "MPF:MPImage1:MPImageLength": 1001,
            "MPF:MPImage2:MPImageLength": 2002,
            "MPF:MPImage3:MPImageLength": 3003,
        }

        result = conformance.compare(exiftool, self.THREE_MPF_ROWS_FROM_OXIDEX)

        self.assertEqual(result["matched"], ["MPImageLength"] * 3)
        self.assertEqual(result["value_diff"], [])
        self.assertEqual(result["missing"], {})
        self.assertEqual(result["extra"], {})

    def test_family_0_collapsed_oracle_key_scores_two_correct_rows_extra(self):
        # What the old `-G -s -j -a` invocation handed compare(): ExifTool's
        # writer had already kept one of the three MPImageLength entries, so
        # the other two OxiDex rows -- both correct -- could only be EXTRA.
        # This is the defect the -G0:1 request removes; compare() itself is
        # blameless, which is why this case passes on the old code too.
        exiftool = {"MPF:MPImageLength": 1001}

        result = conformance.compare(exiftool, self.THREE_MPF_ROWS_FROM_OXIDEX)

        self.assertEqual(result["matched"], ["MPImageLength"])
        self.assertEqual(result["value_diff"], [])
        self.assertEqual(result["missing"], {})
        self.assertEqual(result["extra"], {
            "MPImage2:MPImageLength": ("MPImage2", 2002),
            "MPImage3:MPImageLength": ("MPImage3", 3003),
        })

    def test_run_exiftool_asks_for_family_0_1_and_4_groups(self):
        # Family 4 ('Copy N') is what keeps a repeat inside one family-1
        # group from collapsing in ExifTool's one-entry-per-key JSON writer:
        # residual 393/25,269 occurrences under -G0:1, 0 under -G0:1:4, over
        # the author's 200-file subset (residual.py, pinned 13.59).
        seen = {}

        class Oracle:
            def command(self, extra):
                return ["exiftool", *extra]

        class Done:
            stdout = '[{"SourceFile": "x.jpg", "MPF:MPImage1:MPImageLength": 1001}]'

        def fake_run(argv, **_kwargs):
            seen["argv"] = argv
            return Done()

        with mock.patch.object(conformance.subprocess, "run", fake_run):
            et = conformance.run_exiftool(Oracle(), "x.jpg")

        self.assertEqual(seen["argv"], ["exiftool", "-G0:1:4", "-s", "-j", "-a", "x.jpg"])
        self.assertEqual(et["MPF:MPImage1:MPImageLength"], 1001)

    def test_file_type_is_read_through_split_oracle_key(self):
        self.assertEqual(conformance.file_type({"File:FileType": "JPEG"}), "JPEG")
        self.assertEqual(conformance.file_type({"File:File:FileType": "JPEG"}), "JPEG")
        self.assertEqual(conformance.file_type({"FileType": "JPEG"}), "JPEG")
        self.assertIsNone(conformance.file_type({"EXIF:IFD0:Make": "Canon"}))


class OxidexKeyShapeTests(unittest.TestCase):
    """OxiDex keys split by the pre-f3b5f5e6 rule: first segment, then the rest.

    OxiDex emits keys whose middle segment is part of the NAME, not a group
    (src/parsers/document/ooxml.rs 'OOXML:Custom:{name}', src/writers/
    png_writer.rs 'PNG:{chunk}:{keyword}'; 26 such keys on t/images/OOXML.docx
    and one on PNG.png under the 25a2109e binary). f3b5f5e6 applied the
    oracle's last-segment rule to both sides, which rewrote 'Custom:Division'
    to 'Division' and matched it against the oracle's 'XML:Division' -- the
    census stopped seeing a name defect that is OxiDex's to fix. The two
    sides have their own rules now; this class pins the OxiDex one.
    """

    def test_split_oxidex_key_keeps_everything_after_the_group_as_the_name(self):
        split = conformance.split_oxidex_key
        self.assertEqual(split("OOXML:Custom:Division"), ("OOXML", "Custom:Division"))
        self.assertEqual(split("PNG:tEXt:comment"), ("PNG", "tEXt:comment"))
        self.assertEqual(split("IFD0:Make"), ("IFD0", "Make"))
        self.assertEqual(split("MPImage2:MPImageLength"), ("MPImage2", "MPImageLength"))
        self.assertEqual(split("Make"), ("", "Make"))

    def test_the_two_rules_differ_exactly_where_a_name_carries_a_colon(self):
        # Same input, two answers: that is why tags_by_name refuses a default.
        self.assertEqual(conformance.split_oracle_key("OOXML:Custom:Division"),
                         ("OOXML", "Division"))
        self.assertEqual(conformance.split_oxidex_key("OOXML:Custom:Division"),
                         ("OOXML", "Custom:Division"))
        self.assertEqual(conformance.split_oracle_key("PNG:tEXt:comment"),
                         ("PNG", "comment"))
        self.assertEqual(conformance.split_oxidex_key("PNG:tEXt:comment"),
                         ("PNG", "tEXt:comment"))

    def test_tags_by_name_requires_the_side_to_be_named(self):
        with self.assertRaises(TypeError):
            conformance.tags_by_name({"IFD0:Make": "Canon"})

    def test_oxidex_colon_in_name_is_exposed_not_masked(self):
        # A non-distinctive value ('42' is too short for infer_renames to
        # pair on), so the pair cannot be explained away as a RENAME either:
        # the oracle's 'Division' is MISSING and OxiDex's 'Custom:Division'
        # is EXTRA -- exactly what the 25a2109e script reported. Under
        # f3b5f5e6's shared last-segment rule this compared as one match.
        result = conformance.compare(
            {"XML:Division": "42"},
            {"OOXML:Custom:Division": "42"},
        )

        self.assertEqual(result["matched"], [])
        self.assertEqual(result["value_diff"], [])
        self.assertEqual(result["missing"], {"Division": ("XML", "42")})
        self.assertEqual(result["extra"], {"Custom:Division": ("OOXML", "42")})
        self.assertEqual(result["renames"], [])

    def test_oxidex_png_chunk_key_is_exposed_as_a_rename_when_the_value_identifies_it(self):
        # PNG.png: the oracle prints 'PNG:Comment' = 'test comment', OxiDex
        # 'PNG:tEXt:comment'. The value is distinctive, so infer_renames
        # pairs them and reports the RENAME 'tEXt:comment' -> 'Comment' --
        # a name fix OxiDex owes, visible in the report, not a silent match.
        result = conformance.compare(
            {"PNG:Comment": "test comment"},
            {"PNG:tEXt:comment": "test comment"},
        )

        self.assertEqual(result["matched"], [])
        self.assertEqual(result["renames"], [("tEXt:comment", "Comment", "test comment")])
        self.assertEqual(result["missing"], {})
        self.assertEqual(result["extra"], {})


class LeftoverOccurrenceTests(unittest.TestCase):
    """Every unmatched occurrence is one report row; none is overwritten.

    compare() keys leftover rows by occurrence_name(family-0 group, name).
    Under -G0:1 two oracle rows from different family-1 groups spell the
    same family-0 key, and the dict write was last-writer-wins: on DNG.dng
    'EXIF:Compression' survived with IFD1's 'JPEG' while IFD0's
    'Uncompressed' vanished, so MISSING was a lower bound (187 occurrences
    lost over a reviewer's 200-file format-breadth subset; 969 leftovers ->
    968 rows on a JPEG subset). place_occurrence suffixes ' (2)', ' (3)'.
    """

    def test_two_leftover_oracle_occurrences_sharing_a_family_0_key_both_count(self):
        exiftool = {
            "EXIF:IFD0:Compression": "Uncompressed",
            "EXIF:IFD1:Compression": "JPEG",
        }

        result = conformance.compare(exiftool, {})

        self.assertEqual(len(result["missing"]), 2)
        self.assertEqual(result["missing"], {
            "EXIF:Compression": ("EXIF", "Uncompressed"),
            "EXIF:Compression (2)": ("EXIF", "JPEG"),
        })
        self.assertEqual(result["matched"], [])
        self.assertEqual(result["value_diff"], [])
        self.assertEqual(result["extra"], {})

    def test_first_occurrence_in_oracle_order_keeps_the_bare_key(self):
        exiftool = {
            "EXIF:IFD0:XResolution": 72,
            "EXIF:IFD1:XResolution": 300,
            "EXIF:SubIFD:XResolution": 600,
        }

        result = conformance.compare(exiftool, {})

        self.assertEqual(list(result["missing"].items()), [
            ("EXIF:XResolution", ("EXIF", 72)),
            ("EXIF:XResolution (2)", ("EXIF", 300)),
            ("EXIF:XResolution (3)", ("EXIF", 600)),
        ])

    def test_a_matched_occurrence_does_not_shift_the_suffixes_of_the_leftovers(self):
        exiftool = {
            "EXIF:IFD0:Compression": "Uncompressed",
            "EXIF:IFD1:Compression": "JPEG",
            "EXIF:SubIFD:Compression": "JPEG",
        }
        oxidex = {"IFD1:Compression": "JPEG"}

        result = conformance.compare(exiftool, oxidex)

        # Tier 2 (exact value, any group) consumes ONE of the two 'JPEG'
        # rows; the two leftovers are reported, in order, without a gap.
        self.assertEqual(result["matched"], ["Compression"])
        self.assertEqual(result["missing"], {
            "EXIF:Compression": ("EXIF", "Uncompressed"),
            "EXIF:Compression (2)": ("EXIF", "JPEG"),
        })
        self.assertEqual(result["extra"], {})

    def test_place_occurrence_is_symmetric_for_the_extra_side(self):
        # An `oxidex -j` dict cannot collide (its key IS 'group:name'), so
        # the EXTRA path is exercised through the helper directly.
        rows = {}
        self.assertEqual(conformance.place_occurrence(rows, "PNG:Comment", "PNG", "a"),
                         "PNG:Comment")
        self.assertEqual(conformance.place_occurrence(rows, "PNG:Comment", "PNG", "b"),
                         "PNG:Comment (2)")
        self.assertEqual(conformance.place_occurrence(rows, "PNG:Comment", "PNG", "c"),
                         "PNG:Comment (3)")
        self.assertEqual(rows, {
            "PNG:Comment": ("PNG", "a"),
            "PNG:Comment (2)": ("PNG", "b"),
            "PNG:Comment (3)": ("PNG", "c"),
        })

    def test_unique_names_are_untouched(self):
        result = conformance.compare({"EXIF:IFD0:Make": "Canon"}, {})
        self.assertEqual(result["missing"], {"Make": ("EXIF", "Canon")})


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
