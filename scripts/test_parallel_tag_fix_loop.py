import tempfile
import unittest
from pathlib import Path

from parallel_tag_fix_loop import classify_worker_exit, parse_worker_summary


class ParseWorkerSummaryTests(unittest.TestCase):
    def _write(self, tmpdir, text):
        path = Path(tmpdir) / "worker-1.log"
        path.write_text(text)
        return path

    def test_missing_file_is_zero_zero_no_summary(self):
        result = parse_worker_summary(Path("/nonexistent/worker-1.log"))
        self.assertEqual(result, (0, 0, False))

    def test_no_summary_yet_is_zero_zero_no_summary(self):
        # A worker that crashed before ever printing "stopped after N
        # rounds" (e.g. an uncaught exception) looks identical to a real
        # no-work exit on fixed/failed counts alone -- has_summary is what
        # tells the two apart. See classify_worker_exit.
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(tmpdir, "   Compiling oxidex v1.2.1\n")
            self.assertEqual(parse_worker_summary(path), (0, 0, False))

    def test_parses_real_summary_with_work_done(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(
                tmpdir,
                "stopped after 2 rounds\n"
                "  fixed:   1 tags\n"
                "  failed:  0 attempts\n"
                "  cycles reset (blacklist exhausted): 0\n",
            )
            self.assertEqual(parse_worker_summary(path), (1, 0, True))

    def test_parses_real_summary_with_no_work_done(self):
        # This is the genuine "nothing left in the shared pool" case --
        # has_summary=True is what distinguishes it from a crash that also
        # reports (0, 0).
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(
                tmpdir,
                "All tags found -- nothing left to fix.\n"
                "stopped after 1 rounds\n"
                "  fixed:   0 tags\n"
                "  failed:  0 attempts\n"
                "  cycles reset (blacklist exhausted): 0\n",
            )
            self.assertEqual(parse_worker_summary(path), (0, 0, True))

    def test_parses_failed_attempts_with_zero_fixed(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(
                tmpdir,
                "stopped after 2 rounds\n"
                "  fixed:   0 tags\n"
                "  failed:  2 attempts\n"
                "  cycles reset (blacklist exhausted): 0\n",
            )
            self.assertEqual(parse_worker_summary(path), (0, 2, True))


class ClassifyWorkerExitTests(unittest.TestCase):
    def test_nonzero_returncode_is_crashed_even_with_a_summary(self):
        # Shouldn't happen in practice (main() always returns 0 once it
        # reaches the summary print), but a nonzero exit is never safe to
        # read as "no work" regardless of what the log otherwise shows.
        self.assertEqual(classify_worker_exit(1, True, 0, 0), "crashed")

    def test_zero_returncode_but_no_summary_is_crashed(self):
        # The exact bug this was written to fix: a worker that crashed
        # (e.g. a network timeout building cargo) before printing its
        # summary must not be mistaken for "the shared tag pool is empty".
        self.assertEqual(classify_worker_exit(0, False, 0, 0), "crashed")

    def test_clean_exit_with_real_zero_summary_is_no_work(self):
        self.assertEqual(classify_worker_exit(0, True, 0, 0), "no_work")

    def test_clean_exit_with_fixed_tags_is_respawn(self):
        self.assertEqual(classify_worker_exit(0, True, 1, 0), "respawn")

    def test_clean_exit_with_only_failed_attempts_is_respawn(self):
        self.assertEqual(classify_worker_exit(0, True, 0, 2), "respawn")


if __name__ == "__main__":
    unittest.main()
