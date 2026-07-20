import io
import tempfile
import unittest
from pathlib import Path

from watch_parallel_fix import (
    GREEN,
    RED,
    YELLOW,
    discover_formats,
    main,
    parse_status,
    render,
)


class ParseStatusTests(unittest.TestCase):
    def _write(self, tmpdir, text):
        path = Path(tmpdir) / "JPEG.log"
        path.write_text(text)
        return path

    def test_missing_file_is_waiting(self):
        label, color, detail = parse_status(Path("/nonexistent/JPEG.log"))
        self.assertEqual(label, "waiting")

    def test_empty_file_is_waiting(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(tmpdir, "")
            label, color, detail = parse_status(path)
            self.assertEqual(label, "waiting")

    def test_unrecognized_output_is_busy_with_last_line_as_detail(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(tmpdir, "   Compiling oxidex v1.2.1\n   Compiling foo v0.1.0\n")
            label, color, detail = parse_status(path)
            self.assertEqual(label, "busy")
            self.assertIn("Compiling foo", detail)

    def test_gap_delta_closing_gaps_is_green(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(tmpdir, "[JPEG] gaps 12 -> 9\n")
            label, color, detail = parse_status(path)
            self.assertEqual(label, "attempt")
            self.assertEqual(color, GREEN)
            self.assertIn("(+3)", detail)

    def test_gap_delta_regressing_is_red(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(tmpdir, "[JPEG] gaps 9 -> 12\n")
            label, color, detail = parse_status(path)
            self.assertEqual(color, RED)
            self.assertIn("(-3)", detail)

    def test_gap_delta_unchanged_is_yellow(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(tmpdir, "[JPEG] gaps 9 -> 9\n")
            label, color, detail = parse_status(path)
            self.assertEqual(color, YELLOW)

    def test_fixed_line_wins_over_earlier_gap_delta_line(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(
                tmpdir,
                "[JPEG] gaps 12 -> 9\n[JPEG] FIXED: closed 3 gaps (committed)\n",
            )
            label, color, detail = parse_status(path)
            self.assertEqual(label, "fixed")
            self.assertEqual(color, GREEN)
            self.assertIn("+3 gaps closed", detail)

    def test_review_rejected_is_yellow(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(tmpdir, "[JPEG] review REJECTED: hardcodes sample value\n")
            label, color, detail = parse_status(path)
            self.assertEqual(label, "rejected")
            self.assertEqual(color, YELLOW)

    def test_gap_count_did_not_decrease_is_reverted_red(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(tmpdir, "[JPEG] gap count did not decrease, reverting\n")
            label, color, detail = parse_status(path)
            self.assertEqual(label, "reverted")
            self.assertEqual(color, RED)

    def test_build_failed_is_red(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(tmpdir, "[JPEG] build failed: no diff in model response\n")
            label, color, detail = parse_status(path)
            self.assertEqual(label, "build-fail")
            self.assertEqual(color, RED)

    def test_stopped_summary_is_done_and_wins_over_everything_earlier(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = self._write(
                tmpdir,
                "[JPEG] gaps 12 -> 9\n[JPEG] FIXED: closed 3 gaps (committed)\n"
                "stopped after 3 rounds\n  fixed:   1 formats\n",
            )
            label, color, detail = parse_status(path)
            self.assertEqual(label, "done")
            self.assertIn("stopped after 3 rounds", detail)


class DiscoverFormatsTests(unittest.TestCase):
    def test_lists_log_stems_sorted(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "NEF.log").write_text("")
            (tmp / "AVI.log").write_text("")
            (tmp / "not-a-log.txt").write_text("")
            self.assertEqual(discover_formats(tmp), ["AVI", "NEF"])


class RenderTests(unittest.TestCase):
    def test_includes_a_line_per_format(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "NEF.log").write_text("[NEF] gaps 5 -> 2\n")
            output = render(tmp, ["NEF"])
            self.assertIn("NEF", output)
            self.assertIn("attempt", output)


class MainLoopTests(unittest.TestCase):
    def test_waits_until_a_log_file_appears_then_renders_and_exits_on_interrupt(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            sleeps = []

            def fake_sleep(interval):
                sleeps.append(interval)
                if len(sleeps) == 1:
                    (tmp / "NEF.log").write_text("[NEF] gaps 5 -> 2\n")
                elif len(sleeps) == 2:
                    raise KeyboardInterrupt

            out = io.StringIO()
            exit_code = main(["--log-dir", str(tmp), "--interval", "0.1"], sleep_fn=fake_sleep, stdout=out)

            self.assertEqual(exit_code, 0)
            self.assertIn("Waiting for logs", out.getvalue())
            self.assertIn("NEF", out.getvalue())
            self.assertEqual(sleeps, [0.1, 0.1])


if __name__ == "__main__":
    unittest.main()
