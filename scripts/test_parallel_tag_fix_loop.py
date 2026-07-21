import tempfile
import unittest
from pathlib import Path

from parallel_tag_fix_loop import parse_worker_summary


class ParseWorkerSummaryTests(unittest.TestCase):
    def _write(self, tmpdir, text):
        path = Path(tmpdir) / "worker-1.log"
        path.write_text(text)
        return path

    def test_missing_file_is_zero_zero(self):
        fixed, failed = parse_worker_summary(Path("/nonexistent/worker-1.log"))
        self.assertEqual((fixed, failed), (0, 0))

    def test_no_summary_yet_is_zero_zero(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(tmpdir, "   Compiling oxidex v1.2.1\n")
            self.assertEqual(parse_worker_summary(path), (0, 0))

    def test_parses_real_summary_with_work_done(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(
                tmpdir,
                "stopped after 2 rounds\n"
                "  fixed:   1 tags\n"
                "  failed:  0 attempts\n"
                "  cycles reset (blacklist exhausted): 0\n",
            )
            self.assertEqual(parse_worker_summary(path), (1, 0))

    def test_parses_real_summary_with_no_work_done(self):
        # This is the "nothing left in the shared pool" case -- must be
        # distinguishable from "did work and hit max-tags-per-process" so
        # the caller knows not to respawn this slot.
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(
                tmpdir,
                "All tags found -- nothing left to fix.\n"
                "stopped after 1 rounds\n"
                "  fixed:   0 tags\n"
                "  failed:  0 attempts\n"
                "  cycles reset (blacklist exhausted): 0\n",
            )
            self.assertEqual(parse_worker_summary(path), (0, 0))

    def test_parses_failed_attempts_with_zero_fixed(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(
                tmpdir,
                "stopped after 2 rounds\n"
                "  fixed:   0 tags\n"
                "  failed:  2 attempts\n"
                "  cycles reset (blacklist exhausted): 0\n",
            )
            self.assertEqual(parse_worker_summary(path), (0, 2))


if __name__ == "__main__":
    unittest.main()
