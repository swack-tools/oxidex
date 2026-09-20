"""Black-box CLI contract tests for the maintained census entry point."""

import pathlib
import subprocess
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[3]
SCRIPT = ROOT / "tools/exiftool-tables/genshare/census.sh"
MANIFEST = ROOT / "tools/exiftool-tables/genshare/testdata/bounded-corpus.txt"


class CensusCliTests(unittest.TestCase):
    def test_manifest_is_a_three_file_bounded_corpus(self):
        rows = [row for row in MANIFEST.read_text(encoding="utf-8").splitlines() if row]
        self.assertEqual(rows, ["Apple/Apple_iPhone11.jpg", "AIFF.aif", "Garmin.fit"])

    def test_unknown_or_unsafe_token_exits_two_before_oracle_or_corpus_work(self):
        result = subprocess.run(
            [
                "bash", str(SCRIPT),
                "--repository", str(ROOT),
                "--target-dir", str(ROOT / "target-for-test"),
                "--output", str(ROOT / "census-for-test"),
                "--corpus", "/does/not/exist",
                "--perl", "/does/not/exist",
                "--exiftool-dir", "/does/not/exist",
                "--tokens", "engine,conv",
            ],
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("conv", result.stderr)
        self.assertNotIn("oracle", result.stdout.lower())


if __name__ == "__main__":
    unittest.main()
