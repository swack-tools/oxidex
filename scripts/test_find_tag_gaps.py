import json
import unittest
from pathlib import Path
from unittest.mock import patch, MagicMock

from find_tag_gaps import group_gaps_by_format, locate_parser_files, run_full_comparison, run_format_comparison

FIXTURE = Path(__file__).resolve().parent.parent / "tests" / "fixtures" / "comparison_report_sample.json"


class GroupGapsByFormatTests(unittest.TestCase):
    def setUp(self):
        with open(FIXTURE) as f:
            self.report = json.load(f)

    def test_sorts_by_gap_count_descending(self):
        gaps = group_gaps_by_format(self.report)
        counts = [g["gap_count"] for g in gaps]
        self.assertEqual(counts, sorted(counts, reverse=True))

    def test_skips_formats_with_no_gaps(self):
        gaps = group_gaps_by_format(self.report)
        formats = {g["format"] for g in gaps}
        self.assertNotIn("PNG", formats)

    def test_gap_count_is_missing_plus_differences(self):
        gaps = group_gaps_by_format(self.report)
        nef = next(g for g in gaps if g["format"] == "NEF")
        self.assertEqual(nef["gap_count"], len(nef["missing_tags"]) + len(nef["value_differences"]))
        self.assertEqual(nef["gap_count"], 4)

    def test_includes_missing_tags_and_value_differences_verbatim(self):
        gaps = group_gaps_by_format(self.report)
        jpeg = next(g for g in gaps if g["format"] == "JPEG")
        self.assertEqual(jpeg["missing_tags"][0]["name"], "LensModel")
        self.assertEqual(jpeg["value_differences"][0]["tag_key"], "EXIF:ISO")


class LocateParserFilesTests(unittest.TestCase):
    def test_jpeg_maps_to_a_real_directory(self):
        files = locate_parser_files("JPEG")
        self.assertTrue(any("src/parsers/jpeg" in f or "src/core" in f for f in files))
        self.assertGreater(len(files), 0)

    def test_unknown_format_with_no_matching_directory_returns_empty(self):
        files = locate_parser_files("TotallyMadeUpFormat")
        self.assertEqual(files, [])


# The /tmp/... literals below are inert test-fixture values passed to a
# mocked subprocess.run -- no real filesystem I/O happens in this file.
class RunFullComparisonTests(unittest.TestCase):
    @patch("find_tag_gaps.subprocess.run")
    def test_invokes_just_with_cache_dir_env(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0)
        result = run_full_comparison("/tmp/fake-cache", repo_root=Path("/fake/repo"))  # nosec B108
        args, kwargs = mock_run.call_args
        self.assertEqual(args[0], ["just", "compare-exiftool-full"])
        self.assertEqual(kwargs["cwd"], Path("/fake/repo"))
        self.assertEqual(kwargs["env"]["EXIFTOOL_CACHE_DIR"], "/tmp/fake-cache")  # nosec B108
        self.assertEqual(result, Path("/fake/repo/comparison.json"))


class RunFormatComparisonTests(unittest.TestCase):
    @patch("find_tag_gaps.ensure_tag_comparison_built")
    @patch("find_tag_gaps.subprocess.run")
    def test_invokes_tag_comparison_with_format_flag(self, mock_run, mock_ensure):
        mock_run.return_value = MagicMock(returncode=0)
        result = run_format_comparison("NEF", "/tmp/fake-cache", repo_root=Path("/fake/repo"))  # nosec B108
        mock_ensure.assert_called_once_with(Path("/fake/repo"))
        args, kwargs = mock_run.call_args
        self.assertIn("--format", args[0])
        self.assertIn("NEF", args[0])
        self.assertIn("--samples", args[0])
        self.assertIn("/tmp/fake-cache/combined-samples", args[0])  # nosec B108
        self.assertEqual(result, Path("/tmp/tagcmp-NEF.json"))  # nosec B108


if __name__ == "__main__":
    unittest.main()
