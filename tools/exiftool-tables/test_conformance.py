#!/usr/bin/env python3
"""Focused regression tests for conformance.py's matching rules."""

import importlib.util
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import unittest
from collections import Counter
from pathlib import Path
from unittest import mock


MODULE_PATH = Path(__file__).with_name("conformance.py")
SPEC = importlib.util.spec_from_file_location("conformance", MODULE_PATH)
conformance = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(conformance)


class OracleResolutionTests(unittest.TestCase):
    @staticmethod
    def _source(tmp, version="13.59"):
        root = tmp / "exiftool-source"
        (root / "lib" / "Image").mkdir(parents=True)
        (root / "t" / "images").mkdir(parents=True)
        (root / "exiftool").write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        (root / "lib" / "Image" / "ExifTool.pm").write_text(
            f"$VERSION = '{version}';\n", encoding="utf-8")
        (root / "t" / "images" / "OOXML.docx").write_bytes(b"docx")
        perl = tmp / "perl"
        perl.write_text(
            "#!/bin/sh\n"
            "case \" $* \" in\n"
            f"  *' -ver'*) printf '%s\\n' '{version}' ;;\n"
            "  *) printf 'DOCX\\n' ;;\n"
            "esac\n",
            encoding="utf-8",
        )
        perl.chmod(0o755)
        return root, perl

    def test_normal_resolution_accepts_ci_source_and_perl(self):
        with tempfile.TemporaryDirectory() as directory:
            root, perl = self._source(Path(directory))
            with mock.patch.dict(os.environ, {"EXIFTOOL_PERL": str(perl)}, clear=False):
                oracle = conformance.resolve_oracle(root)
            self.assertEqual(oracle.version, conformance.expected_exiftool_version())
            self.assertEqual(Path(oracle.interpreter), perl.resolve())
            self.assertEqual(oracle.argv[-2:], ["-config", ""])

    def test_strict_release_rejects_ambient_selector_even_with_explicit_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root, perl = self._source(Path(directory))
            with mock.patch.dict(os.environ, {"EXIFTOOL_PERL": str(perl)}, clear=False):
                with self.assertRaisesRegex(conformance.exiftool_oracle.OracleError,
                                            "ambient selectors"):
                    conformance.resolve_oracle(root, perl=str(perl), strict=True)

    def test_strict_release_requires_explicit_source_and_interpreter(self):
        with mock.patch.dict(os.environ, {}, clear=True), \
                mock.patch.object(conformance.subprocess, "run",
                                  side_effect=AssertionError("implicit oracle probe")):
            with self.assertRaisesRegex(conformance.exiftool_oracle.OracleError,
                                        "requires explicit"):
                conformance.resolve_oracle(strict=True)

    def test_resolution_uses_the_checkout_pin_for_an_upgrade(self):
        with tempfile.TemporaryDirectory() as directory:
            root, perl = self._source(Path(directory), version="13.60")
            with mock.patch.object(conformance, "expected_exiftool_version",
                                   return_value="13.60"):
                oracle = conformance.resolve_oracle(root, perl=str(perl))
            self.assertEqual(oracle.version, "13.60")
            self.assertEqual(oracle.pinned_version, "13.60")


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
            "ID3v1:Title": "A 4s sample for testing embedded",
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

        self.assertEqual(seen["argv"], [
            "exiftool", "-config", "", "-G0:1:4", "-s", "-j", "-a", "x.jpg",
        ])
        self.assertEqual(et["MPF:MPImage1:MPImageLength"], 1001)

    def test_run_exiftool_forces_deterministic_locale(self):
        seen = {}

        class Oracle:
            def command(self, extra):
                return ["exiftool", *extra]

        class Done:
            stdout = '[{"File:FileType": "JPEG"}]'

        def fake_run(argv, **kwargs):
            seen["argv"] = argv
            seen["env"] = kwargs["env"]
            return Done()

        with mock.patch.object(conformance.subprocess, "run", fake_run):
            conformance.run_exiftool(Oracle(), "x.jpg")

        self.assertEqual(seen["env"]["LC_ALL"], "C")
        self.assertEqual(seen["env"]["LANG"], "C")

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


# Runs conformance.main() end to end in a fresh interpreter, with the four
# instrument seams (oracle, git state, binary, header) and the two tool calls
# faked from a fixtures file, so the --json-out it writes is the real one.
# Also records the iteration order of the raw tag-name set compare() builds
# for the first fixture, so the caller can prove the hash seed reordered it.
_JSON_OUT_DRIVER = r"""
import importlib.util, json, sys
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

module_path, corpus, json_out, fixtures_path, set_order_out = sys.argv[1:]
spec = importlib.util.spec_from_file_location("conformance", module_path)
conformance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(conformance)
fixtures = json.loads(Path(fixtures_path).read_text(encoding="utf-8"))

binary_path = Path(corpus).parent / "oxidex"
binary_path.write_bytes(b"determinism test binary")
oracle_root = Path(corpus).parent / "oracle"
(oracle_root / "lib" / "Image").mkdir(parents=True, exist_ok=True)
(oracle_root / "t" / "images").mkdir(parents=True, exist_ok=True)
(oracle_root / "exiftool").write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
(oracle_root / "lib" / "Image" / "ExifTool.pm").write_text(
    "$VERSION = '13.59';\n", encoding="utf-8")
(oracle_root / "t" / "images" / "OOXML.docx").write_bytes(b"docx")
perl_path = Path(corpus).parent / "perl"
perl_path.write_text(
    "#!/bin/sh\ncase \" $* \" in *' -ver'*) printf '13.59\\n' ;; *) printf 'DOCX\\n' ;; esac\n",
    encoding="utf-8")
perl_path.chmod(0o755)
oracle = SimpleNamespace(
    version="13.59", pinned_version="13.59", source="pinned source tree",
    interpreter=str(perl_path),
    missing_modules=[], verified=True,
    argv=[str(perl_path), f"-I{oracle_root / 'lib'}",
          str(oracle_root / "exiftool"),
          "-config", ""],
    check_container_support=lambda _docx: None,
    provenance=lambda: "ExifTool 13.59 (pinned, via pinned source tree)",
    display=lambda: " ".join(oracle.argv),
)
git = SimpleNamespace(
    repo_root=Path(module_path).resolve().parents[2],
    commit="0123456789abcdef0123456789abcdef01234567", describe="0123456",
    dirty=False, dirty_files=[], short=lambda: "0123456 (clean)",
)
binary = SimpleNamespace(kind="oxidex", requested="oxidex", path=binary_path,
                          mtime=0.0, size=binary_path.stat().st_size)

first = fixtures[min(fixtures)]
names = (conformance.tags_by_name(first["exiftool"], conformance.split_oracle_key).keys()
         | conformance.tags_by_name(first["oxidex"], conformance.split_oxidex_key).keys())
Path(set_order_out).write_text(json.dumps(list(names)), encoding="utf-8")

inst = conformance.instrument
with mock.patch.object(conformance, "resolve_oracle", lambda *args, **kwargs: oracle), \
        mock.patch.object(inst, "git_state", lambda: git), \
        mock.patch.object(inst, "refuse_if_dirty", lambda _git, _tool: False), \
        mock.patch.object(inst, "resolve_binary", lambda _req, kind: binary), \
        mock.patch.object(inst, "print_header", lambda **_kw: None), \
        mock.patch.object(conformance, "run_exiftool",
                          lambda _oracle, p: fixtures[Path(p).name]["exiftool"]), \
        mock.patch.object(conformance, "run_oxidex",
                          lambda _binary, p: fixtures[Path(p).name]["oxidex"]), \
        mock.patch.object(sys, "argv",
                          ["conformance.py", corpus, "--json-out", json_out]):
    conformance.main()
"""


_AUTHENTICATED_RECEIPT_DRIVER = r"""
import importlib.util, json, os, sys
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

module_path, corpus, json_out, fixtures_path = sys.argv[1:5]
mutation = sys.argv[5] if len(sys.argv) > 5 else ""
spec = importlib.util.spec_from_file_location("conformance", module_path)
conformance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(conformance)
fixtures = json.loads(Path(fixtures_path).read_text(encoding="utf-8"))

binary_path = Path(corpus).parent / "oxidex"
binary_path.write_bytes(b"authenticated test binary")
oracle_root = Path(corpus).parent / "oracle"
(oracle_root / "lib" / "Image").mkdir(parents=True, exist_ok=True)
(oracle_root / "t" / "images").mkdir(parents=True, exist_ok=True)
(oracle_root / "exiftool").write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
(oracle_root / "lib" / "Image" / "ExifTool.pm").write_text(
    "$VERSION = '13.59';\n", encoding="utf-8")
(oracle_root / "t" / "images" / "OOXML.docx").write_bytes(b"docx")
perl_path = Path(corpus).parent / "perl"
perl_path.write_text(
    "#!/bin/sh\ncase \" $* \" in *' -ver'*) printf '13.59\\n' ;; *) printf 'DOCX\\n' ;; esac\n",
    encoding="utf-8")
perl_path.chmod(0o755)
oracle = SimpleNamespace(
    version="13.59",
    pinned_version="13.59",
    source="pinned source tree",
    interpreter=str(perl_path),
    missing_modules=[],
    verified=True,
    argv=[str(perl_path), f"-I{oracle_root / 'lib'}",
          str(oracle_root / "exiftool"),
          "-config", ""],
    provenance=lambda: f"ExifTool 13.59 (pinned, perl {perl_path}, via pinned source tree)",
    display=lambda: " ".join(oracle.argv),
    check_container_support=lambda _docx: None,
)
git = SimpleNamespace(
    repo_root=Path(module_path).resolve().parents[2],
    commit="0123456789abcdef0123456789abcdef01234567",
    describe="0123456",
    dirty=False,
    dirty_files=[],
    short=lambda: "0123456 (0123456789ab, clean)",
)
binary = SimpleNamespace(
    kind="oxidex",
    requested="./target/debug/oxidex",
    path=binary_path,
    mtime=0.0,
    size=binary_path.stat().st_size,
)
git_after = SimpleNamespace(
    repo_root=git.repo_root,
    commit="f" * 40 if mutation == "source" else git.commit,
    describe=git.describe,
    dirty=False,
    dirty_files=[],
    short=git.short,
)
mutated = False
oxidex_calls = 0

def fake_oxidex(_binary, path):
    global mutated, oxidex_calls
    oxidex_calls += 1
    if mutation == "corpus" and not mutated:
        Path(path).write_bytes(b"mutated corpus input")
        mutated = True
    if mutation == "replay_corpus" and oxidex_calls > 2 and not mutated:
        Path(path).write_bytes(b"mutated during validator replay")
        mutated = True
    return fixtures[Path(path).name]["oxidex"]

with mock.patch.object(conformance, "resolve_oracle", lambda *args, **kwargs: oracle), \
        mock.patch.object(conformance.instrument, "git_state", side_effect=[git, git_after, git_after]), \
        mock.patch.object(conformance.instrument, "refuse_if_dirty", lambda _git, _tool: False), \
        mock.patch.object(conformance.instrument, "resolve_binary", lambda _req, kind: binary), \
        mock.patch.object(conformance.instrument, "print_header", lambda **_kw: None), \
        mock.patch.object(conformance, "run_exiftool", lambda _oracle, p: fixtures[Path(p).name]["exiftool"]), \
        mock.patch.object(conformance, "run_oxidex", fake_oxidex), \
        mock.patch.object(sys, "argv", ["conformance.py", corpus, "--min-files", "2",
                                          "--min-tags", "1", "--json-out", json_out]):
    conformance.main()

if mutation in {"forge", "subset", "oracle_manifest", "per_format", "per_file",
                "renames", "missing", "extra", "severity", "replay_corpus"}:
    data = json.loads(Path(json_out).read_text(encoding="utf-8"))
    expected_contract = {
        "selection": {
            "roots": [str(Path(corpus).resolve())],
            "recursive": False,
            "only": None,
            "extensions": None,
            "excluded_extensions": [],
        },
        "floors": {"min_files": 2, "min_tags": 1},
    }
    if mutation == "forge":
        data.update({
            "oracle_occurrences": 1,
            "candidate_occurrences": 1,
            "matched_occurrences": 1,
            "missing_occurrences": 0,
            "extra_occurrences": 0,
            "value_occurrences": 0,
            "rename_source_occurrences": 0,
            "rename_target_occurrences": 0,
            "oracle_tag_count": 1,
        })
    elif mutation == "subset":
        manifest = data["instrument"]["corpus_manifest"]
        first = sorted(manifest["files"])[0]
        data["instrument"]["selection"]["only"] = Path(first).name
        data["instrument"]["corpus_manifest"] = conformance.corpus_manifest([first])
        data["instrument"]["file_count"] = 1
        data["instrument"]["selected_file_count"] = 1
        data["instrument"]["scored_file_count"] = 1
        rows = [
            row for row in data["instrument"]["measurement_transcript"]["rows"]
            if row["path"] == first
        ]
        data["instrument"]["measurement_transcript"] = conformance.measurement_transcript(rows)
        row = rows[0]
        for key in (
                "oracle_occurrences", "candidate_occurrences", "matched_occurrences",
                "missing_occurrences", "extra_occurrences", "value_occurrences",
                "rename_source_occurrences", "rename_target_occurrences"):
            data[key] = row[key]
        data["oracle_tag_count"] = row["oracle_occurrences"]
        data["instrument"]["floors"] = {"min_files": 1, "min_tags": 1}
    elif mutation == "oracle_manifest":
        include_root = Path(corpus).parent / f"r3-unmanifested-{os.getpid()}"
        (include_root / "Image").mkdir(parents=True, exist_ok=True)
        (include_root / "Image" / "ExifTool.pm").write_text(
            "# adversarial unmanifested oracle include\n", encoding="utf-8")
        argv = data["instrument"]["oracle"]["argv"]
        data["instrument"]["oracle"]["argv"] = [argv[0], f"-I{include_root}", *argv[1:]]
        data["instrument"]["oracle"]["command"] = " ".join(
            data["instrument"]["oracle"]["argv"])
    elif mutation == "per_format":
        data["per_format"] = {"FORGED": {"files": 999, "matched": 999}}
    elif mutation == "per_file":
        data["per_file"] = {}
    elif mutation == "renames":
        data["renames"] = {"JPEG": {"forged->claim": 999}}
    elif mutation == "missing":
        data["missing"] = {"JPEG:forged": 999}
    elif mutation == "extra":
        data["extra"] = {"JPEG:forged": 999}
    elif mutation == "severity":
        data["severity"] = {"forged": 999}
    with mock.patch.object(
            conformance, "run_exiftool",
            lambda _oracle, p: fixtures[Path(p).name]["exiftool"]), \
            mock.patch.object(conformance, "run_oxidex", fake_oxidex), \
            mock.patch.object(conformance.exiftool_oracle, "shared", lambda: oracle):
        try:
            conformance.validate_receipt(
                data, current_git=git_after, expected_contract=expected_contract,
                expected_oracle=oracle)
        except conformance.ReceiptError as exc:
            print(f"rejected: {exc}")
            raise SystemExit(0)
    raise SystemExit("accepted forged receipt mutation")
"""


class NativeAuthorityError(RuntimeError):
    """CI supplied native authority is present but cannot be authenticated."""


def _native_oracle_inputs(environ=None):
    """Return verified native inputs, skipping only when none were supplied."""
    environ = os.environ if environ is None else environ
    source_names = ("OXIDEX_PINNED_EXIFTOOL", "EXIFTOOL_SOURCE")
    source_values = [(name, environ[name]) for name in source_names if name in environ]
    perl_supplied = "EXIFTOOL_PERL" in environ
    if not source_values and not perl_supplied:
        return None
    if not source_values or not perl_supplied:
        raise NativeAuthorityError(
            "supplied native authority requires both EXIFTOOL_PERL and "
            "one ExifTool source input when either is supplied"
        )
    perl_value = environ["EXIFTOOL_PERL"]
    try:
        if len(source_values) == 2:
            declared_paths = [Path(value).expanduser().absolute()
                              for _name, value in source_values]
            if declared_paths[0] != declared_paths[1]:
                raise NativeAuthorityError(
                    "OXIDEX_PINNED_EXIFTOOL and EXIFTOOL_SOURCE select "
                    "different source trees"
                )
        source_paths = []
        for name, value in source_values:
            if not value:
                raise OSError(f"{name} is empty")
            source_paths.append((name, Path(value).resolve(strict=True)))
        if len(source_paths) == 2 and source_paths[0][1] != source_paths[1][1]:
            raise NativeAuthorityError(
                "OXIDEX_PINNED_EXIFTOOL and EXIFTOOL_SOURCE select different "
                "source trees"
            )
        source = source_paths[0][1]
        if not perl_value:
            raise OSError("EXIFTOOL_PERL is empty")
        perl = conformance._resolve_executable(perl_value, "native test Perl")
        oracle = conformance.resolve_oracle(source, perl=str(perl))
        conformance.check_oracle_capability(
            oracle, source / "t" / "images" / "OOXML.docx")
    except (OSError, conformance.exiftool_oracle.OracleError,
            conformance.ReceiptError) as exc:
        raise NativeAuthorityError(
            "invalid supplied native authority: "
            f"{source_values!r}, EXIFTOOL_PERL={perl_value!r}: {exc}") from None
    return source, perl


_NATIVE_ORACLE_INPUTS = _native_oracle_inputs()


class NativeAuthorityConfigurationTests(unittest.TestCase):
    def test_absent_native_authority_is_an_optional_skip(self):
        self.assertIsNone(_native_oracle_inputs({}))

    def test_partial_native_authority_is_a_hard_setup_failure(self):
        with self.assertRaisesRegex(RuntimeError, "requires both"):
            _native_oracle_inputs({"EXIFTOOL_PERL": "/missing/perl"})

    def test_invalid_supplied_perl_is_a_hard_setup_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            source, _perl = OracleResolutionTests._source(Path(directory))
            with self.assertRaisesRegex(RuntimeError, "invalid supplied native authority"):
                _native_oracle_inputs({
                    "EXIFTOOL_PERL": "/missing/perl",
                    "OXIDEX_PINNED_EXIFTOOL": str(source),
                })

    def test_invalid_supplied_source_alias_is_a_hard_setup_failure(self):
        with self.assertRaisesRegex(RuntimeError, "invalid supplied native authority"):
            _native_oracle_inputs({
                "EXIFTOOL_PERL": sys.executable,
                "EXIFTOOL_SOURCE": "/missing/exiftool-source",
            })

    def test_mismatched_source_aliases_are_a_hard_setup_failure(self):
        with self.assertRaisesRegex(RuntimeError, "different source trees"):
            _native_oracle_inputs({
                "EXIFTOOL_PERL": "/missing/perl",
                "OXIDEX_PINNED_EXIFTOOL": "/one/exiftool",
                "EXIFTOOL_SOURCE": "/two/exiftool",
            })

    def test_version_skew_in_supplied_source_is_a_hard_setup_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            source, perl = OracleResolutionTests._source(
                Path(directory), version="13.60")
            with self.assertRaisesRegex(RuntimeError, "invalid supplied native authority"):
                _native_oracle_inputs({
                    "EXIFTOOL_PERL": str(perl),
                    "OXIDEX_PINNED_EXIFTOOL": str(source),
                })


_NATIVE_ORACLE_DRIVER = r"""
import importlib.util, os, subprocess, sys, tempfile
from unittest import mock
from pathlib import Path

module_path, source_value, perl_value = sys.argv[1:]
spec = importlib.util.spec_from_file_location("conformance", module_path)
conformance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(conformance)
source = Path(source_value).resolve()
perl = Path(perl_value).resolve()
docx = source / "t" / "images" / "OOXML.docx"

with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    perl_home = root / "hostile-perl"
    perl_home.mkdir()
    config_home = root / "hostile-home"
    config_home.mkdir()
    perl_marker = root / "perl.marker"
    config_marker = root / "config.marker"
    (perl_home / "OracleHijack.pm").write_text(
        "package OracleHijack; BEGIN { open my $fh, '>', $ENV{R7_PERL_MARKER}; "
        "print {$fh} 'executed'; close $fh; } 1;\n", encoding="utf-8")
    (config_home / ".ExifTool_config").write_text(
        "BEGIN { open my $fh, '>', $ENV{R7_CONFIG_MARKER} or die $!; "
        "print {$fh} 'executed'; close $fh; } 1;\n", encoding="utf-8")

    base = os.environ.copy()
    for name in (*conformance.ORACLE_SELECTOR_ENV,
                 *conformance.PERL_STARTUP_ENV, "HOME", "EXIFTOOL_HOME"):
        base.pop(name, None)
    base.update(conformance.ORACLE_LOCALE_ENV)

    # Prove both attack controls are live before proving the sanitized paths.
    unsanitized_perl = dict(base, PERL5OPT="-MOracleHijack",
                            PERL5LIB=str(perl_home), PERLLIB=str(perl_home),
                            R7_PERL_MARKER=str(perl_marker))
    probe = subprocess.run([str(perl), "-e", "1"], env=unsanitized_perl,
                           capture_output=True, text=True)
    if probe.returncode != 0 or not perl_marker.exists():
        raise SystemExit("native PERL5OPT control did not execute")

    unsanitized_config = dict(base, HOME=str(config_home),
                              EXIFTOOL_HOME=str(config_home),
                              R7_CONFIG_MARKER=str(config_marker))
    probe = subprocess.run(
        [str(perl), f"-I{source / 'lib'}", str(source / 'exiftool'),
         "-s3", "-FileType", str(docx)],
        env=unsanitized_config, capture_output=True, text=True)
    if probe.returncode != 0 or not config_marker.exists():
        raise SystemExit("native ExifTool config control did not execute")
    perl_marker.unlink()
    config_marker.unlink()

    hostile = dict(base, HOME=str(config_home), EXIFTOOL_HOME=str(config_home),
                   PERL5OPT="-MOracleHijack", PERL5LIB=str(perl_home),
                   PERLLIB=str(perl_home), R7_PERL_MARKER=str(perl_marker),
                   R7_CONFIG_MARKER=str(config_marker))
    with mock.patch.dict(os.environ, hostile, clear=True):
        oracle = conformance.resolve_oracle(source, perl=str(perl), strict=True)
        if oracle.argv[-2:] != ["-config", ""]:
            raise SystemExit(f"resolver omitted canonical empty config: {oracle.argv!r}")
        identity = conformance.oracle_identity(oracle)
        conformance.check_oracle_capability(oracle, docx)
        conformance.run_exiftool(oracle, docx)
        conformance._validate_oracle_identity(identity, oracle)
        try:
            conformance.resolve_oracle(strict=True)
        except conformance.exiftool_oracle.OracleError as exc:
            if "requires explicit" not in str(exc):
                raise SystemExit(f"strict fallback failed for wrong reason: {exc}")
        else:
            raise SystemExit("strict resolver accepted implicit inputs")
        try:
            conformance._validate_oracle_identity(identity)
        except conformance.ReceiptError as exc:
            if "requires explicit" not in str(exc):
                raise SystemExit(f"validator fallback failed for wrong reason: {exc}")
        else:
            raise SystemExit("validator accepted implicit oracle inputs")
    if perl_marker.exists() or config_marker.exists():
        raise SystemExit("sanitized oracle path executed hostile control")
print("native hostile controls executed unsanitized and were blocked after isolation")
"""


class JsonOutDeterminismTests(unittest.TestCase):
    """--json-out must not depend on Python's string hashing.

    compare() walked `et_by_name.keys() | ox_by_name.keys()`, a set whose
    iteration order follows str hashing and therefore PYTHONHASHSEED, and
    value_diff came out in that order. Two censuses of identical oracle and
    oxidex output then differed in per_file: census fujim vs e1 (2026-09-12,
    4,238 files) had 51 files dict-unequal of which only 16 were real, so
    every per-file consumer had to normalise before diffing.
    """

    SEEDS = ("1", "2")

    @staticmethod
    def _fixtures():
        # 24 value differences, 8 MISSING, 8 EXTRA and one RENAME in one file,
        # a smaller second file of another format: enough distinct names that
        # two hash seeds cannot plausibly iterate them in the same order.
        a_et = {"File:FileType": "JPEG"}
        a_ox = {"File:FileType": "JPEG"}
        for i in range(24):
            a_et[f"EXIF:IFD0:Tag{i:02d}"] = f"expected {i}"
            a_ox[f"IFD0:Tag{i:02d}"] = f"actual {i}"
        for i in range(8):
            a_et[f"MakerNotes:Canon:Missing{i:02d}"] = f"m{i}"
            a_ox[f"XMP:Extra{i:02d}"] = f"x{i}"
        a_et["PNG:Comment"] = "a distinctive test comment"
        a_ox["PNG:tEXt:comment"] = "a distinctive test comment"
        b_et = {"File:FileType": "PNG", "PNG:ImageWidth": 16, "PNG:ImageHeight": 9,
                "PNG:BitDepth": 8, "PNG:ColorType": "RGB"}
        b_ox = {"File:FileType": "PNG", "PNG:ImageWidth": 17, "PNG:ImageHeight": 10,
                "PNG:BitDepth": 16, "PNG:ColorType": "Palette"}
        return {"a.jpg": {"exiftool": a_et, "oxidex": a_ox},
                "b.png": {"exiftool": b_et, "oxidex": b_ox}}

    def _json_out_under_seed(self, tmp, seed):
        out = tmp / f"census-seed{seed}.json"
        set_order = tmp / f"set-order-seed{seed}.json"
        proc = subprocess.run(
            [sys.executable, "-c", _JSON_OUT_DRIVER, str(MODULE_PATH),
             str(tmp / "corpus"), str(out), str(tmp / "fixtures.json"), str(set_order)],
            env={**os.environ, "PYTHONHASHSEED": seed},
            capture_output=True, text=True, timeout=120,
        )
        self.assertEqual(proc.returncode, 0,
                         f"PYTHONHASHSEED={seed}: driver failed\n{proc.stdout}\n{proc.stderr}")
        return out.read_bytes(), json.loads(set_order.read_text(encoding="utf-8"))

    def test_json_out_is_byte_identical_under_two_hash_seeds(self):
        fixtures = self._fixtures()
        with tempfile.TemporaryDirectory() as d:
            tmp = Path(d)
            (tmp / "corpus").mkdir()
            for name in fixtures:
                (tmp / "corpus" / name).write_bytes(b"")
            (tmp / "fixtures.json").write_text(json.dumps(fixtures), encoding="utf-8")

            (json_a, order_a), (json_b, order_b) = (
                self._json_out_under_seed(tmp, seed) for seed in self.SEEDS)

        # Not vacuous: the seeds really did reorder the set compare() builds,
        # and the run really did produce the rows whose order is at stake.
        self.assertEqual(sorted(order_a), sorted(order_b))
        self.assertNotEqual(order_a, order_b,
                            f"PYTHONHASHSEED {self.SEEDS} iterate the name set identically; "
                            "this test would pass on order-dependent code too")
        per_file = json.loads(json_a)["per_file"]
        a_row = next(v for k, v in per_file.items() if k.endswith("a.jpg"))
        self.assertEqual(len(a_row["value_diff"]), 24)
        self.assertEqual(len(a_row["missing"]), 8)
        self.assertEqual(len(a_row["extra"]), 8)

        self.assertEqual(json_a.decode(), json_b.decode())

    def test_json_receipt_authenticates_occurrences_and_instrument(self):
        fixtures = self._fixtures()
        with tempfile.TemporaryDirectory() as d:
            tmp = Path(d)
            corpus = tmp / "corpus"
            corpus.mkdir()
            for name in fixtures:
                (corpus / name).write_bytes(b"")
            fixture_path = tmp / "fixtures.json"
            fixture_path.write_text(json.dumps(fixtures), encoding="utf-8")
            receipt = tmp / "receipt.json"
            expected_corpus_root = str(corpus.resolve())
            proc = subprocess.run(
                [sys.executable, "-c", _AUTHENTICATED_RECEIPT_DRIVER,
                 str(MODULE_PATH), str(corpus), str(receipt), str(fixture_path)],
                capture_output=True, text=True, timeout=120,
            )
            self.assertEqual(proc.returncode, 0,
                             f"receipt driver failed\n{proc.stdout}\n{proc.stderr}")
            data = json.loads(receipt.read_text(encoding="utf-8"))

        self.assertEqual(data["schema"], 1)
        self.assertEqual(data["oracle_occurrences"], 39)
        self.assertEqual(data["candidate_occurrences"], 39)
        self.assertEqual(data["matched_occurrences"], 2)
        self.assertEqual(data["value_occurrences"], 28)
        self.assertEqual(data["missing_occurrences"], 8)
        self.assertEqual(data["extra_occurrences"], 8)
        self.assertEqual(data["rename_source_occurrences"], 1)
        self.assertEqual(data["rename_target_occurrences"], 1)
        self.assertEqual(
            data["oracle_occurrences"],
            data["matched_occurrences"] + data["missing_occurrences"]
            + data["value_occurrences"] + data["rename_source_occurrences"],
        )
        self.assertEqual(
            data["candidate_occurrences"],
            data["matched_occurrences"] + data["extra_occurrences"]
            + data["value_occurrences"] + data["rename_target_occurrences"],
        )

        instrument = data["instrument"]
        self.assertEqual(instrument["corpus_roots"], [expected_corpus_root])
        self.assertEqual(instrument["file_count"], 2)
        self.assertEqual(instrument["selected_file_count"], 2)
        self.assertEqual(instrument["scored_file_count"], 2)
        self.assertEqual(instrument["corpus_manifest"]["file_count"], 2)
        self.assertEqual(instrument["selection"]["roots"], [expected_corpus_root])
        self.assertFalse(instrument["selection"]["recursive"])
        self.assertEqual(instrument["measurement_transcript"]["row_count"], 2)
        self.assertRegex(instrument["measurement_transcript"]["sha256"], r"^[a-f0-9]{64}$")
        self.assertEqual(instrument["floors"], {"min_files": 2, "min_tags": 1})
        self.assertFalse(instrument["repo"]["dirty"])
        self.assertRegex(instrument["repo"]["commit"], r"^[a-f0-9]{40}$")
        self.assertRegex(instrument["repo"]["tree"], r"^[a-f0-9]{40}$")
        self.assertEqual(instrument["oracle"]["version"], "13.59")
        self.assertEqual(instrument["oracle"]["source"], "pinned source tree")
        self.assertTrue(Path(instrument["oracle"]["runtime"]).is_absolute())
        self.assertNotEqual(Path(instrument["oracle"]["runtime"]), Path("perl5.38.2"))
        self.assertTrue(instrument["oracle"]["verified"])
        self.assertEqual(instrument["oracle"]["capability"]["file_type"], "DOCX")
        self.assertGreater(instrument["oracle"]["source_manifest"]["file_count"], 0)
        self.assertRegex(instrument["binary"]["sha256"], r"^[a-f0-9]{64}$")

    def _run_mutation_driver(self, tmp, mutation):
        fixtures = self._fixtures()
        corpus = tmp / "corpus"
        corpus.mkdir()
        for name in fixtures:
            (corpus / name).write_bytes(b"")
        fixture_path = tmp / "fixtures.json"
        fixture_path.write_text(json.dumps(fixtures), encoding="utf-8")
        receipt = tmp / "receipt.json"
        return subprocess.run(
            [sys.executable, "-c", _AUTHENTICATED_RECEIPT_DRIVER,
             str(MODULE_PATH), str(corpus), str(receipt), str(fixture_path), mutation],
            capture_output=True, text=True, timeout=120,
        )

    def test_main_rejects_source_state_changed_during_measurement(self):
        with tempfile.TemporaryDirectory() as d:
            proc = self._run_mutation_driver(Path(d), "source")
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("source repository changed", proc.stderr + proc.stdout)

    def test_main_rejects_corpus_input_changed_during_measurement(self):
        with tempfile.TemporaryDirectory() as d:
            proc = self._run_mutation_driver(Path(d), "corpus")
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("corpus input changed", proc.stderr + proc.stdout)

    def test_validator_rejects_self_authored_unmeasured_counters(self):
        with tempfile.TemporaryDirectory() as d:
            proc = self._run_mutation_driver(Path(d), "forge")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertIn("rejected", proc.stdout)

    def test_validator_rejects_an_arbitrary_corpus_subset(self):
        with tempfile.TemporaryDirectory() as d:
            proc = self._run_mutation_driver(Path(d), "subset")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertIn("rejected", proc.stdout)

    def test_validator_rejects_an_oracle_manifest_omitting_lib(self):
        with tempfile.TemporaryDirectory() as d:
            proc = self._run_mutation_driver(Path(d), "oracle_manifest")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertIn("rejected", proc.stdout)

    def test_validator_rejects_each_published_claim_when_forged(self):
        for mutation in ("per_format", "per_file", "renames", "missing", "extra", "severity"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as d:
                proc = self._run_mutation_driver(Path(d), mutation)
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
            self.assertIn("rejected", proc.stdout)

    def test_validator_rechecks_corpus_after_replay(self):
        with tempfile.TemporaryDirectory() as d:
            proc = self._run_mutation_driver(Path(d), "replay_corpus")
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("corpus input changed", proc.stderr + proc.stdout)

    @unittest.skipUnless(
        _NATIVE_ORACLE_INPUTS,
        "native hostile-oracle coverage requires CI-supplied source and Perl inputs",
    )
    def test_native_oracle_scrubs_hostile_perl_environment(self):
        source, perl = _NATIVE_ORACLE_INPUTS
        proc = subprocess.run(
            [sys.executable, "-c", _NATIVE_ORACLE_DRIVER, str(MODULE_PATH),
             str(source), str(perl)],
            capture_output=True, text=True, timeout=120,
        )
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertIn("native hostile controls executed unsanitized", proc.stdout)

    @unittest.skipUnless(
        _NATIVE_ORACLE_INPUTS,
        "native hostile-oracle coverage requires CI-supplied source and Perl inputs",
    )
    def test_native_oracle_blocks_config_and_ambient_oracle_controls(self):
        source, perl = _NATIVE_ORACLE_INPUTS
        proc = subprocess.run(
            [sys.executable, "-c", _NATIVE_ORACLE_DRIVER, str(MODULE_PATH),
             str(source), str(perl)],
            capture_output=True, text=True, timeout=120,
        )
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertIn("native hostile controls executed unsanitized", proc.stdout)


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


class MeasurementTranscriptTests(unittest.TestCase):
    def test_volatile_file_tags_do_not_invalidate_a_stable_scored_row(self):
        path = "/corpus/AAC.aac"
        oracle_before = {
            "File:System:FileAccessDate": "2026:09:20 14:35:18-07:00",
            "AAC:AAC:AudioObjectType": "LC (Low Complexity)",
        }
        candidate_before = {
            "File:FileAccessDate": "2026:09:20 14:35:18-07:00",
            "AAC:AudioObjectType": "LC (Low Complexity)",
        }
        oracle_after = {
            **oracle_before,
            "File:System:FileAccessDate": "2026:09:20 14:36:42-07:00",
        }
        candidate_after = {
            **candidate_before,
            "File:FileAccessDate": "2026:09:20 14:36:42-07:00",
        }

        before = conformance.transcript_row(
            path,
            oracle_before,
            candidate_before,
            conformance.compare(oracle_before, candidate_before),
        )
        after = conformance.transcript_row(
            path,
            oracle_after,
            candidate_after,
            conformance.compare(oracle_after, candidate_after),
        )

        self.assertEqual(before, after)

        changed_oracle = {
            **oracle_after,
            "AAC:AAC:AudioObjectType": "Main",
        }
        changed_candidate = {
            **candidate_after,
            "AAC:AudioObjectType": "Main",
        }
        changed = conformance.transcript_row(
            path,
            changed_oracle,
            changed_candidate,
            conformance.compare(changed_oracle, changed_candidate),
        )

        stable_counters = {
            key: value
            for key, value in after.items()
            if key not in {"oracle_sha256", "candidate_sha256"}
        }
        changed_counters = {
            key: value
            for key, value in changed.items()
            if key not in {"oracle_sha256", "candidate_sha256"}
        }
        self.assertEqual(stable_counters, changed_counters)
        self.assertNotEqual(after["oracle_sha256"], changed["oracle_sha256"])
        self.assertNotEqual(after["candidate_sha256"], changed["candidate_sha256"])

    def test_transcript_filter_preserves_raw_keys_duplicates_and_candidate_colons(self):
        oracle = {
            "EXIF:IFD0:Copy1:Make": "Canon",
            "EXIF:IFD0:Copy2:Make": "Canon",
            "File:System:FileAccessDate": "volatile",
        }
        candidate = {
            "IFD0:Make": "Canon",
            "IFD1:Make": "Canon",
            "OOXML:Custom:FileAccessDate": "must remain authenticated",
            "File:FileAccessDate": "volatile",
        }

        self.assertEqual(
            conformance.transcript_tags(oracle, conformance.split_oracle_key),
            {
                "EXIF:IFD0:Copy1:Make": "Canon",
                "EXIF:IFD0:Copy2:Make": "Canon",
            },
        )
        self.assertEqual(
            conformance.transcript_tags(candidate, conformance.split_oxidex_key),
            {
                "IFD0:Make": "Canon",
                "IFD1:Make": "Canon",
                "OOXML:Custom:FileAccessDate": "must remain authenticated",
            },
        )


class ReceiptValidationTests(unittest.TestCase):
    @staticmethod
    def _receipt(tmp):
        binary = tmp / "oxidex"
        binary.write_bytes(b"receipt binary")
        digest = hashlib.sha256(binary.read_bytes()).hexdigest()
        return {
            "schema": 1,
            "oracle_occurrences": 4,
            "candidate_occurrences": 4,
            "matched_occurrences": 2,
            "missing_occurrences": 1,
            "extra_occurrences": 1,
            "value_occurrences": 0,
            "rename_source_occurrences": 1,
            "rename_target_occurrences": 1,
            "oracle_tag_count": 4,
            "instrument": {
                "file_count": 1,
                "floors": {"min_files": 1, "min_tags": 1},
                "binary": {"path": str(binary), "sha256": digest},
            },
        }

    def test_rejects_a_receipt_without_measurement_floors(self):
        with tempfile.TemporaryDirectory() as d:
            receipt = self._receipt(Path(d))
            del receipt["instrument"]["floors"]
            with self.assertRaisesRegex(conformance.ReceiptError, "floors"):
                conformance.validate_receipt(receipt)

    def test_rejects_zero_or_negative_measurement_floors(self):
        with tempfile.TemporaryDirectory() as d:
            receipt = self._receipt(Path(d))
            receipt["instrument"]["floors"]["min_files"] = 0
            with self.assertRaisesRegex(conformance.ReceiptError, "floors"):
                conformance.validate_receipt(receipt)

    def test_rejects_a_zero_occurrence_denominator(self):
        with tempfile.TemporaryDirectory() as d:
            receipt = self._receipt(Path(d))
            for key in (
                "oracle_occurrences", "candidate_occurrences", "matched_occurrences",
                "missing_occurrences", "extra_occurrences", "value_occurrences",
                "rename_source_occurrences", "rename_target_occurrences",
                "oracle_tag_count",
            ):
                receipt[key] = 0
            receipt["instrument"]["floors"] = {"min_files": 1, "min_tags": 1}
            with self.assertRaisesRegex(conformance.ReceiptError, "vacuous"):
                conformance.validate_receipt(receipt)

    def test_rejects_omitted_provenance_identity(self):
        with tempfile.TemporaryDirectory() as d:
            receipt = self._receipt(Path(d))
            with self.assertRaisesRegex(conformance.ReceiptError, "provenance"):
                conformance.validate_receipt(receipt)

    def test_rejects_forged_provenance_identity(self):
        with tempfile.TemporaryDirectory() as d:
            receipt = self._receipt(Path(d))
            receipt["instrument"].update({
                "repo": {"root": str(Path.cwd()), "commit": "f" * 40,
                         "tree": "f" * 40, "dirty": False, "dirty_files": []},
                "corpus_roots": [str(Path(d))],
                "file_count": 1,
                "scored_file_count": 1,
                "oracle": {"version": "12.0", "pinned_version": "12.0",
                            "source": "forged", "runtime": "forged",
                            "verified": True, "missing_modules": []},
            })
            with self.assertRaisesRegex(conformance.ReceiptError, "provenance"):
                conformance.validate_receipt(receipt)

    def test_rejects_totals_that_do_not_reconcile(self):
        with tempfile.TemporaryDirectory() as d:
            receipt = self._receipt(Path(d))
            receipt["oracle_occurrences"] = 99
            receipt["oracle_tag_count"] = 99
            with self.assertRaises(conformance.ReceiptError):
                conformance.validate_receipt(receipt)

    def test_rejects_a_candidate_binary_changed_after_measurement(self):
        with tempfile.TemporaryDirectory() as d:
            receipt = self._receipt(Path(d))
            Path(receipt["instrument"]["binary"]["path"]).write_bytes(b"tampered")
            with self.assertRaises(conformance.ReceiptError):
                conformance.validate_receipt(receipt)


if __name__ == "__main__":
    unittest.main()
