import json
import tempfile
import unittest
from pathlib import Path

from log_sweep_review import append_sweep_review, main


class AppendSweepReviewTests(unittest.TestCase):
    def test_writes_one_json_line_with_expected_fields(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "log.jsonl"
            entry = append_sweep_review(
                log_path, "RW2", "IFD0:BlackLevelBlue", "accepted",
                "Matches this codebase's IFD0: convention", commit="7d7896a",
                now_fn=lambda: 1_700_000_000,
            )
            lines = log_path.read_text().splitlines()
        self.assertEqual(len(lines), 1)
        parsed = json.loads(lines[0])
        self.assertEqual(parsed["format"], "RW2")
        self.assertEqual(parsed["tag"], "IFD0:BlackLevelBlue")
        self.assertEqual(parsed["verdict"], "accepted")
        self.assertEqual(parsed["commit"], "7d7896a")
        self.assertEqual(parsed, entry)

    def test_appends_without_clobbering_existing_entries(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "log.jsonl"
            append_sweep_review(log_path, "NEF", "A", "accepted", "r1")
            append_sweep_review(log_path, "NEF", "B", "rejected", "r2")
            lines = log_path.read_text().splitlines()
        self.assertEqual(len(lines), 2)

    def test_rejects_invalid_verdict(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "log.jsonl"
            with self.assertRaises(ValueError):
                append_sweep_review(log_path, "NEF", "A", "maybe", "r")

    def test_creates_parent_directories(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "nested" / "dir" / "log.jsonl"
            append_sweep_review(log_path, "NEF", "A", "accepted", "r")
            self.assertTrue(log_path.exists())


class MainTests(unittest.TestCase):
    def test_cli_writes_expected_entry(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log_path = Path(tmpdir) / "log.jsonl"
            rc = main([
                "--format", "XMP", "--tag", "XMP:AboutCvTermCvId",
                "--verdict", "rejected",
                "--reason", "guessed JSON bracket syntax instead of comma-space text",
                "--commit", "e77bffe",
                "--log-path", str(log_path),
            ])
            parsed = json.loads(log_path.read_text().splitlines()[0])
        self.assertEqual(rc, 0)
        self.assertEqual(parsed["format"], "XMP")
        self.assertEqual(parsed["verdict"], "rejected")


if __name__ == "__main__":
    unittest.main()
